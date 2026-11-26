use std::io::Read;

use rustix::termios::{self, InputModes, LocalModes, OptionalActions, Termios};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Key {
    Char(char),
    Backspace,
    Enter,
    Clear,
    Exit,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum Escape {
    #[default]
    None,
    Started,
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
    after_return: bool,
}

impl Keys {
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
            crate::screen::write("\x1b[?2004l\r\n");
            let _ = termios::tcsetattr(std::io::stdin(), OptionalActions::Now, saved);
        }
    }
}

#[cfg(test)]
#[path = "terminal.tests.rs"]
mod tests;
