//! What the fake agent shows in its pane, as the agent it plays shows it, for Herdr's detection
//! to read and for whoever looks: its terminal title, its screen, and, on herdr-app, its state
//! in OSC 7501.

use std::io::Write as _;

/// The rule Claude Code draws around its input line: a permission form after the last one is
/// what Herdr reads as blocked.
const RULE: &str = "────────────────────────────────────────";

/// How many lines of what it did the screen keeps.
const LOG_LINES: usize = 200;

/// The agent the fake agent plays, by Herdr's name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Plays {
    Claude,
    Codex,
}

impl Plays {
    /// The agent [`crate::variables::PLAYS`] names: Claude Code unless it says `codex`.
    pub(crate) fn from_env() -> Self {
        match std::env::var(crate::variables::PLAYS).as_deref() {
            Ok("codex") => Self::Codex,
            _ => Self::Claude,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// The start of its input line: Claude Code's `❯` in grey and a no-break space, which an
    /// app's courier may wait for before it types, or Codex's `›`.
    fn prompt_glyph(self) -> &'static str {
        match self {
            Self::Claude => "\u{1b}[38;2;153;153;153m❯\u{1b}[0m\u{a0}",
            Self::Codex => "› ",
        }
    }
}

/// Where the screen reports the agent's state besides its title.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StateReport {
    /// Nowhere: Herdr 0.9 reads the title and the screen.
    None,
    /// In OSC 7501, as Claude Code does in a terminal that answers its probe: herdr-app's core.
    Osc,
}

/// The screen and the title of the fake agent.
pub(crate) struct Screen {
    plays: Plays,
    report: StateReport,
    /// What it did, a line each.
    log: Vec<String>,
    typed: String,
    suggestion: Option<String>,
    /// Whether it shows a permission form instead of its input line.
    asking: bool,
    working: bool,
    /// The name of its session, which its title shows.
    session_name: String,
    /// The title the test gave it, shown outside its turns.
    title: Option<String>,
    /// The screen the test gave it, shown while nothing is typed.
    shown: Option<String>,
}

impl Screen {
    pub(crate) fn new(plays: Plays, report: StateReport, session_name: String) -> Self {
        Self {
            plays,
            report,
            log: Vec::new(),
            typed: String::new(),
            suggestion: None,
            asking: false,
            working: false,
            session_name,
            title: None,
            shown: None,
        }
    }

    /// Adds `line` to what it did, and draws.
    pub(crate) fn log(&mut self, line: impl Into<String>) {
        self.log.push(line.into());
        let excess = self.log.len().saturating_sub(LOG_LINES);
        self.log.drain(..excess);
        self.draw();
    }

    pub(crate) fn typed(&self) -> &str {
        &self.typed
    }

    pub(crate) fn suggests(&self) -> bool {
        self.suggestion.is_some()
    }

    /// The input line holds `typed` from now on.
    pub(crate) fn set_typed(&mut self, typed: String) {
        self.typed = typed;
        self.draw();
    }

    /// Takes what is typed, to submit it: the suggestion ends.
    pub(crate) fn submit(&mut self) -> String {
        self.suggestion = None;
        let typed = std::mem::take(&mut self.typed);
        self.draw();
        typed
    }

    pub(crate) fn suggest(&mut self, text: String) {
        self.suggestion = Some(text);
        self.draw();
    }

    pub(crate) fn set_title(&mut self, title: String) {
        self.title = Some(title);
        self.show_title();
    }

    pub(crate) fn set_shown(&mut self, text: String) {
        self.shown = Some(text);
        self.draw();
    }

    pub(crate) fn name_session(&mut self, name: String) {
        self.session_name = name;
        self.show_title();
    }

    /// Shows the agent idle: its idle title, and its input line, which closes any permission
    /// form.
    pub(crate) fn idle(&mut self) {
        self.working = false;
        self.asking = false;
        self.draw();
        self.show_title();
        self.report_state("idle");
    }

    /// Shows the agent working: a title that starts with a spinner.
    pub(crate) fn working(&mut self) {
        self.working = true;
        self.asking = false;
        self.draw();
        self.show_title();
        self.report_state("working");
    }

    /// Shows the agent asking a permission: Claude Code's form under a rule.
    pub(crate) fn asking(&mut self) {
        self.asking = true;
        self.draw();
        self.report_state("blocked:kind=permission");
    }

    /// The agent exits: herdr-app's core hears it go.
    pub(crate) fn exits(&self) {
        self.report_state("clear");
    }

    /// Shows its title again: Herdr 0.9 drops a title set before it detected the agent.
    pub(crate) fn show_title(&self) {
        let name = if self.session_name.is_empty() {
            "Claude Code"
        } else {
            &self.session_name
        };
        let title = match (self.working, self.plays, &self.title) {
            (true, Plays::Claude, _) => format!("⠋ {name}"),
            (true, Plays::Codex, _) => "⠋ Working".to_owned(),
            (false, _, Some(title)) => title.clone(),
            (false, Plays::Claude, None) => format!("✳ {name}"),
            (false, Plays::Codex, None) => String::new(),
        };
        write(&format!("\x1b]0;{title}\x07"));
    }

    fn report_state(&self, state: &str) {
        if self.report == StateReport::Osc {
            let app = match self.plays {
                Plays::Claude => "claude-code",
                Plays::Codex => "codex",
            };
            write(&format!("\x1b]7501;state={state}:app={app}\x1b\\"));
        }
    }

    /// Draws the screen anew: what the test gave it while nothing is typed, or else what it
    /// did, then its permission form or its input line between rules.
    fn draw(&self) {
        let mut lines: Vec<String> = Vec::new();
        match &self.shown {
            Some(shown) if self.typed.is_empty() => lines.extend(shown.lines().map(Into::into)),
            _ => {
                lines.extend(self.log.iter().cloned());
                lines.push(RULE.to_owned());
                if self.asking {
                    lines.extend(PERMISSION_FORM.iter().map(|&line| line.to_owned()));
                } else {
                    lines.push(self.input_line());
                    lines.push(RULE.to_owned());
                }
            }
        }
        write(&format!("\x1b[2J\x1b[H{}", lines.join("\r\n")));
    }

    /// The input line: the text typed, else the suggestion, dim, as Claude Code draws its
    /// suggested prompt.
    fn input_line(&self) -> String {
        let glyph = self.plays.prompt_glyph();
        match (&self.suggestion, self.typed.as_str()) {
            (Some(suggestion), "") => format!("{glyph}\u{1b}[2m{suggestion}\u{1b}[22m"),
            (_, typed) => format!("{glyph}{}", typed.replace('\n', "\r\n")),
        }
    }
}

/// Claude Code's permission form.
const PERMISSION_FORM: [&str; 5] = [
    " Bash command",
    " Do you want to proceed?",
    " ❯ 1. Yes",
    "   2. No",
    " Esc to cancel",
];

/// Writes `text` to the pane: standard output.
pub(crate) fn write(text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}
