//! The holder: the process that `start` leaves running, which holds the sandbox and answers
//! the commands that follow on its socket, each on a thread of its own, until `stop` or until
//! it has been idle too long.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use app_sandbox::{APP_HOME, AppSandbox, Options, Programs};
use eyre::{Context, ContextCompat, Result, ensure};
use serde_json::Value;

use crate::protocol::{self, Info, Reply, Request};

/// How long the holder waits for a connection's request: a guard that fires only on a failure.
const REQUEST_GUARD: Duration = Duration::from_secs(10);

/// What the holder runs.
pub(crate) struct Settings {
    pub(crate) socket: PathBuf,
    pub(crate) log: PathBuf,
    pub(crate) options: Options,
    /// How long the holder waits for a request before it stops the sandbox: a sandbox an agent
    /// forgot keeps drawing on the CPU.
    pub(crate) idle: Duration,
}

/// The running holder, which its threads share.
struct Holder {
    /// The sandbox, until `stop` takes it.
    sandbox: RwLock<Option<AppSandbox>>,
    info: Info,
    /// When the last request came.
    last: Mutex<Instant>,
}

/// Start the sandbox, say so on standard output, which `start` reads, then answer requests
/// until the sandbox stops.
pub(crate) fn serve(settings: &Settings) -> Result<()> {
    let started = start(settings);
    let reply: Reply = match &started {
        Ok((holder, _)) => Ok(serde_json::to_value(&holder.info)?),
        Err(error) => Err(format!("{error:#}")),
    };
    protocol::send(&mut std::io::stdout(), &reply)?;
    let (holder, listener) = started?;
    let holder = Arc::new(holder);
    watch_idleness(&holder, settings);
    for stream in listener.incoming() {
        let mut stream = stream?;
        // A client that never writes would hold every other one back.
        stream.set_read_timeout(Some(REQUEST_GUARD))?;
        let request: Request = match protocol::receive(&stream) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("cannot read a request: {error:#}");
                continue;
            }
        };
        eprintln!("{request:?}");
        *holder.last.lock().unwrap_or_else(PoisonError::into_inner) = Instant::now();
        if let Request::Stop = request {
            let reply = holder.stop().map_err(|error| format!("{error:#}"));
            let _ = std::fs::remove_file(&settings.socket);
            if reply.is_ok() {
                let _ = std::fs::remove_file(&settings.log);
            }
            protocol::send(&mut stream, &reply)?;
            return Ok(());
        }
        let holder = Arc::clone(&holder);
        std::thread::spawn(move || answer(&holder, request, stream));
    }
    Ok(())
}

fn start(settings: &Settings) -> Result<(Holder, UnixListener)> {
    let sandbox = AppSandbox::start(&Programs::pinned()?, &settings.options)?;
    let listener = UnixListener::bind(&settings.socket)
        .wrap_err_with(|| format!("cannot listen on {}", settings.socket.display()))?;
    let info = Info {
        display: sandbox.display().to_owned(),
        directory: sandbox.directory(),
        inside: sandbox.inside().to_owned(),
        app_home: sandbox.directory().join(APP_HOME),
        log: settings.log.clone(),
    };
    let holder = Holder {
        sandbox: RwLock::new(Some(sandbox)),
        info,
        last: Mutex::new(Instant::now()),
    };
    Ok((holder, listener))
}

/// Stop the sandbox once no request came for `settings.idle`, as `stop` would.
fn watch_idleness(holder: &Arc<Holder>, settings: &Settings) {
    let holder = Arc::clone(holder);
    let socket = settings.socket.clone();
    let idle = settings.idle;
    std::thread::spawn(move || {
        loop {
            let last = *holder.last.lock().unwrap_or_else(PoisonError::into_inner);
            let quiet = last.elapsed();
            if quiet >= idle {
                eprintln!("idle for {quiet:?}: stopping");
                let _ = protocol::ask(&socket, &Request::Stop);
                return;
            }
            std::thread::sleep(idle - quiet);
        }
    });
}

fn answer(holder: &Holder, request: Request, mut stream: UnixStream) {
    let reply = holder.handle(request).map_err(|error| format!("{error:#}"));
    if let Err(error) = &reply {
        eprintln!("{error}");
    }
    let _ = protocol::send(&mut stream, &reply);
}

impl Holder {
    fn handle(&self, request: Request) -> Result<Value> {
        let sandbox = self.sandbox.read().unwrap_or_else(PoisonError::into_inner);
        let sandbox = sandbox.as_ref().context("the sandbox stopped")?;
        match request {
            Request::Info => Ok(serde_json::to_value(&self.info)?),
            Request::HerdrApp(arguments) => sandbox.herdr_app(&arguments),
            Request::Hc(arguments) => sandbox.hc(&arguments),
            Request::Shot(file) => {
                std::fs::write(&file, sandbox.shot()?)
                    .wrap_err_with(|| format!("cannot write {}", file.display()))?;
                Ok(Value::String(file.to_string_lossy().into_owned()))
            }
            Request::Mod(directory) => install_mod(sandbox, &directory),
            Request::Run(arguments) => run(sandbox, &arguments),
            Request::Stop => unreachable!("the accepting thread stops the sandbox"),
        }
    }

    /// Stop the sandbox, once the requests in flight are answered, and list the processes it
    /// left.
    fn stop(&self) -> Result<Value> {
        let sandbox = self
            .sandbox
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .context("the sandbox stopped")?;
        let stopped = sandbox.stop()?;
        Ok(serde_json::json!({ "left": stopped.processes }))
    }
}

/// Copy the mod `directory` into the app's `mods`, in place of the copy there, which the app
/// reloads, and return its path inside the sandbox.
fn install_mod(sandbox: &AppSandbox, directory: &Path) -> Result<Value> {
    ensure!(
        directory.join("main.js").is_file(),
        "{} holds no mod: no main.js",
        directory.display()
    );
    let name = directory
        .file_name()
        .context("the mod's directory has a name")?;
    let mods = Path::new(APP_HOME).join("mods");
    let target = sandbox.directory().join(&mods).join(name);
    if target.exists() {
        std::fs::remove_dir_all(&target)?;
    }
    copy_tree(directory, &target)?;
    let inside = sandbox.inside().join(&mods).join(name);
    Ok(Value::String(inside.to_string_lossy().into_owned()))
}

/// Copy the directory `from` to `to`, files and directories, following links: a link out of
/// the mod, or into the user's directories that bubblewrap masks, would dangle in the copy.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .wrap_err_with(|| format!("cannot copy {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Run a program in the sandbox, and return its output, as it printed it.
fn run(sandbox: &AppSandbox, arguments: &[String]) -> Result<Value> {
    let (program, arguments) = arguments.split_first().context("run takes a program")?;
    let output = sandbox
        .command(Path::new(program))
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .wrap_err_with(|| format!("cannot run {program}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    ensure!(
        output.status.success(),
        "{program} failed ({}): {}{}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim(),
        stdout.trim()
    );
    Ok(Value::String(stdout.into_owned()))
}
