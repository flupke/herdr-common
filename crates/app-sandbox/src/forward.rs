//! A forward: the host's `127.0.0.1:<port>`, which reaches the same port of the bubblewrap
//! environment's loopback, which nothing outside the environment reaches otherwise.
//!
//! A connection made inside a network namespace must come from a process in it: this process's
//! threads cannot enter it, since entering the environment's user namespace, as nsenter does
//! before its network namespace, takes a process of one thread. Each connection runs its own
//! relay, socat under nsenter, and this process copies the bytes between the host's connection
//! and the relay's standard input and output.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use eyre::{Context, Result};

/// How long the listener waits after a failed accept before it accepts again.
const ACCEPT_RETRY: Duration = Duration::from_millis(100);

/// A forward, which relays each connection the host's port accepts until a drop. The relays it
/// started end with the environment.
pub(crate) struct Forward {
    /// The host's `127.0.0.1:<port>`, whose port is the sandbox's.
    address: SocketAddr,
    stopping: Arc<AtomicBool>,
    accepting: Option<JoinHandle<()>>,
}

impl Forward {
    /// Listen on the host's `127.0.0.1:port`, and relay each connection through the command
    /// `relay` makes: one that connects inside, and copies the connection to its standard
    /// output and its standard input to the connection.
    pub(crate) fn start(port: u16, relay: impl Fn() -> Command + Send + 'static) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .wrap_err_with(|| format!("cannot listen on the host's 127.0.0.1:{port}"))?;
        let address = listener.local_addr()?;
        let stopping = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stopping);
        let accepting = std::thread::spawn(move || {
            for connection in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(connection) = connection else {
                    // Such as too many open files: wait for a relay to end rather than spin.
                    std::thread::sleep(ACCEPT_RETRY);
                    continue;
                };
                // The relay starts on this thread, so none starts once `stop` joined it.
                let Ok(child) = relay()
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                else {
                    continue;
                };
                std::thread::spawn(move || copy_through(&connection, child));
            }
        });
        Ok(Self {
            address,
            stopping,
            accepting: Some(accepting),
        })
    }

    /// The port the forward reaches, on both sides.
    pub(crate) fn port(&self) -> u16 {
        self.address.port()
    }

    /// Stop listening, and return once no new relay can start.
    fn stop(&mut self) {
        let Some(accepting) = self.accepting.take() else {
            return;
        };
        self.stopping.store(true, Ordering::SeqCst);
        // Wakes the listener, which sees the flag before it relays this connection.
        let _ = TcpStream::connect(self.address);
        let _ = accepting.join();
    }
}

impl Drop for Forward {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Copy `connection` both ways through the relay `child`, until the relay ends. On a failure
/// the relay's input closes, which ends it.
fn copy_through(connection: &TcpStream, mut child: Child) {
    let _ = (|| -> io::Result<()> {
        let (Some(mut to_relay), Some(mut from_relay)) = (child.stdin.take(), child.stdout.take())
        else {
            return Ok(());
        };
        let mut from_host = connection.try_clone()?;
        // Ends once the host's side closes, which closes the relay's input.
        let upstream = std::thread::spawn(move || pass_on(&mut from_host, &mut to_relay));
        let mut to_host = connection;
        let _ = pass_on(&mut from_relay, &mut to_host);
        // The relay ended: the host's side reads the end, and `upstream` stops reading.
        connection.shutdown(Shutdown::Both)?;
        let _ = upstream.join();
        Ok(())
    })();
    let _ = child.wait();
}

/// Write to `to` what `from` reads, as soon as it reads it, until `from` ends. `io::copy` from a
/// socket to a pipe splices, and a blocking splice from a TCP socket waits for its whole length
/// or the socket's end: a request would reach the relay only once the host's side closed.
fn pass_on(from: &mut impl Read, to: &mut impl Write) -> io::Result<()> {
    let mut buffer = [0; 16 * 1024];
    loop {
        match from.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => to.write_all(&buffer[..read])?,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}
