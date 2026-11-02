// How long the desktop event loop waits before it looks again: for a frame
// the pacing caps, a poll, a timed event or nothing.

use std::time::Instant;

use winit::event_loop::ControlFlow;

pub(super) struct LoopControlInputs {
    pub(super) robot_needs_poll: bool,
    pub(super) free_running: bool,
    pub(super) primary_pointer_polled: bool,
    pub(super) drag_poll_deadline: Option<Instant>,
    pub(super) position_poll_deadline: Option<Instant>,
    pub(super) frame_cap_deadline: Option<Instant>,
    pub(super) has_active_animations: bool,
    pub(super) next_event_time: Option<Instant>,
}

pub(super) fn event_loop_control_flow(now: Instant, inputs: LoopControlInputs) -> ControlFlow {
    let due = |deadline: Option<Instant>| deadline.is_some_and(|deadline| deadline <= now);
    if inputs.robot_needs_poll
        || inputs.free_running
        || inputs.primary_pointer_polled
        || due(inputs.drag_poll_deadline)
        || due(inputs.position_poll_deadline)
    {
        return ControlFlow::Poll;
    }
    let deadline = [
        inputs.frame_cap_deadline,
        inputs.position_poll_deadline,
        inputs.drag_poll_deadline,
    ]
    .into_iter()
    .flatten()
    .min();
    match deadline {
        Some(deadline) => ControlFlow::WaitUntil(deadline),
        None if inputs.has_active_animations => ControlFlow::Poll,
        None => inputs
            .next_event_time
            .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
    }
}
