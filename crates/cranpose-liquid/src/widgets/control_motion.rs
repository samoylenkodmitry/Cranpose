use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{Animatable, spring};
use cranpose_core::{
    RuntimeHandle, State, internal::FrameCallbackRegistration, with_current_composer,
};
use cranpose_macros::composable;

use crate::motion::Press;

// iOS holds a touched control's lens up for at least this long, so a quick
// tap shows the whole lift: its toggle tap starts the release 225 ms after
// the touch, where a longer hold releases on lift-off.
const MIN_LIFT_NANOS: u64 = 225_000_000;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ControlContactKind {
    Thumb,
    Segment,
}

pub(super) struct ControlContactMotion {
    channels: RefCell<[Animatable<f32>; 2]>,
    runtime: RuntimeHandle,
    kind: ControlContactKind,
    lifted_at: Cell<Option<u64>>,
    pending_release: RefCell<Option<FrameCallbackRegistration>>,
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
            lifted_at: Cell::new(None),
            pending_release: RefCell::new(None),
        }
    }

    pub(super) fn states(&self) -> (State<f32>, State<f32>) {
        let channels = self.channels.borrow();
        (channels[0].state(), channels[1].state())
    }

    pub(super) fn pressed(self: &Rc<Self>, press: Press, event_time: Option<u64>) {
        let time = event_time.or_else(|| self.runtime.last_frame_time_nanos());
        drop(self.pending_release.take());
        match press {
            Press::Down => {
                self.lifted_at.set(time);
                self.animate(Press::Down, time);
            }
            Press::Up => {
                let release_at = self
                    .lifted_at
                    .take()
                    .map(|lifted| lifted.saturating_add(MIN_LIFT_NANOS))
                    .filter(|release_at| time.is_some_and(|now| *release_at > now));
                match release_at {
                    Some(release_at) => self.release_at(release_at),
                    None => self.animate(Press::Up, time),
                }
            }
        }
    }

    fn release_at(self: &Rc<Self>, release_at: u64) {
        let motion = Rc::downgrade(self);
        let registration = self.runtime.frame_clock().with_frame_nanos(move |frame| {
            let Some(motion) = motion.upgrade() else {
                return;
            };
            if frame >= release_at {
                drop(motion.pending_release.take());
                motion.animate(Press::Up, Some(release_at));
            } else {
                motion.release_at(release_at);
            }
        });
        drop(self.pending_release.replace(Some(registration)));
    }

    fn animate(&self, press: Press, time: Option<u64>) {
        let geometry = match (self.kind, press) {
            (ControlContactKind::Thumb, Press::Down) => spring(0.55, 600.0),
            (ControlContactKind::Thumb, Press::Up) => spring(0.7, 170.0),
            (ControlContactKind::Segment, Press::Down) => spring(1.0, 600.0),
            (ControlContactKind::Segment, Press::Up) => spring(1.0, 650.0),
        };
        let material = match press {
            Press::Down => geometry,
            Press::Up => spring(1.0, 250.0),
        };
        for (channel, animation) in self
            .channels
            .borrow_mut()
            .iter_mut()
            .zip([geometry, material])
        {
            crate::motion::retarget(channel, f32::from(press.is_down()), animation, time);
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
