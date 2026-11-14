use super::*;

#[test]
fn a_redrawn_string_keeps_its_text() {
    shared_plain_annotated_string("SCORE 340");
    let second = shared_plain_annotated_string("SCORE 340");
    assert_eq!(second.text(), "SCORE 340");
    assert!(second.span_styles().is_empty());
}

#[test]
fn distinct_strings_keep_their_own_text() {
    let first = shared_plain_annotated_string("READY");
    let second = shared_plain_annotated_string("GO");
    assert_eq!(first.text(), "READY");
    assert_eq!(second.text(), "GO");
}

#[test]
fn the_pool_survives_overflowing_its_capacity() {
    for index in 0..600 {
        let text = format!("distinct-{index}");
        let shared = shared_plain_annotated_string(&text);
        assert_eq!(shared.text(), text);
    }
    let after = shared_plain_annotated_string("still correct");
    assert_eq!(after.text(), "still correct");
}

#[test]
fn a_redrawn_render_string_keeps_its_text() {
    shared_plain_render_string("SCORE 341");
    let second = shared_plain_render_string("SCORE 341");
    assert_eq!(second.text(), "SCORE 341");
    assert_eq!(
        second.text(),
        shared_plain_annotated_string("SCORE 341").text(),
        "both copies hold the same characters"
    );
}
