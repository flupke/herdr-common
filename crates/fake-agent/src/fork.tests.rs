use super::*;

#[test]
fn a_fork_runs_its_settings_session_start_hooks_unless_claude_code_skips_the_settings() {
    let hooks = json!({"SessionStart": [{"hooks": [
        {"type": "command", "command": "echo first >&2"},
        {"type": "command", "command": "echo second >&2"},
    ]}]});

    assert_eq!(
        session_start_hooks(&json!({"hooks": hooks}).to_string()),
        ["first\n", "second\n"]
    );
    assert_eq!(
        session_start_hooks(&json!({"hooks": hooks, "permissions": {"allow": "Bash"}}).to_string()),
        Vec::<String>::new(),
        "settings whose permissions.allow is no list"
    );
}
