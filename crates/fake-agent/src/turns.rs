//! When the fake agent's turns end. The test ends each turn ([`crate::Command::EndTurn`]), or,
//! with [`crate::variables::TURNS`] `auto`, a turn ends by itself once Herdr saw it, unless the
//! test holds turns. Herdr sees a turn through the agent's title, so a turn of an agent whose
//! title Herdr does not read, such as one that reports its state itself, ends at once.

/// The agent's turn.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Turn {
    #[default]
    None,
    /// A turn is under way, and the agent shows it; Herdr has not reported it working since.
    Unseen,
    /// A turn is under way, and Herdr reported the agent working since it started.
    Seen,
}

#[derive(Debug)]
pub(crate) struct Turns {
    turn: Turn,
    /// Turns end only when the test ends them.
    held: bool,
    /// Herdr reports the agent working now.
    working: bool,
    /// Herdr reads the agent's title.
    title_read: bool,
}

impl Turns {
    pub(crate) fn new(held: bool, title_read: bool) -> Self {
        Self {
            turn: Turn::None,
            held,
            working: false,
            title_read,
        }
    }

    pub(crate) fn under_way(&self) -> bool {
        self.turn != Turn::None
    }

    /// A turn starts, unless one is under way: whether it started.
    pub(crate) fn start(&mut self) -> bool {
        if self.under_way() {
            return false;
        }
        // Herdr that reports the agent working already answers the prompt at once.
        self.turn = if self.working {
            Turn::Seen
        } else {
            Turn::Unseen
        };
        true
    }

    /// The turn ends.
    pub(crate) fn end(&mut self) {
        self.turn = Turn::None;
    }

    pub(crate) fn hold(&mut self, held: bool) {
        self.held = held;
    }

    /// Herdr gave the agent `status`.
    pub(crate) fn herdr_reported(&mut self, status: &str) {
        self.working = status == "working";
        if self.working && self.turn == Turn::Unseen {
            self.turn = Turn::Seen;
        }
    }

    /// Herdr reads the agent's title from now on.
    pub(crate) fn released(&mut self) {
        self.title_read = true;
    }

    /// Whether the turn under way ends by itself now.
    pub(crate) fn due(&self) -> bool {
        !self.held
            && match self.turn {
                Turn::None => false,
                Turn::Unseen => !self.title_read,
                Turn::Seen => true,
            }
    }
}
