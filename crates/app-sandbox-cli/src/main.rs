//! `app-sandbox`: an app sandbox that outlives the command that started it, so that an agent
//! drives it with one shell command after another. `start` leaves a holder process running,
//! which holds the sandbox and answers the commands that follow on a Unix socket, named after
//! the sandbox, in the user's runtime directory.

mod holder;
mod protocol;

use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use app_sandbox::{Isolation, Options};
use clap::{Args, Parser, Subcommand};
use eyre::{Context, ContextCompat, Result, bail, ensure};
use serde_json::Value;

use protocol::{Reply, Request};

/// Keeps a private herdr-app on a headless display running across shell commands: the pinned
/// herdr-app and hc of herdr-common's dev shell, which never show on your display nor take
/// your focus. Each command prints its result: JSON, or the program's text.
#[derive(Parser)]
#[command(name = "app-sandbox")]
struct Cli {
    #[command(subcommand)]
    command: Subcommands,
}

#[derive(Subcommand)]
enum Subcommands {
    /// Start a sandbox, and print where it is (`info`) once its app answers.
    Start {
        /// The sandbox's name, which the other commands take.
        name: String,
        #[command(flatten)]
        launch: Launch,
    },
    /// Print where the sandbox is: its display, its directory as you reach it (`directory`)
    /// and as its processes see it (`inside`), the app's home (`app_home`, which holds
    /// `shared.json`), and the holder's log.
    Info { name: String },
    /// Run `herdr-app <arguments>` against the sandbox's app, such as `do <mod>.<command>`,
    /// `ls`, `status` or `check <mod directory inside>`.
    HerdrApp {
        name: String,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    /// Run `hc <arguments>` against the sandbox's core.
    Hc {
        name: String,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    /// Write a screenshot of the app's window to a PNG file.
    Shot { name: String, file: PathBuf },
    /// Copy a mod's directory into the app's mods, in place of the copy there, which the app
    /// reloads, and print the copy's path inside the sandbox.
    Mod { name: String, directory: PathBuf },
    /// Run a program in the sandbox, on its display, with its environment, in its home, and
    /// print its output as it printed it. It must end on its own: `stop` waits for it.
    Run {
        name: String,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    /// Stop the sandbox, once the commands in flight end, and delete its files and log.
    Stop { name: String },
    /// The holder, which `start` runs.
    #[command(hide = true)]
    Serve {
        #[arg(long)]
        socket: PathBuf,
        #[arg(long)]
        log: PathBuf,
        #[command(flatten)]
        launch: Launch,
    },
}

/// How a sandbox runs.
#[derive(Args)]
struct Launch {
    /// Run without bubblewrap, on a machine where it cannot run.
    #[arg(long)]
    plain: bool,
    /// The display's size, which the app's window fills.
    #[arg(long, default_value = "1400x900", value_parser = size)]
    size: (u16, u16),
    /// Stop the sandbox after this many minutes without a command.
    #[arg(long, default_value_t = 60)]
    idle_minutes: u64,
}

impl Launch {
    fn options(&self) -> Options {
        Options {
            isolation: if self.plain {
                Isolation::Plain
            } else {
                Isolation::Bubblewrap
            },
            display_size: self.size,
            ..Options::default()
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("app-sandbox: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Subcommands) -> Result<()> {
    match command {
        Subcommands::Start { name, launch } => start(&name, &launch),
        Subcommands::Info { name } => ask(&name, &Request::Info),
        Subcommands::HerdrApp { name, arguments } => ask(&name, &Request::HerdrApp(arguments)),
        Subcommands::Hc { name, arguments } => ask(&name, &Request::Hc(arguments)),
        Subcommands::Shot { name, file } => ask(&name, &Request::Shot(std::path::absolute(&file)?)),
        Subcommands::Mod { name, directory } => {
            ask(&name, &Request::Mod(std::fs::canonicalize(&directory)?))
        }
        Subcommands::Run { name, arguments } => ask(&name, &Request::Run(arguments)),
        Subcommands::Stop { name } => ask(&name, &Request::Stop),
        Subcommands::Serve {
            socket,
            log,
            launch,
        } => holder::serve(&holder::Settings {
            socket,
            log,
            options: launch.options(),
            idle: Duration::from_secs(launch.idle_minutes * 60),
        }),
    }
}

/// Start the holder, detached in a process group of its own, and print what it says once the
/// sandbox runs.
fn start(name: &str, launch: &Launch) -> Result<()> {
    let socket = socket(name)?;
    ensure!(
        UnixStream::connect(&socket).is_err(),
        "the sandbox {name} runs already: stop it, or start one with another name"
    );
    // A holder that died left its socket.
    let _ = std::fs::remove_file(&socket);
    let log = socket.with_extension("log");
    let (width, height) = launch.size;
    let mut holder = Command::new(std::env::current_exe()?);
    holder
        .arg("serve")
        .arg("--socket")
        .arg(&socket)
        .arg("--log")
        .arg(&log)
        .arg(format!("--size={width}x{height}"))
        .arg(format!("--idle-minutes={}", launch.idle_minutes))
        .args(launch.plain.then_some("--plain"))
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(&log)?);
    let mut holder = holder.spawn().wrap_err("cannot start the holder")?;
    let said = holder
        .stdout
        .take()
        .context("the holder's output is piped")?;
    let reply = protocol::receive::<Reply>(said).map_err(|error| {
        let mut text = String::new();
        let _ = std::fs::File::open(&log).and_then(|mut log| log.read_to_string(&mut text));
        error.wrap_err(format!("the holder ended:\n{text}"))
    })?;
    match reply {
        Ok(info) => print(&info)?,
        Err(message) => {
            holder.wait()?;
            bail!(message)
        }
    }
    Ok(())
}

fn ask(name: &str, request: &Request) -> Result<()> {
    print(&protocol::ask(&socket(name)?, request)?)
}

/// Print a result: a text as it is, JSON indented. A reader that went away, such as `head`, is
/// no failure.
fn print(value: &Value) -> Result<()> {
    let text = match value {
        Value::String(text) if text.is_empty() || text.ends_with('\n') => text.clone(),
        Value::String(text) => format!("{text}\n"),
        other => format!("{other:#}\n"),
    };
    match std::io::stdout().lock().write_all(text.as_bytes()) {
        Err(error) if error.kind() != std::io::ErrorKind::BrokenPipe => Err(error.into()),
        _ => Ok(()),
    }
}

/// The socket of the sandbox `name`, in a directory only the user reaches: `app-sandbox` in
/// their runtime directory, or `/tmp/app-sandbox-<uid>` without one.
fn socket(name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "a sandbox's name is letters, digits, `-` and `_`: {name:?}"
    );
    let directory = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(runtime) if !runtime.is_empty() => PathBuf::from(runtime).join("app-sandbox"),
        // Not `TMPDIR`: each entry in the dev shell sets another.
        _ => PathBuf::from(format!(
            "/tmp/app-sandbox-{}",
            rustix::process::getuid().as_raw()
        )),
    };
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&directory)?;
    // In the temporary directory, another user could have made it first.
    let metadata = std::fs::metadata(&directory)?;
    ensure!(
        metadata.uid() == rustix::process::getuid().as_raw() && metadata.mode() & 0o777 == 0o700,
        "{} is not yours alone",
        directory.display()
    );
    Ok(directory.join(format!("{name}.sock")))
}

/// A size such as `1400x900`.
fn size(text: &str) -> Result<(u16, u16), String> {
    let parse = || {
        let (width, height) = text.split_once('x')?;
        Some((width.parse().ok()?, height.parse().ok()?))
    };
    parse().ok_or_else(|| format!("a size is <width>x<height>, such as 1400x900: {text:?}"))
}
