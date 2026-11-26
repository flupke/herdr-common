//! A connection made inside a network namespace must come from a process in it: this process's
//! threads cannot enter it, since entering the environment's user namespace, as nsenter does
//! before its network namespace, takes a process of one thread.
//!
//! The relay's standard input is one end of a socket pair. Unlike a pipe, a socket passes on the
//! end of each direction alone: socat's shutdown of its write side once the server inside closes
//! reaches the host's side at once, though the host's client has not closed, and so does this
//! process's shutdown once the host's side closes. Once the server inside closed, what the host
//! writes is lost: a server that half-closes, then reads on, is not relayed.

use std::io;
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use eyre::{Context, Result};

const ACCEPT_RETRY: Duration = Duration::from_millis(100);

pub(crate) struct Forward {
    address: SocketAddr,
    stopping: Arc<AtomicBool>,
    accepting: Option<JoinHandle<()>>,
}

impl Forward {
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
                    std::thread::sleep(ACCEPT_RETRY);
                    continue;
                };
                let Ok((ours, theirs)) = UnixStream::pair() else {
                    continue;
                };
                let Ok(child) = relay()
                    .stdin(OwnedFd::from(theirs))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                else {
                    continue;
                };
                std::thread::spawn(move || copy_through(&connection, ours, child));
            }
        });
        Ok(Self {
            address,
            stopping,
            accepting: Some(accepting),
        })
    }

    pub(crate) fn port(&self) -> u16 {
        self.address.port()
    }

    fn stop(&mut self) {
        let Some(accepting) = self.accepting.take() else {
            return;
        };
        self.stopping.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        let _ = accepting.join();
    }
}

impl Drop for Forward {
    fn drop(&mut self) {
        self.stop();
    }
}

fn copy_through(connection: &TcpStream, relay: UnixStream, mut child: Child) {
    if copy_both_ways(connection, &relay).is_err() {
        let _ = connection.shutdown(Shutdown::Both);
    }
    drop(relay);
    let _ = child.wait();
}

/// Between two sockets `io::copy` writes what it reads at once: from a socket to a pipe it would
/// splice, and a blocking splice from a TCP socket waits for its whole length or the socket's end.
fn copy_both_ways(connection: &TcpStream, relay: &UnixStream) -> io::Result<()> {
    let mut from_host = connection.try_clone()?;
    let mut to_relay = relay.try_clone()?;
    let upstream = std::thread::spawn(move || {
        let _ = io::copy(&mut from_host, &mut to_relay);
        to_relay.shutdown(Shutdown::Write)
    });
    let _ = io::copy(&mut &*relay, &mut &*connection);
    connection.shutdown(Shutdown::Both)?;
    let _ = upstream.join();
    Ok(())
}
