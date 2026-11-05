//! A terminal's screen as the tools return it: each row after its zero-based number, without
//! the spaces and box sides that end it, and each run of rows left empty folded into one line
//! that names them. A row keeps its columns, so a cell still counts from the start of its row,
//! after `row: `.

/// The characters that draw the vertical side of a box.
const SIDES: &[char] = &['│', '┃', '║'];

/// `text`, the screen's rows one per line, as the tools return it.
pub(crate) fn numbered(text: &str) -> String {
    let mut lines = Vec::new();
    let mut empty: Option<(usize, usize)> = None;
    for (row, line) in text.lines().enumerate() {
        let line = line.trim_end_matches(|character: char| {
            character.is_whitespace() || SIDES.contains(&character)
        });
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
