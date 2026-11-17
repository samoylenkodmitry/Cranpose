// How the scroll walks a pinned section: its stage stays in view while the
// scroll moves on, and the scene follows how far it has gone.

use cranpose_ui::{ScrollState, Size};

pub(super) fn unit(size: Size) -> f32 {
    (size.width / 1600.0).min(size.height / 1000.0).max(0.4)
}

pub(super) fn ease(from: f32, to: f32, at: f32) -> f32 {
    let share = ((at - from) / (to - from)).clamp(0.0, 1.0);
    share * share * (3.0 - 2.0 * share)
}

// The same with a quintic ease: a softer start and stop.
pub(super) fn ease_softly(from: f32, to: f32, at: f32) -> f32 {
    let share = ((at - from) / (to - from)).clamp(0.0, 1.0);
    share * share * share * (share * (share * 6.0 - 15.0) + 10.0)
}

#[derive(Clone, Copy)]
pub(super) struct Walk {
    pub scroll: ScrollState,
    pub top: f32,
    pub travel: f32,
}

impl Walk {
    pub(super) fn new(scroll: ScrollState, top: f32, height: f32, pages: f32) -> Self {
        Self {
            scroll,
            top,
            travel: height * (pages - 1.0),
        }
    }

    pub(super) fn progress(self) -> f32 {
        ((self.scroll.value() - self.top) / self.travel).clamp(0.0, 1.0)
    }

    pub(super) fn hold(self) -> f32 {
        (self.scroll.value() - self.top).clamp(0.0, self.travel)
    }
}
