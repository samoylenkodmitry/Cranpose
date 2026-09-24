use super::*;

#[test]
fn tap_classification_escalates_within_time_and_slop() {
    assert_eq!(classify_tap_count(None, 0, 10.0, 10.0, 500, 24.0), 1);
    assert_eq!(
        classify_tap_count(Some((1, 10.0, 10.0)), 100, 11.0, 12.0, 500, 24.0),
        2
    );
    assert_eq!(
        classify_tap_count(Some((2, 10.0, 10.0)), 100, 11.0, 12.0, 500, 24.0),
        3
    );
    assert_eq!(
        classify_tap_count(Some((3, 10.0, 10.0)), 100, 11.0, 12.0, 500, 24.0),
        4
    );
    assert_eq!(
        classify_tap_count(Some((4, 10.0, 10.0)), 100, 11.0, 12.0, 500, 24.0),
        5
    );
}

#[test]
fn tap_classification_resets_past_timeout_or_slop() {
    assert_eq!(
        classify_tap_count(Some((1, 10.0, 10.0)), 600, 10.0, 10.0, 500, 24.0),
        1
    );
    assert_eq!(
        classify_tap_count(Some((1, 10.0, 10.0)), 50, 100.0, 10.0, 500, 24.0),
        1
    );
    assert_eq!(
        classify_tap_count(Some((3, 10.0, 10.0)), 600, 10.0, 10.0, 500, 24.0),
        1
    );
}

#[test]
fn tap_inside_selection_cycles_word_line_paragraph_by_location() {
    use SelectionGranularity::*;

    let mut count = resolve_selection_tap_count(1, 0, true, false);
    assert_eq!(count, 2);
    assert_eq!(tap_selection_granularity(count), Word);

    count = resolve_selection_tap_count(1, count, true, true);
    assert_eq!(count, 3);
    assert_eq!(tap_selection_granularity(count), Line);

    count = resolve_selection_tap_count(1, count, true, true);
    assert_eq!(count, 4);
    assert_eq!(tap_selection_granularity(count), Paragraph);

    count = resolve_selection_tap_count(1, count, true, true);
    assert_eq!(count, 5);
    assert_eq!(tap_selection_granularity(count), Word);

    let reset = resolve_selection_tap_count(1, count, true, false);
    assert_eq!(reset, 2);
    assert_eq!(tap_selection_granularity(reset), Word);
}

#[test]
fn resolve_tap_count_preserves_rapid_multitap_and_caret() {
    assert_eq!(resolve_selection_tap_count(2, 1, false, false), 2);
    assert_eq!(resolve_selection_tap_count(3, 2, true, true), 3);
    assert_eq!(resolve_selection_tap_count(1, 4, false, true), 1);
}

#[test]
fn tap_granularity_grows_then_cycles() {
    use SelectionGranularity::*;
    assert_eq!(tap_selection_granularity(0), Caret);
    assert_eq!(tap_selection_granularity(1), Caret);
    assert_eq!(tap_selection_granularity(2), Word);
    assert_eq!(tap_selection_granularity(3), Line);
    assert_eq!(tap_selection_granularity(4), Paragraph);
    assert_eq!(tap_selection_granularity(5), Word);
    assert_eq!(tap_selection_granularity(6), Line);
    assert_eq!(tap_selection_granularity(7), Paragraph);
    assert_eq!(tap_selection_granularity(8), Word);
}

#[test]
fn paragraph_boundaries_span_blank_line_delimited_blocks() {
    let text = "line one\nline two\n\nsecond para\nstill second\n\n\nthird";
    let (s, e) = find_paragraph_boundaries(text, 3);
    assert_eq!(&text[s..e], "line one\nline two");
    let (s, e) = find_paragraph_boundaries(text, 20);
    assert_eq!(&text[s..e], "second para\nstill second");
    let (s, e) = find_paragraph_boundaries(text, text.len());
    assert_eq!(&text[s..e], "third");
}

#[test]
fn paragraph_boundaries_no_blank_line_is_whole_text() {
    let text = "just\none\nblock";
    assert_eq!(find_paragraph_boundaries(text, 5), (0, text.len()));
}

#[test]
fn paragraph_boundaries_are_unicode_aware() {
    let text = "\u{4e2d}\u{6587}\u{6bb5}\u{843d}\n\n\u{6b21}";
    let first = "\u{4e2d}\u{6587}\u{6bb5}\u{843d}";
    let (s, e) = find_paragraph_boundaries(text, 3);
    assert_eq!(&text[s..e], first);
    assert!(text.is_char_boundary(s) && text.is_char_boundary(e));
}

#[test]
fn line_boundaries_span_between_newlines() {
    let text = "first line\nsecond line\nthird";
    assert_eq!(find_line_boundaries(text, 15), (11, 22));
    assert_eq!(find_line_boundaries(text, 0), (0, 10));
    assert_eq!(find_line_boundaries(text, 25), (23, text.len()));
}

#[test]
fn line_boundaries_handle_unicode_and_empty_lines() {
    let text = "\u{00e9}\u{00e8}\n\n\u{4e2d}\u{6587}";
    let (start, end) = find_line_boundaries(text, "\u{00e9}\u{00e8}\n".len());
    assert_eq!(start, end);
    let last = find_line_boundaries(text, text.len());
    assert_eq!(&text[last.0..last.1], "\u{4e2d}\u{6587}");
}

#[test]
fn handle_path_is_valid_and_spans_the_line_box() {
    let (x, top, bottom) = (40.0_f32, 20.0_f32, 40.0_f32);
    for kind in [
        HandleKind::Cursor,
        HandleKind::SelectionStart,
        HandleKind::SelectionEnd,
    ] {
        let data = handle_path_data(kind, x, top, bottom, HANDLE_RADIUS);
        let path =
            cranpose_ui_graphics::VectorPath::parse(&data).expect("handle path must be valid SVG");
        assert!(!path.is_empty(), "{kind:?} handle must have geometry");
        let bounds = path.bounds();
        assert!(bounds.y <= top + 0.5, "{kind:?} must reach the line top");
        assert!(
            bounds.y + bounds.height >= bottom - 0.5,
            "{kind:?} must reach the line bottom"
        );
        assert!((bounds.x - (x - HANDLE_RADIUS)).abs() <= 0.5);
        assert!((bounds.x + bounds.width - (x + HANDLE_RADIUS)).abs() <= 0.5);
    }
}

#[test]
fn selection_handle_dots_sit_on_the_correct_side_of_the_line() {
    let (x, top, bottom, r) = (40.0_f32, 20.0_f32, 40.0_f32, HANDLE_RADIUS);
    let eps = 0.5_f32;

    let bounds = |kind: HandleKind| {
        let data = handle_path_data(kind, x, top, bottom, r);
        cranpose_ui_graphics::VectorPath::parse(&data)
            .expect("valid handle path")
            .bounds()
    };

    let start = bounds(HandleKind::SelectionStart);
    assert!(
        (start.y - (top - 2.0 * r + HANDLE_DOT_LINE_OVERLAP)).abs() <= eps,
        "start dot must ride on top of the line (top at {}, expected {})",
        start.y,
        top - 2.0 * r + HANDLE_DOT_LINE_OVERLAP
    );
    assert!(
        start.y + start.height <= bottom + eps,
        "start handle must not extend below the line box"
    );

    for kind in [HandleKind::SelectionEnd, HandleKind::Cursor] {
        let b = bounds(kind);
        assert!(
            (b.y + b.height - (bottom + 2.0 * r - HANDLE_DOT_LINE_OVERLAP)).abs() <= eps,
            "{kind:?} dot must hang below the line (bottom at {}, expected {})",
            b.y + b.height,
            bottom + 2.0 * r - HANDLE_DOT_LINE_OVERLAP
        );
        assert!(
            b.y >= top - eps,
            "{kind:?} handle must not extend above the line box"
        );
    }
}

#[test]
fn caret_visual_line_resolves_wrapped_visual_lines() {
    let ranges = vec![0..5usize, 5..9, 10..12];

    assert_eq!(
        caret_visual_line(&ranges, 0, LineAffinity::Downstream),
        (0, 0)
    );
    assert_eq!(
        caret_visual_line(&ranges, 3, LineAffinity::Downstream),
        (0, 0)
    );
    assert_eq!(
        caret_visual_line(&ranges, 5, LineAffinity::Downstream),
        (1, 5)
    );
    assert_eq!(
        caret_visual_line(&ranges, 7, LineAffinity::Downstream),
        (1, 5)
    );
    assert_eq!(
        caret_visual_line(&ranges, 9, LineAffinity::Downstream),
        (1, 5)
    );
    assert_eq!(
        caret_visual_line(&ranges, 11, LineAffinity::Downstream),
        (2, 10)
    );
    assert_eq!(
        caret_visual_line(&ranges, 12, LineAffinity::Downstream),
        (2, 10)
    );
}

#[test]
fn start_handle_grab_never_drifts() {
    let mut grab = HandleGrabOffset::begin_for(108.0, 100.0, false);
    assert_eq!(grab.track(108.0), 8.0);
    assert_eq!(grab.track(160.0), 8.0, "no drift on long downward travel");
    assert_eq!(grab.drift_progress(), 0.0);
}

#[test]
fn grab_offset_has_follow_drift_and_strict_phases() {
    let mut grab = HandleGrabOffset::begin(108.0, 100.0);
    assert_eq!(grab.bias(), 8.0);

    let direct_bias = grab.track(108.0);
    assert_eq!(direct_bias, 8.0, "initial travel follows exactly");
    assert_eq!(108.0 + direct_bias, 116.0);

    let drifting_bias = grab.track(132.0);
    assert!(drifting_bias < 8.0 && drifting_bias > grab_bias_full_view());
    assert!((0.0..1.0).contains(&grab.drift_progress()));

    assert_eq!(grab.track(156.0), grab_bias_full_view());
    assert_eq!(grab.drift_progress(), 1.0);
    assert_eq!(grab.track(220.0), grab_bias_full_view());
}

#[test]
fn grab_offset_is_cadence_independent_and_never_unwinds() {
    let mut single = HandleGrabOffset::begin(108.0, 100.0);
    single.track(140.0);

    let mut sampled = HandleGrabOffset::begin(108.0, 100.0);
    for y in [104.0, 109.0, 116.0, 130.0, 140.0] {
        sampled.track(y);
    }
    assert_eq!(sampled.bias(), single.bias());
    assert_eq!(sampled.drift_progress(), single.drift_progress());

    let migrated = sampled.bias();
    sampled.track(90.0);
    assert_eq!(
        sampled.bias(),
        migrated,
        "upward travel cannot unwind drift"
    );

    let deep = grab_bias_full_view() - 10.0;
    let mut already_visible = HandleGrabOffset::begin(deep, 0.0);
    already_visible.track(100.0);
    assert_eq!(already_visible.bias(), deep);
}

#[test]
fn caret_visual_line_handles_empty_ranges() {
    assert_eq!(caret_visual_line(&[], 5, LineAffinity::Upstream), (0, 0));
    assert_eq!(caret_visual_line(&[], 5, LineAffinity::Downstream), (0, 0));
}

#[test]
fn caret_visual_line_upstream_anchors_shared_wrap_boundary_to_upper_line() {
    let ranges = vec![0..5usize, 5..9, 10..12];

    assert_eq!(
        caret_visual_line(&ranges, 5, LineAffinity::Upstream),
        (0, 0)
    );
    assert_eq!(
        caret_visual_line(&ranges, 5, LineAffinity::Downstream),
        (1, 5)
    );

    assert_eq!(
        caret_visual_line(&ranges, 3, LineAffinity::Upstream),
        (0, 0)
    );
    assert_eq!(
        caret_visual_line(&ranges, 7, LineAffinity::Upstream),
        (1, 5)
    );

    assert_eq!(
        caret_visual_line(&ranges, 10, LineAffinity::Upstream),
        (2, 10)
    );

    assert_eq!(
        caret_visual_line(&ranges, 12, LineAffinity::Upstream),
        (2, 10)
    );
}

#[test]
fn handle_drag_keeps_edges_from_crossing() {
    assert_eq!(
        selection_after_handle_drag(HandleKind::SelectionEnd, 5, 2, 20),
        (5, 6)
    );
    assert_eq!(
        selection_after_handle_drag(HandleKind::SelectionEnd, 5, 12, 20),
        (5, 12)
    );
    assert_eq!(
        selection_after_handle_drag(HandleKind::SelectionStart, 8, 10, 20),
        (7, 8)
    );
    assert_eq!(
        selection_after_handle_drag(HandleKind::SelectionStart, 8, 3, 20),
        (3, 8)
    );
    assert_eq!(
        selection_after_handle_drag(HandleKind::Cursor, 4, 9, 20),
        (9, 9)
    );
}
