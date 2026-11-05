use super::*;

fn keys(typed: &str) -> Vec<Key> {
    let mut keys = Keys::default();
    typed.chars().filter_map(|c| keys.key(c)).collect()
}

#[test]
fn a_bracketed_paste_s_line_breaks_are_text_and_its_marks_no_keys() {
    assert_eq!(
        keys("\u{1b}[200~a\r\nb\nc\u{1b}[201~\r"),
        [
            Key::Char('a'),
            Key::Char('\n'),
            Key::Char('b'),
            Key::Char('\n'),
            Key::Char('c'),
            Key::Enter,
        ]
    );
}

#[test]
fn control_keys_edit_the_line_and_other_escape_sequences_are_skipped() {
    assert_eq!(
        keys("a\u{1b}[A\u{1b}[1;5Cb\u{7f}\u{15}\u{3}\u{4}é"),
        [
            Key::Char('a'),
            Key::Char('b'),
            Key::Backspace,
            Key::Clear,
            Key::Clear,
            Key::Exit,
            Key::Char('é'),
        ]
    );
}
