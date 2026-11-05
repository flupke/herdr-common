//! The fake agent in its pane, as the apps' tests drive it: its standard input stands for the
//! keys typed in its pane, a file for its screen, a script for the app's hooks, and, in Herdr
//! 0.9 (`HERDR_PANE_ID`), a socket the test listens on for Herdr's. Under herdr-app
//! (`HC_TERMINAL_ID`), the test reads the states it reports in OSC 7501, and its titles, off its
//! screen.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Process, Stdio};

use fake_agent::{Command, Event, Events, Reported, Role, variables};

/// The hook: it logs each payload, a line each, and prints the answer the test left for its
/// event, once, or fails when the test said so.
const HOOK: &str = r#"#!/bin/sh
payload=$(cat)
printf '%s\n' "$payload" >> "$(dirname "$0")/hooks.log"
event=$(printf '%s' "$payload" | sed -n 's/.*"hook_event_name":"\([A-Za-z]*\)".*/\1/p')
answer="$(dirname "$0")/$event.answer"
if [ -f "$(dirname "$0")/$event.fails" ]; then echo "hook broken" >&2; exit 1; fi
if [ -f "$answer" ]; then cat "$answer"; rm "$answer"; fi
"#;

/// The Herdr CLI: it logs its arguments, a line each call.
const HERDR: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$(dirname "$0")/herdr.log"
"#;

const SESSION: &str = "0f6c3a52-1d2e-4c1b-9a77-3c2f5e8d9b10";

/// A fake agent started in a test's directory.
struct Started {
    process: Child,
    keys: ChildStdin,
    events: Events,
    role: Role,
    directory: tempfile::TempDir,
}

impl Started {
    /// Starts the fake agent with `args`, in a pane whose environment is `pane`: its screen
    /// goes to `screen` in the directory.
    fn new(args: &[&str], pane: &[(&str, &Path)]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("events.sock");
        let events = Events::listen(&socket).unwrap();
        let hook = script(directory.path(), "hook", HOOK);
        let herdr = script(directory.path(), "herdr", HERDR);
        let screen = std::fs::File::create(directory.path().join("screen")).unwrap();
        let mut process = Process::new(env!("CARGO_BIN_EXE_fake-agent"));
        for variable in [
            "HERDR_PANE_ID",
            "HERDR_SOCKET_PATH",
            "HERDR_BIN_PATH",
            "HC_TERMINAL_ID",
            "CLAUDE_CONFIG_DIR",
            "FORCE_HYPERLINK",
            variables::TURNS,
            variables::SESSION,
            variables::NAME,
        ] {
            process.env_remove(variable);
        }
        process
            .args(args)
            .env(variables::EVENTS, &socket)
            .env(variables::HOOK, hook)
            .env(variables::HERDR, herdr)
            .envs(pane.iter().copied())
            .stdin(Stdio::piped())
            .stdout(screen);
        let mut process = process.spawn().unwrap();
        let keys = process.stdin.take().unwrap();
        let name = pane
            .iter()
            .find(|(name, _)| ["HERDR_PANE_ID", "HC_TERMINAL_ID"].contains(name))
            .map(|(_, pane)| pane.to_string_lossy().into_owned())
            .unwrap();
        Self {
            process,
            keys,
            events,
            role: Role::agent(name),
            directory,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    /// Types `keys` in the pane.
    fn types(&mut self, keys: &str) {
        self.keys.write_all(keys.as_bytes()).unwrap();
    }

    /// The next event of the fake agent that `matches`.
    fn wait_for(&self, what: &str, matches: impl Fn(&Event) -> bool) -> Event {
        self.events
            .wait_for(what, |reported| {
                reported.from == self.role && matches(&reported.event)
            })
            .event
    }

    /// The fake agent's events up to its answer to `command`, but its answer.
    fn obey(&self, command: &Command) -> Vec<Event> {
        let mut events: Vec<Event> = self
            .events
            .obey(&self.role, command)
            .into_iter()
            .filter(|reported| reported.from == self.role)
            .map(|reported| reported.event)
            .collect();
        events.pop();
        events
    }

    /// The payloads of the hooks it ran.
    fn hooks(&self) -> Vec<serde_json::Value> {
        lines(&self.path("hooks.log"))
            .iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// The hook of `event` answers `answer`, once.
    fn answers(&self, event: &str, answer: &serde_json::Value) {
        std::fs::write(self.path(&format!("{event}.answer")), answer.to_string()).unwrap();
    }

    fn transcript(&self, session: &str) -> Vec<serde_json::Value> {
        lines(&fake_agent::transcript(
            &self.path("claude-config"),
            session,
        ))
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
    }
}

fn script(directory: &Path, name: &str, text: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// What `payloads` say of each hook: its event, then the field that tells it apart.
fn summary(payloads: &[serde_json::Value]) -> Vec<String> {
    payloads
        .iter()
        .map(|payload| {
            let event = payload["hook_event_name"].as_str().unwrap();
            let detail = match event {
                "SessionStart" => payload["source"].to_string(),
                "UserPromptSubmit" => payload["prompt"].to_string(),
                "Stop" => payload["stop_hook_active"].to_string(),
                "SessionEnd" => payload["reason"].to_string(),
                _ => String::new(),
            };
            format!("{event} {detail}")
        })
        .collect()
}

#[test]
fn the_test_ends_each_turn_with_the_stop_hook_which_may_block_it() {
    let mut agent = Started::new(
        &["--session-id", SESSION, "--", "Read the kickoff."],
        &[("HC_TERMINAL_ID", Path::new("t1"))],
    );

    agent.wait_for("the kickoff", |event| {
        *event
            == Event::PromptReceived {
                text: "Read the kickoff.".into(),
            }
    });
    agent.wait_for("the kickoff's turn", |event| *event == Event::TurnStarted);
    agent.answers(
        "Stop",
        &serde_json::json!({"decision": "block", "reason": "Go on."}),
    );
    let blocked = agent.obey(&Command::EndTurn);
    // A prompt typed while it works joins its turn.
    agent.types("And this.\r");
    agent.wait_for("the prompt typed", |event| {
        matches!(event, Event::PromptReceived { .. })
    });
    let ended = agent.obey(&Command::EndTurn);
    let exited = agent.obey(&Command::Exit);

    assert!(agent.process.wait().unwrap().success());
    assert_eq!(
        blocked,
        [Event::TurnEnded {
            blocked: Some("Go on.".into())
        }]
    );
    assert_eq!(ended, [Event::TurnEnded { blocked: None }]);
    assert_eq!(exited, [Event::Exited]);
    let hooks = agent.hooks();
    assert_eq!(
        summary(&hooks),
        [
            r#"SessionStart "startup""#,
            r#"UserPromptSubmit "Read the kickoff.""#,
            "Stop false",
            r#"UserPromptSubmit "And this.""#,
            "Stop true",
            r#"SessionEnd "prompt_input_exit""#,
        ]
    );
    let transcript = agent.transcript(SESSION);
    assert!(hooks.iter().all(|payload| {
        payload["session_id"] == SESSION
            && payload["transcript_path"]
                == fake_agent::transcript(&agent.path("claude-config"), SESSION)
                    .to_str()
                    .unwrap()
    }));
    assert_eq!(
        transcript
            .iter()
            .map(|entry| entry["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["user", "assistant", "attachment", "assistant"]
    );
    assert_eq!(transcript[0]["origin"]["kind"], "human");
    assert_eq!(transcript[2]["attachment"]["type"], "queued_command");
    assert_eq!(transcript[2]["attachment"]["prompt"], "And this.");
    // herdr-app's core hears each state in OSC 7501.
    let screen = std::fs::read_to_string(agent.path("screen")).unwrap();
    assert_eq!(states(&screen), ["idle", "working", "idle", "clear"]);
}

#[test]
fn a_prompt_its_hook_blocks_goes_back_in_its_input_line_and_starts_no_turn() {
    let mut agent = Started::new(
        &["--session-id", SESSION],
        &[("HC_TERMINAL_ID", Path::new("t1"))],
    );
    agent.wait_for("its start", |event| matches!(event, Event::Started { .. }));
    agent.answers(
        "UserPromptSubmit",
        &serde_json::json!({"decision": "block", "reason": "Not now."}),
    );

    agent.types("Do it.\r");
    let blocked = agent.wait_for("the prompt blocked", |event| {
        matches!(event, Event::PromptBlocked { .. })
    });
    // Enter again submits what the input line holds.
    agent.types("\r");
    let received = agent.events.events_until("the prompt", |reported| {
        matches!(reported.event, Event::TurnStarted)
    });

    assert_eq!(
        blocked,
        Event::PromptBlocked {
            text: "Do it.".into()
        }
    );
    let received: Vec<_> = received
        .into_iter()
        .map(|reported| reported.event)
        .collect();
    assert_eq!(
        received,
        [
            Event::PromptReceived {
                text: "Do it.".into()
            },
            Event::TurnStarted
        ]
    );
    let transcript = agent.transcript(SESSION);
    assert_eq!(
        transcript
            .iter()
            .map(|entry| entry["message"]["content"].clone())
            .collect::<Vec<_>>(),
        ["Do it."],
        "Claude Code erases a blocked prompt from its session"
    );
}

#[test]
fn claude_code_s_own_commands_start_sessions_and_run_no_prompt_hook() {
    let mut agent = Started::new(
        &["--session-id", SESSION],
        &[("HC_TERMINAL_ID", Path::new("t1"))],
    );
    agent.wait_for("its start", |event| matches!(event, Event::Started { .. }));

    agent.types("/clear\r");
    let marked = agent.events.mark(|marker| agent_types(&agent, marker));
    agent.types("/bg\r/resume ");
    agent.types(SESSION);
    agent.types("\r\u{4}");
    agent.wait_for("its exit", |event| *event == Event::Exited);

    assert!(agent.process.wait().unwrap().success());
    assert!(
        !marked
            .iter()
            .any(|reported| matches!(reported.event, Event::PromptReceived { .. })),
        "{marked:?}"
    );
    let hooks = agent.hooks();
    assert_eq!(
        summary(&hooks),
        [
            r#"SessionStart "startup""#,
            r#"SessionEnd "clear""#,
            r#"SessionStart "clear""#,
            r#"SessionStart "fork""#,
            r#"SessionStart "resume""#,
            r#"SessionEnd "prompt_input_exit""#,
        ]
    );
    let cleared = hooks[2]["session_id"].as_str().unwrap();
    let forked = hooks[3]["session_id"].as_str().unwrap();
    assert_ne!(cleared, SESSION);
    assert_ne!(forked, cleared);
    assert_eq!(hooks[4]["session_id"], SESSION);
    // Under herdr-app, where it reports no session to Herdr, it tells the test of each start.
    assert_eq!(
        session_starts(&agent),
        [
            (SESSION, "startup"),
            (cleared, "clear"),
            (forked, "fork"),
            (SESSION, "resume"),
        ]
        .map(|(session, start)| (session.to_owned(), start.to_owned()))
    );
    let transcripts = agent.path("claude-config/projects/fake-agent");
    assert_eq!(
        fake_agent::transcripts(&agent.path("claude-config")),
        transcripts
    );
    let mut kept: Vec<_> = std::fs::read_dir(transcripts)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    kept.sort();
    let mut expected = [SESSION, cleared, forked].map(|session| format!("{session}.jsonl"));
    expected.sort();
    assert_eq!(kept, expected, "the transcripts' project folder");
    assert_eq!(
        agent.transcript(cleared),
        [
            serde_json::json!({"type": "continued-in", "sessionId": cleared,
            "continuedInSessionId": forked})
        ]
    );
}

/// Each session start `agent` reported, with how it started.
fn session_starts(agent: &Started) -> Vec<(String, String)> {
    agent
        .events
        .try_received()
        .unwrap()
        .into_iter()
        .filter(|reported| reported.from == agent.role)
        .filter_map(|reported| match reported.event {
            Event::SessionStarted { session, start } => Some((session, start)),
            _ => None,
        })
        .collect()
}

/// Types `text` in the pane of `agent`, then Enter.
fn agent_types(agent: &Started, text: &str) {
    let mut keys = &agent.keys;
    keys.write_all(format!("{text}\r").as_bytes()).unwrap();
}

/// A Herdr 0.9 that the test plays on its socket: it takes the agent's subscription and its
/// question, then sends what the test says.
struct FakeHerdr {
    events: std::os::unix::net::UnixStream,
}

impl FakeHerdr {
    /// Answers the subscription and the agent's status of the fake agent that connects to
    /// `listener`: idle.
    fn accept(listener: &UnixListener) -> Self {
        let (subscription, _) = listener.accept().unwrap();
        let mut request = String::new();
        BufReader::new(&subscription)
            .read_line(&mut request)
            .unwrap();
        let mut events = subscription;
        writeln!(events, r#"{{"id":"fake-agent","result":{{}}}}"#).unwrap();
        let (mut question, _) = listener.accept().unwrap();
        BufReader::new(&question).read_line(&mut request).unwrap();
        writeln!(
            question,
            r#"{{"id":"fake-agent","result":{{"agent":{{"agent_status":"idle"}}}}}}"#
        )
        .unwrap();
        Self { events }
    }

    fn says(&mut self, event: &str, data: &serde_json::Value) {
        writeln!(
            self.events,
            "{}",
            serde_json::json!({"event": event, "data": data})
        )
        .unwrap();
    }
}

#[test]
fn in_herdr_a_turn_ends_by_itself_once_herdr_saw_it() {
    let herdr_directory = tempfile::tempdir().unwrap();
    let socket = herdr_directory.path().join("herdr.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let herdr = std::thread::spawn(move || FakeHerdr::accept(&listener));
    let mut agent = Started::new(
        &["--session-id", SESSION],
        &[
            ("HERDR_PANE_ID", Path::new("%1")),
            ("HERDR_SOCKET_PATH", &socket),
            (variables::TURNS, Path::new("auto")),
        ],
    );
    let mut herdr = herdr.join().unwrap();
    let reported = agent.wait_for("its session", |event| {
        matches!(event, Event::SessionReported { .. })
    });

    agent.types("Work.\r");
    agent.wait_for("its turn", |event| *event == Event::TurnStarted);
    let before = agent.events.mark(|marker| agent_types(&agent, marker));
    herdr.says(
        "pane.agent_status_changed",
        &serde_json::json!({"pane_id": "%1", "agent_status": "working"}),
    );
    agent.wait_for("the end of its turn", |event| {
        *event == Event::TurnEnded { blocked: None }
    });
    // Herdr's news of another pane is not the agent's.
    herdr.says(
        "pane_agent_detected",
        &serde_json::json!({"pane_id": "%2", "released": true}),
    );
    herdr.says(
        "pane_agent_detected",
        &serde_json::json!({"pane_id": "%1", "released": true}),
    );
    agent.wait_for("its release", |event| *event == Event::Released);
    // Held, a turn Herdr saw goes on until the test lets turns end.
    agent.obey(&Command::HoldTurns { hold: true });
    agent.types("Hold on.\r");
    agent.wait_for("its held turn", |event| *event == Event::TurnStarted);
    let held = agent.events.mark(|marker| agent_types(&agent, marker));
    let released = agent.obey(&Command::HoldTurns { hold: false });
    agent.obey(&Command::ReportSession);
    agent.obey(&Command::Exit);

    assert!(agent.process.wait().unwrap().success());
    assert_eq!(
        reported,
        Event::SessionReported {
            session: SESSION.into(),
            start: "startup".into()
        }
    );
    for (events, what) in [
        (&before, "a turn Herdr did not see"),
        (&held, "a held turn"),
    ] {
        assert!(
            !events
                .iter()
                .any(|reported: &Reported| matches!(reported.event, Event::TurnEnded { .. })),
            "{what} ended: {events:?}"
        );
    }
    assert_eq!(released, [Event::TurnEnded { blocked: None }]);
    // Herdr's news comes in order: news of another pane before the agent's own release.
    let release_count = agent
        .events
        .try_received()
        .unwrap()
        .into_iter()
        .filter(|reported| reported.event == Event::Released)
        .count();
    assert_eq!(
        release_count, 1,
        "the release of another pane is not the agent's"
    );
    let transcript = fake_agent::transcript(&agent.path("claude-config"), SESSION);
    let session_report = [
        "pane",
        "report-agent-session",
        "%1",
        "--source",
        "herdr:claude",
        "--agent",
        "claude",
        "--agent-session-id",
        SESSION,
        "--agent-session-path",
        transcript.to_str().unwrap(),
        "--session-start-source",
        "startup",
    ];
    assert_eq!(
        herdr_calls(&agent),
        [session_report, session_report],
        "its start's report, then the one the test asked"
    );
}

/// The calls of Herdr's CLI the fake agent made, a session report's sequence number left out
/// once checked.
fn herdr_calls(agent: &Started) -> Vec<Vec<String>> {
    lines(&agent.path("herdr.log"))
        .iter()
        .map(|line| {
            let mut call: Vec<String> = line.split(' ').map(str::to_owned).collect();
            if let Some(sequence) = call.iter().position(|word| word == "--seq") {
                assert!(call[sequence + 1].parse::<u128>().is_ok(), "{call:?}");
                call.drain(sequence..sequence + 2);
            }
            call
        })
        .collect()
}

#[test]
fn an_agent_that_reports_its_state_ends_each_turn_at_once_until_herdr_releases_it() {
    let herdr_directory = tempfile::tempdir().unwrap();
    let socket = herdr_directory.path().join("herdr.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let herdr = std::thread::spawn(move || FakeHerdr::accept(&listener));
    let mut agent = Started::new(
        &[],
        &[
            ("HERDR_PANE_ID", Path::new("%1")),
            ("HERDR_SOCKET_PATH", &socket),
            (variables::TURNS, Path::new("auto")),
            (variables::REPORT_STATE, Path::new("1")),
            (variables::SESSION, Path::new("")),
        ],
    );
    let mut herdr = herdr.join().unwrap();
    let started = agent.events.events_until("its start", |reported| {
        matches!(reported.event, Event::Started { .. })
    });

    agent.types("Reported.\r");
    agent.wait_for("its turn's end", |event| {
        *event == Event::TurnEnded { blocked: None }
    });
    herdr.says(
        "pane_agent_detected",
        &serde_json::json!({"pane_id": "%1", "released": true}),
    );
    agent.wait_for("its release", |event| *event == Event::Released);
    agent.types("Read.\r");
    agent.wait_for("its next turn", |event| *event == Event::TurnStarted);
    let unseen = agent.events.mark(|marker| agent_types(&agent, marker));
    agent.obey(&Command::Exit);

    assert!(agent.process.wait().unwrap().success());
    let started: Vec<_> = started.into_iter().map(|reported| reported.event).collect();
    assert_eq!(
        started[1..],
        [
            Event::StateReported {
                state: "idle".into()
            },
            Event::Started { args: Vec::new() }
        ],
        "no session reported"
    );
    assert!(
        !unseen
            .iter()
            .any(|reported| matches!(reported.event, Event::TurnEnded { .. })),
        "once released, a turn waits for Herdr: {unseen:?}"
    );
    assert_eq!(
        herdr_calls(&agent),
        [[
            "pane",
            "report-agent",
            "%1",
            "--source",
            fake_agent::STATE_SOURCE,
            "--agent",
            "claude",
            "--state",
            "idle"
        ]]
    );
    assert!(
        !summary(&agent.hooks())
            .iter()
            .any(|hook| hook.starts_with("SessionStart")),
        "no session, no SessionStart"
    );
}

#[test]
fn the_test_s_commands_run_claude_code_s_hooks_and_show_on_its_screen() {
    let mut agent = Started::new(&[], &[("HC_TERMINAL_ID", Path::new("t1"))]);
    agent.wait_for("its start", |event| matches!(event, Event::Started { .. }));

    agent.obey(&Command::PermissionPrompt);
    agent.obey(&Command::StopFailure {
        error: "rate_limit".into(),
    });
    agent.obey(&Command::StartBackground {
        kind: "shell".into(),
    });
    agent.obey(&Command::EndTurn);
    let finished = agent.obey(&Command::FinishBackground);
    agent.obey(&Command::EndTurn);
    agent.obey(&Command::Say {
        text: "See [the PR](https://x/1).".into(),
    });
    agent.obey(&Command::ShowTitle {
        title: "A title".into(),
    });
    agent.obey(&Command::Suggest {
        text: "Try this".into(),
    });
    agent.types("x");
    let typed = agent.wait_for("the line typed", |event| {
        matches!(event, Event::InputLine { .. })
    });
    // The screen the test gives shows while nothing is typed.
    agent.types("\u{15}");
    agent.wait_for("the line cleared", |event| {
        *event
            == Event::InputLine {
                text: String::new(),
            }
    });
    agent.obey(&Command::ShowScreen {
        text: "A whole screen".into(),
    });
    agent.obey(&Command::Exit);

    assert!(agent.process.wait().unwrap().success());
    assert_eq!(
        finished,
        [
            Event::PromptReceived {
                text: "<task-notification>\n<task-id>bg0</task-id>\n<status>completed</status>\n\
                       </task-notification>"
                    .into()
            },
            Event::TurnStarted
        ]
    );
    assert_eq!(typed, Event::InputLine { text: "x".into() });
    let hooks = agent.hooks();
    let named = |event: &str| {
        hooks
            .iter()
            .find(|payload| payload["hook_event_name"] == event)
            .unwrap_or_else(|| panic!("no {event} hook"))
    };
    assert_eq!(
        named("Notification")["notification_type"],
        "permission_prompt"
    );
    assert_eq!(named("StopFailure")["error"], "rate_limit");
    assert_eq!(
        named("Stop")["background_tasks"],
        serde_json::json!([{"id": "bg0", "type": "shell", "status": "running"}])
    );
    let screen = std::fs::read_to_string(agent.path("screen")).unwrap();
    for shown in [
        " Do you want to proceed?",
        "See the PR (https://x/1).",
        "\x1b]0;A title\x07",
        "\u{1b}[2mTry this\u{1b}[22m",
        "\x1b[2J\x1b[HA whole screen",
        "\x1b]7501;state=blocked:kind=permission:app=claude-code",
    ] {
        assert!(screen.contains(shown), "{shown:?} not shown");
    }
}

#[test]
fn the_switch_files_swallow_a_prompt_and_keep_a_resume_unheard_or_late() {
    let agent = Started::new(
        &["--session-id", SESSION],
        &[("HC_TERMINAL_ID", Path::new("t1"))],
    );
    agent.wait_for("its start", |event| matches!(event, Event::Started { .. }));
    let swallow = agent.path("swallow");
    let unreported = agent.path("unreported-resume");

    std::fs::write(&swallow, "").unwrap();
    agent_types(&agent, "Swallowed.");
    let swallowed = agent.events.mark(|marker| agent_types(&agent, marker));
    std::fs::remove_file(&swallow).unwrap();
    std::fs::write(&unreported, "").unwrap();
    agent_types(&agent, "/resume unheard");
    agent.wait_for("the unheard resume", |event| {
        matches!(event, Event::ResumeReceived { .. })
    });
    std::fs::write(&unreported, "1").unwrap();
    agent_types(&agent, "/resume late");
    // The agent reads its keys and the test's commands in the order they reach it.
    agent.wait_for("the late resume's start", |event| {
        *event
            == Event::SessionStarted {
                session: "late".into(),
                start: "resume".into(),
            }
    });
    let hooks_at_start = agent.hooks();
    agent.obey(&Command::Exit);

    let swallowed: Vec<_> = swallowed
        .into_iter()
        .map(|reported| reported.event)
        .filter(|event| !matches!(event, Event::Marker { .. }))
        .collect();
    assert_eq!(
        swallowed,
        [Event::PromptReceived {
            text: "Swallowed.".into()
        }],
        "a swallowed prompt starts no turn"
    );
    let starts: Vec<_> = agent
        .hooks()
        .into_iter()
        .filter(|payload| payload["hook_event_name"] == "SessionStart")
        .map(|payload| payload["session_id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(starts, [SESSION, "late"], "the unheard resume runs no hook");
    assert_eq!(
        session_starts(&agent),
        [(SESSION, "startup"), ("late", "resume")]
            .map(|(session, start)| (session.to_owned(), start.to_owned())),
        "nor tells the test of a start"
    );
    assert_eq!(
        hooks_at_start.last().unwrap()["session_id"],
        "late",
        "the start is told once its hook ran"
    );
    assert!(!unreported.exists(), "the agent removes the switch");
}

#[test]
fn an_app_s_hook_that_fails_ends_the_fake_agent() {
    let agent = Started::new(&[], &[("HC_TERMINAL_ID", Path::new("t1"))]);
    agent.wait_for("its start", |event| matches!(event, Event::Started { .. }));
    std::fs::write(agent.path("UserPromptSubmit.fails"), "").unwrap();

    agent_types(&agent, "Break.");
    let gone = agent.events.events_until("its end", |reported| {
        reported.from == agent.role && reported.event == Event::Disconnected
    });

    assert!(
        !gone
            .iter()
            .any(|reported| matches!(reported.event, Event::PromptReceived { .. })),
        "{gone:?}"
    );
    let Started { mut process, .. } = agent;
    assert!(!process.wait().unwrap().success());
}

#[test]
fn it_shows_itself_working_without_a_turn() {
    let mut agent = Started::new(&[], &[("HC_TERMINAL_ID", Path::new("t1"))]);
    agent.wait_for("its start", |event| matches!(event, Event::Started { .. }));

    let working = agent.obey(&Command::ShowWorking { working: true });
    let idle = agent.obey(&Command::ShowWorking { working: false });
    agent.obey(&Command::Exit);

    agent.process.wait().unwrap();
    assert_eq!(working, [], "no turn starts");
    assert_eq!(idle, [], "no turn ends");
    assert_eq!(
        summary(&agent.hooks()),
        [
            r#"SessionStart "startup""#,
            r#"SessionEnd "prompt_input_exit""#
        ]
    );
    let screen = std::fs::read_to_string(agent.path("screen")).unwrap();
    assert_eq!(
        titles(&screen),
        ["✳ Claude Code", "⠋ Claude Code", "✳ Claude Code"]
    );
    // herdr-app's core hears it in OSC 7501.
    assert_eq!(states(&screen), ["idle", "working", "idle", "clear"]);
}

#[test]
fn a_pasted_prompt_costs_a_few_screens_not_one_per_key() {
    let mut agent = Started::new(&[], &[("HC_TERMINAL_ID", Path::new("t1"))]);
    agent.wait_for("its start", |event| matches!(event, Event::Started { .. }));
    // 6.4 KB on 100 lines, as herdr-ruminate pastes.
    let prompt = vec!["x".repeat(63); 100].join("\n");

    agent.types(&format!("\u{1b}[200~{prompt}\u{1b}[201~\r"));
    let received = agent.wait_for("the prompt", |event| {
        matches!(event, Event::PromptReceived { .. })
    });

    assert_eq!(
        received,
        Event::PromptReceived {
            text: prompt.clone()
        }
    );
    // A draw per key writes the line typed so far each time: some 20 MB here.
    let screen = std::fs::metadata(agent.path("screen")).unwrap().len();
    let bound = 32 * prompt.len() as u64;
    assert!(
        screen < bound,
        "{screen} bytes drawn for a paste, over {bound}"
    );
}

/// The states `screen` reports to herdr-app's core in OSC 7501, in order.
fn states(screen: &str) -> Vec<&str> {
    screen
        .split("\x1b]7501;state=")
        .skip(1)
        .map(|report| report.split(':').next().unwrap())
        .collect()
}

/// The titles `screen` shows, in order.
fn titles(screen: &str) -> Vec<&str> {
    screen
        .split("\x1b]0;")
        .skip(1)
        .map(|title| title.split('\x07').next().unwrap())
        .collect()
}

#[test]
fn its_title_names_its_session_and_shows_a_given_title_outside_its_turns() {
    let mut agent = Started::new(&["-n", "first"], &[("HC_TERMINAL_ID", Path::new("t1"))]);
    agent.wait_for("its start", |event| matches!(event, Event::Started { .. }));
    agent.answers(
        "UserPromptSubmit",
        &serde_json::json!({"hookSpecificOutput": {"sessionTitle": "renamed"}}),
    );

    agent.types("Work.\r");
    agent.wait_for("its turn", |event| *event == Event::TurnStarted);
    agent.obey(&Command::ShowTitle {
        title: "Given".into(),
    });
    let during = titles(&std::fs::read_to_string(agent.path("screen")).unwrap()).len();
    agent.obey(&Command::EndTurn);
    agent.obey(&Command::Exit);

    agent.process.wait().unwrap();
    let screen = std::fs::read_to_string(agent.path("screen")).unwrap();
    let titles = titles(&screen);
    assert_eq!(titles[0], "✳ first");
    assert!(titles[..during].contains(&"⠋ renamed"), "{titles:?}");
    assert!(!titles[..during].contains(&"Given"), "{titles:?}");
    assert_eq!(titles.last(), Some(&"Given"));
}

#[test]
fn as_codex_it_shows_codex_s_glyph_title_and_state() {
    let mut agent = Started::new(
        &[],
        &[
            ("HC_TERMINAL_ID", Path::new("t1")),
            (variables::PLAYS, Path::new("codex")),
        ],
    );
    agent.wait_for("its start", |event| matches!(event, Event::Started { .. }));

    agent.types("Work.\r");
    agent.wait_for("its turn", |event| *event == Event::TurnStarted);
    agent.obey(&Command::EndTurn);
    agent.obey(&Command::Exit);

    agent.process.wait().unwrap();
    let screen = std::fs::read_to_string(agent.path("screen")).unwrap();
    assert_eq!(titles(&screen), ["", "⠋ Working", ""]);
    assert!(screen.contains("› "), "codex's glyph");
    assert!(screen.contains("\x1b]7501;state=working:app=codex"));
}
