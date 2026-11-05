//! An agent's reply, as Claude Code 2.1.314 draws its Markdown links (seen in a terminal whose
//! `TERM_PROGRAM` is Herdr's): with `FORCE_HYPERLINK` set, each a terminal link (OSC 8) on
//! its text, in blue; without it, the text, then the link's target in parentheses.

use std::fmt::Write as _;
use std::path::Path;

/// The variable that has Claude Code write terminal links whatever the terminal.
const FORCE_HYPERLINK: &str = "FORCE_HYPERLINK";

/// Whether Claude Code launched with `args` writes terminal links: `FORCE_HYPERLINK` is set,
/// other than to `0`, in the `env` of the settings layer `--settings` names, which Claude Code
/// applies over its environment, or else in its environment.
pub(crate) fn hyperlinks(args: &[String]) -> bool {
    let layer = args
        .iter()
        .position(|arg| arg == "--settings")
        .and_then(|at| args.get(at + 1))
        .and_then(|path| std::fs::read_to_string(Path::new(path)).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|settings| settings["env"][FORCE_HYPERLINK].as_str().map(str::to_owned));
    layer
        .or_else(|| std::env::var(FORCE_HYPERLINK).ok())
        .is_some_and(|value| !value.is_empty() && value != "0")
}

/// `text` as Claude Code draws it, its `[label](target)` links as `hyperlinks` says.
pub(crate) fn render(text: &str, hyperlinks: bool) -> String {
    let mut drawn = String::new();
    let mut rest = text;
    while let Some((before, label, target, after)) = next_link(rest) {
        drawn.push_str(before);
        let _ = if hyperlinks {
            write!(
                drawn,
                "\x1b]8;;{target}\x07\x1b[94m{label}\x1b]8;;\x07\x1b[39m"
            )
        } else {
            write!(drawn, "{label} ({target})")
        };
        rest = after;
    }
    drawn.push_str(rest);
    drawn
}

/// The first Markdown link of `text`: what comes before it, its label, its target, and what
/// comes after it.
fn next_link(text: &str) -> Option<(&str, &str, &str, &str)> {
    let middle = text.find("](")?;
    // The label's own `[`: a `[` before it opens no link.
    let open = text[..middle].rfind('[')?;
    let close = middle + text[middle..].find(')')?;
    Some((
        &text[..open],
        &text[open + 1..middle],
        &text[middle + 2..close],
        &text[close + 1..],
    ))
}

#[cfg(test)]
#[path = "reply.tests.rs"]
mod tests;
