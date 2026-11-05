//! The fake agent's terminal: the keys typed in its pane, read as a terminal agent's composer
//! reads them, and the mode it reads them in.

use std::io::Read;

use rustix::termios::{self, InputModes, LocalModes, OptionalActions, Termios};

/// A key typed in the pane, as the input line takes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Key {
    Char(char),
    Backspace,
    Enter,
    /// Ctrl-U or Ctrl-C: the input line empties.
    Clear,
    /// Ctrl-D: the agent exits.
    Exit,
}

/// Where the reader is in what the terminal sends.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum Escape {
    #[default]
    None,
    /// After `ESC`.
    Started,
    /// In a control sequence, after `ESC [`, with its parameters so far.
    Csi(String),
}

/// Reads keys from what the terminal sends: a carriage return or a line feed is Enter, except
/// in a bracketed paste, where it is a line break of the text, as is a carriage return and a
/// line feed; another escape sequence is
/// skipped.
#[derive(Default)]
struct Keys {
    escape: Escape,
    pasting: bool,
    /// The last character was a carriage return: a line feed after it ends the same line.
    after_return: bool,
}

impl Keys {
    /// The key `c` is, read next, if any.
    fn key(&mut self, c: char) -> Option<Key> {
        let after_return = std::mem::replace(&mut self.after_return, c == '\r');
        match std::mem::take(&mut self.escape) {
            Escape::None => self.plain(c, after_return),
            Escape::Started => {
                if c == '[' {
                    self.escape = Escape::Csi(String::new());
                }
                None
            }
            Escape::Csi(mut parameters) => {
                if ('@'..='~').contains(&c) {
                    self.pasting = match (parameters.as_str(), c) {
                        ("200", '~') => true,
                        ("201", '~') => false,
                        _ => self.pasting,
                    };
                } else {
                    parameters.push(c);
                    self.escape = Escape::Csi(parameters);
                }
                None
            }
        }
    }

    fn plain(&mut self, c: char, after_return: bool) -> Option<Key> {
        match c {
            '\u{1b}' => {
                self.escape = Escape::Started;
                None
            }
            '\n' if self.pasting && after_return => None,
            '\r' | '\n' if self.pasting => Some(Key::Char('\n')),
            '\r' | '\n' => Some(Key::Enter),
            '\u{7f}' | '\u{8}' => Some(Key::Backspace),
            '\u{15}' | '\u{3}' => Some(Key::Clear),
            '\u{4}' => Some(Key::Exit),
            c => (!c.is_control()).then_some(Key::Char(c)),
        }
    }
}

/// Reads the keys typed in the pane, on standard input, and hands those of each read to `send`
/// together, so that the agent draws once per read rather than once per key, until the input
/// ends or `send` returns false.
pub(crate) fn read_keys(mut send: impl FnMut(Vec<Key>) -> bool) {
    let mut stdin = std::io::stdin().lock();
    // Up to a pipe's capacity: a test's pipe hands a paste over in one read, a pty in reads of
    // at most 4 KiB.
    let mut bytes = vec![0u8; 64 * 1024];
    let mut pending = Vec::new();
    let mut keys = Keys::default();
    loop {
        let read = match stdin.read(&mut bytes) {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        pending.extend_from_slice(&bytes[..read]);
        let valid = match std::str::from_utf8(&pending) {
            Ok(text) => text.len(),
            // A character cut at the read's end waits for the rest.
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => {
                pending.clear();
                continue;
            }
        };
        let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
        pending.drain(..valid);
        let read: Vec<Key> = text.chars().filter_map(|c| keys.key(c)).collect();
        if !read.is_empty() && !send(read) {
            return;
        }
    }
}

/// The pane's terminal in the mode a terminal agent reads keys in: no echo, each key as it
/// comes, Ctrl-C and Ctrl-D as keys, and bracketed paste; its own mode again once dropped. On
/// standard input that is no terminal, as in a test, it changes nothing.
pub(crate) struct RawMode(Option<Termios>);

impl RawMode {
    pub(crate) fn enter() -> Self {
        let stdin = std::io::stdin();
        let Ok(saved) = termios::tcgetattr(&stdin) else {
            return Self(None);
        };
        let mut raw = saved.clone();
        raw.local_modes
            .remove(LocalModes::ECHO | LocalModes::ICANON | LocalModes::ISIG | LocalModes::IEXTEN);
        raw.input_modes.remove(InputModes::IXON | InputModes::ICRNL);
        let _ = termios::tcsetattr(&stdin, OptionalActions::Now, &raw);
        crate::screen::write("\x1b[?2004h");
        Self(Some(saved))
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if let Some(saved) = &self.0 {
            // The shell's prompt goes on a line of its own.
            crate::screen::write("\x1b[?2004l\r\n");
            let _ = termios::tcsetattr(std::io::stdin(), OptionalActions::Now, saved);
        }
    }
}

#[cfg(test)]
#[path = "terminal.tests.rs"]
mod tests;
