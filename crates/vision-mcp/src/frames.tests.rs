use super::*;

// The lines are hc 0.1.18's `hc events --json`, as it printed them in a sandbox.

const SNAPSHOT: &str = r#"{"channel":null,"seq":6,"terminals":[{"address":"t1","state":"live","title":"sh"},{"address":"t2","state":"live","title":"vision frame=3 ack=1 size=80x24"}]}"#;

fn title(terminal: &str, title: &str) -> String {
    serde_json::json!({"jsonrpc": "2.0", "method": "event", "params": {
        "data": {"title": title}, "seq": 7, "terminal": terminal, "type": "title"}})
    .to_string()
}

#[test]
fn a_marker_in_a_title_is_its_terminals_latest_frame_until_another_title() {
    let frames = Frames::default();
    frames.observe(SNAPSHOT);
    assert_eq!(frames.latest("t1"), None);
    assert_eq!(frames.latest("t2").map(|marker| marker.frame), Some(3));
    frames.observe(&title("t1", "vision frame=1 ack=0 size=80x24"));
    frames.observe(&title("t2", "vim"));
    assert_eq!(frames.latest("t1").map(|marker| marker.frame), Some(1));
    assert_eq!(frames.latest("t2"), None);
}

#[test]
fn a_wait_ends_when_the_terminals_program_finishes() {
    let frames = Frames::default();
    frames.observe(SNAPSHOT);
    frames.observe(
        r#"{"jsonrpc":"2.0","method":"event","params":{"data":{},"seq":8,"terminal":"t2","type":"finished"}}"#,
    );
    let waited = frames.wait("t2", Duration::from_secs(60), |marker| marker.frame > 3);
    assert!(matches!(waited, Err(Stalled::Ended)), "{waited:?}");
}

#[test]
fn a_spent_guard_reports_the_latest_frame() {
    let frames = Frames::default();
    frames.observe(SNAPSHOT);
    let waited = frames.wait("t2", Duration::ZERO, |marker| marker.frame > 3);
    assert!(
        matches!(waited, Err(Stalled::Guard(Some(marker))) if marker.frame == 3),
        "{waited:?}"
    );
}

#[test]
fn a_wait_ends_when_the_program_that_painted_leaves() {
    let frames = Arc::new(Frames::default());
    frames.observe(SNAPSHOT);
    let observer = Arc::clone(&frames);
    let waiting = std::thread::spawn(move || {
        observer.wait("t2", Duration::from_secs(60), |marker| marker.frame > 3)
    });
    frames.observe(&title("t2", ""));
    let waited = waiting.join().unwrap();
    assert!(matches!(waited, Err(Stalled::Left)), "{waited:?}");
}
