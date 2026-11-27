use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

pub(crate) struct Hooks {
    program: Option<(PathBuf, &'static [&'static str])>,
    cwd: PathBuf,
}

impl Hooks {
    pub(crate) fn new(program: Option<PathBuf>, arguments: &'static [&'static str]) -> Self {
        Self {
            program: program.map(|program| (program, arguments)),
            cwd: std::env::current_dir().unwrap_or_default(),
        }
    }

    pub(crate) fn run(
        &self,
        session: Option<&str>,
        transcript: Option<&Path>,
        prompt_id: Option<&str>,
        event: &str,
        fields: &Value,
    ) -> Printed {
        let Some((program, arguments)) = &self.program else {
            return Printed(String::new());
        };
        let mut payload = json!({
            "session_id": session.unwrap_or_default(),
            "transcript_path": transcript.unwrap_or(Path::new("")),
            "cwd": self.cwd,
            "permission_mode": "default",
            "hook_event_name": event,
        });
        if let Some(prompt_id) = prompt_id {
            payload["prompt_id"] = prompt_id.into();
        }
        for (key, value) in fields.as_object().into_iter().flatten() {
            payload[key] = value.clone();
        }
        let mut child = Command::new(program)
            .args(*arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("cannot run {}: {error}", program.display()));
        let mut input = child.stdin.take().expect("standard input is piped");
        input
            .write_all(payload.to_string().as_bytes())
            .expect("the hook reads its payload");
        drop(input);
        let output = child.wait_with_output().expect("the hook ends");
        assert!(
            output.status.success(),
            "the {event} hook failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        Printed(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

pub(crate) struct Printed(String);

impl Printed {
    fn json(&self) -> Option<Value> {
        serde_json::from_str(self.0.trim()).ok()
    }

    /// The reason of the block decision it printed, if it printed one: a `Stop` hook keeps
    /// Claude Code working on it, a `UserPromptSubmit` hook drops the prompt.
    pub(crate) fn block_reason(&self) -> Option<String> {
        let decision = self.json()?;
        if decision["decision"] != "block" {
            return None;
        }
        Some(decision["reason"].as_str().unwrap_or_default().to_owned())
    }

    /// The session's name a `SessionStart` or `UserPromptSubmit` hook printed, if it printed
    /// one: Claude Code renames the session with it.
    pub(crate) fn session_title(&self) -> Option<String> {
        self.specific("sessionTitle")
    }

    /// The context a `SessionStart` or `UserPromptSubmit` hook printed, if it printed one:
    /// Claude Code adds it to the session's.
    pub(crate) fn additional_context(&self) -> Option<String> {
        self.specific("additionalContext")
    }

    fn specific(&self, field: &str) -> Option<String> {
        Some(
            self.json()?["hookSpecificOutput"][field]
                .as_str()?
                .to_owned(),
        )
    }
}
