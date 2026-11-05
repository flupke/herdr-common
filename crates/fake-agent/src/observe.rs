//! The hook that a real Claude Code runs in the fake agent's place ([`crate::observing_hooks`]):
//! it reports what the agent does on the event socket, as the fake agent reports it, so that
//! one test body waits on the same events whichever agent runs in the pane.

use std::io::Read as _;
use std::path::Path;
use std::sync::mpsc;

use crate::connection::Connection;
use crate::protocol::{Event, Role};
use crate::{App, variables};

/// Reports the event of Claude Code's hook on standard input: a submitted prompt, which a hook
/// sees as it is submitted, as [`Event::PromptReceived`]; a session the agent resumed as
/// [`Event::ResumeReceived`], then [`Event::SessionReported`] and [`Event::SessionStarted`]; a
/// session it started as [`Event::SessionReported`] and [`Event::SessionStarted`]. A
/// subagent's session is not the agent's, nor is a fork's: a fork runs the agent's hooks,
/// since it inherits its configuration.
///
/// # Panics
/// Panics when its payload is no JSON, or the event socket is not there.
pub(crate) fn run(app: &App) {
    let mut payload = String::new();
    std::io::stdin()
        .read_to_string(&mut payload)
        .expect("the hook's payload");
    let event: serde_json::Value = serde_json::from_str(&payload).expect("a JSON payload");
    let field = |name: &str| event[name].as_str().unwrap_or_default().to_owned();
    let events = match field("hook_event_name").as_str() {
        _ if in_a_fork() => Vec::new(),
        "UserPromptSubmit" => vec![Event::PromptReceived {
            text: (app.read_prompt)(&unwrap_pastes(&field("prompt"))),
        }],
        "SessionStart" if event.get("agent_id").is_none() => {
            let (session, start) = (field("session_id"), field("source"));
            let resumed = (start == "resume").then(|| Event::ResumeReceived {
                session: session.clone(),
            });
            resumed
                .into_iter()
                .chain([
                    Event::SessionReported {
                        session: session.clone(),
                        start: start.clone(),
                    },
                    Event::SessionStarted { session, start },
                ])
                .collect()
        }
        _ => Vec::new(),
    };
    let socket = std::env::var_os(variables::EVENTS).expect("the test's event socket");
    let (commands, _) = mpsc::channel();
    let connection = Connection::connect(
        Path::new(&socket),
        Role::agent(crate::agent_name()),
        |c| c,
        commands,
    )
    .expect("the test listens on its event socket");
    for event in &events {
        connection.report(event);
    }
}

/// Whether the hook runs under a fork: a process among its ancestors has `--fork-session` in
/// its command line. It reads Linux's `/proc`: elsewhere, it finds no fork.
fn in_a_fork() -> bool {
    let mut pid = std::process::id().to_string();
    while let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
        let Some(parent) = status
            .lines()
            .find_map(|line| line.strip_prefix("PPid:"))
            .map(str::trim)
        else {
            return false;
        };
        if parent == "0" {
            return false;
        }
        parent.clone_into(&mut pid);
        let command_line = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if command_line
            .split(|byte| *byte == 0)
            .any(|argument| argument == b"--fork-session")
        {
            return true;
        }
    }
    false
}

const PASTE_OPEN: &str = "<pasted_content id=\"";

/// `prompt` with each long paste in it as it was pasted. Claude Code (2.1.309) reports a long
/// paste, such as a long prompt Herdr types, wrapped as `<pasted_content id="N">` and a line
/// break, the text, then `</pasted_content id="N">` and a line break.
fn unwrap_pastes(prompt: &str) -> String {
    let mut text = String::with_capacity(prompt.len());
    let mut rest = prompt;
    while let Some(start) = rest.find(PASTE_OPEN) {
        let Some((id, body)) = rest[start + PASTE_OPEN.len()..].split_once("\">") else {
            break;
        };
        let Some((pasted, after)) = body.split_once(&format!("</pasted_content id=\"{id}\">"))
        else {
            break;
        };
        text.push_str(&rest[..start]);
        text.push_str(pasted.strip_prefix('\n').unwrap_or(pasted));
        rest = after.strip_prefix('\n').unwrap_or(after);
    }
    text.push_str(rest);
    text
}
