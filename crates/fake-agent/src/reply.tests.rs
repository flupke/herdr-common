use super::*;

#[test]
fn a_markdown_link_is_a_terminal_link_or_its_text_and_target() {
    let text = "See [the PR](https://x/1) and [a [nested] label](https://x/2).";

    assert_eq!(
        render(text, true),
        "See \x1b]8;;https://x/1\x07\x1b[94mthe PR\x1b]8;;\x07\x1b[39m and [a \
         \x1b]8;;https://x/2\x07\x1b[94mnested] label\x1b]8;;\x07\x1b[39m."
    );
    assert_eq!(
        render(text, false),
        "See the PR (https://x/1) and [a nested] label (https://x/2)."
    );
}

#[test]
fn the_settings_layer_s_force_hyperlink_decides_over_the_environment() {
    let directory = tempfile::tempdir().unwrap();
    let layer = |value: &str| {
        let path = directory.path().join(format!("settings-{value}.json"));
        std::fs::write(
            &path,
            serde_json::json!({"env": {"FORCE_HYPERLINK": value}}).to_string(),
        )
        .unwrap();
        vec!["--settings".to_owned(), path.to_string_lossy().into_owned()]
    };

    assert!(hyperlinks(&layer("1")));
    assert!(!hyperlinks(&layer("0")));
}
