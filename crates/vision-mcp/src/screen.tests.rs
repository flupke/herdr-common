use super::*;

#[test]
fn numbers_rows_trims_their_ends_and_folds_empty_runs() {
    let text = "  alpha   \n\n beta\n   │\n\n   │  \n│ gamma │ x\n  ┃ \n\n";
    assert_eq!(
        numbered(text),
        "0:   alpha\n1:\n2:  beta\n3-5: (empty)\n6: │ gamma │ x\n7-8: (empty)"
    );
}
