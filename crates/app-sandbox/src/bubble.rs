//! The bubblewrap environment of [`Isolation::Bubblewrap`](crate::Isolation::Bubblewrap): a
//! PID namespace, a network namespace with a loopback alone, and a mount namespace whose `/tmp`
//! is a tmpfs of its own. Every process of the sandbox runs in it, and every file it writes
//! goes in the tmpfs. Once the environment ends, the kernel kills each process left in it and
//! frees the tmpfs, also when the process that holds the sandbox dies by `SIGKILL`: nothing is
//! ever deleted, and nothing is created outside.
//!
//! The tmpfs is mounted on `/tmp` rather than on a new empty directory: such a directory would
//! outlive a `SIGKILL` of the holder, which then cannot remove it. The network namespace keeps
//! the X display's abstract socket (`@/tmp/.X11-unix/X0`) from clashing with the user's
//! displays or reaching them, and keeps the app off the network.
//!
//! The rest of the host's filesystem is shared, except the directories the environment masks:
//! each shows empty inside, under a tmpfs of its own, while the host's copy stays as it was.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use eyre::{Context, ContextCompat, Result};
use serde::Deserialize;

/// Where the tmpfs is, inside: the sandbox's directory as its processes see it.
const INSIDE: &str = "/tmp";

/// A running bubblewrap environment. It ends once it lets go of its holder's standard input: on
/// [`end`](Self::end), on a drop, and when the process that holds it dies, since the kernel then
/// closes the pipe.
///
/// The holder, `cat` on the pipe, runs as the namespace's first process (`--as-pid-1`): once it
/// ends, the kernel kills the rest of the namespace. bwrap's own first process would instead
/// wait for every process of the namespace, a detached one included.
///
/// bwrap's `--die-with-parent` is left out: it sets `PR_SET_PDEATHSIG`, which fires when the
/// *thread* that spawned bwrap ends, such as a Tokio blocking thread after ten idle seconds.
pub(crate) struct Bubble {
    /// bwrap, which runs the holder.
    holder: Child,
    /// nsenter, found on this process's `PATH`: a command's own `PATH`, the sandbox's, would
    /// look in the host's `/tmp/bin` first.
    nsenter: PathBuf,
    /// The host PID of the namespace's first process: `/proc/<init>/root` shows the tmpfs from
    /// outside, and nsenter enters the namespaces through it.
    init: u32,
    /// The inode of the PID namespace, as `/proc/<pid>/ns/pid` names it.
    namespace: u64,
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
    /// Start an environment that shows each directory of `masked` empty, and return once its
    /// tmpfs is mounted. A directory of `masked` that does not exist stays absent: bwrap would
    /// create it on the host, which `/` shares.
    pub(crate) fn start(masked: &[PathBuf]) -> Result<Self> {
        let nsenter = on_path("nsenter").context("no nsenter on PATH: run in the dev shell")?;
        let masks = masked
            .iter()
            .filter(|directory| directory.is_dir())
            .flat_map(|directory| [OsString::from("--tmpfs"), directory.into()]);
        // bwrap writes its report on standard error, with its errors, as soon as it created the
        // namespaces, then closes it: before it mounted the tmpfs, so a path through
        // `/proc/<init>/root` still reaches the host's `/tmp`. The holder says `ready` on
        // standard output once it runs, past every mount.
        let mut holder = Command::new("bwrap")
            .args(["--dev-bind", "/", "/", "--tmpfs", INSIDE])
            .args(masks)
            .args(["--unshare-pid", "--as-pid-1", "--unshare-net"])
            .args(["--proc", "/proc", "--info-fd", "2", "--"])
            .args(["sh", "-c", "echo ready; exec cat >/dev/null"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .wrap_err("cannot run bwrap (bubblewrap): run in the dev shell")?;
        let report = holder.stderr.take().context("bwrap's errors are piped")?;
        let report = std::io::read_to_string(report)?;
        let ready = holder.stdout.take().context("bwrap's output is piped")?;
        let ready = BufReader::new(ready).lines().next().transpose()?;
        match serde_json::from_str::<Info>(&report) {
            Ok(info) if ready.as_deref() == Some("ready") => Ok(Self {
                holder,
                nsenter,
                init: info.child_pid,
                namespace: info.pid_namespace,
            }),
            _ => {
                drop(holder.stdin.take());
                let status = holder.wait()?;
                eyre::bail!("bwrap did not start ({status}): {}", report.trim());
            }
        }
    }

    /// The sandbox's directory, as its processes see it.
    pub(crate) fn inside() -> &'static Path {
        Path::new(INSIDE)
    }

    /// The path that shows `inside`, a path inside the environment, from outside.
    pub(crate) fn outside(&self, inside: &Path) -> PathBuf {
        let relative = inside.strip_prefix("/").unwrap_or(inside);
        PathBuf::from(format!("/proc/{}/root", self.init)).join(relative)
    }

    /// A command that runs `program` inside the environment, in `directory` (a path inside),
    /// with `environment` alone. nsenter forks the program into the namespaces, and waits for
    /// it. nsenter's own `--wd` opens its directory before it enters the mount namespace,
    /// outside: `env` changes directory once inside.
    pub(crate) fn command(
        &self,
        program: &Path,
        directory: &Path,
        environment: &[(OsString, OsString)],
    ) -> Command {
        let mut command = Command::new(&self.nsenter);
        command
            .args([
                "--preserve-credentials",
                "--user",
                "--mount",
                "--pid",
                "--net",
                &format!("--target={}", self.init),
                "--",
                "env",
            ])
            .arg(format!("--chdir={}", directory.display()))
            .arg(program)
            .env_clear()
            .envs(environment.iter().cloned());
        command
    }

    /// The nsenter that runs each [`command`](Self::command).
    pub(crate) fn nsenter(&self) -> &Path {
        &self.nsenter
    }

    /// The processes in the environment's PID namespace, its first one included, by host PID.
    pub(crate) fn processes(&self) -> Vec<u32> {
        let name = format!("pid:[{}]", self.namespace);
        crate::process::all()
            .into_iter()
            .filter(|pid| {
                std::fs::read_link(format!("/proc/{pid}/ns/pid"))
                    .is_ok_and(|link| link.as_os_str() == name.as_str())
            })
            .collect()
    }

    /// End the environment, and return once bwrap ended: the kernel killed every process in it
    /// and freed its tmpfs.
    pub(crate) fn end(&mut self) -> Result<()> {
        drop(self.holder.stdin.take());
        self.holder.wait().wrap_err("cannot wait for bwrap")?;
        Ok(())
    }
}

impl Drop for Bubble {
    fn drop(&mut self) {
        let _ = self.end();
    }
}

/// The program `name` as this process's `PATH` finds it.
fn on_path(name: &str) -> Option<PathBuf> {
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

#[cfg(test)]
#[path = "bubble.tests.rs"]
mod tests;
