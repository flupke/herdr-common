//! The frames the sandbox's terminals paint, as herdr-core reports them: an app that speaks
//! vision-signal ends each painted frame with a [`FrameMarker`] title, which `hc events`
//! reports once the core has read the frame. Waiting on these events, a session reads a
//! screen only once the app has reacted.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use app_sandbox::AppSandbox;
use eyre::{ContextCompat, Result, WrapErr};
use serde_json::Value;
use vision_signal::FrameMarker;

/// How long [`FrameWatch::start`] waits for the core's first snapshot: a guard that fires only
/// on a failure.
const SNAPSHOT_GUARD: Duration = Duration::from_secs(30);

/// What a terminal last reported.
#[derive(Clone, Copy, Debug, Default)]
struct Terminal {
    /// The latest frame marker: `None` before the first frame, and once another title replaced
    /// it, as an app that quits sets (vision-signal's `FrameMarker::RESET`). hc reports a new
    /// foreground program after the frames it painted at times, so that is no sign.
    latest: Option<FrameMarker>,
    /// Whether the terminal's program ended, or the terminal went.
    ended: bool,
}

/// Why a wait for a frame ended without one.
#[derive(Debug)]
pub(crate) enum Stalled {
    /// The terminal's program ended.
    Ended,
    /// The program that painted the frames set another title: it quit.
    Left,
    /// `hc events` stopped.
    Stopped(String),
    /// No such frame came within the guard; the latest one, if any.
    Guard(Option<FrameMarker>),
}

impl std::error::Error for Stalled {}

impl std::fmt::Display for Stalled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ended => write!(formatter, "the terminal's program ended"),
            Self::Left => write!(formatter, "the program that painted frames left"),
            Self::Stopped(reason) => write!(formatter, "hc events stopped: {reason}"),
            Self::Guard(None) => write!(formatter, "nothing came in time, and no frame"),
            Self::Guard(Some(marker)) => write!(
                formatter,
                "nothing came in time; the latest frame is {} (acknowledging request {})",
                marker.frame, marker.acknowledged
            ),
        }
    }
}

#[derive(Default)]
struct State {
    terminals: HashMap<String, Terminal>,
    /// Whether the first snapshot came.
    snapshot: bool,
    stopped: Option<String>,
}

/// The frames of the sandbox's terminals.
#[derive(Default)]
pub(crate) struct Frames {
    state: Mutex<State>,
    changed: Condvar,
}

impl Frames {
    /// Take one line of `hc events` into account: a snapshot of the terminals, the fresh one
    /// that follows events it missed, or an event.
    fn observe(&self, line: &str) {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let snapshot = if message["method"] == "missed" {
            &message["params"]["terminals"]
        } else {
            &message["terminals"]
        };
        let mut state = self.lock();
        if let Some(terminals) = snapshot.as_array() {
            state.snapshot = true;
            for terminal in terminals {
                let Some(address) = terminal["address"].as_str() else {
                    continue;
                };
                let entry = state.terminals.entry(address.to_owned()).or_default();
                entry.ended = terminal["state"] == "finished";
                entry.latest = terminal["title"].as_str().and_then(FrameMarker::parse);
            }
        } else if message["method"] == "event" {
            let event = &message["params"];
            let Some(address) = event["terminal"].as_str() else {
                return;
            };
            let entry = state.terminals.entry(address.to_owned()).or_default();
            match event["type"].as_str() {
                Some("title") => {
                    entry.latest = event["data"]["title"].as_str().and_then(FrameMarker::parse);
                }
                Some("finished" | "lost" | "removed") => entry.ended = true,
                Some("restarted") => *entry = Terminal::default(),
                _ => return,
            }
        } else {
            return;
        }
        self.changed.notify_all();
    }

    /// `hc events` stopped, so no frame comes any more.
    fn stop(&self, reason: String) {
        self.lock().stopped.get_or_insert(reason);
        self.changed.notify_all();
    }

    /// The latest frame marker of `terminal`: `None` while it never painted one, as for a
    /// program that does not speak vision-signal.
    pub(crate) fn latest(&self, terminal: &str) -> Option<FrameMarker> {
        self.lock().terminals.get(terminal)?.latest
    }

    /// Wait for a frame of `terminal` that satisfies `until`, the latest one first, for at most
    /// `guard`. A terminal whose program paints no marker, or no longer, ends the wait.
    pub(crate) fn wait(
        &self,
        terminal: &str,
        guard: Duration,
        mut until: impl FnMut(&FrameMarker) -> bool,
    ) -> Result<FrameMarker, Stalled> {
        let deadline = Instant::now() + guard;
        let mut state = self.lock();
        loop {
            let reported = state.terminals.get(terminal).copied().unwrap_or_default();
            if let Some(marker) = reported.latest.filter(|marker| until(marker)) {
                return Ok(marker);
            }
            if reported.ended {
                return Err(Stalled::Ended);
            }
            if reported.latest.is_none() {
                return Err(Stalled::Left);
            }
            if let Some(reason) = &state.stopped {
                return Err(Stalled::Stopped(reason.clone()));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Stalled::Guard(reported.latest));
            }
            state = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Wait until the first snapshot came, for at most `guard`.
    fn wait_snapshot(&self, guard: Duration) -> Result<()> {
        let state = self
            .changed
            .wait_timeout_while(self.lock(), guard, |state| {
                !state.snapshot && state.stopped.is_none()
            })
            .unwrap_or_else(PoisonError::into_inner)
            .0;
        if let Some(reason) = &state.stopped {
            eyre::bail!("hc events stopped before its first snapshot: {reason}");
        }
        eyre::ensure!(
            state.snapshot,
            "hc events gave no snapshot within {guard:?}"
        );
        Ok(())
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `hc events` in the sandbox, which feeds [`Frames`] until dropped.
pub(crate) struct FrameWatch {
    frames: Arc<Frames>,
    /// In bubblewrap, the nsenter that started `hc events`: its end leaves `hc events` running
    /// until the sandbox stops, and the thread that reads it ends then.
    events: Child,
}

impl FrameWatch {
    /// Follow the events of the core of `sandbox`, and return once its first snapshot came: no
    /// marker painted since goes unseen.
    pub(crate) fn start(sandbox: &AppSandbox) -> Result<Self> {
        let mut events = sandbox
            .command(&sandbox.programs().hc)
            .args(["events", "--json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .wrap_err("cannot run hc events")?;
        let stdout = events.stdout.take().context("standard output is piped")?;
        let frames = Arc::new(Frames::default());
        let observer = Arc::clone(&frames);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => observer.observe(&line),
                    Err(error) => return observer.stop(error.to_string()),
                }
            }
            observer.stop("it ended".into());
        });
        let watch = Self { frames, events };
        watch.frames.wait_snapshot(SNAPSHOT_GUARD)?;
        Ok(watch)
    }

    pub(crate) fn frames(&self) -> &Frames {
        &self.frames
    }
}

impl Drop for FrameWatch {
    fn drop(&mut self) {
        let _ = self.events.kill();
        let _ = self.events.wait();
    }
}

#[cfg(test)]
#[path = "frames.tests.rs"]
mod tests;
