//! The fake agent in its pane: it reads the keys typed there and the test's commands, and does
//! with them what the agent it plays does, one at a time.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::json;

use crate::connection::Connection;
use crate::herdr::{self, Herdr, News};
use crate::hooks::{Hooks, Printed};
use crate::launch::{self, Start};
use crate::protocol::{Command, Event, Role, marker_of};
use crate::screen::{Plays, Screen, StateReport};
use crate::terminal::{self, Key, RawMode};
use crate::transcript::Transcript;
use crate::turns::Turns;
use crate::{App, Directory, reply, variables};

/// The command, typed in its pane, that ends Claude Code's session.
const EXIT: &str = "/exit";
/// The command, typed in its pane, that has Claude Code start a new session.
const CLEAR: &str = "/clear";
/// The command, typed in its pane, that has Claude Code fork its session into its background
/// daemon.
const FORK: &str = "/bg";
/// The command, typed in its pane with a session's ID, that has Claude Code resume that
/// session.
const RESUME: &str = "/resume ";

/// The report Claude Code submits once a session's background work ends.
const BACKGROUND_REPORT: &str =
    "<task-notification>\n<task-id>bg0</task-id>\n<status>completed</status>\n</task-notification>";

/// What reaches the agent, one at a time.
enum Input {
    /// The keys of one read of its pane.
    Keys(Vec<Key>),
    Command(Command),
    Herdr(News),
}

/// Whether the agent goes on after an input.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Next {
    GoOn,
    Exit,
}

/// Runs the agent that Claude Code's `args` launch, until it exits.
///
/// # Panics
/// Panics when its environment lacks what it needs.
pub(crate) fn run(app: &App, args: &[String]) {
    let pane = std::env::var("HERDR_PANE_ID").ok();
    let terminal = std::env::var("HC_TERMINAL_ID").ok();
    let events = std::env::var_os(variables::EVENTS).expect("the test's event socket");
    let (sender, inputs) = mpsc::channel();
    let connection = Connection::connect(
        Path::new(&events),
        Role::agent(crate::agent_name()),
        Input::Command,
        sender.clone(),
    )
    .expect("the test listens on its event socket");
    // Follows Herdr before it shows anything, so that it misses nothing Herdr says of it.
    if let (Some(pane), Some(socket)) = (&pane, std::env::var_os("HERDR_SOCKET_PATH")) {
        herdr::follow(Path::new(&socket), pane, Input::Herdr, sender.clone())
            .expect("Herdr's socket");
    }
    let raw_mode = RawMode::enter();
    let mut agent = Agent::new(app, args, connection, pane, terminal.is_some());
    agent.starts(args);
    std::thread::spawn(move || terminal::read_keys(|keys| sender.send(Input::Keys(keys)).is_ok()));
    if let Some(prompt) = launch::first_prompt(args) {
        agent.prompt(prompt);
    }
    agent.settle();
    for input in inputs {
        let next = match input {
            Input::Keys(keys) => agent.keys(keys),
            Input::Command(command) => agent.obey(command),
            Input::Herdr(news) => agent.hear(news),
        };
        if next == Next::Exit {
            break;
        }
        agent.settle();
    }
    drop(raw_mode);
}

/// A running fake agent.
struct Agent<'a> {
    app: &'a App,
    plays: Plays,
    connection: Connection,
    hooks: Hooks,
    /// The Herdr 0.9 of its pane; none on herdr-app or in a plain terminal.
    herdr: Option<Herdr>,
    /// Whether it reports its state to Herdr itself.
    reports_state: bool,
    directory: Directory,
    transcript: Transcript,
    /// Its session, and how it started.
    session: Option<String>,
    start: Start,
    screen: Screen,
    turns: Turns,
    /// Whether its last stop was blocked: Claude Code then says so in the next one.
    blocked: bool,
    /// The kinds of its background work that runs, which its stops list.
    background: Vec<String>,
    /// Whether it writes a Markdown link as a terminal link, as its launch has Claude Code do.
    hyperlinks: bool,
}

impl<'a> Agent<'a> {
    fn new(
        app: &'a App,
        args: &[String],
        connection: Connection,
        pane: Option<String>,
        on_herdr_app: bool,
    ) -> Self {
        let plays = Plays::from_env();
        let directory = Directory::from_env();
        let herdr = pane.map(|pane| Herdr::new(herdr_cli(), pane));
        let reports_state =
            herdr.is_some() && std::env::var(variables::REPORT_STATE).as_deref() == Ok("1");
        let held = std::env::var(variables::TURNS).as_deref() != Ok("auto");
        let report = if herdr.is_none() && on_herdr_app {
            StateReport::Osc
        } else {
            StateReport::None
        };
        let session_name = launch::flag_value(args, "-n").unwrap_or_default();
        Self {
            app,
            plays,
            connection,
            hooks: Hooks::new(
                std::env::var_os(variables::HOOK).map(PathBuf::from),
                app.hook_arguments,
            ),
            transcript: Transcript::new(directory.configuration()),
            directory,
            session: None,
            start: Start::Startup,
            screen: Screen::new(plays, report, session_name),
            turns: Turns::new(held, herdr.is_some() && !reports_state),
            herdr,
            reports_state,
            blocked: false,
            background: Vec::new(),
            hyperlinks: reply::hyperlinks(args),
        }
    }

    /// Its start: it shows itself idle, reports its state if it does, starts its session, and
    /// tells the test.
    fn starts(&mut self, args: &[String]) {
        self.screen
            .log(format!("fake {} {}", self.plays.name(), args.join(" ")));
        self.screen.idle();
        if self.reports_state
            && let Some(herdr) = &self.herdr
        {
            herdr.report_state(self.plays.name(), "idle");
            self.report(&Event::StateReported {
                state: "idle".to_owned(),
            });
        }
        let (session, start) = launch::session(args, std::env::var(variables::SESSION).ok());
        if let Some(session) = session {
            self.session_starts(session, start);
        }
        self.report(&Event::Started {
            args: args.to_vec(),
        });
    }

    fn report(&self, event: &Event) {
        self.connection.report(event);
    }

    fn transcript_path(&self) -> Option<PathBuf> {
        Some(self.transcript.path(self.session.as_deref()?))
    }

    /// Runs the hook of `event` with its own `fields`: what it printed.
    fn hook(&self, event: &str, fields: &serde_json::Value) -> Printed {
        self.hooks.run(
            self.session.as_deref(),
            self.transcript_path().as_deref(),
            event,
            fields,
        )
    }

    /// The session `session` starts, from `start`: its `SessionStart` hooks run, the app's,
    /// then Herdr's integration's, which reports it to Herdr, and it tells the test.
    fn session_starts(&mut self, session: String, start: Start) {
        self.transcript.start(&session);
        self.session = Some(session.clone());
        self.start = start;
        let printed = self.hook("SessionStart", &json!({"source": start.as_str()}));
        self.show(&printed);
        self.report_session();
        self.report(&Event::SessionStarted {
            session,
            start: start.as_str().to_owned(),
        });
    }

    /// Reports its session to Herdr 0.9, as Claude's Herdr integration does.
    fn report_session(&self) {
        let (Some(herdr), Some(session), Some(transcript)) =
            (&self.herdr, &self.session, self.transcript_path())
        else {
            return;
        };
        let start = self.start.as_str();
        herdr.report_session(self.plays.name(), session, &transcript, start);
        self.report(&Event::SessionReported {
            session: session.clone(),
            start: start.to_owned(),
        });
    }

    /// Shows the session's name a hook printed, as Claude Code shows a renamed session's, in
    /// its pane and its title, and the context it added to the session's.
    fn show(&mut self, printed: &Printed) {
        if let Some(name) = printed.session_title() {
            self.screen.log(format!("session name: {name}"));
            self.screen.name_session(name);
        }
        if let Some(context) = printed.additional_context() {
            self.screen.log(format!("session context: {context}"));
        }
    }

    /// Ends the turn under way while it is due to end by itself.
    fn settle(&mut self) {
        while self.turns.due() {
            self.end_turn();
        }
    }

    /// The keys of one read: it edits its input line with them and shows it before each Enter
    /// and Ctrl-D and at the end, rather than per key. Each Enter submits it, and its turns
    /// settle after.
    fn keys(&mut self, keys: Vec<Key>) -> Next {
        // The input line, once a key edited it, until it shows.
        let mut edited: Option<String> = None;
        for key in keys {
            if !matches!(key, Key::Enter | Key::Exit) {
                edit(
                    edited.get_or_insert_with(|| self.screen.typed().to_owned()),
                    key,
                );
                continue;
            }
            if let Some(typed) = edited.take() {
                self.show_typed(typed);
            }
            if key == Key::Exit {
                return self.exit();
            }
            let text = self.screen.submit();
            if self.submit(&text) == Next::Exit {
                return Next::Exit;
            }
            self.settle();
        }
        if let Some(typed) = edited {
            self.show_typed(typed);
        }
        Next::GoOn
    }

    /// Its input line holds `typed` from now on, which it reports while it suggests a prompt.
    fn show_typed(&mut self, typed: String) {
        self.screen.set_typed(typed);
        if self.screen.suggests() {
            self.report(&Event::InputLine {
                text: self.screen.typed().to_owned(),
            });
        }
    }

    /// What was typed is submitted: Claude Code's own commands, a marker, or a prompt.
    fn submit(&mut self, text: &str) -> Next {
        if let Some(id) = marker_of(text) {
            self.report(&Event::Marker { id });
            return Next::GoOn;
        }
        match text.trim() {
            // Claude Code runs its own commands: no prompt, no `UserPromptSubmit`.
            EXIT => return self.exit(),
            CLEAR => self.clear(),
            FORK => self.fork(),
            command if command.starts_with(RESUME) => {
                self.resume(command[RESUME.len()..].trim());
            }
            _ => self.prompt(text),
        }
        Next::GoOn
    }

    /// A prompt: it runs the `UserPromptSubmit` hook, which may block it, as Claude Code
    /// erases a blocked prompt from its session; otherwise it writes it in its transcript, reads it, and
    /// works on it unless the test has it swallow it. A prompt that comes while it works joins
    /// its turn, as Claude Code's does after a tool call.
    fn prompt(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let printed = self.hook("UserPromptSubmit", &json!({"prompt": text}));
        self.show(&printed);
        if printed.block_reason().is_some() {
            // Claude Code puts a blocked prompt back in its input line.
            self.screen.set_typed(text.to_owned());
            self.report(&Event::PromptBlocked {
                text: text.to_owned(),
            });
            return;
        }
        if let Some(session) = &self.session {
            self.transcript
                .prompt(session, text, self.turns.under_way());
        }
        // Read before it reports the prompt, so that a test that waits for that report and
        // then stops the swallowing finds the prompt swallowed.
        let swallowed = self.directory.swallow().exists();
        self.screen.log(format!("prompt: {text}"));
        self.report(&Event::PromptReceived {
            text: (self.app.read_prompt)(text),
        });
        if !swallowed && self.turns.start() {
            self.screen.working();
            self.report(&Event::TurnStarted);
        }
    }

    /// The `Stop` hook: a block decision keeps it working on the reason, as Claude Code does,
    /// and its next stop says it was blocked; otherwise it is idle.
    fn end_turn(&mut self) {
        if let Some(session) = &self.session {
            self.transcript.answer(session);
        }
        let background: Vec<_> = self
            .background
            .iter()
            .enumerate()
            .map(|(n, kind)| json!({"id": format!("bg{n}"), "type": kind, "status": "running"}))
            .collect();
        let printed = self.hook(
            "Stop",
            &json!({"stop_hook_active": self.blocked, "background_tasks": background}),
        );
        let blocked = printed.block_reason();
        self.blocked = blocked.is_some();
        if let Some(reason) = &blocked {
            self.screen.log(format!("stop blocked: {reason}"));
        } else {
            self.screen.log("turn ended");
            self.turns.end();
            self.screen.idle();
        }
        self.report(&Event::TurnEnded { blocked });
    }

    /// `/clear`: the session ends, and a new one starts in the same process.
    fn clear(&mut self) {
        self.hook("SessionEnd", &json!({"reason": "clear"}));
        self.session_starts(launch::new_session_id(), Start::Clear);
        self.screen
            .log(format!("cleared: session {}", self.session()));
        self.turns.end();
        self.screen.idle();
    }

    /// `/bg`, which Claude Code 2.1.318 runs: it forks the session into a new one, which its
    /// background daemon runs, in a process whose environment names the pane still. The old
    /// session's transcript names the new one. The pane shows the new session.
    fn fork(&mut self) {
        let fork = launch::new_session_id();
        if let Some(old) = &self.session {
            self.transcript.continued_in(old, &fork);
        }
        self.session_starts(fork, Start::Fork);
        self.screen
            .log(format!("forked: session {}", self.session()));
        self.turns.end();
        self.screen.idle();
    }

    /// Claude Code's `/resume <session>`: it goes on in `session`, whose `SessionStart` runs
    /// unless the test keeps it unheard ([`Directory::unreported_resume`]).
    fn resume(&mut self, session: &str) {
        // The switch is read before the report: a test that sets the next resume's switch once
        // it sees this one's must not reach this one.
        let unreported = self.directory.unreported_resume();
        let switch = std::fs::read_to_string(&unreported);
        if switch.is_ok() {
            std::fs::remove_file(&unreported).expect("the switch is the agent's to remove");
        }
        self.report(&Event::ResumeReceived {
            session: session.to_owned(),
        });
        let Ok(late) = switch else {
            self.session_starts(session.to_owned(), Start::Resume);
            return;
        };
        if let Ok(late) = late.trim().parse() {
            std::thread::sleep(Duration::from_millis(late));
            self.session_starts(session.to_owned(), Start::Resume);
        } else {
            self.transcript.start(session);
            self.session = Some(session.to_owned());
            self.start = Start::Resume;
        }
    }

    fn session(&self) -> &str {
        self.session.as_deref().unwrap_or_default()
    }

    /// The session ends, by `/exit` or Ctrl-D in its pane or by the test's command, and the
    /// agent exits.
    fn exit(&mut self) -> Next {
        self.hook("SessionEnd", &json!({"reason": "prompt_input_exit"}));
        self.screen.exits();
        self.screen.log("exited");
        self.report(&Event::Exited);
        Next::Exit
    }

    fn hear(&mut self, news: News) -> Next {
        match news {
            News::Status(status) => self.turns.herdr_reported(&status),
            News::Detected { released } => {
                if released {
                    self.turns.released();
                    self.report(&Event::Released);
                }
                self.screen.show_title();
            }
        }
        Next::GoOn
    }

    /// Does what `command` says, then answers it.
    fn obey(&mut self, command: Command) -> Next {
        let next = if self.obey_turn(&command) || self.obey_display(&command) {
            Next::GoOn
        } else {
            self.obey_session(&command)
        };
        // A turn the command lets end ends before the answer.
        if next == Next::GoOn {
            self.settle();
        }
        self.report(&Event::Obeyed { command });
        next
    }

    /// Obeys `command` if it is about its turns: whether it was.
    fn obey_turn(&mut self, command: &Command) -> bool {
        match command {
            Command::EndTurn => self.end_turn(),
            Command::HoldTurns { hold } => self.turns.hold(*hold),
            Command::StopFailure { error } => {
                self.hook(
                    "StopFailure",
                    &json!({"error": error, "error_details": null}),
                );
                self.screen.log(format!("turn failed: {error}"));
                self.turns.end();
                self.screen.idle();
            }
            Command::PermissionPrompt => {
                self.hook(
                    "Notification",
                    &json!({
                        "message": "Claude needs your permission to use Bash",
                        "notification_type": "permission_prompt",
                    }),
                );
                self.screen.asking();
            }
            Command::StartBackground { kind } => {
                self.background.push(kind.clone());
                self.screen.log(format!("background {kind} started"));
            }
            Command::FinishBackground => {
                self.background.clear();
                self.prompt(BACKGROUND_REPORT);
            }
            _ => return false,
        }
        true
    }

    /// Obeys `command` if it is about what it shows: whether it was.
    fn obey_display(&mut self, command: &Command) -> bool {
        match command {
            Command::Say { text } => self.screen.log(reply::render(text, self.hyperlinks)),
            Command::Suggest { text } => self.screen.suggest(text.clone()),
            Command::ShowTitle { title } => self.screen.set_title(title.clone()),
            Command::ShowWorking { working: true } => self.screen.working(),
            Command::ShowWorking { working: false } => self.screen.idle(),
            Command::ShowScreen { text } => self.screen.set_shown(text.clone()),
            _ => return false,
        }
        true
    }

    /// Obeys `command`, about its session.
    fn obey_session(&mut self, command: &Command) -> Next {
        match command {
            Command::ReportSession => {
                self.report_session();
                self.screen.log("session reported");
            }
            Command::Release => {
                self.turns.released();
                // Herdr reads its title from now on.
                self.screen.show_title();
            }
            Command::Exit => return self.exit(),
            Command::Crash => std::process::exit(1),
            command => panic!("the fake agent got a fork's command: {command:?}"),
        }
        Next::GoOn
    }
}

/// Edits the input line `typed` with `key`, which neither submits it nor exits.
fn edit(typed: &mut String, key: Key) {
    match key {
        Key::Char(c) => typed.push(c),
        Key::Backspace => {
            typed.pop();
        }
        Key::Clear => typed.clear(),
        Key::Enter | Key::Exit => {}
    }
}

/// The `herdr` the agent reports to Herdr with.
///
/// # Panics
/// Panics when its environment names none: the user's, on `PATH`, is never the one.
fn herdr_cli() -> PathBuf {
    std::env::var_os(variables::HERDR)
        .or_else(|| std::env::var_os("HERDR_BIN_PATH"))
        .map(PathBuf::from)
        .expect("the herdr binary, in FAKE_AGENT_HERDR or HERDR_BIN_PATH")
}
