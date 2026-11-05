//! The environment of [`Isolation::Plain`](crate::Isolation::Plain): a temporary directory,
//! and the processes that carry its mark, with every process under them.
//!
//! Each process the sandbox starts carries the mark, a variable that names the directory, and
//! passes it on to the processes it starts, such as the core that detaches from the app; one
//! that drops it stays found as long as it runs under a marked one. Nothing ends them when the
//! process that holds the sandbox dies by `SIGKILL`: they and the directory stay behind.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use eyre::{Context, Result};
use rustix::process::Signal;

use crate::process;

/// The variable that marks the sandbox's processes, set to its directory.
const MARK: &str = "APP_SANDBOX";

/// A plain environment.
pub(crate) struct Plain {
    directory: Option<tempfile::TempDir>,
    /// `MARK=<directory>`, as `/proc/<pid>/environ` holds it.
    mark: Vec<u8>,
}

impl Plain {
    /// Create the environment's directory, in the system's temporary directory.
    pub(crate) fn start() -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("app-sandbox-")
            .tempdir()
            .wrap_err("cannot create the sandbox's directory")?;
        let mark = [
            MARK.as_bytes(),
            b"=",
            directory.path().as_os_str().as_encoded_bytes(),
        ]
        .concat();
        Ok(Self {
            directory: Some(directory),
            mark,
        })
    }

    /// The sandbox's directory, which its processes and this process see alike.
    pub(crate) fn inside(&self) -> &Path {
        self.directory
            .as_ref()
            .map_or(Path::new(""), tempfile::TempDir::path)
    }

    /// A command that runs `program` with `environment` alone and the mark, in `directory`.
    pub(crate) fn command(
        &self,
        program: &Path,
        directory: &Path,
        environment: &[(OsString, OsString)],
    ) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(directory)
            .env_clear()
            .envs(environment.iter().cloned())
            .env(MARK, self.inside());
        command
    }

    /// The processes that carry the mark, and every process under them, by PID.
    pub(crate) fn processes(&self) -> Vec<u32> {
        let parents: BTreeMap<u32, u32> = process::all()
            .into_iter()
            .filter_map(|pid| Some((pid, process::parent(pid)?)))
            .collect();
        let mut found: BTreeSet<u32> = parents
            .keys()
            .copied()
            .filter(|pid| self.marks(*pid))
            .collect();
        loop {
            let under: Vec<u32> = parents
                .iter()
                .filter(|(pid, parent)| found.contains(parent) && !found.contains(pid))
                .map(|(pid, _)| *pid)
                .collect();
            if under.is_empty() {
                return found.into_iter().collect();
            }
            found.extend(under);
        }
    }

    fn marks(&self, pid: u32) -> bool {
        std::fs::read(format!("/proc/{pid}/environ")).is_ok_and(|environ| {
            environ
                .split(|byte| *byte == 0)
                .any(|variable| variable == self.mark.as_slice())
        })
    }

    /// `SIGKILL` to every process of the environment, then delete its directory. Each is
    /// stopped first, until no new one shows: a process stopped forks no more.
    pub(crate) fn end(&mut self) -> Result<()> {
        let Some(directory) = self.directory.take() else {
            return Ok(());
        };
        let mut stopped: BTreeSet<u32> = BTreeSet::new();
        loop {
            let found: Vec<u32> = self
                .processes()
                .into_iter()
                .filter(|pid| !stopped.contains(pid))
                .collect();
            if found.is_empty() {
                break;
            }
            process::signal(Signal::STOP, &found);
            stopped.extend(found);
        }
        process::signal(Signal::KILL, &stopped.into_iter().collect::<Vec<_>>());
        let path = directory.path().to_owned();
        directory
            .close()
            .wrap_err_with(|| format!("cannot delete {}", path.display()))
    }
}

impl Drop for Plain {
    fn drop(&mut self) {
        let _ = self.end();
    }
}
