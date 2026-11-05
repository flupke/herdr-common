use super::*;

#[test]
fn numbers_rows_trims_their_ends_and_folds_empty_runs() {
    let text = "  alpha   \n\n beta\n   │\n\n   │  \n│ gamma │ x\n  ┃ \n\n";
    assert_eq!(
        numbered(text),
        "0:   alpha\n1:\n2:  beta\n3-5: (empty)\n6: │ gamma │ x\n7-8: (empty)"
    );
}

#[test]
fn changed_rows_ignore_what_the_screen_drops_and_count_missing_rows_as_empty() {
    let before = "title\nold  │\nsame\n\nlast";
    let after = "title\nnew\nsame   ┃\n";
    assert_eq!(changed_rows(before, after), [1, 4]);
}
