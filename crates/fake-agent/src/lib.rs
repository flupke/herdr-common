//! A fake agent for the apps' tests: a program that plays Claude Code, or Codex, in a pane of a
//! private Herdr or herdr-app, at the level the apps and Herdr see. It reads the prompts typed in
//! its pane, shows its state as Claude Code does (its title, its screen, OSC 7501 on herdr-app),
//! runs the app's Claude Code hooks with Claude Code's payloads, keeps Claude Code's transcript,
//! and reports its session to Herdr as Claude's Herdr integration does. It tells the test what it
//! does on an event socket, and takes the test's commands there ([`Events`]).
//!
//! The app runs it from a program of its own, such as a binary of its test workspace, or an ignored
//! test that a launcher script runs: [`run`] with the app's [`App`] and the arguments
//! Claude Code was launched with. The program is what Herdr launches as `claude` (or `codex`): a
//! link of that name to it, or a launcher script, first on the pane's `PATH`. What changes from
//! one test to the next reaches it in its environment ([`variables`]).
//!
//! The same program plays the rest of Claude Code that the apps' tests need: a fork Claude Code
//! runs in print mode (`-p --resume <parent> --fork-session`), and, as the hook of a real Claude
//! Code ([`observing_hooks`]), the reporter of what that agent does, so that one test body waits
//! on the same events whichever agent runs.

mod agent;
mod connection;
mod events;
mod fork;
mod herdr;
mod hooks;
mod launch;
mod observe;
mod protocol;
mod reply;
mod screen;
mod terminal;
mod transcript;
mod turns;

use std::path::{Path, PathBuf};

pub use events::{Events, GUARD};
pub use protocol::{Command, Event, Reported, Role};

/// What one app's fake agent does its own way. What changes from one test to the next goes in
/// [`variables`] instead.
#[derive(Clone, Copy, Debug)]
pub struct App {
    /// The arguments of the program [`variables::HOOK`] names that run the app's Claude Code
    /// hook, which reads Claude Code's payload on its standard input: `["hook"]` for
    /// `herdr-bowtie hook`.
    pub hook_arguments: &'static [&'static str],
    /// What the agent reads with a prompt, which it reports ([`Event::PromptReceived`],
    /// [`Event::ForkStarted`]): the prompt, or the prompt with what it names, such as a turn's
    /// files read as the prompt arrives.
    pub read_prompt: fn(&str) -> String,
}

impl Default for App {
    /// No hook arguments, and a prompt read as it is.
    fn default() -> Self {
        Self {
            hook_arguments: &[],
            read_prompt: str::to_owned,
        }
    }
}

/// The variables a fake agent reads, beside Herdr's own (`HERDR_PANE_ID`, `HERDR_SOCKET_PATH`),
/// herdr-app's (`HC_TERMINAL_ID`) and Claude Code's (`CLAUDE_CONFIG_DIR`).
pub mod variables {
    /// The test's event socket ([`crate::Events`]). Required.
    pub const EVENTS: &str = "FAKE_AGENT_EVENTS";
    /// The agent's name on the event socket ([`crate::Role::Agent`]); its Herdr pane, or its
    /// herdr-app terminal, by default.
    pub const NAME: &str = "FAKE_AGENT_NAME";
    /// Its [`Directory`](crate::Directory); the event socket's directory by default.
    pub const DIRECTORY: &str = "FAKE_AGENT_DIRECTORY";
    /// The `herdr` it reports to Herdr with; Herdr's own `HERDR_BIN_PATH` without it.
    pub const HERDR: &str = "FAKE_AGENT_HERDR";
    /// The program that runs the app's hooks, with [`crate::App::hook_arguments`]; without
    /// it, the agent runs no hook.
    pub const HOOK: &str = "FAKE_AGENT_HOOK";
    /// The agent it plays, by Herdr's name: `claude` (the default) or `codex`.
    pub const PLAYS: &str = "FAKE_AGENT_PLAYS";
    /// The session it starts, when its arguments name none; a new one when unset, and none
    /// when empty: it then runs no `SessionStart`, reports no session and keeps no transcript
    /// until a `/resume`.
    pub const SESSION: &str = "FAKE_AGENT_SESSION";
    /// `1`: in Herdr 0.9, it reports its state to Herdr itself, under
    /// [`crate::STATE_SOURCE`], until Herdr releases it. Otherwise Herdr detects it and reads
    /// its state from its title.
    pub const REPORT_STATE: &str = "FAKE_AGENT_REPORT_STATE";
    /// `auto`: each turn ends by itself once Herdr saw it, unless the test holds turns.
    /// Otherwise the test ends each turn ([`crate::Command::EndTurn`]).
    pub const TURNS: &str = "FAKE_AGENT_TURNS";
}

/// The source the fake agent reports its state to Herdr under, which a test names to release
/// it (`herdr pane release-agent --source`).
pub const STATE_SOURCE: &str = "fake-agent";

/// The program's first argument when a real Claude Code's hook runs it ([`observing_hooks`]).
const OBSERVE: &str = "observe";

/// Runs the fake agent as Claude Code launched with `args`: the agent in its pane, until it
/// exits; a fork, in print mode; or the reporter of a real Claude Code's hook, with
/// [`OBSERVE`] first.
///
/// # Panics
/// Panics when its environment lacks what it needs, such as the event socket.
pub fn run(app: &App, args: &[String]) {
    if args.first().map(String::as_str) == Some(OBSERVE) {
        observe::run(app);
    } else if args.iter().any(|arg| arg == "-p" || arg == "--print") {
        fork::run(app, args);
    } else {
        agent::run(app, args);
    }
}

/// The agent's name on the event socket: [`variables::NAME`], else its Herdr pane, else its
/// herdr-app terminal, else `agent`.
fn agent_name() -> String {
    std::env::var(variables::NAME)
        .or_else(|_| std::env::var("HERDR_PANE_ID"))
        .or_else(|_| std::env::var("HC_TERMINAL_ID"))
        .unwrap_or_else(|_| "agent".to_owned())
}

/// The hooks, in Claude Code's settings, with which a real Claude Code has `program` report the
/// prompts it reads and the sessions it starts and resumes, as the fake agent reports them, on
/// the event socket its environment names.
pub fn observing_hooks(program: &Path) -> serde_json::Value {
    // Claude Code runs a hook's command through the shell.
    let program = program.to_string_lossy().replace('\'', r"'\''");
    let hook = serde_json::json!([{"hooks": [{
        "type": "command",
        "command": format!("'{program}' {OBSERVE}"),
        "timeout": 10,
    }]}]);
    serde_json::json!({"SessionStart": hook, "UserPromptSubmit": hook})
}

/// The directory [`variables::DIRECTORY`] names: the switches that must reach the agent before
/// a prompt, which it reads as it reads the prompt, what its forks record, and Claude Code's
/// configuration when `CLAUDE_CONFIG_DIR` names none.
#[derive(Clone, Debug)]
pub struct Directory(PathBuf);

impl Directory {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    /// The directory [`variables::DIRECTORY`] names, or else the event socket's.
    ///
    /// # Panics
    /// Panics when the environment names neither.
    pub(crate) fn from_env() -> Self {
        let path = std::env::var_os(variables::DIRECTORY)
            .map(PathBuf::from)
            .or_else(|| {
                let events = PathBuf::from(std::env::var_os(variables::EVENTS)?);
                events.parent().map(Path::to_owned)
            })
            .expect("the fake agent's directory, or the test's event socket");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// While this file exists, the agent reads each prompt without starting a turn on it.
    pub fn swallow(&self) -> PathBuf {
        self.0.join("swallow")
    }

    /// This file keeps Herdr and the app's hooks from hearing of the agent's next `/resume`,
    /// as when Claude Code's session hooks fail; the agent removes it. When it holds a number
    /// of milliseconds, they hear of it that late instead, as when the hooks come late: the
    /// agent reads nothing typed meanwhile.
    pub fn unreported_resume(&self) -> PathBuf {
        self.0.join("unreported-resume")
    }

    /// What the fork of `session` recorded, by `kind`: its arguments (`args`), the names of
    /// its environment's variables (`env`), its settings (`settings`), and the prompt it read
    /// (`prompt`).
    pub fn fork_record(&self, session: &str, kind: &str) -> PathBuf {
        self.0.join("forks").join(format!("{session}.{kind}"))
    }

    /// Claude Code's configuration directory: `CLAUDE_CONFIG_DIR`, or else `claude-config`
    /// here, so that a fake agent never writes in the user's, and an app may link the agent as
    /// `claude` in this directory.
    pub fn configuration(&self) -> PathBuf {
        std::env::var_os("CLAUDE_CONFIG_DIR")
            .map_or_else(|| self.0.join("claude-config"), PathBuf::from)
    }
}

/// The project folder under Claude Code's configuration directory `configuration` where the
/// fake agent and its forks keep their sessions' transcripts, as Claude Code keeps a project's.
pub fn transcripts(configuration: &Path) -> PathBuf {
    configuration.join("projects/fake-agent")
}

/// The transcript that the fake agent and its forks keep for `session` under Claude Code's
/// configuration directory `configuration`, as Claude Code does.
pub fn transcript(configuration: &Path, session: &str) -> PathBuf {
    transcripts(configuration).join(format!("{session}.jsonl"))
}
