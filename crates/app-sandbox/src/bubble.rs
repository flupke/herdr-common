//! The bubble: the bubblewrap environment of
//! [`Isolation::Bubblewrap`](crate::Isolation::Bubblewrap), which a caller may also run alone,
//! with no herdr-app, such as for a Herdr server of its own.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use eyre::{Context, ContextCompat, Result};
use serde::Deserialize;

/// Where the tmpfs is, inside: the sandbox's directory as its processes see it.
const INSIDE: &str = "/tmp";

/// A running bubble: a bubblewrap environment with a PID namespace and a mount namespace of its
/// own, whose `/tmp` ([`inside`](Self::inside)) is a tmpfs of its own, and a network the caller
/// picks ([`Network`]). Every process [`command`](Self::command) starts runs in it, and every
/// file it writes in `/tmp` goes in the tmpfs, which the host reaches through
/// [`outside`](Self::outside). Once the bubble ends, the kernel kills each process left in it and
/// frees the tmpfs, also when the process that holds it dies by `SIGKILL`: nothing is ever
/// deleted, and nothing is created outside.
///
/// The rest of the host's filesystem is shared, except the directories the bubble masks, such
/// as the [`user_directories`](crate::user_directories): each shows empty inside, under a tmpfs
/// of its own, while the host's copy stays as it was.
///
/// It ends once it lets go of its init's standard input: on [`end`](Self::end), on a drop, and
/// when the process that holds it dies, since the kernel then closes the pipe.
///
/// The tmpfs is mounted on `/tmp` rather than on a new empty directory: such a directory would
/// outlive a `SIGKILL` of the process that holds the bubble, which then cannot remove it.
///
/// Its init, `cat` on the pipe, runs as the namespace's first process (`--as-pid-1`): once it
/// ends, the kernel kills the rest of the namespace. bwrap's own first process would instead
/// wait for every process of the namespace, a detached one included.
///
/// bwrap's `--die-with-parent` is left out: it sets `PR_SET_PDEATHSIG`, which fires when the
/// *thread* that spawned bwrap ends, such as a Tokio blocking thread after ten idle seconds.
pub struct Bubble {
    /// bwrap, which runs the init.
    bwrap: Child,
    entry: Entry,
    /// The inode of the PID namespace, as `/proc/<pid>/ns/pid` names it.
    namespace: u64,
}

/// The network a bubble's processes reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    /// A network namespace of its own, with a loopback alone: the bubble reaches no port of the
    /// host, and no abstract socket, such as the user's X display's (`@/tmp/.X11-unix/X0`),
    /// which an X server in the bubble would otherwise clash with. The app sandbox's, whose host
    /// reaches a port through [`AppSandbox::forward`](crate::AppSandbox::forward).
    Loopback,
    /// The host's: a port a process of the bubble listens on answers on the host's loopback,
    /// and the bubble reaches every port and abstract socket the host does, the user's X
    /// display's included.
    Host,
}

/// How a command enters a running bubble. A thread that keeps a copy, such as a
/// [`Forward`](crate::forward::Forward)'s, stops using it before the bubble ends: once the
/// first process's PID is free, another process may take it.
#[derive(Clone)]
pub(crate) struct Entry {
    /// nsenter, found on this process's `PATH`: a command's own `PATH`, the sandbox's, would
    /// look in the host's `/tmp/bin` first.
    nsenter: PathBuf,
    network: Network,
    /// The host PID of the namespace's first process: `/proc/<init>/root` shows the tmpfs from
    /// outside, and nsenter enters the namespaces through it.
    init: u32,
}

/// What bwrap's `--info-fd` reports.
#[derive(Deserialize)]
struct Info {
    #[serde(rename = "child-pid")]
    child_pid: u32,
    #[serde(rename = "pid-namespace")]
    pid_namespace: u64,
}

impl Bubble {
    /// Start a bubble that shows each directory of `masked` empty, on `network`, and return
    /// once its tmpfs is mounted. A directory of `masked` that does not exist stays absent:
    /// bwrap would create it on the host, which `/` shares. bwrap and nsenter run from this
    /// process's `PATH`, such as the dev shell's.
    pub fn start(masked: &[PathBuf], network: Network) -> Result<Self> {
        let nsenter = on_path("nsenter").context("no nsenter on PATH: run in the dev shell")?;
        let masks = masked
            .iter()
            .filter(|directory| directory.is_dir())
            .flat_map(|directory| [OsString::from("--tmpfs"), directory.into()]);
        // bwrap writes its report on standard error, with its errors, as soon as it created the
        // namespaces, then closes it: before it mounted the tmpfs, so a path through
        // `/proc/<init>/root` still reaches the host's `/tmp`. The init says `ready` on
        // standard output once it runs, past every mount.
        let mut bwrap = Command::new("bwrap")
            .args(["--dev-bind", "/", "/", "--tmpfs", INSIDE])
            .args(masks)
            .args(["--unshare-pid", "--as-pid-1"])
            .args((network == Network::Loopback).then_some("--unshare-net"))
            .args(["--proc", "/proc", "--info-fd", "2", "--"])
            .args(["sh", "-c", "echo ready; exec cat >/dev/null"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .wrap_err("cannot run bwrap (bubblewrap): run in the dev shell")?;
        let report = bwrap.stderr.take().context("bwrap's errors are piped")?;
        let report = std::io::read_to_string(report)?;
        let ready = bwrap.stdout.take().context("bwrap's output is piped")?;
        let ready = BufReader::new(ready).lines().next().transpose()?;
        match serde_json::from_str::<Info>(&report) {
            Ok(info) if ready.as_deref() == Some("ready") => Ok(Self {
                bwrap,
                entry: Entry {
                    nsenter,
                    network,
                    init: info.child_pid,
                },
                namespace: info.pid_namespace,
            }),
            _ => {
                drop(bwrap.stdin.take());
                let status = bwrap.wait()?;
                eyre::bail!("bwrap did not start ({status}): {}", report.trim());
            }
        }
    }

    /// The bubble's tmpfs, `/tmp`, as its processes see it.
    pub fn inside() -> &'static Path {
        Path::new(INSIDE)
    }

    /// The path that shows `inside`, a path inside the bubble, from outside: under
    /// `/proc/<PID>/root`, valid while the bubble runs.
    pub fn outside(&self, inside: &Path) -> PathBuf {
        let relative = inside.strip_prefix("/").unwrap_or(inside);
        PathBuf::from(format!("/proc/{}/root", self.entry.init)).join(relative)
    }

    /// A command that runs `program` inside the bubble, in `directory` (a path inside), with
    /// `environment` alone: nsenter forks the program into the bubble's namespaces, and waits
    /// for it.
    pub fn command(
        &self,
        program: &Path,
        directory: &Path,
        environment: &[(OsString, OsString)],
    ) -> Command {
        self.entry.command(program, directory, environment)
    }

    /// How a command enters the bubble.
    pub(crate) fn entry(&self) -> &Entry {
        &self.entry
    }

    /// The nsenter that runs each [`command`](Self::command).
    pub(crate) fn nsenter(&self) -> &Path {
        &self.entry.nsenter
    }

    /// The command line that runs `program` inside the bubble as [`command`](Self::command)
    /// does, with `environment` in its arguments, whatever environment runs it: nsenter's path
    /// first, for a terminal to launch, such as a pane of a Herdr client.
    pub fn launch(
        &self,
        program: &Path,
        directory: &Path,
        environment: &[(OsString, OsString)],
    ) -> Vec<OsString> {
        let variables = environment.iter().map(|(name, value)| {
            let mut variable = name.clone();
            variable.push("=");
            variable.push(value);
            variable
        });
        let mut line = vec![self.entry.nsenter.clone().into_os_string()];
        line.extend(self.entry.arguments());
        line.extend(["-i".into(), chdir(directory)]);
        line.extend(variables);
        line.push(program.into());
        line
    }

    /// The host PID of the bubble's first process: unique while the bubble runs.
    pub fn id(&self) -> u32 {
        self.entry.init
    }

    /// The processes in the bubble's PID namespace, its first one included, by host PID: none
    /// once it ended.
    pub fn processes(&self) -> Vec<u32> {
        let name = format!("pid:[{}]", self.namespace);
        crate::process::all()
            .into_iter()
            .filter(|pid| {
                std::fs::read_link(format!("/proc/{pid}/ns/pid"))
                    .is_ok_and(|link| link.as_os_str() == name.as_str())
            })
            .collect()
    }

    /// End the bubble, and return once bwrap ended: the kernel killed every process in it and
    /// freed its tmpfs. Ending it again does nothing.
    pub fn end(&mut self) -> Result<()> {
        drop(self.bwrap.stdin.take());
        self.bwrap.wait().wrap_err("cannot wait for bwrap")?;
        Ok(())
    }
}

impl Entry {
    /// [`Bubble::command`]. nsenter's own `--wd` opens its directory before it enters the mount
    /// namespace, outside: `env` changes directory once inside. A bubble on the host's network
    /// is entered without `--net`: the host's network namespace belongs to no namespace of the
    /// bubble's, so entering it would be refused.
    pub(crate) fn command(
        &self,
        program: &Path,
        directory: &Path,
        environment: &[(OsString, OsString)],
    ) -> Command {
        let mut command = Command::new(&self.nsenter);
        command
            .args(self.arguments())
            .arg(chdir(directory))
            .arg(program)
            .env_clear()
            .envs(environment.iter().cloned());
        command
    }

    /// nsenter's arguments that enter the bubble and run `env`, up to `env`'s own.
    fn arguments(&self) -> Vec<OsString> {
        let mut arguments: Vec<OsString> = ["--preserve-credentials", "--user", "--mount", "--pid"]
            .map(OsString::from)
            .into();
        if self.network == Network::Loopback {
            arguments.push("--net".into());
        }
        arguments.extend([
            format!("--target={}", self.init).into(),
            "--".into(),
            "env".into(),
        ]);
        arguments
    }
}

impl Drop for Bubble {
    fn drop(&mut self) {
        let _ = self.end();
    }
}

/// `env`'s option that runs its program in `directory`.
fn chdir(directory: &Path) -> OsString {
    let mut option = OsString::from("--chdir=");
    option.push(directory);
    option
}

/// The program `name` as this process's `PATH` finds it.
pub(crate) fn on_path(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|path| {
            path.is_absolute()
                && path
                    .metadata()
                    .is_ok_and(|file| file.is_file() && file.permissions().mode() & 0o111 != 0)
        })
}
