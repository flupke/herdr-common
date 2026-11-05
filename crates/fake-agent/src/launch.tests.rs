use super::*;

#[test]
fn the_session_and_its_start_are_the_ones_claude_was_launched_with() {
    let old = "0f6c3a52-1d2e-4c1b-9a77-3c2f5e8d9b10";
    let new = "7d1e0b6a-5c4f-4e2a-8b39-1a2b3c4d5e6f";
    let session_id = format!("--session-id={old}");
    let resume = format!("--resume={old}");
    let cases = [
        (
            "a new session",
            vec!["--model", "m", "--session-id", old, "-n", "x"],
            None,
            Some(old),
            Start::Startup,
        ),
        (
            "a new session, joined",
            vec![session_id.as_str()],
            Some(new),
            Some(old),
            Start::Startup,
        ),
        (
            "a resume",
            vec!["--resume", old],
            None,
            Some(old),
            Start::Resume,
        ),
        (
            "a resume, joined",
            vec![resume.as_str(), "--model", "m"],
            None,
            Some(old),
            Start::Resume,
        ),
        (
            "a forked resume",
            vec!["--resume", old, "--fork-session", "--session-id", new],
            None,
            Some(new),
            Start::Fork,
        ),
        (
            "the variable's session",
            vec!["--model", "m"],
            Some(new),
            Some(new),
            Start::Startup,
        ),
        ("no session", vec![], Some(""), None, Start::Startup),
    ];
    for (case, launched, variable, expected, start) in cases {
        let launched: Vec<String> = launched.into_iter().map(str::to_owned).collect();
        let variable = variable.map(str::to_owned);
        assert_eq!(
            session(&launched, variable),
            (expected.map(str::to_owned), start),
            "{case}"
        );
    }
}

#[test]
fn a_session_none_names_is_new_each_time() {
    let (first, _) = session(&[], None);
    let (second, _) = session(&[], None);

    assert_ne!(first, second);
}
