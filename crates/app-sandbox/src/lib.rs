//! A private herdr-app on a headless display, for visual tests that must never fake input on
//! the user's display nor take their focus: the pinned herdr-app and herdr-core (`hc`), with
//! their own `HERDR_APP_HOME`, `HC_SESSION` and XDG directories, drawn by Mesa's software
//! Vulkan on a headless Weston, whose Xwayland is the app's only display. Its processes get
//! none of the variables that name the user's displays, window manager, Herdr or herdr-app.
//!
//! [`Isolation`] says how far it goes: in bubblewrap by default, where the user's directories
//! and runtime directory show empty and the kernel cleans up after it, or plain, where
//! bubblewrap cannot run.

mod bubble;
mod environment;
mod forward;
mod plain;
mod process;
mod restart;

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use eyre::{Context, ContextCompat, Result, ensure};
use serde_json::Value;

use bubble::Bubble;
use environment::{Inputs, WAYLAND_SOCKET};
use forward::Forward;
use plain::Plain;
pub use restart::Restart;

/// The app's home (`HERDR_APP_HOME`) in the sandbox's directory.
pub const APP_HOME: &str = "config/herdr-app";

/// What Weston writes once its Xwayland listens, followed by the display's name (`:0`).
const DISPLAY_READY: &str = "xserver listening on display ";

/// How long the sandbox waits for its display, its app or the end of what a restart killed
/// before it fails: a guard that fires only on a failure.
const GUARD: Duration = Duration::from_secs(60);

/// How often the sandbox asks the app whether its window shows the core's layout, as it starts
/// and restarts: it says nothing on its own.
const LAYOUT_POLL: Duration = Duration::from_millis(100);

/// The arguments of `herdr-app do` that ask the window whether its layout is loaded: the rename
/// of a space no layout holds, which changes nothing. The window refuses it as `no space`
/// once it loaded its layout from the core, and as unavailable before ("This machine's layout
/// is unavailable", "doc.get: lost the core").
const LAYOUT_PROBE: [&str; 3] = [
    "do",
    "space.rename",
    r#"{"id": "sp00000000000000000000000000000000", "name": "layout probe"}"#,
];

/// The programs a sandbox runs. [`Programs::pinned`] gives the dev shell's pinned releases,
/// which tests run; a caller may run another herdr-app, such as its own build. Each path is
/// absolute, and in bubblewrap outside `/tmp` and the user's masked directories, which the
/// sandbox's processes see empty.
#[derive(Clone, Debug)]
pub struct Programs {
    /// herdr-app (`TEST_HERDR_APP_BIN_PATH`).
    pub app: PathBuf,
    /// herdr-core, `hc` (`TEST_HC_BIN_PATH`): the app and its terminals find it first on
    /// their `PATH`.
    pub hc: PathBuf,
    /// Weston (`TEST_WESTON_BIN_PATH`), which starts its Xwayland.
    pub weston: PathBuf,
    /// Mesa's software Vulkan driver (`TEST_HERDR_APP_VULKAN_DRIVER`): the app draws on the
    /// CPU, the same on every machine, and never opens the user's GPU.
    pub vulkan_driver: PathBuf,
}

impl Programs {
    /// The pinned programs the dev shell's variables name.
    pub fn pinned() -> Result<Self> {
        let variable = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .with_context(|| format!("{name} is not set: run in herdr-common's dev shell"))
        };
        Ok(Self {
            app: variable("TEST_HERDR_APP_BIN_PATH")?,
            hc: variable("TEST_HC_BIN_PATH")?,
            weston: variable("TEST_WESTON_BIN_PATH")?,
            vulkan_driver: variable("TEST_HERDR_APP_VULKAN_DRIVER")?,
        })
    }
}

/// How a sandbox keeps its processes from the user's.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Isolation {
    /// A bubblewrap environment, with PID, network and mount namespaces of its own: the user's
    /// herdr-app, hc and Herdr directories and their runtime directory, which holds the sockets
    /// of their Wayland display and window manager, show empty inside, and nothing reaches the
    /// network: its loopback is its own, which the host reaches only through
    /// [`AppSandbox::forward`]. Its files are in a tmpfs, `/tmp` inside, which the host reaches
    /// through [`AppSandbox::directory`]: a script or argument the sandbox reads names a path
    /// inside, under [`AppSandbox::inside`]. The host's own `/tmp`, where `TMPDIR` and so
    /// `tempfile` put files by default, does not show inside; the rest of the host's files show
    /// at their own paths, so a file or socket the test and the sandbox share by its path goes
    /// there, such as under `target/`. Once the sandbox stops, or the process that holds it
    /// dies, `SIGKILL` included, the kernel kills its processes and frees its files.
    #[default]
    Bubblewrap,
    /// No namespace, for a machine where bubblewrap cannot run, such as one that forbids
    /// unprivileged user namespaces. Its files are in a temporary directory, and its processes
    /// carry a mark that [`AppSandbox::stop`] finds them by, with every process under them: one
    /// that both drops the mark and leaves its parents, such as `setsid env -i <command>`,
    /// outlives the sandbox. A process can still reach the user's sockets by their path, and a
    /// `SIGKILL` of the process that holds the sandbox leaves its processes and files behind.
    Plain,
}

/// What a sandbox runs, and how.
#[derive(Clone, Debug)]
pub struct Options {
    pub isolation: Isolation,
    /// The display's size, in pixels, which the app's window fills.
    pub display_size: (u16, u16),
    /// Variables added to the environment of every process of the sandbox, the app's
    /// terminals included, over the inherited ones and the sandbox's defaults, such as
    /// `BROWSER=true`, `TERM` and `SHELL`: a `BROWSER` of the caller's runs instead. A path in
    /// it is one as the sandbox's processes see it, under [`AppSandbox::inside`]. What the
    /// isolation stands on stays the sandbox's own: a display or window manager (`DISPLAY`,
    /// `WAYLAND_DISPLAY`, `SWAYSOCK`) is dropped, and `HOME`, `TMPDIR`, the XDG directories,
    /// `HERDR_APP_HOME`, `HC_SESSION` and the Vulkan driver are the sandbox's; a `PATH` comes
    /// after the sandbox's `bin`, so one that finds a fake agent first still finds the
    /// pinned `hc` before it.
    pub environment: Vec<(OsString, OsString)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            isolation: Isolation::default(),
            display_size: (1400, 900),
            environment: Vec::new(),
        }
    }
}

/// What a stop left behind.
#[derive(Debug, Default)]
pub struct Stopped {
    /// The processes of the sandbox still running, by host PID.
    pub processes: Vec<u32>,
}

/// A running sandbox: its display, and the app on it.
pub struct AppSandbox {
    jail: Jail,
    programs: Programs,
    environment: Vec<(OsString, OsString)>,
    /// The display's name, such as `:0`.
    display: String,
    /// The processes that started Weston and the app: in bubblewrap, the nsenter that started
    /// each, which ends once it ended.
    weston: Option<Child>,
    app: Option<Child>,
    /// The number of the next screenshot's file.
    shots: AtomicU32,
    /// The ports of the sandbox's loopback the host reaches ([`AppSandbox::forward`]).
    forwards: Mutex<Vec<Forward>>,
}

impl AppSandbox {
    /// Start the display, then the app, which starts its core, and return once the app's window
    /// shows the core's layout: `herdr-app do` changes it.
    pub fn start(programs: &Programs, options: &Options) -> Result<Self> {
        for path in [
            &programs.app,
            &programs.hc,
            &programs.weston,
            &programs.vulkan_driver,
        ] {
            ensure!(
                path.is_absolute(),
                "the sandbox takes absolute paths: {}",
                path.display()
            );
        }
        let jail = match options.isolation {
            Isolation::Bubblewrap => Jail::Bubble(Bubble::start(&environment::user_directories(
                std::env::vars_os(),
            ))?),
            Isolation::Plain => Jail::Plain(Plain::start()?),
        };
        let mut sandbox = Self {
            environment: environment::environment(
                &Inputs {
                    root: jail.inside(),
                    vulkan_driver: &programs.vulkan_driver,
                    added: &options.environment,
                },
                std::env::vars_os(),
            ),
            jail,
            programs: programs.clone(),
            display: String::new(),
            weston: None,
            app: None,
            shots: AtomicU32::new(0),
            forwards: Mutex::new(Vec::new()),
        };
        sandbox.prepare(options.display_size)?;
        sandbox.start_display(options.display_size)?;
        sandbox.start_app()?;
        Ok(sandbox)
    }

    /// Create the sandbox's directories and the app's home.
    fn prepare(&self, (width, height): (u16, u16)) -> Result<()> {
        let root = self.directory();
        for name in ["home", "config", "state", "data", "cache", "bin"] {
            std::fs::create_dir(root.join(name))?;
        }
        create_directory(&root.join("runtime"), 0o700)?;
        if let Jail::Bubble(_) = self.jail {
            // Xwayland's sockets, in the tmpfs rather than the host's /tmp.
            create_directory(&root.join(".X11-unix"), 0o1777)?;
        }
        // The app and its terminals find the pinned hc and herdr-app first on their PATH,
        // never the user's.
        std::os::unix::fs::symlink(&self.programs.hc, root.join("bin/hc"))?;
        std::os::unix::fs::symlink(&self.programs.app, root.join("bin/herdr-app"))?;
        write_executable(&root.join("bin/xdg-open"), &xdg_open(self.inside()))?;
        // The app opens as a person's who skipped its first-run welcome and already ran this
        // version: no welcome, no "Updated to" pill over its status bar.
        let app_home = root.join(APP_HOME);
        std::fs::create_dir(&app_home)?;
        std::fs::write(
            app_home.join("shared.json"),
            r#"{"frame.welcomed": "skipped"}"#,
        )?;
        std::fs::write(
            app_home.join("update-notes.json"),
            serde_json::json!({"last_run": app_version(&self.programs.app)?, "presented": []})
                .to_string(),
        )?;
        // The window opens at the display's size, which the kiosk shell would give it a moment
        // later: its first frame, and so a screenshot taken as soon as the app answers, already
        // fills the display.
        std::fs::write(
            app_home.join("window.json"),
            serde_json::json!({"state": "windowed", "bounds": [0, 0, width, height]}).to_string(),
        )?;
        Ok(())
    }

    fn start_display(&mut self, (width, height): (u16, u16)) -> Result<()> {
        let log = self.directory().join("weston.log");
        let mut weston = self
            .command(&self.programs.weston)
            .args([
                "--backend=headless".to_owned(),
                "--xwayland".to_owned(),
                // The kiosk shell makes the app's window fill the display.
                "--shell=kiosk".to_owned(),
                format!("--socket={WAYLAND_SOCKET}"),
                format!("--width={width}"),
                format!("--height={height}"),
                // The display never idles, as a person at work keeps theirs awake.
                "--idle-time=0".to_owned(),
            ])
            .stdin(Stdio::null())
            .stdout(append(&log)?)
            .stderr(Stdio::piped())
            .spawn()
            .wrap_err("cannot start Weston")?;
        let stderr = weston.stderr.take().context("standard error is piped")?;
        self.weston = Some(weston);
        let ready = copy_until_ready(stderr, &log, DISPLAY_READY.to_owned())?;
        let line = ready.recv_timeout(GUARD).map_err(|_| {
            let log = std::fs::read_to_string(&log).unwrap_or_default();
            eyre::eyre!("Weston's Xwayland did not start:\n{log}")
        })?;
        let (_, display) = line
            .split_once(DISPLAY_READY)
            .context("the ready line names the display")?;
        display.trim().clone_into(&mut self.display);
        self.environment.retain(|(name, _)| name != "DISPLAY");
        self.environment
            .push(("DISPLAY".into(), self.display.clone().into()));
        self.environment.sort();
        Ok(())
    }

    fn start_app(&mut self) -> Result<()> {
        let path = self.directory().join("app.log");
        let log = append(&path)?;
        let mut app = self
            .command(&self.programs.app)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .wrap_err("cannot start herdr-app")?;
        let deadline = Instant::now() + GUARD;
        let answered = loop {
            if self.shows_layout() {
                break Ok(());
            }
            if let Some(status) = app.try_wait()? {
                break Err(eyre::eyre!("the sandbox's herdr-app ended ({status})"));
            }
            if Instant::now() >= deadline {
                break Err(eyre::eyre!(
                    "the sandbox's herdr-app did not show the core's layout within {GUARD:?}"
                ));
            }
            std::thread::sleep(LAYOUT_POLL);
        };
        self.app = Some(app);
        answered.map_err(|error| {
            let log = std::fs::read_to_string(&path).unwrap_or_default();
            error.wrap_err(format!("app.log:\n{log}"))
        })
    }

    /// Whether the app's window answers, its layout loaded from the core ([`LAYOUT_PROBE`]).
    fn shows_layout(&self) -> bool {
        match self.run(&self.programs.app, &LAYOUT_PROBE) {
            Ok(_) => true,
            Err(error) => error.to_string().contains("no space"),
        }
    }

    /// Run `herdr-app <arguments>` against the sandbox's app: `do <command> [json]`, `ls`,
    /// `status`, `commands`. Returns its JSON output, or its text when that is no JSON. No
    /// arguments is refused: it would open a second window.
    pub fn herdr_app<S: AsRef<str>>(&self, arguments: &[S]) -> Result<Value> {
        ensure!(
            !arguments.is_empty(),
            "herdr-app takes a command (do, ls, status, commands, shot): alone, it opens a window"
        );
        self.run(&self.programs.app, arguments)
    }

    /// Run `hc <arguments>` against the sandbox's core, such as `ls`, `read t1` or `send t1
    /// 'ls'`. Returns its JSON output, or its text when that is no JSON.
    pub fn hc<S: AsRef<str>>(&self, arguments: &[S]) -> Result<Value> {
        self.run(&self.programs.hc, arguments)
    }

    /// A screenshot of the app's window, as PNG: `herdr-app shot`, which renders the window's
    /// last frame.
    pub fn shot(&self) -> Result<Vec<u8>> {
        let name = format!("shot-{}.png", self.shots.fetch_add(1, Ordering::Relaxed));
        let inside = self.inside().join(&name);
        self.herdr_app(&["shot", &inside.to_string_lossy()])?;
        let outside = self.directory().join(&name);
        let png = std::fs::read(&outside)
            .wrap_err_with(|| format!("cannot read the screenshot {}", outside.display()))?;
        std::fs::remove_file(&outside)?;
        Ok(png)
    }

    fn run<S: AsRef<str>>(&self, program: &Path, arguments: &[S]) -> Result<Value> {
        let arguments: Vec<&str> = arguments.iter().map(AsRef::as_ref).collect();
        let name = program.file_name().unwrap_or_default().to_string_lossy();
        let output = self
            .command(program)
            .args(&arguments)
            .stdin(Stdio::null())
            .output()
            .wrap_err_with(|| format!("cannot run {name}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        ensure!(
            output.status.success(),
            "{name} {} failed ({}): {}{}",
            arguments.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim(),
            stdout.trim()
        );
        Ok(serde_json::from_str(&stdout).unwrap_or_else(|_| Value::String(stdout.into_owned())))
    }

    /// A command that runs `program` in the sandbox, with the sandbox's environment alone, in
    /// its `HOME`: it reaches the sandbox's display, app and core. Paths in its arguments are
    /// paths as the sandbox's processes see them.
    pub fn command(&self, program: &Path) -> Command {
        let home = self.inside().join("home");
        match &self.jail {
            Jail::Bubble(bubble) => bubble.command(program, &home, &self.environment),
            Jail::Plain(plain) => plain.command(program, &home, &self.environment),
        }
    }

    /// The programs the sandbox runs.
    pub fn programs(&self) -> &Programs {
        &self.programs
    }

    /// The environment of the sandbox's processes: its `DISPLAY`, `HERDR_APP_HOME`,
    /// `HC_SESSION` and XDG directories among them.
    pub fn environment(&self) -> &[(OsString, OsString)] {
        &self.environment
    }

    /// The value of the variable `name` in the environment of the sandbox's processes.
    pub fn variable(&self, name: &str) -> Option<&OsStr> {
        self.environment
            .iter()
            .find(|(variable, _)| variable == name)
            .map(|(_, value)| value.as_os_str())
    }

    /// The X display's name, such as `:0`, which the sandbox's processes reach.
    pub fn display(&self) -> &str {
        &self.display
    }

    /// The X display's socket, as this process reaches it: an X client outside the sandbox,
    /// such as one that fakes input with XTEST, connects to it by this path.
    pub fn display_socket(&self) -> Result<PathBuf> {
        let number = self
            .display
            .strip_prefix(':')
            .context("the display's name starts with a colon")?;
        Ok(self.outside(&Path::new("/tmp/.X11-unix").join(format!("X{number}"))))
    }

    /// The sandbox's directory, as its processes see it: `/tmp` in bubblewrap.
    pub fn inside(&self) -> &Path {
        self.jail.inside()
    }

    /// The sandbox's directory, as this process reaches it while the sandbox runs: the app's
    /// home ([`APP_HOME`]), the core's state, `opened.log` and the logs (`app.log`,
    /// `weston.log`).
    pub fn directory(&self) -> PathBuf {
        self.outside(self.inside())
    }

    /// The path that shows `inside`, a path as the sandbox's processes see it, to this process.
    fn outside(&self, inside: &Path) -> PathBuf {
        match &self.jail {
            Jail::Bubble(bubble) => bubble.outside(inside),
            Jail::Plain(_) => inside.to_owned(),
        }
    }

    /// Let this process, and any other of the host's, reach the sandbox's TCP port `port` on
    /// its loopback, `127.0.0.1:<port>` inside, at the same address on the host's loopback,
    /// until the sandbox stops: an address the sandbox's processes print, such as a server's,
    /// works on both sides. A port forwarded already stays as it is. Fails when the host's port
    /// is taken: a server inside that picks any free port may pick one the host uses, and a
    /// test that picks its port on the host avoids that. A connection the sandbox's port
    /// refuses closes at once.
    ///
    /// In bubblewrap each connection runs socat, from this process's `PATH`, in the sandbox,
    /// where it shows among [`processes`](Self::processes) while the connection lasts. In plain
    /// isolation the sandbox shares the host's network, and this does nothing.
    pub fn forward(&self, port: u16) -> Result<()> {
        ensure!(
            port != 0,
            "a forward takes the port the sandbox listens on, not 0"
        );
        let Jail::Bubble(bubble) = &self.jail else {
            return Ok(());
        };
        let mut forwards = self.forwards.lock().unwrap_or_else(PoisonError::into_inner);
        if forwards.iter().any(|forward| forward.port() == port) {
            return Ok(());
        }
        let socat = bubble::on_path("socat").context("no socat on PATH: run in the dev shell")?;
        let entry = bubble.entry().clone();
        let environment = self.environment.clone();
        let home = self.inside().join("home");
        let forward = Forward::start(port, move || {
            let mut relay = entry.command(&socat, &home, &environment);
            // Once one side closes, socat waits this long for the other side's end, half a
            // second by default: a request whose client half-closes keeps its answer.
            relay.args(["-t", "3600", "STDIO", &format!("TCP4:127.0.0.1:{port}")]);
            relay
        })?;
        forwards.push(forward);
        Ok(())
    }

    /// The processes of the sandbox, by host PID.
    pub fn processes(&self) -> Vec<u32> {
        match &self.jail {
            Jail::Bubble(bubble) => bubble.processes(),
            Jail::Plain(plain) => plain.processes(),
        }
    }

    /// Stop the app, its core and the display, delete the sandbox's files, and say what is
    /// left.
    pub fn stop(mut self) -> Result<Stopped> {
        self.end()?;
        Ok(Stopped {
            processes: self.processes(),
        })
    }

    fn end(&mut self) -> Result<()> {
        // No forward enters the sandbox once it ended, when another process may hold the PID
        // nsenter targets.
        self.forwards
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        if let Jail::Plain(_) = self.jail
            && let Some(mut weston) = self.weston.take()
        {
            // Weston removes Xwayland's socket and lock from the host's /tmp as it stops.
            process::signal(rustix::process::Signal::TERM, &[weston.id()]);
            weston.wait().wrap_err("cannot wait for Weston")?;
        }
        match &mut self.jail {
            Jail::Bubble(bubble) => bubble.end()?,
            Jail::Plain(plain) => plain.end()?,
        }
        for mut started in self.weston.take().into_iter().chain(self.app.take()) {
            started.wait()?;
        }
        Ok(())
    }
}

impl Drop for AppSandbox {
    fn drop(&mut self) {
        let _ = self.end();
    }
}

/// Where the sandbox's processes run.
enum Jail {
    Bubble(Bubble),
    Plain(Plain),
}

impl Jail {
    fn inside(&self) -> &Path {
        match self {
            Self::Bubble(_) => Bubble::inside(),
            Self::Plain(plain) => plain.inside(),
        }
    }
}

/// `path` opened to append to, created when missing: a log that keeps each run's lines.
fn append(path: &Path) -> Result<std::fs::File> {
    std::fs::File::options()
        .create(true)
        .append(true)
        .open(path)
        .wrap_err_with(|| format!("cannot open {}", path.display()))
}

/// Copy what a process writes to its standard error to the end of `log`, on another thread, and
/// send on the returned channel the first line it wrote that contains `ready_line`.
fn copy_until_ready(
    stderr: ChildStderr,
    log: &Path,
    ready_line: String,
) -> Result<mpsc::Receiver<String>> {
    let mut log = append(log)?;
    let (ready, said) = mpsc::channel();
    std::thread::spawn(move || {
        let mut ready = Some(ready);
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else {
                return;
            };
            let _ = writeln!(log, "{line}");
            if line.contains(&ready_line)
                && let Some(ready) = ready.take()
            {
                let _ = ready.send(line);
            }
        }
    });
    Ok(said)
}

/// The version of the herdr-app `app`, as `herdr-app --version` prints it: `0.1.13`.
fn app_version(app: &Path) -> Result<String> {
    let output = Command::new(app)
        .arg("--version")
        .env_clear()
        .output()
        .wrap_err("cannot run herdr-app --version")?;
    let printed = String::from_utf8_lossy(&output.stdout);
    printed
        .trim()
        .strip_prefix("herdr-app ")
        .map(str::to_owned)
        .with_context(|| format!("herdr-app --version printed {printed:?}"))
}

/// Create the directory `path` with the permissions `mode`, which the umask leaves alone.
fn create_directory(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

/// The sandbox's `xdg-open`, for the sandbox at `inside`, which the app runs on Ctrl+click on a
/// link: as `xdg-open`'s generic mode does on a desktop, it runs the first desktop entry of
/// `data/applications` (`XDG_DATA_HOME`) whose `MimeType` claims `x-scheme-handler/<scheme>`
/// for the link's scheme, its `Exec` split on spaces and `%u` given the link, such as an app's
/// handler of its own links. Any other link goes to `opened.log` instead of the user's browser.
fn xdg_open(inside: &Path) -> String {
    let applications = shell_quote(&inside.join("data/applications").to_string_lossy());
    let opened = shell_quote(&inside.join("opened.log").to_string_lossy());
    format!(
        r#"#!/bin/sh
link="$1"
case $link in
*:*)
  scheme="${{link%%:*}}"
  for entry in {applications}/*.desktop; do
    sed -n 's/^MimeType=//p' "$entry" 2>/dev/null | tr ';' '\n' |
      grep -Fqx "x-scheme-handler/$scheme" || continue
    command=$(sed -n 's/^Exec=//p' "$entry" | head -n 1)
    [ -n "$command" ] || continue
    set -f
    set -- $command
    set +f
    for argument; do
      shift
      [ "$argument" = %u ] && argument="$link"
      set -- "$@" "$argument"
    done
    exec "$@"
  done
  ;;
esac
printf '%s\n' "$*" >>{opened}
"#
    )
}

fn write_executable(path: &Path, text: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// `text` quoted for a POSIX shell.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}
