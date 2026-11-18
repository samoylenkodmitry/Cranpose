// A double tap resets a `zoomable` that a pinch or the app transformed when the
// two taps land close together. A press that moved past the drag threshold is
// not the first tap of a double tap.

use std::{cell::Cell, rc::Rc};

use cranpose_app_shell::{AppShell, PointerEventTime};
use cranpose_core::{location_key, remember};
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{Box, BoxSpec, Modifier, Point, ZoomState};

const TRANSFORMED_OFFSET: Point = Point::new(-30.0, 12.0);

// A 320 × 200 box that zooms with the returned state, with a first frame composed.
fn zoomable_shell() -> (AppShell<TestRenderer>, ZoomState) {
    let slot = Rc::new(Cell::new(None));
    let content_slot = Rc::clone(&slot);
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let state = remember(ZoomState::new).with(|state| *state);
            content_slot.set(Some(state));
            Box(
                Modifier::empty().width(320.0).height(200.0).zoomable(state),
                BoxSpec::default(),
                || {},
            );
        },
    );
    shell.set_viewport(320.0, 200.0);
    shell.set_buffer_size(320, 200);
    shell.update();
    (shell, slot.get().expect("the box composes its state"))
}

// No frame runs before or after the event.
fn at(shell: &AppShell<TestRenderer>, millis: i64) -> PointerEventTime {
    PointerEventTime {
        platform_time_ms: Some(millis),
        ..shell.exact_pointer_event_time(None)
    }
}

fn press(shell: &mut AppShell<TestRenderer>, x: f32, y: f32, millis: i64) {
    let time = at(shell, millis);
    shell.set_cursor_at_event_time(x, y, time);
    assert!(
        shell.pointer_pressed_at_event_time(time),
        "the press lands on the box"
    );
}

fn move_to(shell: &mut AppShell<TestRenderer>, x: f32, y: f32, millis: i64) {
    let time = at(shell, millis);
    shell.set_cursor_at_event_time(x, y, time);
}

fn release(shell: &mut AppShell<TestRenderer>, millis: i64) {
    let time = at(shell, millis);
    shell.pointer_released_at_event_time(time);
}

fn tap(shell: &mut AppShell<TestRenderer>, x: f32, y: f32, millis: i64) {
    press(shell, x, y, millis);
    release(shell, millis + 50);
}

#[test]
fn a_second_tap_close_to_the_first_resets_the_transform() {
    let (mut shell, state) = zoomable_shell();
    state.set_scale(2.0);

    tap(&mut shell, 10.0, 100.0, 1_000);
    tap(&mut shell, 13.0, 98.0, 1_200);

    assert!(!state.is_transformed(), "a double tap resets the zoom");
}

#[test]
fn a_second_tap_far_from_the_first_does_not_reset() {
    let (mut shell, state) = zoomable_shell();
    state.set_scale(2.0);

    tap(&mut shell, 10.0, 100.0, 1_000);
    tap(&mut shell, 310.0, 100.0, 1_200);

    assert_eq!(
        state.scale_non_reactive(),
        2.0,
        "taps further apart than the double-tap slop are two single taps"
    );
}

#[test]
fn a_press_that_moved_past_the_drag_threshold_is_not_the_first_tap() {
    let (mut shell, state) = zoomable_shell();
    state.set_offset(TRANSFORMED_OFFSET);

    press(&mut shell, 10.0, 100.0, 1_000);
    move_to(&mut shell, 70.0, 100.0, 1_025);
    release(&mut shell, 1_050);
    tap(&mut shell, 70.0, 100.0, 1_200);

    assert_eq!(
        state.offset_non_reactive(),
        TRANSFORMED_OFFSET,
        "a drag that ended in place is no tap, so the next tap is not a double tap"
    );
}
