use std::{cell::RefCell, rc::Rc};

use cranpose_animation::{Animatable, spring};
use cranpose_core::{RuntimeHandle, State, with_current_composer};
use cranpose_macros::composable;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ControlContactKind {
    Thumb,
    Segment,
}

pub(super) struct ControlContactMotion {
    channels: RefCell<[Animatable<f32>; 2]>,
    runtime: RuntimeHandle,
    kind: ControlContactKind,
}

impl ControlContactMotion {
    fn new(runtime: RuntimeHandle, kind: ControlContactKind) -> Self {
        Self {
            channels: RefCell::new([
                Animatable::new(0.0, runtime.clone()),
                Animatable::new(0.0, runtime.clone()),
            ]),
            runtime,
            kind,
        }
    }

    pub(super) fn states(&self) -> (State<f32>, State<f32>) {
        let channels = self.channels.borrow();
        (channels[0].state(), channels[1].state())
    }

    pub(super) fn pressed(&self, down: bool, event_time: Option<u64>) {
        let geometry = match (self.kind, down) {
            (ControlContactKind::Thumb, true) => spring(0.55, 600.0),
            (ControlContactKind::Thumb, false) => spring(0.7, 170.0),
            (ControlContactKind::Segment, true) => spring(1.0, 600.0),
            (ControlContactKind::Segment, false) => spring(1.0, 650.0),
        };
        let material = if down { geometry } else { spring(1.0, 250.0) };
        let time = event_time.or_else(|| self.runtime.last_frame_time_nanos());
        for (channel, animation) in self
            .channels
            .borrow_mut()
            .iter_mut()
            .zip([geometry, material])
        {
            let target = f32::from(down);
            if channel.target() != target {
                if let Some(time) = time {
                    channel.animate_to_at(target, animation, time);
                } else {
                    channel.animateTo(target, animation);
                }
            }
        }
    }
}

#[composable]
pub(super) fn remember_control_contact(kind: ControlContactKind) -> Rc<ControlContactMotion> {
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(move || Rc::new(ControlContactMotion::new(runtime, kind)))
            .with(Rc::clone)
    })
}

#[cfg(test)]
#[path = "tests/control_motion_tests.rs"]
mod tests;
