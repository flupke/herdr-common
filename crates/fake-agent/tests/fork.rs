//! The fork the fake agent plays when Claude Code runs in print mode.

use std::fs;
use std::io::Write;
use std::process::{Command as Process, Stdio};

use fake_agent::{Command, Directory, Event, Events, Role, variables};

/// A fork started as Claude Code's `ARGUMENTS` say, on the prompt "Take the turn of keep.",
/// from the session `parent`, with its events, its directory and its Claude Code configuration.
struct StartedFork {
    process: std::process::Child,
    events: Events,
    role: Role,
    fake: Directory,
    configuration: std::path::PathBuf,
    _directory: tempfile::TempDir,
}

const ARGUMENTS: [&str; 6] = [
    "-p",
    "--resume",
    "parent",
    "--fork-session",
    "--session-id",
    "fork",
];

fn start_fork() -> StartedFork {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("events.sock");
    let events = Events::listen(&socket).unwrap();
    let configuration = directory.path().join("claude-config");
    let parent = fake_agent::transcript(&configuration, "parent");
    fs::create_dir_all(parent.parent().unwrap()).unwrap();
    fs::write(&parent, "{\"type\":\"user\",\"uuid\":\"start\"}\n").unwrap();
    let fake = Directory::new(directory.path().join("agent"));
    let mut process = Process::new(env!("CARGO_BIN_EXE_fake-agent"))
        .args(ARGUMENTS)
        .env(variables::EVENTS, &socket)
        .env(variables::DIRECTORY, fake.path())
        .env("CLAUDE_CONFIG_DIR", &configuration)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(b"Take the turn of keep.")
        .unwrap();
    StartedFork {
        process,
        events,
        role: Role::fork("fork"),
        fake,
        configuration,
        _directory: directory,
    }
}

/// The lines of Claude Code's stream that a fork printed.
fn stream(output: &std::process::Output) -> Vec<serde_json::Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_fork_copies_its_parent_s_session_and_its_output_answers_its_tool_call() {
    let StartedFork {
        process: fork,
        events,
        role,
        fake,
        configuration,
        _directory,
    } = start_fork();

    let started = events.wait_for("the fork's start", |reported| {
        matches!(reported.event, Event::ForkStarted { .. })
    });
    events.obey(
        &role,
        &Command::CallTool {
            name: "mcp__app__submit".into(),
        },
    );
    let ended = events.obey(&role, &Command::EndTurn);
    let output = fork.wait_with_output().unwrap();

    assert_eq!(started.from, role);
    assert_eq!(
        started.event,
        Event::ForkStarted {
            parent: "parent".into(),
            prompt: "Take the turn of keep.".into(),
        }
    );
    assert!(
        ended
            .iter()
            .any(|reported| reported.event == Event::TurnEnded { blocked: None })
    );
    let session = fs::read_to_string(fake_agent::transcript(&configuration, "fork")).unwrap();
    assert!(
        session.starts_with("{\"type\":\"user\",\"uuid\":\"start\"}\n"),
        "{session}"
    );
    let record = |kind: &str| fs::read_to_string(fake.fork_record("fork", kind)).unwrap();
    assert_eq!(record("args"), ARGUMENTS.join("\n"));
    assert_eq!(record("prompt"), "Take the turn of keep.");
    assert_eq!(record("settings"), "", "no --settings");
    assert!(
        record("env")
            .lines()
            .any(|name| name == "CLAUDE_CONFIG_DIR"),
        "the names of its environment's variables"
    );
    // Claude Code's stream: the call, its result, then the end of the turn.
    let stream = stream(&output);
    let call = stream
        .iter()
        .position(|line| line["message"]["content"][0]["name"] == "mcp__app__submit")
        .expect("the fork's call");
    assert_eq!(
        stream[call + 1]["message"]["content"][0]["tool_use_id"],
        stream[call]["message"]["content"][0]["id"]
    );
    assert_eq!(stream.last().unwrap()["type"], "result");
}

#[test]
fn a_fork_told_to_crash_exits_with_a_failure_and_no_result() {
    let StartedFork {
        process: fork,
        events,
        role,
        // Kept to the end: the fork's files are under it.
        _directory,
        ..
    } = start_fork();
    events.wait_for("the fork's start", |reported| {
        matches!(reported.event, Event::ForkStarted { .. })
    });

    events.send(&role, &Command::Crash);

    let output = fork.wait_with_output().unwrap();
    assert!(!output.status.success(), "{:?}", output.status);
    assert!(
        stream(&output).iter().all(|line| line["type"] != "result"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let after = events.events_until("the fork's exit", |reported| {
        reported.from == role && reported.event == Event::Disconnected
    });
    assert!(
        !after
            .iter()
            .any(|reported| matches!(reported.event, Event::TurnEnded { .. }))
    );
}
