//! A vision session: an app sandbox, seen through its terminals' screens and its window's
//! pixels, and driven through its display as its user would drive it.

use std::time::{Duration, Instant};

use app_sandbox::{AppSandbox, Stopped};
use eyre::{ContextCompat, Result, bail, ensure};
use serde::Serialize;
use serde_json::Value;
use vision_display::Display;
use vision_signal::acknowledgement_request;

use crate::frames::{FrameWatch, Stalled};
use crate::screen::numbered;

/// The title of herdr-app's window.
const WINDOW_TITLE: &str = "Herdr";

/// How long [`Vision::start`] waits for the app's window to fill the display.
const WINDOW_GUARD: Duration = Duration::from_secs(30);

/// The key that pastes into a terminal of herdr-app (`terminal.paste` in its `keys.json`).
const TERMINAL_PASTE: &str = "ctrl+shift+v";

/// How long an app that speaks vision-signal may take to acknowledge an input, busy as the
/// reviewer loading a large repository: a guard that fires only when it never does.
const ACKNOWLEDGEMENT_GUARD: Duration = Duration::from_secs(30);

/// How long an action on a screen that sends no event waits for the window's pixels to change,
/// before it returns the screen as it is: an action may change nothing.
const CHANGE_GUARD: Duration = Duration::from_secs(1);

/// How long such an action then waits for the pixels to stay unchanged for [`SETTLE_QUIET`],
/// before it returns the screen as it is: an animation never settles.
const SETTLE_GUARD: Duration = Duration::from_secs(2);

/// How long the pixels must stay unchanged to count as settled.
const SETTLE_QUIET: Duration = Duration::from_millis(250);

/// How often a screen that sends no event is looked at again: the window's pixels, or the text
/// of a terminal whose program does not speak vision-signal.
const SCREEN_POLL: Duration = Duration::from_millis(20);

/// An input, as the app's user would give it.
pub enum Input<'a> {
    /// Keys pressed in order, with their modifiers: `ctrl+shift+t`, `enter`, `a`.
    Keys(&'a [String]),
    /// Text typed, each character as its key; a newline as Enter.
    Text(&'a str),
    /// Text pasted into the focused terminal, as its user's clipboard pastes it.
    Paste(&'a str),
    /// A left click on the window's pixel, with modifiers held (`ctrl`, `shift`, `alt`).
    Click {
        x: u16,
        y: u16,
        modifiers: &'a [String],
    },
    /// A drag with the left button between two of the window's pixels, with modifiers held;
    /// with `hold`, the button stays down at the end, until a click, or a drag without `hold`,
    /// releases it.
    Drag {
        from: (u16, u16),
        to: (u16, u16),
        modifiers: &'a [String],
        hold: bool,
    },
}

/// How the app showed it reacted to an input.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reaction {
    /// The focused terminal's program speaks vision-signal, and painted this frame once it
    /// read the input.
    Acknowledged { frame: u64 },
    /// Nothing tells when the app read the input: the window's pixels changed within a second,
    /// or not, then stayed unchanged for a quarter of a second within two, or not.
    Pixels { changed: bool, settled: bool },
}

/// A terminal's screen.
#[derive(Clone, Debug, Serialize)]
pub struct Screen {
    /// The terminal's hc address, such as `t1`.
    pub terminal: String,
    /// The frame the screen holds at least, for a program that speaks vision-signal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<u64>,
    /// The visible rows, one line each.
    #[serde(skip)]
    pub text: String,
}

impl Screen {
    /// The text as the tools return it: numbered rows, trimmed, with empty runs folded.
    pub fn numbered(&self) -> String {
        numbered(&self.text)
    }
}

/// A running vision session.
pub struct Vision {
    /// Dropped before the sandbox: the connection leaves the display before it ends.
    display: Display,
    sandbox: AppSandbox,
    /// Dropped after the sandbox, which ends the `hc events` it reads.
    frames: FrameWatch,
    /// The latest acknowledgement request sent.
    requests: u64,
}

impl Vision {
    /// Watch `sandbox`'s terminals and connect to its display, once its window fills it. The
    /// app's own setup, its spaces and the programs in them, goes before or after.
    pub fn start(sandbox: AppSandbox) -> Result<Self> {
        let frames = FrameWatch::start(&sandbox)?;
        let display = Display::connect(&sandbox.display_socket()?, WINDOW_TITLE, WINDOW_GUARD)
            .map_err(|error| {
                let log = std::fs::read_to_string(sandbox.directory().join("app.log"))
                    .unwrap_or_default();
                error.wrap_err(format!("the app's window did not show; its log:\n{log}"))
            })?;
        Ok(Self {
            display,
            sandbox,
            frames,
            requests: 0,
        })
    }

    /// The sandbox, for what an app does there beside the vision tools.
    pub fn sandbox(&self) -> &AppSandbox {
        &self.sandbox
    }

    /// The terminal in the focused pane, by its hc address; `None` when the focused pane shows
    /// none, as a mod's.
    pub fn focused_terminal(&self) -> Result<Option<String>> {
        let status = self.sandbox.herdr_app(&["status"])?;
        let Some(pane) = status["focused_pane"].as_str() else {
            return Ok(None);
        };
        let listed = self.sandbox.herdr_app(&["ls"])?;
        Ok(terminal_of_pane(&listed, pane))
    }

    /// `terminal`, or the focused one when `None`.
    fn terminal(&self, terminal: Option<&str>) -> Result<String> {
        match terminal {
            Some(terminal) => Ok(terminal.to_owned()),
            None => self.focused_terminal()?.context(
                "the focused pane shows no terminal (a mod's, or none): name a terminal, or take a \
                 screenshot",
            ),
        }
    }

    /// The screen of `terminal`, or of the focused one, as it is now.
    pub fn screen(&self, terminal: Option<&str>) -> Result<Screen> {
        let terminal = self.terminal(terminal)?;
        // The marker comes first: the screen read after it holds that frame at least.
        let frame = self
            .frames
            .frames()
            .latest(&terminal)
            .map(|marker| marker.frame);
        let read = self.sandbox.hc(&["read", &terminal, "--json"])?;
        let text = read["text"]
            .as_str()
            .with_context(|| format!("hc read {terminal} gave no text: {read}"))?
            .to_owned();
        Ok(Screen {
            terminal,
            frame,
            text,
        })
    }

    /// Give `input`, and return once the app reacted: with the frame that acknowledges it, when
    /// the focused terminal's program speaks vision-signal and keeps the focus, and no held drag
    /// keeps the button down; otherwise once the
    /// window's pixels changed and settled. Returns the focused terminal's screen then, if a
    /// terminal has the focus.
    pub fn act(&mut self, input: &Input<'_>) -> Result<(Reaction, Option<Screen>)> {
        let speaking = self
            .focused_terminal()?
            .filter(|terminal| self.frames.frames().latest(terminal).is_some());
        let before = self.display.fingerprint()?;
        self.give(input)?;
        // An input that moved the focus, such as a click on another pane, leaves the request
        // nobody to read it. The paste key would leave a held drag on herdr-app's panels unable
        // to drop.
        let acknowledged = match speaking {
            Some(terminal)
                if !self.display.held()
                    && self.focused_terminal()?.as_deref() == Some(&terminal) =>
            {
                self.acknowledge(&terminal)?
            }
            _ => None,
        };
        let reaction = match acknowledged {
            Some(reaction) => reaction,
            None => self.pixels(before)?,
        };
        let screen = match self.focused_terminal()? {
            Some(terminal) => Some(self.screen(Some(&terminal))?),
            None => None,
        };
        Ok((reaction, screen))
    }

    fn give(&self, input: &Input<'_>) -> Result<()> {
        match input {
            Input::Keys(keys) => self.display.keys(keys),
            Input::Text(text) => self.display.type_text(text),
            Input::Paste(text) => self.display.paste(text, TERMINAL_PASTE),
            Input::Click { x, y, modifiers } => self.display.click(*x, *y, modifiers),
            Input::Drag {
                from,
                to,
                modifiers,
                hold,
            } => self.display.drag(*from, *to, modifiers, *hold),
        }
    }

    /// Paste an acknowledgement request after the input, and wait for the frame of `terminal`
    /// that names it: the app read the input before it. `None` when the program left meanwhile,
    /// as one the input made quit: its shell may show the request typed at its prompt.
    fn acknowledge(&mut self, terminal: &str) -> Result<Option<Reaction>> {
        self.requests += 1;
        let request = self.requests;
        self.display
            .paste(&acknowledgement_request(request), TERMINAL_PASTE)?;
        match self
            .frames
            .frames()
            .wait(terminal, ACKNOWLEDGEMENT_GUARD, |marker| {
                marker.acknowledged >= request
            }) {
            Ok(marker) => Ok(Some(Reaction::Acknowledged {
                frame: marker.frame,
            })),
            Err(Stalled::Left) => Ok(None),
            Err(error) => bail!("{terminal} did not acknowledge the input: {error}"),
        }
    }

    /// Wait for the window's pixels to differ from `before`, then to settle.
    fn pixels(&self, before: u64) -> Result<Reaction> {
        let deadline = Instant::now() + CHANGE_GUARD;
        let changed = loop {
            if self.display.fingerprint()? != before {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(SCREEN_POLL);
        };
        let deadline = Instant::now() + SETTLE_GUARD;
        let mut last = self.display.fingerprint()?;
        let mut since = Instant::now();
        let settled = loop {
            if since.elapsed() >= SETTLE_QUIET {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(SCREEN_POLL);
            let now = self.display.fingerprint()?;
            if now != last {
                last = now;
                since = Instant::now();
            }
        };
        Ok(Reaction::Pixels { changed, settled })
    }

    /// Wait until the screen of `terminal`, or of the focused one, shows `text`, or no longer
    /// does when `absent`, for at most `guard`, and return that screen. The screen is looked at
    /// once, then after each frame its program paints when it speaks vision-signal, else every
    /// few milliseconds.
    pub fn wait_for(
        &self,
        text: &str,
        absent: bool,
        guard: Duration,
        terminal: Option<&str>,
    ) -> Result<Screen> {
        ensure!(!text.is_empty(), "the text to wait for is empty");
        let terminal = self.terminal(terminal)?;
        let deadline = Instant::now() + guard;
        loop {
            let screen = self.screen(Some(&terminal))?;
            if screen.text.contains(text) != absent {
                return Ok(screen);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "{terminal} {} {text:?} after {guard:?}; its screen:\n{}",
                    if absent {
                        "still shows"
                    } else {
                        "does not show"
                    },
                    screen.numbered()
                );
            }
            match screen.frame {
                Some(seen) => {
                    // No frame within the guard: the next look shows the screen and fails. A
                    // program that left: the next looks poll.
                    match self
                        .frames
                        .frames()
                        .wait(&terminal, remaining, |marker| marker.frame > seen)
                    {
                        Ok(_) | Err(Stalled::Guard(_) | Stalled::Left) => {}
                        Err(stalled) => {
                            // What it printed as it ended shows with no frame.
                            let last = self.screen(Some(&terminal))?;
                            if last.text.contains(text) != absent {
                                return Ok(last);
                            }
                            bail!(
                                "{terminal} paints no more frames ({stalled}); its screen:\n{}",
                                last.numbered()
                            );
                        }
                    }
                }
                None => std::thread::sleep(SCREEN_POLL.min(remaining)),
            }
        }
    }

    /// A screenshot of the app's window, as PNG: herdr-app's own rendering of its last frame.
    pub fn screenshot(&self) -> Result<Vec<u8>> {
        self.sandbox.shot()
    }

    /// Leave the display, then stop the sandbox, and say what is left.
    pub fn stop(self) -> Result<Stopped> {
        let Self {
            display,
            sandbox,
            frames,
            ..
        } = self;
        drop(display);
        let stopped = sandbox.stop();
        drop(frames);
        stopped
    }
}

/// The terminal `herdr-app ls` lists in `pane`.
fn terminal_of_pane(listed: &Value, pane: &str) -> Option<String> {
    listed["spaces"]
        .as_array()?
        .iter()
        .filter_map(|space| space["tabs"].as_array())
        .flatten()
        .filter_map(|tab| tab["terminals"].as_array())
        .flatten()
        .find(|terminal| terminal["pane"] == pane)?["terminal"]
        .as_str()
        .map(str::to_owned)
}
