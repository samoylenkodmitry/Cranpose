use super::*;

fn rect(y: f32, height: f32) -> Rect {
    Rect {
        x: 0.0,
        y,
        width: 100.0,
        height,
    }
}

#[test]
fn visible_caret_needs_no_scroll() {
    let viewport = rect(0.0, 1000.0);
    let caret = rect(400.0, 20.0);
    assert_eq!(scroll_delta_to_reveal(caret, viewport, 0.0), 0.0);
}

#[test]
fn caret_behind_keyboard_scrolls_content_up() {
    let viewport = rect(0.0, 1000.0);
    let caret = rect(700.0, 20.0);
    let delta = scroll_delta_to_reveal(caret, viewport, 400.0);
    assert!(
        delta > 0.0,
        "expected a positive (content-up) delta, got {delta}"
    );
    assert!(
        (delta - (720.0 + BRING_INTO_VIEW_MARGIN - 600.0)).abs() < 0.01,
        "delta {delta} should reveal the caret bottom plus a margin"
    );
    let revealed_bottom = caret.y + caret.height - delta;
    assert!(revealed_bottom <= 600.0 - BRING_INTO_VIEW_MARGIN + 0.01);
}

#[test]
fn caret_just_below_fold_uses_margin() {
    let viewport = rect(0.0, 1000.0);
    let caret = rect(580.0, 20.0);
    let delta = scroll_delta_to_reveal(caret, viewport, 400.0);
    assert!((delta - BRING_INTO_VIEW_MARGIN).abs() < 0.01, "got {delta}");
}

#[test]
fn caret_above_viewport_top_scrolls_content_down() {
    let viewport = rect(200.0, 800.0);
    let caret = rect(150.0, 20.0);
    let delta = scroll_delta_to_reveal(caret, viewport, 0.0);
    assert!(
        delta < 0.0,
        "expected a negative (content-down) delta, got {delta}"
    );
    assert!(
        (delta - (150.0 - BRING_INTO_VIEW_MARGIN - 200.0)).abs() < 0.01,
        "delta {delta} should reveal the caret top with a margin"
    );
}

#[test]
fn no_usable_space_does_not_scroll() {
    let viewport = rect(0.0, 300.0);
    let caret = rect(280.0, 20.0);
    assert_eq!(scroll_delta_to_reveal(caret, viewport, 400.0), 0.0);
}

#[test]
fn responder_identity_equality() {
    let r = BringIntoViewResponder::new(|_, _| {});
    assert_eq!(r, r.clone());
    let other = BringIntoViewResponder::new(|_, _| {});
    assert_ne!(r, other);
}

#[test]
fn responder_forwards_request() {
    let seen: Rc<Cell<Option<(f32, f32)>>> = Rc::new(Cell::new(None));
    let seen2 = seen.clone();
    let r = BringIntoViewResponder::new(move |caret: Rect, ime: f32| {
        seen2.set(Some((caret.y, ime)));
    });
    r.bring_into_view(rect(42.0, 20.0), 300.0);
    assert_eq!(seen.get(), Some((42.0, 300.0)));
}
