//! The host's processes, as `/proc` shows them, and signals to them by host PID.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

use eyre::{Result, ensure};
use rustix::process::{Pid, Signal, kill_process};

/// How often [`kill_trees`] looks whether the processes it killed ended: the kernel says so
/// only to their parent.
const ENDED_POLL: Duration = Duration::from_millis(5);

/// Every process's PID, from `/proc`.
pub(crate) fn all() -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
        .collect()
}

/// The fields of `/proc/<pid>/stat` from the state on: none once the process is gone. The
/// command's name before them, in parentheses, may hold spaces and parentheses, so they start
/// after the last `)`.
fn stat(pid: u32) -> Option<Vec<String>> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(')')?;
    Some(fields.split_whitespace().map(str::to_owned).collect())
}

/// The parent of the process `pid`: none once it is gone.
pub(crate) fn parent(pid: u32) -> Option<u32> {
    stat(pid)?.get(1)?.parse().ok()
}

/// The state of the process `pid`, such as `S`, `T` (stopped) or `Z` (a zombie its parent has
/// yet to reap): none once it is gone.
fn state(pid: u32) -> Option<String> {
    stat(pid)?.into_iter().next()
}

/// Send `signal` to each process of `pids`. A process that ended meanwhile is no error.
pub(crate) fn signal(signal: Signal, pids: &[u32]) {
    for pid in pids {
        if let Some(pid) = i32::try_from(*pid).ok().and_then(Pid::from_raw) {
            let _ = kill_process(pid, signal);
        }
    }
}

/// `SIGKILL` to each process of `roots` and every process under them, as a crash or a reboot
/// ends them, and return once each ended, within `guard`. Each is stopped first, from the roots
/// down, until no new one shows: a process stopped forks no more, and one killed before its
/// children would leave them to a parent out of reach.
///
/// `nsenter`, the program that runs a bubblewrap sandbox's commands, stops itself when its
/// child stops, and reaps it only once continued: a root whose parent runs it is waited for
/// until reaped, its parent continued (`SIGCONT`) whenever stopped. No other process outside
/// the trees gets a signal.
pub(crate) fn kill_trees(roots: &[u32], nsenter: Option<&Path>, guard: Duration) -> Result<()> {
    let mut stopped: BTreeSet<u32> = BTreeSet::new();
    let mut found: Vec<u32> = roots.to_vec();
    while !found.is_empty() {
        signal(Signal::STOP, &found);
        stopped.extend(&found);
        found = all()
            .into_iter()
            .filter(|pid| !stopped.contains(pid))
            .filter(|pid| parent(*pid).is_some_and(|parent| stopped.contains(&parent)))
            .collect();
    }
    signal(Signal::KILL, &stopped.iter().copied().collect::<Vec<_>>());
    let nsenter = nsenter.and_then(|path| std::fs::canonicalize(path).ok());
    let runs_nsenter = |pid: u32| {
        nsenter.as_ref().is_some_and(|nsenter| {
            std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|exe| &exe == nsenter)
        })
    };
    let deadline = Instant::now() + guard;
    loop {
        let running = stopped
            .iter()
            .copied()
            .find(|pid| state(*pid).is_some_and(|state| !matches!(state.as_str(), "Z" | "X")));
        let unreaped: Vec<u32> = roots
            .iter()
            .copied()
            .filter(|root| parent(*root).is_some_and(runs_nsenter))
            .collect();
        let stopped_parents: Vec<u32> = unreaped
            .iter()
            .filter_map(|root| parent(*root))
            .filter(|parent| state(*parent).as_deref() == Some("T"))
            .collect();
        signal(Signal::CONT, &stopped_parents);
        let Some(waiting) = running.or(unreaped.first().copied()) else {
            return Ok(());
        };
        ensure!(
            Instant::now() < deadline,
            "the process {waiting} did not end within {guard:?} of its SIGKILL"
        );
        std::thread::sleep(ENDED_POLL);
    }
}
