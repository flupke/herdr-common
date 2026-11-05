//! A terminal's screen as the tools return it: each row after its zero-based number, without
//! the spaces and box sides that end it, and each run of rows left empty folded into one line
//! that names them. A row keeps its columns, so a cell still counts from the start of its row,
//! after `row: `. Beside it, the rows whose text differs from an earlier read of the screen.

/// The characters that draw the vertical side of a box.
const SIDES: &[char] = &['│', '┃', '║'];

/// `text`, the screen's rows one per line, as the tools return it.
pub(crate) fn numbered(text: &str) -> String {
    let mut lines = Vec::new();
    let mut empty: Option<(usize, usize)> = None;
    for (row, line) in text.lines().enumerate() {
        let line = shown(line);
        if line.is_empty() {
            empty = Some(empty.map_or((row, row), |(first, _)| (first, row)));
            continue;
        }
        lines.extend(empty.take().map(fold));
        lines.push(format!("{row}: {line}"));
    }
    lines.extend(empty.map(fold));
    lines.join("\n")
}

/// The rows of `after` whose text, as the tools return it, differs from the same row of
/// `before`: a row past the end of either counts as empty.
pub(crate) fn changed_rows(before: &str, after: &str) -> Vec<usize> {
    let before: Vec<&str> = before.lines().map(shown).collect();
    let after: Vec<&str> = after.lines().map(shown).collect();
    (0..before.len().max(after.len()))
        .filter(|row| before.get(*row).unwrap_or(&"") != after.get(*row).unwrap_or(&""))
        .collect()
}

/// `line` without the spaces and box sides that end it.
fn shown(line: &str) -> &str {
    line.trim_end_matches(|character: char| character.is_whitespace() || SIDES.contains(&character))
}

fn fold((first, last): (usize, usize)) -> String {
    if first == last {
        format!("{first}:")
    } else {
        format!("{first}-{last}: (empty)")
    }
}

#[cfg(test)]
#[path = "screen.tests.rs"]
mod tests;
