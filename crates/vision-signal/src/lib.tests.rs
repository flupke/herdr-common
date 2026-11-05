use super::*;

// The titles and requests are a format apps and the vision core share: each test spells it out.

#[test]
fn a_marker_is_the_title_the_core_reads() {
    let marker = FrameMarker {
        frame: 12,
        acknowledged: 3,
        columns: 100,
        rows: 30,
    };
    assert_eq!(marker.title(), "vision frame=12 ack=3 size=100x30");
    assert_eq!(
        marker.escape_sequence(),
        "\x1b]2;vision frame=12 ack=3 size=100x30\x07"
    );
    assert_eq!(
        FrameMarker::parse("vision frame=12 ack=3 size=100x30"),
        Some(marker)
    );
}

#[test]
fn other_titles_carry_no_marker() {
    for title in [
        "",
        "zsh",
        "vision",
        "vision frame=1 ack=0",
        "vision frame=x ack=0 size=1x1",
        "vision frame=1 ack=0 size=1x1 extra",
    ] {
        assert_eq!(FrameMarker::parse(title), None, "{title:?}");
    }
}

#[test]
fn a_request_is_the_text_the_app_reads() {
    assert_eq!(acknowledgement_request(7), "vision-acknowledge:7");
    assert_eq!(
        parse_acknowledgement_request("vision-acknowledge:7"),
        Some(7)
    );
    assert_eq!(parse_acknowledgement_request("hello"), None);
}
