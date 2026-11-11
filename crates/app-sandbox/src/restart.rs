//! Restarts of the sandbox's app, as a reboot or a crash restarts it: its core, its window,
//! or both.

use std::path::Path;
use std::time::Instant;

use crate::{AppSandbox, GUARD, Jail, LAYOUT_POLL, process};
use eyre::{Result, WrapErr, ensure};

/// What herdr-app writes to its log once it reconnected to a new core.
const RECONNECTED: &str = "reconnected to the core";

/// What [`AppSandbox::restart`] restarts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Restart {
    /// The core: `SIGKILL` to every hc process of the sandbox (the window's `hc connect`, the
    /// core, herdr-core's holders, and an `hc events` or any other hc command running) and
    /// every program under them, as a crash of the core, or a reboot, ends them. The window stays,
    /// and starts a new core, which gives each kept terminal a shell, at its address, in a few
    /// seconds.
    Core,
    /// A reboot whose session starts the core before the window: `SIGKILL` to the window and
    /// every process under it, then to the core as [`Restart::Core`] does, then a core started
    /// alone (`hc server start`), which gives each kept terminal a shell. The window stays
    /// closed until [`Restart::Window`].
    Reboot,
    /// The window, opened again once a [`Restart::Reboot`] closed it.
    Window,
}

impl AppSandbox {
    /// Restart the app's core, its window, or both, as `what` says, and return once the app
    /// is back: after [`Restart::Core`] and [`Restart::Window`], once the window shows the
    /// core's layout, as [`AppSandbox::start`] does; after [`Restart::Reboot`], once the new
    /// core answers. Refused when there is nothing to restart: no core, no window to close,
    /// or a window already open; and a core restart while the window is closed, which would
    /// start no core. The processes are found by their executable, the programs'
    /// paths: a herdr-app or hc that a script wraps goes unfound.
    pub fn restart(&mut self, what: Restart) -> Result<()> {
        match what {
            Restart::Core => {
                ensure!(self.window_open(), "the window is closed: open it first");
                let logged = self.app_log()?.len();
                let cores = self.running(&self.programs.hc);
                ensure!(!cores.is_empty(), "the sandbox runs no core");
                self.kill_trees(&cores)?;
                self.await_reconnection(logged)
            }
            Restart::Reboot => {
                let windows = self.running(&self.programs.app);
                ensure!(!windows.is_empty(), "the window is closed already");
                // The window's tree holds its `hc connect` and the core under it, and whatever
                // else it started.
                self.kill_trees(&windows)?;
                if let Some(mut app) = self.app.take() {
                    app.wait().wrap_err("cannot wait for the window")?;
                }
                // A core started apart from the window, such as by a reboot before.
                self.kill_trees(&self.running(&self.programs.hc))?;
                self.hc(&["server", "start"])?;
                Ok(())
            }
            Restart::Window => {
                ensure!(
                    !self.window_open(),
                    "the window is open: a reboot closes it"
                );
                let opened = self.start_app();
                if opened.is_err() {
                    // A window that never showed the layout ends, so that a restart may open
                    // it again.
                    let _ = self.kill_trees(&self.running(&self.programs.app));
                    if let Some(mut app) = self.app.take() {
                        let _ = app.wait();
                    }
                }
                opened
            }
        }
    }

    /// Whether the app's window runs.
    pub fn window_open(&self) -> bool {
        !self.running(&self.programs.app).is_empty()
    }

    /// `SIGKILL` to each process of `roots` and every process under them
    /// ([`process::kill_trees`]), and return once they ended.
    fn kill_trees(&self, roots: &[u32]) -> Result<()> {
        let nsenter = match &self.jail {
            Jail::Bubble(bubble) => Some(bubble.nsenter()),
            Jail::Plain(_) => None,
        };
        process::kill_trees(roots, nsenter, GUARD)
    }

    /// Wait until the window reconnected to a new core, which it says in its log alone (a line
    /// of `app.log` past its first `logged` bytes says `reconnected to the core`), then loaded
    /// its layout from it. Until it noticed the loss, the window still shows its layout, and
    /// `herdr-app status` may report no error.
    fn await_reconnection(&self, logged: usize) -> Result<()> {
        let deadline = Instant::now() + GUARD;
        loop {
            let log = self.app_log()?;
            let reconnected = log
                .get(logged..)
                .is_some_and(|written| written.contains(RECONNECTED));
            if reconnected && self.shows_layout() {
                return Ok(());
            }
            ensure!(self.window_open(), "the window ended; app.log:\n{log}");
            ensure!(
                Instant::now() < deadline,
                "the window did not reconnect to a new core within {GUARD:?}; app.log:\n{log}"
            );
            std::thread::sleep(LAYOUT_POLL);
        }
    }

    /// What the app wrote to `app.log`.
    fn app_log(&self) -> Result<String> {
        let path = self.directory().join("app.log");
        std::fs::read_to_string(&path).wrap_err_with(|| format!("cannot read {}", path.display()))
    }

    /// The processes of the sandbox that run `program`, by host PID.
    fn running(&self, program: &Path) -> Vec<u32> {
        let Ok(program) = std::fs::canonicalize(program) else {
            return Vec::new();
        };
        self.processes()
            .into_iter()
            .filter(|pid| {
                std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|exe| exe == program)
            })
            .collect()
    }
}
