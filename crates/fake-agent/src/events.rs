//! The test's end of the event socket.
//!
//! A test listens on a Unix socket with [`Events::listen`] and names it to the fake agents it
//! starts in [`crate::variables::EVENTS`] ([`Events::environment`]). Each fake agent connects,
//! says who it is ([`Role`]), then writes one JSON line per [`Event`]; the test sends it
//! [`Command`]s on the same connection. The test waits on what the fake agents report instead
//! of polling, and proves that something did not happen by ordering: what a fake agent did
//! before it reported a later event, it reported before that event ([`Events::mark`]).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use crate::protocol::{Command, Event, Reported, Role, marker_prompt};
use crate::variables;

/// How long a wait of [`Events`] lasts before it fails, saying what it waited for: a guard
/// that fires only when a fake agent is stuck or gone.
pub const GUARD: Duration = Duration::from_secs(90);

/// Every fake agent's events, in the order each reported them, and each one's connection by
/// its role, for commands. A fake agent that connects again in a role, such as one started
/// again in the same pane, takes the role's commands from then on.
///
/// It keeps every event it received, so that a test may wait on the events in turn
/// ([`Self::events_until`]) and on what all of them say together ([`Self::wait_until`]). A
/// wait that runs out fails the test; a program that lives on, such as a vision server, takes
/// the error of [`Self::try_wait_until`] and [`Self::try_send`] instead. Dropping it stops
/// listening.
pub struct Events {
    path: PathBuf,
    /// Tells the thread that accepts connections to stop.
    stopped: Arc<AtomicBool>,
    /// Each event, or why a fake agent's line was not one.
    incoming: Mutex<mpsc::Receiver<Result<Reported, String>>>,
    /// Every event received, and how many of them [`Self::events_until`] went through.
    received: Mutex<Received>,
    connections: Arc<Connections>,
    markers: AtomicU64,
}

#[derive(Default)]
struct Received {
    events: Vec<Reported>,
    read: usize,
}

/// The connections of the fake agents, by role, as they connect.
#[derive(Default)]
struct Connections {
    /// The connection of each role, and its number.
    writers: Mutex<BTreeMap<Role, (u64, UnixStream)>>,
    connected: Condvar,
    next: AtomicU64,
}

impl Events {
    /// Listens at `path`, under the test's private directory.
    pub fn listen(path: &Path) -> std::io::Result<Self> {
        let listener = UnixListener::bind(path)?;
        let (sender, events) = mpsc::channel();
        let connections = Arc::new(Connections::default());
        let accepted = Arc::clone(&connections);
        let stopped = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stopped);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    return;
                };
                if stopping.load(Ordering::Acquire) {
                    return;
                }
                let (sender, connections) = (sender.clone(), Arc::clone(&accepted));
                thread::spawn(move || read_connection(&stream, &sender, &connections));
            }
        });
        Ok(Self {
            path: path.to_owned(),
            stopped,
            incoming: Mutex::new(events),
            received: Mutex::default(),
            connections,
            markers: AtomicU64::new(0),
        })
    }

    /// The environment variable, `NAME=value`, that names this socket to a fake agent.
    pub fn environment(&self) -> String {
        format!("{}={}", variables::EVENTS, self.path.display())
    }

    /// The next event that `matches`, skipping the others; fails after [`GUARD`], when none
    /// came, with `what` it waited for.
    ///
    /// # Panics
    /// Panics when the wait runs out.
    pub fn wait_for(&self, what: &str, matches: impl FnMut(&Reported) -> bool) -> Reported {
        self.events_until(what, matches)
            .pop()
            .expect("events_until returns the event that matched")
    }

    /// Every event after those an earlier call went through, up to and including the first
    /// that `matches`; fails after [`GUARD`], when none came, with `what` it waited for.
    ///
    /// # Panics
    /// Panics when the wait runs out.
    pub fn events_until(
        &self,
        what: &str,
        mut matches: impl FnMut(&Reported) -> bool,
    ) -> Vec<Reported> {
        let mut checked = self.lock_received().read;
        loop {
            let mut received = self.lock_received();
            if let Some(found) = received.events[checked..].iter().position(&mut matches) {
                let end = checked + found + 1;
                let events = received.events[received.read..end].to_vec();
                received.read = end;
                return events;
            }
            checked = received.events.len();
            drop(received);
            self.receive(what, GUARD)
                .unwrap_or_else(|error| panic!("{error}"));
        }
    }

    /// Every event received, once `holds` holds for them; fails after [`GUARD`], when it does
    /// not, with `what` it waited for.
    ///
    /// # Panics
    /// Panics when the wait runs out.
    pub fn wait_until(&self, what: &str, holds: impl FnMut(&[Reported]) -> bool) -> Vec<Reported> {
        self.try_wait_until(what, GUARD, holds)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// Every event received, once `holds` holds for them; an error that says what came since
    /// the last wait, when no event came for `guard`.
    pub fn try_wait_until(
        &self,
        what: &str,
        guard: Duration,
        mut holds: impl FnMut(&[Reported]) -> bool,
    ) -> Result<Vec<Reported>, String> {
        loop {
            let received = self.lock_received();
            if holds(&received.events) {
                return Ok(received.events.clone());
            }
            drop(received);
            self.receive(what, guard)?;
        }
    }

    /// Every event received so far, with those that came since and wait to be read; an error
    /// when a fake agent wrote a line that is no event.
    pub fn try_received(&self) -> Result<Vec<Reported>, String> {
        let events = self.incoming.lock().unwrap_or_else(PoisonError::into_inner);
        let mut received = self.lock_received();
        while let Ok(event) = events.try_recv() {
            received.events.push(event?);
        }
        Ok(received.events.clone())
    }

    /// Receives the next event; an error that names `what` the test waited for, when none came
    /// for `guard`.
    fn receive(&self, what: &str, guard: Duration) -> Result<(), String> {
        let next = self
            .incoming
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recv_timeout(guard);
        let mut received = self.lock_received();
        let before = &received.events[received.read..];
        let event = next
            .map_err(|error| {
                format!("no fake agent reported {what} ({error}); since the last wait: {before:#?}")
            })?
            .map_err(|error| format!("{error}; since the last wait: {before:#?}"))?;
        received.events.push(event);
        Ok(())
    }

    fn lock_received(&self) -> std::sync::MutexGuard<'_, Received> {
        self.received.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Submits a new marker with `submit`, which types the prompt it gets into a fake agent's
    /// pane, and returns every event up to that agent's report of it: what the agent did with
    /// what came in its pane before the marker.
    ///
    /// # Panics
    /// Panics when the wait runs out.
    pub fn mark(&self, submit: impl FnOnce(&str)) -> Vec<Reported> {
        let id = self.markers.fetch_add(1, Ordering::Relaxed);
        submit(&marker_prompt(id));
        self.events_until(&format!("the marker {id}"), |reported| {
            reported.event == Event::Marker { id }
        })
    }

    /// Has the fake agent of `role` obey `command`, and returns every event up to its answer
    /// ([`Event::Obeyed`]), as [`Self::events_until`] does; fails after [`GUARD`] when it
    /// does not connect or answer.
    ///
    /// # Panics
    /// Panics when the wait runs out.
    pub fn obey(&self, role: &Role, command: &Command) -> Vec<Reported> {
        self.send(role, command);
        self.events_until(&format!("{role:?}'s answer to {command:?}"), |reported| {
            reported.from == *role
                && matches!(&reported.event, Event::Obeyed { command: obeyed } if obeyed == command)
        })
    }

    /// Sends `command` to the fake agent of `role`, once it connected; fails after [`GUARD`]
    /// when it does not.
    ///
    /// # Panics
    /// Panics when the wait runs out.
    pub fn send(&self, role: &Role, command: &Command) {
        self.try_send(role, command, GUARD)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// Sends `command` to the fake agent of `role`, once it connected; an error when it does
    /// not within `guard`, or is gone.
    pub fn try_send(&self, role: &Role, command: &Command, guard: Duration) -> Result<(), String> {
        let mut line = serde_json::to_vec(command).map_err(|error| error.to_string())?;
        line.push(b'\n');
        let writers = self
            .connections
            .writers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (mut writers, _) = self
            .connections
            .connected
            .wait_timeout_while(writers, guard, |writers| !writers.contains_key(role))
            .unwrap_or_else(PoisonError::into_inner);
        let (_, writer) = writers
            .get_mut(role)
            .ok_or_else(|| format!("the fake agent {role:?} did not connect"))?;
        writer
            .write_all(&line)
            .map_err(|error| format!("the fake agent {role:?} is gone: {error}"))
    }
}

impl Drop for Events {
    /// Stops accepting connections: a connection of its own wakes the thread that accepts them.
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = UnixStream::connect(&self.path);
    }
}

/// Reads the events of one fake agent into `sender`, then reports that it disconnected.
fn read_connection(
    stream: &UnixStream,
    sender: &mpsc::Sender<Result<Reported, String>>,
    connections: &Connections,
) {
    let number = connections.next.fetch_add(1, Ordering::Relaxed);
    let mut role = None;
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else {
            break;
        };
        let event: Event = match serde_json::from_str(&line) {
            Ok(event) => event,
            Err(error) => {
                let _ = sender.send(Err(format!("a fake agent wrote {line:?}: {error}")));
                break;
            }
        };
        if let Event::Hello { role: hello } = &event
            && let Ok(writer) = stream.try_clone()
        {
            connections
                .writers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(hello.clone(), (number, writer));
            connections.connected.notify_all();
            role = Some(hello.clone());
        }
        let Some(from) = role.clone() else {
            let _ = sender.send(Err(format!(
                "a fake agent reported {event:?} before it said who it is"
            )));
            break;
        };
        let _ = sender.send(Ok(Reported { from, event }));
    }
    let Some(from) = role else {
        return;
    };
    let mut writers = connections
        .writers
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    // A fake agent of the same role that connected since keeps its connection.
    if writers
        .get(&from)
        .is_some_and(|(owner, _)| *owner == number)
    {
        writers.remove(&from);
    }
    drop(writers);
    let _ = sender.send(Ok(Reported {
        from,
        event: Event::Disconnected,
    }));
}

#[cfg(test)]
#[path = "events.tests.rs"]
mod tests;
