use super::*;
use crate::connection::Connection;
use crate::protocol::marker_of;

/// A fake agent's connection as `role`, and the commands it gets.
fn connect(path: &Path, role: Role) -> (Connection, mpsc::Receiver<Command>) {
    let (sender, commands) = mpsc::channel();
    let connection = Connection::connect(path, role, |command| command, sender).unwrap();
    (connection, commands)
}

fn agent() -> Role {
    Role::agent("%1")
}

#[test]
fn each_fake_agent_reports_its_events_in_order_then_its_end() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = Events::listen(&path).unwrap();
    let (agent_connection, _) = connect(&path, agent());

    agent_connection.report(&Event::PromptReceived {
        text: "first".into(),
    });
    agent_connection.report(&Event::TurnStarted);
    drop(agent_connection);

    let reported: Vec<_> = events
        .events_until("the agent's end", |reported| {
            reported.event == Event::Disconnected
        })
        .into_iter()
        .inspect(|reported| assert_eq!(reported.from, agent()))
        .map(|reported| reported.event)
        .collect();
    assert_eq!(
        reported,
        [
            Event::Hello { role: agent() },
            Event::PromptReceived {
                text: "first".into()
            },
            Event::TurnStarted,
            Event::Disconnected,
        ]
    );
}

#[test]
fn a_command_reaches_the_fake_agent_of_its_role_only() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = Events::listen(&path).unwrap();
    let (_first, first_commands) = connect(&path, Role::fork("first"));
    let (_second, second_commands) = connect(&path, Role::fork("second"));

    events.send(&Role::fork("second"), &Command::Crash);
    events.send(&Role::fork("first"), &Command::EndTurn);

    assert_eq!(second_commands.recv().unwrap(), Command::Crash);
    assert_eq!(first_commands.recv().unwrap(), Command::EndTurn);
}

#[test]
fn an_order_returns_what_came_before_the_fake_agent_s_answer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = Events::listen(&path).unwrap();
    let (agent_connection, commands) = connect(&path, agent());
    let (other, _) = connect(&path, Role::agent("%2"));
    std::thread::spawn(move || {
        let command = commands.recv().unwrap();
        other.report(&Event::TurnStarted);
        agent_connection.report(&Event::TurnEnded { blocked: None });
        // An answer to another command, then the answer.
        agent_connection.report(&Event::Obeyed {
            command: Command::Exit,
        });
        agent_connection.report(&Event::Obeyed { command });
        drop(other);
    });

    let reported: Vec<_> = events
        .obey(&agent(), &Command::EndTurn)
        .into_iter()
        .filter(|reported| reported.from == agent())
        .map(|reported| reported.event)
        .filter(|event| !matches!(event, Event::Hello { .. }))
        .collect();

    assert_eq!(
        reported,
        [
            Event::TurnEnded { blocked: None },
            Event::Obeyed {
                command: Command::Exit
            },
            Event::Obeyed {
                command: Command::EndTurn
            },
        ]
    );
}

#[test]
fn a_marker_returns_what_the_agent_reported_before_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = Events::listen(&path).unwrap();
    let (agent_connection, _) = connect(&path, agent());
    agent_connection.report(&Event::TurnEnded { blocked: None });

    let before = events.mark(|prompt| {
        // The fake agent reads the marker as its next prompt.
        agent_connection.report(&Event::Marker {
            id: marker_of(prompt).unwrap(),
        });
    });

    let before: Vec<_> = before.into_iter().map(|reported| reported.event).collect();
    assert_eq!(
        before,
        [
            Event::Hello { role: agent() },
            Event::TurnEnded { blocked: None },
            Event::Marker { id: 0 },
        ]
    );
    assert_eq!(marker_of("a prompt"), None);
}

#[test]
fn a_fake_agent_that_connects_again_takes_the_commands_of_its_role() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = Events::listen(&path).unwrap();
    let (first, _) = connect(&path, agent());
    events.wait_for("the first agent", |reported| {
        matches!(reported.event, Event::Hello { .. })
    });
    let (_second, commands) = connect(&path, agent());
    events.wait_for("the second agent", |reported| {
        matches!(reported.event, Event::Hello { .. })
    });

    drop(first);
    events.wait_for("the first agent's end", |reported| {
        reported.event == Event::Disconnected
    });
    events.send(&agent(), &Command::EndTurn);

    assert_eq!(commands.recv().unwrap(), Command::EndTurn);
}

#[test]
fn a_wait_on_every_event_sees_those_a_wait_in_turn_went_through() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = Events::listen(&path).unwrap();
    let (agent_connection, _) = connect(&path, agent());
    agent_connection.report(&Event::TurnStarted);
    agent_connection.report(&Event::TurnEnded { blocked: None });
    events.wait_for("the end of the turn", |reported| {
        matches!(reported.event, Event::TurnEnded { .. })
    });
    agent_connection.report(&Event::Exited);

    let received = events.wait_until("the agent's exit", |received| {
        received
            .iter()
            .any(|reported| reported.event == Event::Exited)
    });

    assert!(
        received
            .iter()
            .any(|reported| reported.event == Event::TurnStarted)
    );
    // A wait in turn goes on after the events the previous one went through.
    assert_eq!(events.wait_for("the exit", |_| true).event, Event::Exited);
}

#[test]
#[should_panic(expected = "a fake agent wrote")]
fn a_line_that_is_no_event_fails_the_test() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = Events::listen(&path).unwrap();
    let mut fake = UnixStream::connect(&path).unwrap();

    fake.write_all(b"not json\n").unwrap();

    events.wait_for("anything", |_| true);
}

#[test]
fn a_wait_or_a_command_that_finds_no_fake_agent_in_time_is_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = Events::listen(&path).unwrap();
    let (_agent, _) = connect(&path, agent());
    events.wait_for("the agent's hello", |reported| {
        matches!(reported.event, Event::Hello { .. })
    });

    let waited = events
        .try_wait_until("a fork", Duration::ZERO, |received| {
            received
                .iter()
                .any(|reported| matches!(reported.from, Role::Fork { .. }))
        })
        .unwrap_err();
    let sent = events
        .try_send(&Role::fork("absent"), &Command::EndTurn, Duration::ZERO)
        .unwrap_err();

    assert!(
        waited.starts_with("no fake agent reported a fork"),
        "{waited}"
    );
    assert!(sent.contains("did not connect"), "{sent}");
    assert!(
        events
            .try_send(&agent(), &Command::EndTurn, Duration::ZERO)
            .is_ok()
    );
}
