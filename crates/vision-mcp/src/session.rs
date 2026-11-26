use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use app_sandbox::{AppSandbox, Restart, Stopped};
use eyre::{ContextCompat, Result, bail, ensure};
use serde::Serialize;
use serde_json::Value;
use vision_display::Display;
use vision_signal::acknowledgement_request;

use crate::frames::{FrameWatch, Stalled};
use crate::screen::{changed_rows, numbered};

const WINDOW_TITLE: &str = "Herdr";

const WINDOW_GUARD: Duration = Duration::from_secs(30);

/// The key that pastes into a terminal of herdr-app (`terminal.paste` in its `keys.json`).
const TERMINAL_PASTE: &str = "ctrl+shift+v";

const ACKNOWLEDGEMENT_GUARD: Duration = Duration::from_secs(30);

const CHANGE_GUARD: Duration = Duration::from_secs(1);

const QUIET_GUARD: Duration = Duration::from_secs(2);

const QUIET: Duration = Duration::from_millis(250);

const SCREEN_POLL: Duration = Duration::from_millis(20);

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
    /// The zero-based rows whose text differs from the previous screen of this terminal the
    /// session returned; `None` for the first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_rows: Option<Vec<usize>>,
}

impl Screen {
    /// The text as the tools return it: numbered rows, trimmed, with empty runs folded.
    pub fn numbered(&self) -> String {
        numbered(&self.text)
    }
}

const WINDOW_CLOSED: &str = "the window is closed since a reboot: restart the window first";

pub struct Vision {
    display: Option<Display>,
    sandbox: AppSandbox,
    frames: FrameWatch,
    requests: u64,
    returned: Mutex<HashMap<String, String>>,
}

impl Vision {
    /// Watch `sandbox`'s terminals and connect to its display, once its window fills it. The
    /// app's own setup, its spaces and the programs in them, goes before or after.
    pub fn start(sandbox: AppSandbox) -> Result<Self> {
        let frames = FrameWatch::start(&sandbox)?;
        let display = connect(&sandbox)?;
        Ok(Self {
            display: Some(display),
            sandbox,
            frames,
            requests: 0,
            returned: Mutex::default(),
        })
    }

    pub fn sandbox(&self) -> &AppSandbox {
        &self.sandbox
    }

    /// Restart the app's core, its window, or both, as `what` says
    /// ([`AppSandbox::restart`]), and see and drive the app again: the session follows the new
    /// core's terminals, and connects to the window once it opens again. After a core restart
    /// and a window's, it returns once the window's pixels settled, or after two seconds. A
    /// kept terminal's shell comes back a moment later, and its view in the window drops keys
    /// until it reattached, which no event says: type into it once the window shows its shell,
    /// or through `hc send`. While a
    /// [`Restart::Reboot`] keeps the window closed, the tools that need it fail, and `screen`
    /// and `wait_for` read a terminal named by its address.
    pub fn restart(&mut self, what: Restart) -> Result<()> {
        let restarted = self.sandbox.restart(what);
        if what == Restart::Reboot && !self.sandbox.window_open() {
            self.display = None;
        }
        restarted?;
        match what {
            Restart::Core => self.frames = FrameWatch::start(&self.sandbox)?,
            Restart::Reboot => {
                self.frames = FrameWatch::start(&self.sandbox)?;
                return Ok(());
            }
            Restart::Window => self.display = Some(connect(&self.sandbox)?),
        }
        // A kept terminal may still show the core lost for a moment.
        quiet(self.display()?)?;
        Ok(())
    }

    fn display(&self) -> Result<&Display> {
        self.display.as_ref().context(WINDOW_CLOSED)
    }

    /// The terminal in the focused pane, by its hc address; `None` when the focused pane shows
    /// none, as a mod's.
    pub fn focused_terminal(&self) -> Result<Option<String>> {
        self.display()?;
        let status = self.sandbox.herdr_app(&["status"])?;
        let Some(pane) = status["focused_pane"].as_str() else {
            return Ok(None);
        };
        let listed = self.sandbox.herdr_app(&["ls"])?;
        Ok(terminal_of_pane(&listed, pane))
    }

    fn terminal(&self, terminal: Option<&str>) -> Result<String> {
        match terminal {
            Some(terminal) => Ok(terminal.to_owned()),
            None => self.focused_terminal()?.context(
                "the focused pane shows no terminal (a mod's, or none): name a terminal, or take a \
                 screenshot",
            ),
        }
    }

    /// The screen of `terminal`, or of the focused one, as it is now, with the rows that
    /// changed since the session last returned one of this terminal.
    pub fn screen(&self, terminal: Option<&str>) -> Result<Screen> {
        let screen = self.read(terminal)?;
        Ok(self.returned(screen))
    }

    fn returned(&self, mut screen: Screen) -> Screen {
        screen.changed_rows = self
            .returned
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(screen.terminal.clone(), screen.text.clone())
            .map(|before| changed_rows(&before, &screen.text));
        screen
    }

    fn read(&self, terminal: Option<&str>) -> Result<Screen> {
        let terminal = self.terminal(terminal)?;
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
            changed_rows: None,
        })
    }

    /// Give `input`, and return once the app reacted: with the frame that acknowledges it, when
    /// the focused terminal's program speaks vision-signal and keeps the focus, and no held drag
    /// keeps the button down; otherwise once the
    /// window's pixels changed and settled. Returns the focused terminal's screen then, if a
    /// terminal has the focus.
    pub fn act(&mut self, input: &Input<'_>) -> Result<(Reaction, Option<Screen>)> {
        self.react(Some(input))
    }

    /// Wait for the app to show a change no input of the session made, such as keys an app's
    /// own tools send its terminal, and return the focused terminal's screen then, if a
    /// terminal has the focus. When its program speaks vision-signal, the reaction is the frame
    /// that acknowledges a request pasted now, and no other part of the window is waited on
    /// (as [`Self::settle_pixels`] does):
    /// the program reads the request after what reached its terminal before, and shows a
    /// change it learns another way (a socket, a file) only if it handled it before. Otherwise,
    /// and while a held drag keeps the button down, the window's pixels must change within a
    /// second, then stay quiet: `changed: false` says the window showed the change before the
    /// call, or never did. [`Self::settle_pixels`] waits on the whole window.
    pub fn settle(&mut self) -> Result<(Reaction, Option<Screen>)> {
        self.react(None)
    }

    /// [`Self::settle`] on the whole window's pixels, whatever has the focus: for a change
    /// outside the focused terminal, such as one an app's own tools make to herdr-app's panels
    /// or to another pane, or one a vision-signal program learns another way than its terminal.
    pub fn settle_pixels(&mut self) -> Result<(Reaction, Option<Screen>)> {
        let before = self.display()?.fingerprint()?;
        let reaction = self.pixels(before)?;
        Ok((reaction, self.focused_screen()?))
    }

    fn react(&mut self, input: Option<&Input<'_>>) -> Result<(Reaction, Option<Screen>)> {
        let before = self.display()?.fingerprint()?;
        let speaking = self
            .focused_terminal()?
            .filter(|terminal| self.frames.frames().latest(terminal).is_some());
        if let Some(input) = input {
            self.give(input)?;
        }
        // An input that moved the focus, such as a click on another pane, leaves the request
        // nobody to read it. The paste key would leave a held drag on herdr-app's panels unable
        // to drop.
        let acknowledged = match speaking {
            Some(terminal)
                if !self.display()?.held()
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
        Ok((reaction, self.focused_screen()?))
    }

    fn focused_screen(&self) -> Result<Option<Screen>> {
        match self.focused_terminal()? {
            Some(terminal) => Ok(Some(self.screen(Some(&terminal))?)),
            None => Ok(None),
        }
    }

    fn give(&self, input: &Input<'_>) -> Result<()> {
        let display = self.display()?;
        match input {
            Input::Keys(keys) => display.keys(keys),
            Input::Text(text) => display.type_text(text),
            Input::Paste(text) => display.paste(text, TERMINAL_PASTE),
            Input::Click { x, y, modifiers } => display.click(*x, *y, modifiers),
            Input::Drag {
                from,
                to,
                modifiers,
                hold,
            } => display.drag(*from, *to, modifiers, *hold),
        }
    }

    fn acknowledge(&mut self, terminal: &str) -> Result<Option<Reaction>> {
        self.requests += 1;
        let request = self.requests;
        self.display()?
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
            Err(error) => bail!("{terminal} did not acknowledge request {request}: {error}"),
        }
    }

    fn pixels(&self, before: u64) -> Result<Reaction> {
        let display = self.display()?;
        let deadline = Instant::now() + CHANGE_GUARD;
        let changed = loop {
            if display.fingerprint()? != before {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(SCREEN_POLL);
        };
        let settled = quiet(display)?;
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
            let screen = self.read(Some(&terminal))?;
            if screen.text.contains(text) != absent {
                return Ok(self.returned(screen));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let screen = self.returned(screen);
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
                    match self
                        .frames
                        .frames()
                        .wait(&terminal, remaining, |marker| marker.frame > seen)
                    {
                        Ok(_) | Err(Stalled::Guard(_) | Stalled::Left) => {}
                        Err(stalled) => {
                            let last = self.read(Some(&terminal))?;
                            if last.text.contains(text) != absent {
                                return Ok(self.returned(last));
                            }
                            let last = self.returned(last);
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
        self.display()?;
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

fn quiet(display: &Display) -> Result<bool> {
    let deadline = Instant::now() + QUIET_GUARD;
    let mut last = display.fingerprint()?;
    let mut since = Instant::now();
    loop {
        if since.elapsed() >= QUIET {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(SCREEN_POLL);
        let now = display.fingerprint()?;
        if now != last {
            last = now;
            since = Instant::now();
        }
    }
}

fn connect(sandbox: &AppSandbox) -> Result<Display> {
    Display::connect(&sandbox.display_socket()?, WINDOW_TITLE, WINDOW_GUARD).map_err(|error| {
        let log = std::fs::read_to_string(sandbox.directory().join("app.log")).unwrap_or_default();
        error.wrap_err(format!("the app's window did not show; its log:\n{log}"))
    })
}

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
