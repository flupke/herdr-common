//! What an app gives its fake agent in its [`App`]: the arguments of its hook program, and what
//! the agent reads with a prompt. An app runs the fake agent from a program of its own, as this
//! test binary does: its harness runs as the fake agent.

use std::os::unix::fs::PermissionsExt;
use std::process::{Command as Process, Stdio};

use fake_agent::{App, Command, Event, Events, Role, variables};

/// The app's hook: it logs its arguments.
const HOOK: &str =
    "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' \"$*\" >> \"$(dirname \"$0\")/hooks.log\"\n";

#[test]
#[ignore = "runs as the fake agent of an_app_s_fake_agent_runs_its_hooks_and_reads_its_prompts"]
fn fake_agent_of_the_app() {
    let app = App {
        hook_arguments: &["agent-hook", "--quiet"],
        read_prompt: |prompt| format!("{prompt} (with the turn's files)"),
    };
    fake_agent::run(&app, &["--".to_owned(), "Kickoff.".to_owned()]);
}

#[test]
fn an_app_s_fake_agent_runs_its_hooks_and_reads_its_prompts() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("events.sock");
    let events = Events::listen(&socket).unwrap();
    let hook = directory.path().join("hook");
    std::fs::write(&hook, HOOK).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut agent = Process::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "fake_agent_of_the_app",
            "--nocapture",
        ])
        .env_remove("HERDR_PANE_ID")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env("HC_TERMINAL_ID", "t1")
        .env(variables::EVENTS, &socket)
        .env(variables::HOOK, &hook)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();

    let received = events.wait_for("the kickoff", |reported| {
        matches!(reported.event, Event::PromptReceived { .. })
    });
    events.obey(&Role::agent("t1"), &Command::Exit);

    assert!(agent.wait().unwrap().success());
    assert_eq!(
        received.event,
        Event::PromptReceived {
            text: "Kickoff. (with the turn's files)".into()
        }
    );
    let calls = std::fs::read_to_string(directory.path().join("hooks.log")).unwrap();
    assert!(
        calls.lines().all(|call| call == "agent-hook --quiet") && calls.lines().count() == 3,
        "SessionStart, UserPromptSubmit, SessionEnd: {calls}"
    );
}
