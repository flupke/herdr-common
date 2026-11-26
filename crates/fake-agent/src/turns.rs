#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Turn {
    #[default]
    None,
    Unseen,
    Seen,
}

#[derive(Debug)]
pub(crate) struct Turns {
    turn: Turn,
    held: bool,
    working: bool,
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

    pub(crate) fn start(&mut self) -> bool {
        if self.under_way() {
            return false;
        }
        self.turn = if self.working {
            Turn::Seen
        } else {
            Turn::Unseen
        };
        true
    }

    pub(crate) fn end(&mut self) {
        self.turn = Turn::None;
    }

    pub(crate) fn hold(&mut self, held: bool) {
        self.held = held;
    }

    pub(crate) fn herdr_reported(&mut self, status: &str) {
        self.working = status == "working";
        if self.working && self.turn == Turn::Unseen {
            self.turn = Turn::Seen;
        }
    }

    pub(crate) fn released(&mut self) {
        self.title_read = true;
    }

    pub(crate) fn due(&self) -> bool {
        !self.held
            && match self.turn {
                Turn::None => false,
                Turn::Unseen => !self.title_read,
                Turn::Seen => true,
            }
    }
}
