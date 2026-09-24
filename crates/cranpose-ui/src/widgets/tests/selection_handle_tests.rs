use super::*;
use crate::text_selection::HANDLE_RADIUS;

const LINE_HEIGHT: f32 = 20.0;

#[test]
fn cursor_handle_touch_box_sits_at_or_below_the_line_bottom() {
    let shape = handle_shape(HandleKind::Cursor, HANDLE_RADIUS, LINE_HEIGHT);
    assert!(
        shape.tip_in_box.y.abs() < 0.01,
        "cursor handle anchor must sit at the top edge of its touch box \
         (tip_in_box.y = {}), so it never overlaps the text line above",
        shape.tip_in_box.y
    );
    assert!(
        shape.box_size.height >= 2.0 * HANDLE_RADIUS,
        "cursor handle box must extend below the anchor so the dot is grabbable"
    );
    assert!(
        !shape.path_data.contains('L'),
        "cursor handle must draw only its dot (no stem rectangle): {}",
        shape.path_data
    );
}

#[test]
fn grab_region_is_finger_sized() {
    for kind in [
        HandleKind::Cursor,
        HandleKind::SelectionStart,
        HandleKind::SelectionEnd,
    ] {
        let rect = handle_grab_rect(
            kind,
            Point { x: 100.0, y: 100.0 },
            HANDLE_RADIUS,
            LINE_HEIGHT,
        );
        assert!(
            rect.width >= 48.0,
            "{kind:?}: grab region must be at least a fingertip wide, got {}",
            rect.width
        );
        assert!(
            rect.height >= 32.0,
            "{kind:?}: grab region must be at least a fingertip tall, got {}",
            rect.height
        );
    }
}

#[test]
fn start_and_end_grab_regions_mirror_about_the_line() {
    let tip = Point { x: 100.0, y: 100.0 };
    let line_top = tip.y - LINE_HEIGHT;
    let start = handle_grab_rect(HandleKind::SelectionStart, tip, HANDLE_RADIUS, LINE_HEIGHT);
    let end = handle_grab_rect(HandleKind::SelectionEnd, tip, HANDLE_RADIUS, LINE_HEIGHT);

    assert!(
        (start.width - end.width).abs() < 0.01 && (start.height - end.height).abs() < 0.01,
        "start {start:?} and end {end:?} grab boxes must be the same size"
    );
    let start_above = line_top - start.y;
    let end_below = (end.y + end.height) - tip.y;
    assert!(
        (start_above - end_below).abs() < 0.01,
        "start reach above the line ({start_above}) must equal end reach below ({end_below})"
    );
    assert!(
        ((start.y + start.height) - tip.y).abs() < 0.01,
        "start grab box must stop at the line bottom (no slop below)"
    );
    assert!(
        (end.y - line_top).abs() < 0.01,
        "end grab box must start at the line top (no slop above)"
    );
}

#[test]
fn grab_regions_cover_the_dot_and_stem_but_not_neighbouring_lines() {
    let tip = Point { x: 100.0, y: 100.0 };
    let line_top = tip.y - LINE_HEIGHT;
    let contains =
        |r: Rect, x: f32, y: f32| x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height;

    let cursor = handle_grab_rect(HandleKind::Cursor, tip, HANDLE_RADIUS, LINE_HEIGHT);
    assert!(contains(cursor, tip.x - 16.0, tip.y + 12.0));
    assert!(contains(cursor, tip.x + 16.0, tip.y + 12.0));
    assert!(
        !contains(cursor, tip.x, tip.y - 2.0),
        "cursor: a press on the glyph line belongs to the field"
    );

    let start = handle_grab_rect(HandleKind::SelectionStart, tip, HANDLE_RADIUS, LINE_HEIGHT);
    assert!(
        contains(start, tip.x, line_top - HANDLE_RADIUS),
        "start: a press on the dot above the line must grab the handle"
    );
    assert!(
        contains(start, tip.x, tip.y - LINE_HEIGHT * 0.5),
        "start: a press on the stem column (on the line) must grab the handle"
    );
    assert!(
        !contains(start, tip.x, tip.y + 4.0),
        "start: a press below the line belongs to the field"
    );

    let end = handle_grab_rect(HandleKind::SelectionEnd, tip, HANDLE_RADIUS, LINE_HEIGHT);
    assert!(
        contains(end, tip.x, tip.y + HANDLE_RADIUS),
        "end: a press on the dot below the line must grab the handle"
    );
    assert!(
        contains(end, tip.x, tip.y - LINE_HEIGHT * 0.5),
        "end: a press on the stem column (on the line) must grab the handle"
    );
    assert!(
        !contains(end, tip.x, line_top - 4.0),
        "end: a press above the line belongs to the field"
    );
}
