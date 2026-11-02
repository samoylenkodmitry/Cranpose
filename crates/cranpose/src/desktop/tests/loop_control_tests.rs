use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

use cranpose_app_shell::AppShell;
use cranpose_foundation::lazy::LazyListScope;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::{Box, BoxSpec, LazyColumn, LazyColumnSpec, Modifier, composable};
use winit::event_loop::ControlFlow;

use super::{LoopControlInputs, event_loop_control_flow};

const VSYNC: Duration = Duration::from_micros(16_667);

thread_local! {
    static COMPOSED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

// A list 120 high of rows 40 high, at rest, that keeps `beyond` rows after
// its viewport.
fn resting_list(beyond: usize) -> AppShell<WgpuRenderer> {
    cranpose_ui::set_idle_prefetch_passes(true);
    COMPOSED.with(|composed| composed.borrow_mut().clear());
    let mut shell = AppShell::new_with_size(
        WgpuRenderer::new(&[]),
        cranpose_core::location_key(file!(), line!(), column!()),
        move || RestingList(beyond),
        (320, 120),
        (320.0, 120.0),
    );
    shell.update();
    shell
}

#[composable]
fn RestingList(beyond: usize) {
    let mut spec = LazyColumnSpec::new();
    spec.beyond_bounds_item_count = beyond;
    LazyColumn(
        Modifier::empty().size_points(320.0, 120.0),
        cranpose_ui::rememberLazyListState(),
        spec,
        |scope| {
            scope.items(100, |index| {
                COMPOSED.with(|composed| composed.borrow_mut().push(index));
                Box(
                    Modifier::empty().size_points(320.0, 40.0),
                    BoxSpec::default(),
                    || {},
                );
            });
        },
    );
}

fn composed_rows() -> usize {
    COMPOSED.with(|composed| {
        let mut rows = composed.borrow().clone();
        rows.sort_unstable();
        rows.dedup();
        rows.len()
    })
}

fn quiet_loop() -> LoopControlInputs {
    LoopControlInputs {
        robot_needs_poll: false,
        free_running: false,
        redraw_requested: false,
        primary_pointer_polled: false,
        drag_poll_deadline: None,
        position_poll_deadline: None,
        frame_cap_deadline: None,
        has_active_animations: false,
        next_event_time: None,
    }
}

#[test]
fn a_waiting_loop_composes_the_rows_a_resting_list_left_beyond_its_viewport() {
    let mut shell = resting_list(2);
    assert_eq!(composed_rows(), 3, "the frame composes the rows in view");
    assert!(shell.has_idle_prefetch_work());

    let now = Instant::now();
    let inputs = quiet_loop().prefetch_idle(&mut shell, now, VSYNC);
    assert_eq!(
        composed_rows(),
        5,
        "the wait composes the two rows after the view"
    );
    assert!(!shell.has_idle_prefetch_work());
    assert_eq!(
        inputs.next_event_time, None,
        "with nothing left the loop waits"
    );
}

#[test]
fn a_frame_due_now_leaves_the_rows_for_a_later_wait() {
    for inputs in [
        LoopControlInputs {
            redraw_requested: true,
            ..quiet_loop()
        },
        LoopControlInputs {
            free_running: true,
            ..quiet_loop()
        },
    ] {
        let mut shell = resting_list(2);
        let inputs = inputs.prefetch_idle(&mut shell, Instant::now(), VSYNC);
        assert_eq!(composed_rows(), 3, "nothing runs before the frame");
        assert!(shell.has_idle_prefetch_work());
        assert_eq!(inputs.next_event_time, None);
    }
}

#[test]
fn a_wait_too_short_for_a_pass_runs_none_and_does_not_spin() {
    let mut shell = resting_list(2);
    let now = Instant::now();
    let timer = now + Duration::from_micros(500);
    let inputs = LoopControlInputs {
        next_event_time: Some(timer),
        ..quiet_loop()
    }
    .prefetch_idle(&mut shell, now, VSYNC);
    assert_eq!(composed_rows(), 3);
    assert_eq!(
        inputs.next_event_time,
        Some(timer),
        "the loop waits for its timer as it would have"
    );
}

#[test]
fn rows_left_after_the_passes_have_the_loop_look_again_at_once() {
    let mut shell = resting_list(12);
    let now = Instant::now();
    let inputs = quiet_loop().prefetch_idle(&mut shell, now, Duration::from_secs(1));
    assert!(composed_rows() > 3, "passes ran");
    assert!(
        shell.has_idle_prefetch_work(),
        "one wait runs at most four passes"
    );
    assert_eq!(inputs.next_event_time, Some(now));
}

#[test]
fn a_drag_poll_ahead_is_waited_for_and_one_that_is_due_is_run() {
    let now = Instant::now();
    let ahead = now + Duration::from_millis(16);
    assert_eq!(
        event_loop_control_flow(
            now,
            LoopControlInputs {
                drag_poll_deadline: Some(ahead),
                ..quiet_loop()
            }
        ),
        ControlFlow::WaitUntil(ahead),
        "a poll ahead is a deadline, not a reason to spin"
    );
    assert_eq!(
        event_loop_control_flow(
            now,
            LoopControlInputs {
                drag_poll_deadline: Some(now),
                ..quiet_loop()
            }
        ),
        ControlFlow::Poll
    );
    assert_eq!(
        event_loop_control_flow(now, quiet_loop()),
        ControlFlow::Wait
    );
}
