//! The environment of the sandbox's processes, and the user's directories the bubblewrap
//! environment masks.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The core's session (`HC_SESSION`), which names its socket in the private runtime directory.
const CORE_SESSION: &str = "sandbox";

/// The Wayland socket of the sandbox's Weston, in the private runtime directory. No
/// `WAYLAND_DISPLAY` names it, and a Wayland client looks for `wayland-0` without one: the app
/// opens its window on Weston's Xwayland, whose `DISPLAY` the sandbox sets once it runs.
pub(crate) const WAYLAND_SOCKET: &str = "wayland-sandbox";

/// What the environment is made from.
pub(crate) struct Inputs<'a> {
    /// The sandbox's directory, as its processes see it.
    pub(crate) root: &'a Path,
    /// Mesa's software Vulkan driver.
    pub(crate) vulkan_driver: &'a Path,
    /// The variables the caller adds, after the inherited ones.
    pub(crate) added: &'a [(OsString, OsString)],
}

/// The environment of the sandbox's processes: `inherited`, this process's, without what
/// reaches the user's Herdr, herdr-app or herdr-core (`HERDR_*`, `HC_*`, such as an agent's
/// `HC_TERMINAL_ID` when the caller runs in a herdr-app terminal), their displays and window
/// manager, their configuration, SSH agent, session bus or Claude Code session; then the
/// caller's `added`, without displays; then the sandbox's own: private directories, `bin`
/// first on `PATH`, Mesa's software Vulkan driver, no sound, no usage report, and
/// `BROWSER=true`, which opens nothing. The display's `DISPLAY` comes once it runs.
pub(crate) fn environment(
    inputs: &Inputs,
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    let mut environment: BTreeMap<OsString, OsString> = inherited
        .into_iter()
        .filter(|(name, _)| !reaches_the_user(name))
        .chain(
            inputs
                .added
                .iter()
                .filter(|(name, _)| !names_a_display(name))
                .cloned(),
        )
        .collect();
    let path = |name: &str| inputs.root.join(name).into_os_string();
    let mut search = path("bin");
    if let Some(inherited) = environment.get(OsStr::new("PATH")) {
        search.push(":");
        search.push(inherited);
    }
    environment.extend([
        ("PATH".into(), search),
        ("HOME".into(), path("home")),
        ("XDG_CONFIG_HOME".into(), path("config")),
        ("XDG_STATE_HOME".into(), path("state")),
        ("XDG_DATA_HOME".into(), path("data")),
        ("XDG_CACHE_HOME".into(), path("cache")),
        ("XDG_RUNTIME_DIR".into(), path("runtime")),
        ("HERDR_APP_HOME".into(), path(crate::APP_HOME)),
        // hc's manual asks tests for a session of their own, beside private XDG directories.
        ("HC_SESSION".into(), CORE_SESSION.into()),
        ("TMPDIR".into(), inputs.root.into()),
        ("SHELL".into(), "/bin/sh".into()),
        ("PS1".into(), "$ ".into()),
        ("TERM".into(), "xterm-256color".into()),
        ("BROWSER".into(), "true".into()),
        ("VK_DRIVER_FILES".into(), inputs.vulkan_driver.into()),
        ("VK_ICD_FILENAMES".into(), inputs.vulkan_driver.into()),
        ("HERDR_APP_NO_SOUND".into(), "1".into()),
        ("HERDR_TELEMETRY".into(), "0".into()),
        ("DO_NOT_TRACK".into(), "1".into()),
    ]);
    environment.into_iter().collect()
}

/// Whether the inherited variable `name` reaches the user's Herdr tools, desktop, accounts or
/// Claude Code session (`CLAUDECODE`, `CLAUDE_CODE_*`, `CLAUDE_CONFIG_DIR`, and
/// `FORCE_HYPERLINK`, which Claude Code sets for the commands it runs).
fn reaches_the_user(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    names_a_display(name)
        || bytes.starts_with(b"HERDR_")
        || bytes.starts_with(b"HC_")
        || bytes.starts_with(b"XDG_")
        || bytes.starts_with(b"CLAUDE")
        || matches!(
            bytes,
            b"FORCE_HYPERLINK"
                | b"DBUS_SESSION_BUS_ADDRESS"
                | b"SSH_AUTH_SOCK"
                | b"TMUX"
                | b"TMUX_PANE"
                | b"PS1"
        )
}

/// Whether the variable `name` names a display or a window manager, which only the sandbox's
/// own display may fill.
fn names_a_display(name: &OsStr) -> bool {
    matches!(
        name.as_encoded_bytes(),
        b"DISPLAY"
            | b"WAYLAND_DISPLAY"
            | b"WAYLAND_SOCKET"
            | b"XAUTHORITY"
            | b"SWAYSOCK"
            | b"I3SOCK"
    )
}

/// The user's directories the bubblewrap environment masks, from `inherited`, this process's
/// environment: those of the user's herdr-app and herdr-core (`hc`), the app's home, the core's
/// configuration and its state; Herdr 0.9's configuration directory and the one that holds its
/// socket; and the runtime directory (`XDG_RUNTIME_DIR`), which holds the core's sockets and the
/// desktop's: the Wayland display, the session bus, the window manager's IPC, which moves the
/// user's focus. A directory that holds `HOME` is left out, since masking it would hide the
/// user's files from the sandbox's programs, and so is one inside another: masking the outer
/// one hides it.
pub(crate) fn user_directories(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<PathBuf> {
    let inherited: BTreeMap<OsString, OsString> = inherited.into_iter().collect();
    let variable = |name: &str| {
        inherited
            .get(OsStr::new(name))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let home = variable("HOME");
    let under_home = |path: &str| home.as_ref().map(|home| home.join(path));
    let config = variable("XDG_CONFIG_HOME").or_else(|| under_home(".config"));
    let state = variable("XDG_STATE_HOME").or_else(|| under_home(".local/state"));
    let mut directories = Vec::new();
    for directory in [
        variable("HERDR_APP_HOME"),
        config.as_ref().map(|config| config.join("herdr-app")),
        config.as_ref().map(|config| config.join("herdr-core")),
        state.map(|state| state.join("herdr-core")),
        config.as_ref().map(|config| config.join("herdr")),
        variable("HERDR_SOCKET_PATH")
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_owned),
        variable("XDG_RUNTIME_DIR"),
    ]
    .into_iter()
    .flatten()
    {
        let holds_home = home
            .as_ref()
            .is_some_and(|home| home.starts_with(&directory));
        if directory.is_absolute() && !holds_home && !directories.contains(&directory) {
            directories.push(directory);
        }
    }
    let outer = directories.clone();
    directories.retain(|directory| {
        !outer
            .iter()
            .any(|other| other != directory && directory.starts_with(other))
    });
    directories
}

#[cfg(test)]
#[path = "environment.tests.rs"]
mod tests;
