//! A bare bubble, with no herdr-app, as a caller runs one for a server of its own: it shows the
//! user's directories empty inside and leaves them on the host, keeps its files in its tmpfs,
//! and ends with every process it ran, a detached one included. On the host's network, a
//! terminal runs a program in it from its launch line alone.

use std::ffi::OsString;
use std::io::Read;
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;

use app_sandbox::{Bubble, Network, user_directories};

#[test]
fn masks_the_user_s_directories_then_ends_with_every_process() {
    // The user's directories, outside the host's /tmp, which the bubble's tmpfs hides whole.
    let user = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let path = |relative: &str| user.path().join(relative);
    for file in [
        "home/notes",
        "config/herdr-app/window.json",
        "config/other/kept",
        "runtime/wayland-1",
    ] {
        std::fs::create_dir_all(path(file).parent().unwrap()).unwrap();
        std::fs::write(path(file), "").unwrap();
    }
    let inherited: [(OsString, OsString); 3] = [
        ("HOME".into(), path("home").into()),
        ("XDG_CONFIG_HOME".into(), path("config").into()),
        ("XDG_RUNTIME_DIR".into(), path("runtime").into()),
    ];
    let mut bubble = Bubble::start(&user_directories(inherited), Network::Loopback).unwrap();
    let environment = [("PATH".into(), std::env::var_os("PATH").unwrap())];
    let sh = |script: &str, argument: &Path| {
        bubble
            .command(Path::new("/bin/sh"), Bubble::inside(), &environment)
            .args(["-c", script, &argument.to_string_lossy()])
            .status()
            .unwrap()
            .success()
    };
    let exists_inside = |path: &Path| sh("test -e \"$0\"", path);

    assert!(exists_inside(&path("home/notes")));
    assert!(exists_inside(&path("config/other/kept")));
    assert!(exists_inside(&path("config/herdr-app")));
    assert!(!exists_inside(&path("config/herdr-app/window.json")));
    assert!(exists_inside(&path("runtime")));
    assert!(!exists_inside(&path("runtime/wayland-1")));
    // A user directory the host lacks stays absent, inside and on the host.
    assert!(!exists_inside(&path("config/herdr-core")));
    assert!(!path("config/herdr-core").exists());

    // A process that leaves its session and its parent, as `setsid nohup` does, and a file
    // that names it by its PID inside.
    let pid_file = Bubble::inside().join(format!("bubble-test-{}", std::process::id()));
    assert!(sh(
        "setsid sleep 600 </dev/null >/dev/null 2>&1 & printf %s \"$!\" >\"$0\"",
        &pid_file,
    ));
    assert!(!pid_file.exists(), "the file went outside the tmpfs");
    let outside = bubble.outside(&pid_file);
    let detached = std::fs::read_to_string(&outside).unwrap();
    let running = bubble.processes();
    assert!(
        running
            .iter()
            .any(|pid| inner_pid(*pid).as_deref() == Some(detached.as_str())),
        "the detached process {detached} is not among {running:?}"
    );
    assert!(running.contains(&bubble.id()));

    bubble.end().unwrap();
    assert_eq!(bubble.processes(), Vec::<u32>::new());
    for pid in running {
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "{pid} still runs"
        );
    }
    assert!(!outside.exists());
    assert!(path("config/herdr-app/window.json").exists());
    assert!(path("runtime/wayland-1").exists());
    assert!(!path("config/herdr-core").exists());
}

/// The PID of the process `pid` in its own PID namespace: the last of its `NSpid`.
fn inner_pid(pid: u32) -> Option<String> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("NSpid:"))?;
    line.split_whitespace().last().map(str::to_owned)
}

#[test]
fn on_the_host_s_network_a_launch_line_reaches_a_host_port_with_its_environment_alone() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut bubble = Bubble::start(&[], Network::Host).unwrap();
    let environment = [
        ("PATH".into(), std::env::var_os("PATH").unwrap()),
        ("GREETING".into(), "hello".into()),
        ("PORT".into(), port.to_string().into()),
    ];
    let line = bubble.launch(Path::new("/bin/sh"), Bubble::inside(), &environment);
    // As a terminal launches it: with an environment of its own, which the line drops.
    let status = Command::new(&line[0])
        .args(&line[1..])
        .args([
            "-c",
            r#"printf '%s %s' "$GREETING" "${LEAKED-none}" | socat - "TCP4:127.0.0.1:$PORT""#,
        ])
        .env("LEAKED", "yes")
        .status()
        .unwrap();
    assert!(status.success());

    let mut received = String::new();
    listener
        .accept()
        .unwrap()
        .0
        .read_to_string(&mut received)
        .unwrap();
    assert_eq!(received, "hello none");
    bubble.end().unwrap();
}
