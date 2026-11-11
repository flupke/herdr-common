//! What an app under visual testing tells the vision core (`crates/vision-mcp`) through the byte
//! stream of its terminal, so that herdr-core reports each signal after the
//! screen it follows.
//!
//! - Each painted frame ends with a terminal title, the [`FrameMarker`], that numbers it.
//!   herdr-core reports a changed title as an event, so the vision core waits on an event
//!   instead of a delay, and the screen it reads then holds that frame at least.
//! - The vision core pastes an [`acknowledgement_request`] after its own input. The app reads the
//!   input first, then the request, and names the request in the next frame's marker, which it
//!   paints even when nothing else changed: that frame shows the app's reaction to the input.
//!
//! What the app owes, besides the markers:
//!
//! - It turns on bracketed paste (`\x1b[?2004h`): the request comes as a paste, which the app
//!   takes out of its input with [`parse_acknowledgement_request`] before any widget sees it.
//! - It writes [`FrameMarker::RESET`] as it quits, so the vision core stops waiting on its frames
//!   in a terminal whose shell paints none.
//!
//! This is the one crate of herdr-common that an app links into what it builds, so it has no
//! dependency, and an app takes it as a `git` dependency (ADR 0003).

/// The terminal title a frame marker starts with.
const TITLE_PREFIX: &str = "vision";

/// The pasted text that asks for an acknowledgement, before its number.
const REQUEST_PREFIX: &str = "vision-acknowledge:";

/// What the app's terminal title says after a painted frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameMarker {
    /// The frames painted since the app started, this one included.
    pub frame: u64,
    /// The latest acknowledgement request the app read, 0 before the first.
    pub acknowledged: u64,
    /// The terminal's width in cells when the frame was painted.
    pub columns: u16,
    /// The terminal's height in cells when the frame was painted.
    pub rows: u16,
}

impl FrameMarker {
    /// The escape sequence that empties the terminal title, which carries no marker then: the
    /// app writes it as it quits.
    pub const RESET: &str = "\x1b]2;\x07";

    /// The terminal title that carries this marker.
    pub fn title(&self) -> String {
        format!(
            "{TITLE_PREFIX} frame={} ack={} size={}x{}",
            self.frame, self.acknowledged, self.columns, self.rows
        )
    }

    /// The escape sequence that sets the terminal title to [`Self::title`] (OSC 2): the app
    /// writes it after the frame's last byte.
    pub fn escape_sequence(&self) -> String {
        format!("\x1b]2;{}\x07", self.title())
    }

    /// The marker a terminal title carries, if it carries one.
    pub fn parse(title: &str) -> Option<Self> {
        let mut fields = title.strip_prefix(TITLE_PREFIX)?.split_whitespace();
        let frame = fields.next()?.strip_prefix("frame=")?.parse().ok()?;
        let acknowledged = fields.next()?.strip_prefix("ack=")?.parse().ok()?;
        let (columns, rows) = fields.next()?.strip_prefix("size=")?.split_once('x')?;
        if fields.next().is_some() {
            return None;
        }
        Some(Self {
            frame,
            acknowledged,
            columns: columns.parse().ok()?,
            rows: rows.parse().ok()?,
        })
    }
}

/// The text the vision core pastes to ask for acknowledgement `id`.
pub fn acknowledgement_request(id: u64) -> String {
    format!("{REQUEST_PREFIX}{id}")
}

/// The acknowledgement a pasted text asks for, if it is a request.
pub fn parse_acknowledgement_request(text: &str) -> Option<u64> {
    text.strip_prefix(REQUEST_PREFIX)?.parse().ok()
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
