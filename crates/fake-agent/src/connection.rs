//! A fake agent's end of the event socket.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Mutex, PoisonError, mpsc};
use std::thread;

use crate::protocol::{Command, Event, Role};

/// The connection of one fake agent to the test.
pub(crate) struct Connection {
    writer: Mutex<UnixStream>,
}

impl Connection {
    /// Connects to the socket at `path` as `role`, says so, and returns the connection and the
    /// commands the test sends, which `wrap` turns into what the caller reads.
    pub(crate) fn connect<T: Send + 'static>(
        path: &Path,
        role: Role,
        wrap: fn(Command) -> T,
        commands: mpsc::Sender<T>,
    ) -> std::io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        let reader = BufReader::new(stream.try_clone()?);
        thread::spawn(move || {
            for line in reader.lines() {
                let Ok(line) = line else {
                    return;
                };
                let command = serde_json::from_str(&line)
                    .unwrap_or_else(|error| panic!("a fake agent's command {line:?}: {error}"));
                if commands.send(wrap(command)).is_err() {
                    return;
                }
            }
        });
        let connection = Self {
            writer: Mutex::new(stream),
        };
        connection.report(&Event::Hello { role });
        Ok(connection)
    }

    /// Reports `event` to the test.
    pub(crate) fn report(&self, event: &Event) {
        let mut line = serde_json::to_vec(event).expect("an event is JSON");
        line.push(b'\n');
        let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        // A test that already ended no longer reads.
        let _ = writer.write_all(&line);
    }
}

impl Drop for Connection {
    /// Closes the connection, which the thread reading the test's commands also holds.
    fn drop(&mut self) {
        let writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = writer.shutdown(std::net::Shutdown::Both);
    }
}
