//! A fork Claude Code runs in print mode (`-p --resume <parent> --fork-session`), with its
//! stream of JSON lines on its standard output. It records its arguments, environment, settings
//! and prompt in its [`Directory`], copies its parent's transcript, runs its settings'
//! `SessionStart` hooks and prints them as Claude Code does, then works until the test ends its
//! turn or crashes it, or its caller stops it. It reports each step on the event socket, and
//! takes its commands there.

use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::Path;
use std::sync::mpsc;

use serde_json::json;

use crate::connection::Connection;
use crate::launch::{self, flag_value};
use crate::protocol::{Command, Event, Role};
use crate::{App, Directory, transcript, variables};

/// The usage each of its answers reports.
const USAGE: &str = r#"{"input_tokens":3,"cache_creation_input_tokens":20,"cache_read_input_tokens":1000,"output_tokens":7}"#;

/// Runs the fork that Claude Code's `args` start.
///
/// # Panics
/// Panics when its arguments or environment lack what it needs.
pub(crate) fn run(app: &App, args: &[String]) {
    let session = flag_value(args, "--session-id").unwrap_or_else(launch::new_session_id);
    let parent = flag_value(args, "--resume").expect("a fork resumes its parent's session");
    let directory = Directory::from_env();
    let (sender, commands) = mpsc::channel();
    let events = std::env::var_os(variables::EVENTS).expect("the test's event socket");
    let connection = Connection::connect(
        Path::new(&events),
        Role::fork(session.clone()),
        |command| command,
        sender,
    )
    .expect("the test listens on its event socket");
    let mut prompt = String::new();
    // Claude Code in print mode reads its prompt on its standard input.
    io::stdin()
        .read_to_string(&mut prompt)
        .expect("the fork's prompt");
    let configuration = directory.configuration();
    let own = transcript(&configuration, &session);
    copy_parent(&configuration, &parent, &own);
    let settings = flag_value(args, "--settings")
        .map(|path| fs::read_to_string(path).expect("a fork's settings file"))
        .unwrap_or_default();
    let prompt = (app.read_prompt)(&prompt);
    record(&directory, &session, args, &settings, &prompt);
    connection.report(&Event::ForkStarted { parent, prompt });
    for stderr in session_start_hooks(&settings) {
        print(&json!({"type": "system", "subtype": "hook_response",
            "hook_name": "SessionStart:fork", "stderr": stderr}));
    }
    print(&json!({"type": "system", "subtype": "init", "session_id": session}));
    print(
        &serde_json::from_str(&format!(
            r#"{{"type":"assistant","message":{{"id":"m1","usage":{USAGE}}}}}"#
        ))
        .expect("an answer is JSON"),
    );
    work(&connection, &commands);
    // The turn ends in the fork's transcript, as Claude Code's does.
    transcript::append(
        &own,
        &json!({"type": "assistant", "uuid": "fork-answer",
            "message": {"model": "fake-model", "stop_reason": "end_turn"}}),
    );
    print(&json!({"type": "result", "subtype": "success", "is_error": false}));
    connection.report(&Event::TurnEnded { blocked: None });
    connection.report(&Event::Obeyed {
        command: Command::EndTurn,
    });
}

/// Obeys the test's commands until it ends the fork's turn, or nobody follows the fork any
/// more.
fn work(connection: &Connection, commands: &mpsc::Receiver<Command>) {
    for (call, command) in commands.iter().enumerate() {
        match command {
            Command::EndTurn => return,
            Command::Crash => {
                eprintln!("fake fork: crashed as it was told to");
                std::process::exit(1);
            }
            Command::CallTool { name } => {
                let id = format!("tool-{call}");
                print(
                    &json!({"type": "assistant", "message": {"id": format!("m{}", call + 2),
                    "content": [{"type": "tool_use", "id": id, "name": name, "input": {}}],
                    "usage": serde_json::from_str::<serde_json::Value>(USAGE)
                        .expect("the usage is JSON")}}),
                );
                print(&json!({"type": "user", "message": {"content":
                    [{"type": "tool_result", "tool_use_id": id}]}}));
                connection.report(&Event::Obeyed {
                    command: Command::CallTool { name },
                });
            }
            command => panic!("a fork got an agent's command {command:?}"),
        }
    }
}

/// Copies the transcript of the session `parent` to `own`, then the fork's prompt.
fn copy_parent(configuration: &Path, parent: &str, own: &Path) {
    let mut history = fs::read_to_string(transcript(configuration, parent))
        .unwrap_or_else(|error| panic!("the transcript of the fork's parent {parent}: {error}"));
    history.push_str("{\"type\":\"user\",\"uuid\":\"fork-prompt\"}\n");
    if let Some(directory) = own.parent() {
        fs::create_dir_all(directory).expect("the transcripts' directory");
    }
    fs::write(own, history).expect("the fork's transcript");
}

/// Records what the fork of `session` started with, in `directory`.
fn record(directory: &Directory, session: &str, args: &[String], settings: &str, prompt: &str) {
    let names: Vec<_> = std::env::vars_os()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    for (kind, content) in [
        ("args", args.join("\n")),
        ("env", names.join("\n")),
        ("settings", settings.to_owned()),
        ("prompt", prompt.to_owned()),
    ] {
        let path = directory.fork_record(session, kind);
        fs::create_dir_all(path.parent().expect("a record is in a directory"))
            .expect("the forks' directory");
        fs::write(path, content).expect("the fork's record");
    }
}

/// Prints a line of Claude Code's stream.
fn print(line: &serde_json::Value) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// The standard error of each `SessionStart` hook of `settings`, run as Claude Code runs them;
/// none for settings Claude Code skips as a whole, such as those whose `permissions.allow` is
/// not a list.
fn session_start_hooks(settings: &str) -> Vec<String> {
    let Ok(settings) = serde_json::from_str::<serde_json::Value>(settings) else {
        return Vec::new();
    };
    if settings
        .pointer("/permissions/allow")
        .is_some_and(|allow| !allow.is_array())
    {
        return Vec::new();
    }
    settings
        .pointer("/hooks/SessionStart")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|matcher| matcher.get("hooks")?.as_array())
        .flatten()
        .filter_map(|hook| hook.get("command")?.as_str())
        .map(|command| {
            let output = std::process::Command::new("sh")
                .args(["-c", command])
                .output()
                .expect("sh runs a hook");
            String::from_utf8_lossy(&output.stderr).into_owned()
        })
        .collect()
}

#[cfg(test)]
#[path = "fork.tests.rs"]
mod tests;
