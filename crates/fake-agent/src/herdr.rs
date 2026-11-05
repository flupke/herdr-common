//! What the fake agent tells Herdr 0.9 with its CLI, and hears from Herdr's socket: the status
//! Herdr gives its pane's agent, and Herdr's detection of it.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use serde_json::{Value, json};

/// Claude's Herdr integration reports a session under `herdr:<agent>`: Herdr resumes only a
/// session reported so.
const INTEGRATION_SOURCE: &str = "herdr:";

/// The Herdr 0.9 of the fake agent's pane, through its CLI.
pub(crate) struct Herdr {
    cli: PathBuf,
    pane: String,
}

impl Herdr {
    pub(crate) fn new(cli: PathBuf, pane: String) -> Self {
        Self { cli, pane }
    }

    /// Reports `agent`'s session `session` to Herdr as Claude's Herdr integration does with
    /// `pane.report_agent_session`: with the session's transcript, how it started, and the time
    /// in nanoseconds as its sequence number, so that each report is newer than the last.
    pub(crate) fn report_session(
        &self,
        agent: &str,
        session: &str,
        transcript: &Path,
        start: &str,
    ) {
        let sequence = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is past 1970")
            .as_nanos()
            .to_string();
        self.run(&[
            "pane",
            "report-agent-session",
            &self.pane,
            "--source",
            &format!("{INTEGRATION_SOURCE}{agent}"),
            "--agent",
            agent,
            "--seq",
            &sequence,
            "--agent-session-id",
            session,
            "--agent-session-path",
            &transcript.to_string_lossy(),
            "--session-start-source",
            start,
        ]);
    }

    /// Reports `agent`'s state `state` to Herdr under [`crate::STATE_SOURCE`].
    pub(crate) fn report_state(&self, agent: &str, state: &str) {
        self.run(&[
            "pane",
            "report-agent",
            &self.pane,
            "--source",
            crate::STATE_SOURCE,
            "--agent",
            agent,
            "--state",
            state,
        ]);
    }

    /// Runs the Herdr CLI with `args`, which must succeed.
    fn run(&self, args: &[&str]) {
        let output = Command::new(&self.cli)
            .args(args)
            .stdout(Stdio::null())
            .output()
            .unwrap_or_else(|error| panic!("cannot run {}: {error}", self.cli.display()));
        assert!(
            output.status.success(),
            "herdr {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// What Herdr says of the fake agent's pane.
#[derive(Debug)]
pub(crate) enum News {
    /// Herdr gives its agent this status: `working`, `idle`, `blocked`, `done`, `unknown`.
    Status(String),
    /// Herdr detected an agent in the pane, or released the one it held (`released`).
    Detected { released: bool },
}

/// Subscribes to what Herdr, listening at `socket`, says of `pane`, and sends it on `news`,
/// wrapped by `wrap`, from another thread: the agent's status now, then each change. The
/// subscription holds once this returns, so the agent misses nothing it shows after.
///
/// # Panics
/// Panics when Herdr refuses the subscription.
pub(crate) fn follow<T: Send + 'static>(
    socket: &Path,
    pane: &str,
    wrap: fn(News) -> T,
    news: mpsc::Sender<T>,
) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(socket)?;
    let subscriptions = json!([
        {"type": "pane.agent_status_changed", "pane_id": pane},
        {"type": "pane.agent_detected"},
    ]);
    let request = json!({"id": "fake-agent", "method": "events.subscribe",
        "params": {"subscriptions": subscriptions}});
    writeln!(stream, "{request}")?;
    let mut reader = BufReader::new(stream);
    let answer = read_json(&mut reader)?;
    assert!(answer.get("error").is_none(), "Herdr refused: {answer}");
    if let Some(status) = status_now(socket, pane)? {
        let _ = news.send(wrap(News::Status(status)));
    }
    let pane = pane.to_owned();
    thread::spawn(move || {
        while let Ok(envelope) = read_json(&mut reader) {
            if let Some(said) = news_of(&envelope, &pane)
                && news.send(wrap(said)).is_err()
            {
                return;
            }
        }
    });
    Ok(())
}

/// The status Herdr gives the agent of `pane` now; none when it knows no agent there.
fn status_now(socket: &Path, pane: &str) -> std::io::Result<Option<String>> {
    let mut stream = UnixStream::connect(socket)?;
    let request = json!({"id": "fake-agent", "method": "agent.get", "params": {"target": pane}});
    writeln!(stream, "{request}")?;
    let answer = read_json(&mut BufReader::new(stream))?;
    Ok(answer["result"]["agent"]["agent_status"]
        .as_str()
        .map(str::to_owned))
}

/// What the event `envelope` of Herdr's stream says of `pane`, if anything.
fn news_of(envelope: &Value, pane: &str) -> Option<News> {
    let data = &envelope["data"];
    // Herdr names an event with a dot, as its subscription, or an underscore, as its schema.
    match envelope["event"].as_str()?.replace('.', "_").as_str() {
        "pane_agent_status_changed" => Some(News::Status(data["agent_status"].as_str()?.into())),
        "pane_agent_detected" if data["pane_id"] == pane => Some(News::Detected {
            released: data["released"].as_bool().unwrap_or(false),
        }),
        _ => None,
    }
}

fn read_json(reader: &mut impl BufRead) -> std::io::Result<Value> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(std::io::ErrorKind::UnexpectedEof.into());
    }
    serde_json::from_str(&line).map_err(std::io::Error::other)
}
