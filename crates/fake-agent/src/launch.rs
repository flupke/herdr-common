//! What Claude Code's arguments say about the session it starts, as Claude Code 2.1.318 reads
//! them.

/// The flag that gives Claude Code its session's ID.
const SESSION_ID: &str = "--session-id";
/// The flag that has Claude Code resume a session, by its ID.
const RESUME: &str = "--resume";
/// The flag that has Claude Code go on from the resumed session's conversation in a new
/// session: the one `--session-id` names, or a new one.
const FORK_SESSION: &str = "--fork-session";

/// How a session started, as `SessionStart` says it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Start {
    Startup,
    Resume,
    Clear,
    Fork,
}

impl Start {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Resume => "resume",
            Self::Clear => "clear",
            Self::Fork => "fork",
        }
    }
}

/// The session Claude Code launched with `args` starts, and how: a resume with
/// `--fork-session` starts as a fork, in the session `--session-id` names. When `args` name no
/// session, `variable` ([`crate::variables::SESSION`]) does: none when it is empty, a new one
/// when it is unset.
pub(crate) fn session(args: &[String], variable: Option<String>) -> (Option<String>, Start) {
    let resumed = flag_value(args, RESUME);
    let forked = args.iter().any(|arg| arg == FORK_SESSION);
    let start = match (&resumed, forked) {
        (Some(_), true) => Start::Fork,
        (Some(_), false) => Start::Resume,
        (None, _) => Start::Startup,
    };
    let named = resumed
        .filter(|_| !forked)
        .or_else(|| flag_value(args, SESSION_ID));
    let session = match (named, variable) {
        (Some(session), _) => Some(session),
        (None, Some(variable)) => (!variable.is_empty()).then_some(variable),
        (None, None) => Some(new_session_id()),
    };
    (session, start)
}

/// The value Claude Code was given with `flag`, as `--flag <value>` or `--flag=<value>`.
pub(crate) fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().enumerate().find_map(|(at, arg)| {
        if arg == flag {
            args.get(at + 1).cloned()
        } else {
            arg.strip_prefix(flag)?.strip_prefix('=').map(str::to_owned)
        }
    })
}

/// The prompt Claude Code was launched with, after `--`, which it works on at once.
pub(crate) fn first_prompt(args: &[String]) -> Option<&str> {
    match args {
        [.., end_of_flags, prompt] if end_of_flags == "--" => Some(prompt),
        _ => None,
    }
}

/// A new random session ID, as Claude Code picks one when it is given none: a version 4 UUID.
pub(crate) fn new_session_id() -> String {
    use std::fmt::Write as _;
    use std::io::Read as _;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut bytes))
        .expect("random bytes");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    });
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

#[cfg(test)]
#[path = "launch.tests.rs"]
mod tests;
