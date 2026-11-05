//! The transcript of the fake agent's session, a file of JSON lines, as Claude Code keeps one
//! per session.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Appends `entry` to the transcript at `path`.
///
/// # Panics
/// Panics when the transcript cannot be written.
pub(crate) fn append(path: &Path, entry: &Value) {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory).expect("the transcripts' directory");
    }
    let mut file = std::fs::File::options()
        .create(true)
        .append(true)
        .open(path)
        .expect("the transcript");
    writeln!(file, "{entry}").expect("the transcript is written");
}

/// The transcript of the agent's session under Claude Code's configuration directory, and the
/// count of the prompts it took.
pub(crate) struct Transcript {
    configuration: PathBuf,
    turns: usize,
}

impl Transcript {
    pub(crate) fn new(configuration: PathBuf) -> Self {
        Self {
            configuration,
            turns: 0,
        }
    }

    /// The transcript of `session`.
    pub(crate) fn path(&self, session: &str) -> PathBuf {
        crate::transcript(&self.configuration, session)
    }

    /// The session `session` starts: its transcript exists from now on, as a fork of it reads
    /// it.
    ///
    /// # Panics
    /// Panics when the transcript cannot be written.
    pub(crate) fn start(&self, session: &str) {
        let path = self.path(session);
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory).expect("the transcripts' directory");
        }
        std::fs::File::options()
            .create(true)
            .append(true)
            .open(path)
            .expect("the transcript");
    }

    /// The prompt `text`, which the user typed: a line of theirs, or, while a turn runs, a
    /// `queued_command` attachment that the turn reads, as Claude Code writes them (2.1.318).
    pub(crate) fn prompt(&mut self, session: &str, text: &str, queued: bool) {
        self.turns += 1;
        // Claude Code marks every prompt typed in the pane as the user's.
        let human = json!({"kind": "human"});
        let uuid = format!("prompt-{}", self.turns);
        let entry = if queued {
            json!({
                "type": "attachment",
                "uuid": uuid,
                "attachment": {
                    "type": "queued_command",
                    "prompt": text,
                    "commandMode": "prompt",
                    "origin": human,
                },
            })
        } else {
            json!({
                "type": "user",
                "uuid": uuid,
                "message": {"role": "user", "content": text},
                "origin": human,
            })
        };
        append(&self.path(session), &entry);
    }

    /// The agent's answer, which ends its turn.
    pub(crate) fn answer(&self, session: &str) {
        append(
            &self.path(session),
            &json!({
                "type": "assistant",
                "uuid": format!("answer-{}", self.turns),
                "message": {
                    "role": "assistant",
                    "model": "fake-model",
                    "stop_reason": "end_turn",
                    "content": [{"type": "text", "text": "Done."}],
                },
            }),
        );
    }

    /// The session `old` goes on in `new`: Claude Code 2.1.318 writes it in the old session's
    /// transcript.
    pub(crate) fn continued_in(&self, old: &str, new: &str) {
        append(
            &self.path(old),
            &json!({"type": "continued-in", "sessionId": old, "continuedInSessionId": new}),
        );
    }
}
