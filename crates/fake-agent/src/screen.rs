use std::io::Write as _;

/// The rule Claude Code draws around its input line: a permission form after the last one is
/// what Herdr reads as blocked.
const RULE: &str = "────────────────────────────────────────";

const LOG_LINES: usize = 200;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Plays {
    Claude,
    Codex,
}

impl Plays {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StateReport {
    /// Nowhere: Herdr 0.9 reads the title and the screen.
    None,
    /// In OSC 7501, as Claude Code does in a terminal that answers its probe: herdr-app's core.
    Osc,
}

pub(crate) struct Screen {
    plays: Plays,
    report: StateReport,
    log: Vec<String>,
    typed: String,
    suggestion: Option<String>,
    asking: bool,
    working: bool,
    session_name: String,
    title: Option<String>,
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

    pub(crate) fn set_typed(&mut self, typed: String) {
        self.typed = typed;
        self.draw();
    }

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

    pub(crate) fn idle(&mut self) {
        self.working = false;
        self.asking = false;
        self.draw();
        self.show_title();
        self.report_state("idle");
    }

    pub(crate) fn working(&mut self) {
        self.working = true;
        self.asking = false;
        self.draw();
        self.show_title();
        self.report_state("working");
    }

    pub(crate) fn asking(&mut self) {
        self.asking = true;
        self.draw();
        self.report_state("blocked:kind=permission");
    }

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

    fn input_line(&self) -> String {
        let glyph = self.plays.prompt_glyph();
        match (&self.suggestion, self.typed.as_str()) {
            (Some(suggestion), "") => format!("{glyph}\u{1b}[2m{suggestion}\u{1b}[22m"),
            (_, typed) => format!("{glyph}{}", typed.replace('\n', "\r\n")),
        }
    }
}

const PERMISSION_FORM: [&str; 5] = [
    " Bash command",
    " Do you want to proceed?",
    " ❯ 1. Yes",
    "   2. No",
    " Esc to cancel",
];

pub(crate) fn write(text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}
