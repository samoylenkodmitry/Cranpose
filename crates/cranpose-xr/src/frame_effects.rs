//! The frame callbacks that composables registered.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::CompositionLocal;

use crate::{XrFrame, locals::with_locals};

type Callback = Box<dyn FnMut(&XrFrame)>;

// A callback that one composable set. The composable replaces it in each composition.
#[derive(Default)]
pub(crate) struct Slot {
    callback: RefCell<Option<Callback>>,
}

impl Slot {
    pub(crate) fn set(&self, callback: Callback) {
        *self.callback.borrow_mut() = Some(callback);
    }

    // Calls the callback with no borrow held, so that it may set a new one.
    fn call(&self, frame: &XrFrame) {
        let taken = self.callback.borrow_mut().take();
        if let Some(mut callback) = taken {
            callback(frame);
            let mut slot = self.callback.borrow_mut();
            if slot.is_none() {
                *slot = Some(callback);
            }
        }
    }
}

// The callbacks of the composables that are in the composition, in the order in which they
// entered it. The headset's backend provides it around the app's content and runs it in each
// frame.
#[derive(Clone, Default)]
pub(crate) struct FrameEffects {
    slots: Rc<RefCell<Vec<Rc<Slot>>>>,
}

impl PartialEq for FrameEffects {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.slots, &other.slots)
    }
}

impl FrameEffects {
    // A new slot, which the next frame runs. A composable calls it inside a `remember`.
    pub(crate) fn add(&self) -> Rc<Slot> {
        let slot = Rc::new(Slot::default());
        self.slots.borrow_mut().push(Rc::clone(&slot));
        slot
    }

    pub(crate) fn remove(&self, slot: &Rc<Slot>) {
        self.slots
            .borrow_mut()
            .retain(|kept| !Rc::ptr_eq(kept, slot));
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Runs every callback with `frame`. The registry stays unborrowed while a callback runs.
    pub(crate) fn run(&self, frame: &XrFrame) {
        let mut index = 0;
        loop {
            let slot = self.slots.borrow().get(index).map(Rc::clone);
            let Some(slot) = slot else {
                break;
            };
            slot.call(frame);
            index += 1;
        }
    }
}

pub(crate) fn local_frame_effects() -> CompositionLocal<Option<FrameEffects>> {
    with_locals(|locals| locals.frame_effects.clone())
}
