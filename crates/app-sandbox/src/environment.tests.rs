use super::*;

fn variables(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

#[test]
fn keeps_the_user_s_displays_herdr_and_claude_out_and_lets_the_caller_override_defaults_alone() {
    let inherited = variables(&[
        ("PATH", "/usr/bin"),
        ("LANG", "C.UTF-8"),
        ("HOME", "/home/user"),
        ("DISPLAY", ":1"),
        ("WAYLAND_DISPLAY", "wayland-1"),
        ("WAYLAND_SOCKET", "3"),
        ("XAUTHORITY", "/home/user/.Xauthority"),
        ("SWAYSOCK", "/run/user/1000/sway-ipc.sock"),
        ("I3SOCK", "/run/user/1000/i3-ipc.sock"),
        ("HERDR_ENV", "1"),
        ("HERDR_APP_HOME", "/home/user/.config/herdr-app"),
        ("HC_TERMINAL_ID", "t2"),
        ("HC_SESSION", "default"),
        ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ("XDG_DATA_DIRS", "/usr/share"),
        ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
        ("SSH_AUTH_SOCK", "/run/user/1000/ssh"),
        ("TMUX", "/tmp/tmux"),
        ("TMUX_PANE", "%1"),
        ("PS1", "> "),
        ("CLAUDE_CONFIG_DIR", "/home/user/.claude"),
        ("CLAUDECODE", "1"),
        ("FORCE_HYPERLINK", "1"),
        (
            "CLAUDE_CODE_MESSAGING_SOCKET",
            "/run/user/1000/cc-socks/1.sock",
        ),
        ("TMPDIR", "/tmp/nix-shell.1"),
        // The user's own browser and terminal, which never reach the sandbox.
        ("BROWSER", "firefox"),
        ("TERM", "screen"),
    ]);
    let added = variables(&[
        ("PATH", "/sandbox/stand-ins:/usr/bin"),
        ("LANG", "fr_FR.UTF-8"),
        ("DISPLAY", ":1"),
        ("WAYLAND_DISPLAY", "/run/user/1000/wayland-1"),
        ("BROWSER", "/sandbox/browser"),
        // What the isolation stands on: the sandbox's own wins.
        ("HOME", "/home/user"),
        ("HERDR_APP_HOME", "/home/user/.config/herdr-app"),
        ("HC_SESSION", "default"),
        ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ("TMPDIR", "/tmp"),
        ("VK_DRIVER_FILES", "/gpu.json"),
    ]);
    let environment = environment(
        &Inputs {
            root: Path::new("/sandbox"),
            vulkan_driver: Path::new("/mesa/lvp.json"),
            added: &added,
        },
        inherited,
    );
    let environment: Vec<_> = environment
        .iter()
        .map(|(name, value)| (name.to_str().unwrap(), value.to_str().unwrap()))
        .collect();
    assert_eq!(
        environment,
        [
            ("BROWSER", "/sandbox/browser"),
            ("DO_NOT_TRACK", "1"),
            ("HC_SESSION", "sandbox"),
            ("HERDR_APP_HOME", "/sandbox/config/herdr-app"),
            ("HERDR_APP_NO_SOUND", "1"),
            ("HERDR_TELEMETRY", "0"),
            ("HOME", "/sandbox/home"),
            ("LANG", "fr_FR.UTF-8"),
            ("PATH", "/sandbox/bin:/sandbox/stand-ins:/usr/bin"),
            ("PS1", "$ "),
            ("SHELL", "/bin/sh"),
            ("TERM", "xterm-256color"),
            ("TMPDIR", "/sandbox"),
            ("VK_DRIVER_FILES", "/mesa/lvp.json"),
            ("VK_ICD_FILENAMES", "/mesa/lvp.json"),
            ("XDG_CACHE_HOME", "/sandbox/cache"),
            ("XDG_CONFIG_HOME", "/sandbox/config"),
            ("XDG_DATA_HOME", "/sandbox/data"),
            ("XDG_RUNTIME_DIR", "/sandbox/runtime"),
            ("XDG_STATE_HOME", "/sandbox/state"),
        ]
    );
}

/// A case of `user_directories`: the variables set beside `HOME`, and the directories masked,
/// in any order.
type Case<'a> = (&'a [(&'a str, &'a str)], &'a [&'a str]);

#[test]
fn masks_the_user_s_herdr_and_runtime_directories_and_never_one_that_holds_home() {
    let defaults = [
        "/home/user/.config/herdr-app",
        "/home/user/.config/herdr-core",
        "/home/user/.local/state/herdr-core",
        "/home/user/.config/herdr",
    ];
    let cases: [Case; 5] = [
        (&[], &defaults),
        (
            &[
                ("XDG_RUNTIME_DIR", "/run/user/1000"),
                ("HERDR_SOCKET_PATH", "/run/user/1000/herdr/herdr.sock"),
                ("HERDR_APP_HOME", "/srv/app-home"),
            ],
            &[
                "/srv/app-home",
                "/home/user/.config/herdr-app",
                "/home/user/.config/herdr-core",
                "/home/user/.local/state/herdr-core",
                "/home/user/.config/herdr",
                "/run/user/1000",
            ],
        ),
        (
            &[
                ("XDG_CONFIG_HOME", "/home/user/config"),
                ("XDG_STATE_HOME", "/home/user/state"),
            ],
            &[
                "/home/user/config/herdr-app",
                "/home/user/config/herdr-core",
                "/home/user/state/herdr-core",
                "/home/user/config/herdr",
            ],
        ),
        // A socket in HOME or above it masks only the configurations and the state.
        (&[("HERDR_SOCKET_PATH", "/home/user/herdr.sock")], &defaults),
        (&[("XDG_RUNTIME_DIR", "relative")], &defaults),
    ];
    for (set, masked) in cases {
        let mut inherited = variables(set);
        inherited.push(("HOME".into(), "/home/user".into()));
        let mut directories = user_directories(inherited);
        directories.sort();
        let mut expected: Vec<PathBuf> = masked.iter().map(PathBuf::from).collect();
        expected.sort();
        assert_eq!(directories, expected, "{set:?}");
    }
}
