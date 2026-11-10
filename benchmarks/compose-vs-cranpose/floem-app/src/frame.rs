//! The frame clock: a view that asks for the next frame once it painted one.

use std::{cell::Cell, rc::Rc, time::Duration};

use floem::{
    View, ViewId,
    action::exec_after,
    context::PaintCx,
    reactive::{RwSignal, SignalGet, SignalUpdate},
};
use perf_data::FrameIndex;

/// The frame every screen follows, in a signal.
#[derive(Clone, Copy)]
pub struct Frame(RwSignal<u32>);

impl Frame {
    pub fn get(self) -> u32 {
        self.0.get()
    }

    /// The frame for views that paint it and have no effect to run.
    pub fn now(self) -> u32 {
        self.0.get_untracked()
    }
}

struct Clock {
    index: Cell<FrameIndex>,
    pending: Cell<bool>,
    signal: RwSignal<u32>,
}

impl Clock {
    fn tick(&self) {
        self.pending.set(false);
        let mut index = self.index.get();
        if index.frozen() {
            return;
        }
        index.advance();
        self.index.set(index);
        self.signal.set(index.get());
    }
}

pub struct FrameClock {
    id: ViewId,
    clock: Rc<Clock>,
}

/// The frame signal and the view that advances it once per painted frame,
/// up to the frame the launch freezes on.
pub fn frame_clock(freeze: u32) -> (Frame, FrameClock) {
    let signal = RwSignal::new(0);
    let clock = Rc::new(Clock {
        index: Cell::new(FrameIndex::new(freeze)),
        pending: Cell::new(false),
        signal,
    });
    (
        Frame(signal),
        FrameClock {
            id: ViewId::new(),
            clock,
        },
    )
}

impl View for FrameClock {
    fn id(&self) -> ViewId {
        self.id
    }

    // The next frame is asked for once this one is painted, so a frame never
    // starts before the last one ended.
    fn paint(&mut self, _cx: &mut PaintCx) {
        if self.clock.index.get().frozen() || self.clock.pending.replace(true) {
            return;
        }
        let clock = self.clock.clone();
        exec_after(Duration::ZERO, move |_| clock.tick());
    }
}
