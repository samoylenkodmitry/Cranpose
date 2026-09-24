use super::*;

#[test]
fn cursor_starts_visible() {
    let state = CursorAnimationState::new();
    assert!(state.is_visible());
    assert!(!state.is_active());
}

#[test]
fn start_schedules_blink() {
    let state = CursorAnimationState::new();
    state.start();
    assert!(state.is_active());
    assert!(state.next_blink_time().is_some());
}

#[test]
fn stop_clears_blink() {
    let state = CursorAnimationState::new();
    state.start();
    state.stop();
    assert!(!state.is_active());
    assert!(state.next_blink_time().is_none());
    assert!(state.is_visible());
}

#[test]
fn tick_toggles_visibility() {
    let state = CursorAnimationState::new();
    state.start();
    assert!(state.is_visible());

    let future_time =
        Instant::now() + CursorAnimationState::BLINK_INTERVAL + Duration::from_millis(1);
    let changed = state.tick(future_time);

    assert!(changed);
    assert!(!state.is_visible());

    let future_time2 =
        future_time + CursorAnimationState::BLINK_INTERVAL + Duration::from_millis(1);
    let changed2 = state.tick(future_time2);

    assert!(changed2);
    assert!(state.is_visible());
}

#[test]
fn cursor_blink_is_scoped_by_app_context() {
    let first = crate::render_state::AppContext::new_with_density(1.0);
    let second = crate::render_state::AppContext::new_with_density(1.0);

    first.enter(|| {
        stop_cursor_blink();
        start_cursor_blink();
        assert!(next_cursor_blink_time().is_some());
    });

    second.enter(|| {
        stop_cursor_blink();
        assert!(next_cursor_blink_time().is_none());
    });

    first.enter(|| {
        assert!(next_cursor_blink_time().is_some());
        stop_cursor_blink();
    });
}
