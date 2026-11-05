//! What crosses the event socket between a test and its fake agents: one JSON line per
//! [`Event`] from a fake agent, one per [`Command`] from the test.

use serde::{Deserialize, Serialize};

/// Which fake agent a connection is.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Role {
    /// A fake agent in a pane, by its name: [`crate::variables::NAME`], or else its pane.
    Agent { name: String },
    /// A fork Claude Code runs in print mode (`-p --resume <parent> --fork-session`), which
    /// takes the session `session`.
    Fork { session: String },
}

impl Role {
    /// The fake agent named `name`, such as the pane it runs in.
    pub fn agent(name: impl Into<String>) -> Self {
        Self::Agent { name: name.into() }
    }

    /// The fork that takes the session `session`.
    pub fn fork(session: impl Into<String>) -> Self {
        Self::Fork {
            session: session.into(),
        }
    }
}

/// What a fake agent reports.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// The first line of a connection: who the fake agent is.
    Hello { role: Role },
    /// It started with these arguments, and its session started.
    Started { args: Vec<String> },
    /// Its session `session` started from `start` (`startup`, `resume`, `clear` or `fork`),
    /// in Herdr 0.9, in herdr-app and in a plain terminal alike. The fake agent reports it once
    /// its `SessionStart` hooks ran: the app's, then, in Herdr 0.9, Herdr's integration's
    /// ([`Event::SessionReported`] first). A resume the test keeps unheard
    /// ([`crate::Directory::unreported_resume`]) reports none. A real Claude Code's observing
    /// hook ([`crate::observing_hooks`]) is one of its `SessionStart` hooks, which Claude Code
    /// runs together: it reports the start in no order with the app's hook.
    SessionStarted { session: String, start: String },
    /// It reported its session to Herdr as Claude's Herdr integration does, from `start`:
    /// `startup`, `resume`, `clear` or `fork`. The fake agent reports it in Herdr 0.9 alone; a
    /// real Claude Code's observing hook, in any terminal.
    SessionReported { session: String, start: String },
    /// It reported its state to Herdr itself ([`crate::variables::REPORT_STATE`]).
    StateReported { state: String },
    /// Herdr released it: Herdr reads its title from now on.
    Released,
    /// It read a prompt submitted in its pane, or the one it started with, as the app's
    /// [`crate::App::read_prompt`] gives it.
    PromptReceived { text: String },
    /// Its `UserPromptSubmit` hook blocked the prompt `text`: it puts it back in its input
    /// line, as Claude Code does, and does not work on it.
    PromptBlocked { text: String },
    /// It read Claude Code's own `/resume <session>`.
    ResumeReceived { session: String },
    /// It read the marker `id` ([`crate::Events::mark`]): it read what came in its pane
    /// before.
    Marker { id: u64 },
    /// It started a turn, and shows itself working.
    TurnStarted,
    /// It ended its turn: its `Stop` hook printed a block decision with this reason, and it
    /// works on, or it did not, and it is idle.
    TurnEnded { blocked: Option<String> },
    /// A key changed the text typed in its input line, which it reports while it suggests a
    /// prompt ([`Command::Suggest`]).
    InputLine { text: String },
    /// It did what `command` says: its answer to each command, once the command's effects and
    /// their events are done.
    Obeyed { command: Command },
    /// It exits: `/exit` or Ctrl-D in its pane, or [`Command::Exit`].
    Exited,
    /// A fork started on `prompt`, from the session `parent`.
    ForkStarted { parent: String, prompt: String },
    /// The connection closed: the fake agent's process ended, however it ended. The test's
    /// side reports it; no fake agent writes it.
    Disconnected,
}

/// What a test has a fake agent do, as Claude Code would. A fake agent answers each with
/// [`Event::Obeyed`], except [`Command::Crash`], after which it is gone.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    /// End the turn: the `Stop` hook, reported as [`Event::TurnEnded`]. A fork ends its turn
    /// and exits.
    EndTurn,
    /// End each turn only on [`Command::EndTurn`] (`hold`), or by itself once Herdr saw it
    /// work ([`crate::variables::TURNS`] says which from its start).
    HoldTurns { hold: bool },
    /// The turn ends on an API error: the `StopFailure` hook, then idle.
    StopFailure { error: String },
    /// Ask the user for a permission: the `Notification` hook of a permission prompt, and
    /// Claude Code's form on its screen.
    PermissionPrompt,
    /// Report its session to Herdr again, as at its start.
    ReportSession,
    /// Herdr reads its title from now on, as if Herdr had released it.
    Release,
    /// Start background work of `kind`, such as `shell` or `subagent`, which its `Stop` hooks
    /// list as running until [`Command::FinishBackground`].
    StartBackground { kind: String },
    /// Its background work ends: Claude Code submits the end's report, a turn it works on.
    FinishBackground,
    /// Write `text` as its reply, its Markdown links drawn as Claude Code draws them.
    Say { text: String },
    /// Suggest `text` as the next prompt: its input line shows it dim while nothing is typed
    /// there, until a submit.
    Suggest { text: String },
    /// Show `title` outside its turns, for Herdr's detection to read.
    ShowTitle { title: String },
    /// Show itself working when `working`, as Claude Code does: a spinner title, and OSC 7501
    /// `working` under herdr-app; idle again otherwise. It starts and ends no turn and runs no
    /// hook: the agent at work on what no prompt of the test started. A turn's start or end,
    /// `/clear` and `/bg` show their own state over it.
    ShowWorking { working: bool },
    /// Show `text` as its whole screen while nothing is typed in it.
    ShowScreen { text: String },
    /// Exit: the `SessionEnd` hook, then [`Event::Exited`], and it ends.
    Exit,
    /// Exit at once with a failure, without a hook or a result, as a Claude Code that crashed.
    Crash,
    /// A fork calls the tool `name`, such as an MCP tool: its output shows the call and its
    /// result.
    CallTool { name: String },
}

/// One event, and the fake agent that reported it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reported {
    pub from: Role,
    pub event: Event,
}

/// The prompt that a fake agent reads as the marker `id`: it reports [`Event::Marker`] and
/// does nothing else with it.
pub(crate) fn marker_prompt(id: u64) -> String {
    format!("{MARKER} {id}")
}

/// The marker a prompt is, if it is one.
pub(crate) fn marker_of(prompt: &str) -> Option<u64> {
    prompt.trim().strip_prefix(MARKER)?.trim().parse().ok()
}

const MARKER: &str = "::fake-agent-marker";
