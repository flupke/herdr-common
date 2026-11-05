//! What the command and the holder say to each other on the holder's socket: one request per
//! connection, one JSON line each way.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What the command asks the holder. A path in it is one as this process sees it, absolute.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) enum Request {
    /// Where the sandbox is: an [`Info`].
    Info,
    /// `herdr-app <arguments>` against the sandbox's app.
    HerdrApp(Vec<String>),
    /// `hc <arguments>` against the sandbox's core.
    Hc(Vec<String>),
    /// A screenshot of the app's window, written to this PNG file.
    Shot(PathBuf),
    /// Copy this mod directory into the app's `mods`, replacing the copy there.
    Mod(PathBuf),
    /// A program and its arguments, run in the sandbox.
    Run(Vec<String>),
    /// Stop the sandbox, then the holder.
    Stop,
}

/// The holder's answer: the command's output, or why it failed.
pub(crate) type Reply = Result<Value, String>;

/// Where a sandbox is, which `start` and `info` print.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Info {
    /// The X display's name, such as `:0`.
    pub(crate) display: String,
    /// The sandbox's directory, as this process reaches it.
    pub(crate) directory: PathBuf,
    /// The same directory, as the sandbox's processes see it.
    pub(crate) inside: PathBuf,
    /// The app's home (`HERDR_APP_HOME`), as this process reaches it: `shared.json` holds the
    /// mods' shared state.
    pub(crate) app_home: PathBuf,
    /// The holder's log.
    pub(crate) log: PathBuf,
}

/// Write `message` as one JSON line.
pub(crate) fn send(stream: &mut impl Write, message: &impl Serialize) -> Result<()> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    stream.write_all(&line)?;
    stream.flush()?;
    Ok(())
}

/// Read one JSON line.
pub(crate) fn receive<T: for<'de> Deserialize<'de>>(stream: impl std::io::Read) -> Result<T> {
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    if line.is_empty() {
        bail!("the holder closed the connection without an answer");
    }
    serde_json::from_str(&line).wrap_err_with(|| format!("cannot read {line:?}"))
}

/// Ask the holder on `socket`, and return its answer.
pub(crate) fn ask(socket: &Path, request: &Request) -> Result<Value> {
    let mut stream = UnixStream::connect(socket)
        .wrap_err_with(|| format!("no sandbox answers on {}: start it first", socket.display()))?;
    send(&mut stream, request)?;
    receive::<Reply>(stream)?.map_err(|message| eyre::eyre!(message))
}
