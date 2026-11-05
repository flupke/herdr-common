//! A sandbox runs the pinned herdr-app on its own display, shows its window in a screenshot,
//! and leaves nothing behind once it stops, in each isolation; its `xdg-open` runs the desktop
//! entry that claims a link's scheme, as on a desktop, the caller's `BROWSER` runs, and the host
//! reads the files it wrote;
//! the host reaches a port of its loopback through a forward.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, ChildStderr, Stdio};

use app_sandbox::{AppSandbox, Isolation, Options, Programs};

fn shoots_the_pinned_app_then_leaves_nothing_behind(isolation: Isolation) {
    let options = Options {
        isolation,
        display_size: (1000, 700),
        ..Options::default()
    };
    let sandbox = AppSandbox::start(&Programs::pinned().unwrap(), &options).unwrap();
    let directory = sandbox.directory();
    assert!(sandbox.display_socket().unwrap().exists());

    let png = sandbox.shot().unwrap();
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut pixels).unwrap();
    // The kiosk shell makes the window fill the display.
    assert_eq!((frame.width, frame.height), (1000, 700));
    let colours: BTreeSet<&[u8]> = pixels[..frame.buffer_size()]
        .chunks(frame.color_type.samples())
        .collect();
    assert!(colours.len() > 2, "the window drew nothing: {colours:?}");

    // A space with a terminal, whose shell the core runs.
    sandbox
        .herdr_app(&["do", "space.new", r#"{"name": "alpha"}"#])
        .unwrap();
    // hc gives the shell's PID as the sandbox's processes see it.
    let shell = sandbox.hc(&["get", "t1"]).unwrap()["pid"].to_string();
    let running = sandbox.processes();
    assert!(
        running
            .iter()
            .any(|pid| inner_pid(*pid).as_deref() == Some(&shell)),
        "the shell {shell} is not among {running:?}"
    );
    let stopped = sandbox.stop().unwrap();
    assert_eq!(stopped.processes, Vec::<u32>::new());
    for pid in running {
        assert!(ended(pid), "{pid} still runs");
    }
    assert!(!directory.exists());
}

/// The PID of the process `pid` in its own PID namespace: the last of its `NSpid`.
fn inner_pid(pid: u32) -> Option<String> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix("NSpid:"))?;
    line.split_whitespace().last().map(str::to_owned)
}

/// Whether the process `pid` ended: gone, or a zombie its new parent has yet to reap.
fn ended(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |stat| {
        stat.rsplit_once(')')
            .is_some_and(|(_, fields)| fields.trim_start().starts_with('Z'))
    })
}

#[test]
fn in_bubblewrap() {
    shoots_the_pinned_app_then_leaves_nothing_behind(Isolation::Bubblewrap);
}

#[test]
fn plain() {
    shoots_the_pinned_app_then_leaves_nothing_behind(Isolation::Plain);
}

#[test]
fn opens_a_link_with_the_desktop_entry_that_claims_its_scheme_or_the_caller_s_browser() {
    // The handler below, as the sandbox's processes see it: `/tmp` in bubblewrap.
    let options = Options {
        environment: vec![("BROWSER".into(), "/tmp/handler".into())],
        ..Options::default()
    };
    let sandbox = AppSandbox::start(&Programs::pinned().unwrap(), &options).unwrap();
    let (inside, outside) = (sandbox.inside(), sandbox.directory());
    let handler = outside.join("handler");
    std::fs::write(
        &handler,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >>{}\n",
            inside.join("handled.log").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&handler, std::fs::Permissions::from_mode(0o755)).unwrap();
    // The scheme's `+`, which a scheme may hold, matches as itself, not as a pattern.
    std::fs::create_dir_all(outside.join("data/applications")).unwrap();
    std::fs::write(
        outside.join("data/applications/test-link.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nName=Test link\n\
             Exec={} focus %u\nMimeType=text/plain;x-scheme-handler/sandbox+test;\n",
            inside.join("handler").display()
        ),
    )
    .unwrap();

    // The app runs the `xdg-open` its PATH finds.
    for link in ["sandbox+test://agent/1", "https://example.com/"] {
        let status = sandbox
            .command(Path::new("xdg-open"))
            .arg(link)
            .status()
            .unwrap();
        assert!(status.success(), "xdg-open {link}: {status}");
    }
    let status = sandbox
        .command(Path::new("sh"))
        .args(["-c", r#""$BROWSER" "$0""#, "https://browsed.example/"])
        .status()
        .unwrap();
    assert!(status.success(), "$BROWSER: {status}");
    let read = |name: &str| std::fs::read_to_string(outside.join(name)).unwrap();
    assert_eq!(
        read("handled.log"),
        "focus sandbox+test://agent/1\nhttps://browsed.example/\n"
    );
    assert_eq!(read("opened.log"), "https://example.com/\n");
}

#[test]
fn reaches_a_port_of_the_bubblewrap_loopback_through_a_forward_until_it_stops() {
    let sandbox = AppSandbox::start(&Programs::pinned().unwrap(), &Options::default()).unwrap();
    let mut echoing = Server::inside(&sandbox, "EXEC:cat");
    assert!(
        TcpStream::connect(("127.0.0.1", echoing.port)).is_err(),
        "the host reached the sandbox's loopback without a forward"
    );

    sandbox.forward(echoing.port).unwrap();
    // Again, as a test does that forwards each port it reads: the forward stays.
    sandbox.forward(echoing.port).unwrap();
    // A client that closes first, its request sent: it still reads the whole answer.
    let mut connection = TcpStream::connect(("127.0.0.1", echoing.port)).unwrap();
    connection.write_all(b"ping").unwrap();
    connection.shutdown(Shutdown::Write).unwrap();
    let mut echoed = String::new();
    connection.read_to_string(&mut echoed).unwrap();
    assert_eq!(echoed, "ping");

    // A server that closes once it answered, as an HTTP server does: the host reads the end
    // while its side stays open, as a client that cannot half-close keeps it.
    let mut answering = Server::inside(&sandbox, "SYSTEM:read request; echo answer to $request");
    sandbox.forward(answering.port).unwrap();
    let mut connection = TcpStream::connect(("127.0.0.1", answering.port)).unwrap();
    connection.write_all(b"ping\n").unwrap();
    let mut answer = String::new();
    connection.read_to_string(&mut answer).unwrap();
    assert_eq!(answer, "answer to ping\n");

    sandbox.stop().unwrap();
    for server in [&mut echoing, &mut answering] {
        server.process.wait().unwrap();
        assert!(
            TcpStream::connect(("127.0.0.1", server.port)).is_err(),
            "the forward outlived the sandbox"
        );
    }
}

/// A socat server inside the sandbox, on a port free on the host.
struct Server {
    port: u16,
    process: Child,
    /// Kept open to the end: socat dies of a write to a closed pipe.
    _said: ChildStderr,
}

impl Server {
    /// Listen on the sandbox's loopback, and run socat's `address` for each connection; return
    /// once it listens.
    fn inside(sandbox: &AppSandbox, address: &str) -> Self {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let listen = format!("TCP-LISTEN:{port},bind=127.0.0.1");
        let mut process = sandbox
            .command(Path::new("socat"))
            .args(["-d", "-d", &listen, address])
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut said = BufReader::new(process.stderr.take().unwrap());
        let listening = (&mut said)
            .lines()
            .map_while(Result::ok)
            .any(|line| line.contains(&format!("listening on AF=2 127.0.0.1:{port}")));
        assert!(listening, "the server inside never listened");
        Self {
            port,
            process,
            _said: said.into_inner(),
        }
    }
}
