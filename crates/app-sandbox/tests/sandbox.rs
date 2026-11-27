use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::num::NonZeroU16;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, ChildStderr, Stdio};

use app_sandbox::{AppSandbox, Isolation, Options, Programs, Renderer, Rendering};

fn shoots_the_pinned_app_then_leaves_nothing_behind(options: &Options, hertz: f64) {
    let sandbox = AppSandbox::start(&Programs::pinned().unwrap(), options).unwrap();
    let directory = sandbox.directory();
    let rate = vision_display::refresh_hz(&sandbox.display_socket().unwrap()).unwrap();
    assert!(
        (rate - hertz).abs() < 0.5,
        "the display runs at {rate} Hz, not {hertz}"
    );

    let render_nodes: BTreeSet<_> = sandbox
        .processes()
        .into_iter()
        .flat_map(|pid| {
            std::fs::read_dir(format!("/proc/{pid}/fd"))
                .into_iter()
                .flatten()
        })
        .filter_map(|fd| std::fs::read_link(fd.ok()?.path()).ok())
        .filter(|file| file.starts_with("/dev/dri"))
        .collect();
    let gpu = matches!(sandbox.renderer(), Renderer::Gpu { .. });
    assert!(
        render_nodes.iter().all(|file| file
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("renderD")))
            && render_nodes.is_empty() != gpu,
        "drawing with {}, the sandbox opened {render_nodes:?}",
        sandbox.renderer()
    );

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

fn inner_pid(pid: u32) -> Option<String> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix("NSpid:"))?;
    line.split_whitespace().last().map(str::to_owned)
}

fn ended(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |stat| {
        stat.rsplit_once(')')
            .is_some_and(|(_, fields)| fields.trim_start().starts_with('Z'))
    })
}

#[test]
fn in_bubblewrap_at_10_hz_on_the_gpu_when_there_is_one() {
    let options = Options {
        isolation: Isolation::Bubblewrap,
        display_size: (1000, 700),
        display_refresh_hz: NonZeroU16::new(10).unwrap(),
        ..Options::default()
    };
    shoots_the_pinned_app_then_leaves_nothing_behind(&options, 10.0);
}

#[test]
fn plain_at_the_default_rate_in_software() {
    let options = Options {
        isolation: Isolation::Plain,
        display_size: (1000, 700),
        rendering: Rendering::Software,
        ..Options::default()
    };
    shoots_the_pinned_app_then_leaves_nothing_behind(&options, 60.0);
}

#[test]
fn opens_a_link_with_the_desktop_entry_that_claims_its_scheme_or_the_caller_s_browser() {
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
    sandbox.forward(echoing.port).unwrap();
    let mut connection = TcpStream::connect(("127.0.0.1", echoing.port)).unwrap();
    connection.write_all(b"ping").unwrap();
    connection.shutdown(Shutdown::Write).unwrap();
    let mut echoed = String::new();
    connection.read_to_string(&mut echoed).unwrap();
    assert_eq!(echoed, "ping");

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

struct Server {
    port: u16,
    process: Child,
    /// Kept open to the end: socat dies of a write to a closed pipe.
    _said: ChildStderr,
}

impl Server {
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
