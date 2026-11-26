const SIDES: &[char] = &['│', '┃', '║'];

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

pub(crate) fn changed_rows(before: &str, after: &str) -> Vec<usize> {
    let before: Vec<&str> = before.lines().map(shown).collect();
    let after: Vec<&str> = after.lines().map(shown).collect();
    (0..before.len().max(after.len()))
        .filter(|row| before.get(*row).unwrap_or(&"") != after.get(*row).unwrap_or(&""))
        .collect()
}

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
