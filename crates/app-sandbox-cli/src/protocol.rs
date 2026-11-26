use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) enum Request {
    Info,
    HerdrApp(Vec<String>),
    Hc(Vec<String>),
    Shot(PathBuf),
    Mod(PathBuf),
    Run(Vec<String>),
    Stop,
}

pub(crate) type Reply = Result<Value, String>;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Info {
    pub(crate) display: String,
    pub(crate) directory: PathBuf,
    pub(crate) inside: PathBuf,
    pub(crate) app_home: PathBuf,
    pub(crate) log: PathBuf,
}

pub(crate) fn send(stream: &mut impl Write, message: &impl Serialize) -> Result<()> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    stream.write_all(&line)?;
    stream.flush()?;
    Ok(())
}

pub(crate) fn receive<T: for<'de> Deserialize<'de>>(stream: impl std::io::Read) -> Result<T> {
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    if line.is_empty() {
        bail!("the holder closed the connection without an answer");
    }
    serde_json::from_str(&line).wrap_err_with(|| format!("cannot read {line:?}"))
}

pub(crate) fn ask(socket: &Path, request: &Request) -> Result<Value> {
    let mut stream = UnixStream::connect(socket)
        .wrap_err_with(|| format!("no sandbox answers on {}: start it first", socket.display()))?;
    send(&mut stream, request)?;
    receive::<Reply>(stream)?.map_err(|message| eyre::eyre!(message))
}
