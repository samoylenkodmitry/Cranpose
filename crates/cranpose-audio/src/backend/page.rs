// What the page's thread keeps of the open Web Audio streams and recordings. A browser runs the
// page on one thread, and their values of the page stay on it. A sink or a source names its
// stream by a slot, so the sink or the source itself can cross threads as `Send` asks. One
// thread-local static holds both lists: Android gives a process few thread-local keys, and the
// framework keeps to a budget of statics for every target.

use std::{cell::RefCell, rc::Rc};

use super::{web_audio::Stream, web_microphone::Recording};

pub(super) struct Slots<T>(RefCell<Vec<Option<Rc<T>>>>);

impl<T> Slots<T> {
    const fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }

    // Puts `item` into the first free slot and returns the slot.
    pub(super) fn insert(&self, item: Rc<T>) -> usize {
        let mut slots = self.0.borrow_mut();
        let slot = slots.iter().position(Option::is_none).unwrap_or_else(|| {
            slots.push(None);
            slots.len() - 1
        });
        slots[slot] = Some(item);
        slot
    }

    pub(super) fn get(&self, slot: usize) -> Option<Rc<T>> {
        self.0.borrow().get(slot).cloned().flatten()
    }

    pub(super) fn take(&self, slot: usize) -> Option<Rc<T>> {
        self.0.borrow_mut().get_mut(slot).and_then(Option::take)
    }
}

pub(super) struct Page {
    pub(super) streams: Slots<Stream>,
    pub(super) recordings: Slots<Recording>,
}

thread_local! {
    pub(super) static PAGE: Page = const {
        Page {
            streams: Slots::new(),
            recordings: Slots::new(),
        }
    };
}
