//! A sandbox that `app-sandbox start` started outlives it: the commands that follow, each a
//! process of its own, install a mod in it, open the mod, drive it and shoot the window, until
//! `stop` ends the sandbox.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

/// `app-sandbox <arguments>`, with the environment of the test, whose runtime directory the
/// sandbox masks.
fn app_sandbox(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_app-sandbox"))
        .args(arguments)
        .output()
        .unwrap()
}

/// The output of `app-sandbox <arguments>`, which must succeed.
fn succeed(arguments: &[&str]) -> String {
    let output = app_sandbox(arguments);
    assert!(
        output.status.success(),
        "app-sandbox {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Stops the sandbox when the test ends, failed or not, so that no holder waits for its idle
/// limit.
struct Started<'a>(&'a str);

impl Drop for Started<'_> {
    fn drop(&mut self) {
        app_sandbox(&["stop", self.0]);
    }
}

#[test]
fn commands_drive_the_sandbox_that_start_left_running() {
    // A name of its own: other runs of the tests, and agents, run sandboxes on this machine.
    let name = format!("test-{}", std::process::id());
    let name = name.as_str();
    let info: Value = serde_json::from_str(&succeed(&["start", name, "--size=1000x700"])).unwrap();
    let _started = Started(name);
    let directory = Path::new(info["directory"].as_str().unwrap()).to_owned();

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/probe");
    let installed = succeed(&["mod", name, fixture.to_str().unwrap()]);
    succeed(&["herdr-app", name, "check", installed.trim()]);
    succeed(&["herdr-app", name, "do", "space.new", "--name", "vision"]);
    // The app finds the mod in its mods: an unknown one is an error.
    succeed(&["herdr-app", name, "do", "tab.new", "--mod", "probe"]);
    let delivered = succeed(&["herdr-app", name, "do", "probe.ping"]);
    assert_eq!(
        serde_json::from_str::<Value>(&delivered).unwrap(),
        serde_json::json!({"ok": true})
    );

    // `run` prints a program's output as it printed it, even when that output is JSON.
    assert_eq!(
        succeed(&["run", name, "printf", r#"{"a":1}"#]),
        "{\"a\":1}\n"
    );

    let shots = tempfile::tempdir().unwrap();
    let shot = shots.path().join("shot.png");
    succeed(&["shot", name, shot.to_str().unwrap()]);
    let png = std::fs::read(&shot).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    // The header's width and height: the display's size, which the window fills.
    assert_eq!(&png[16..24], &[0, 0, 0x03, 0xe8, 0, 0, 0x02, 0xbc]);

    let stopped: Value = serde_json::from_str(&succeed(&["stop", name])).unwrap();
    assert_eq!(stopped, serde_json::json!({"left": []}));
    assert!(!directory.exists());
    assert!(!app_sandbox(&["info", name]).status.success());
}
