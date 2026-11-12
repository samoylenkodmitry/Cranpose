// The room a taken list keeps at rest.
const ROOM_KEPT: usize = 64;

// A list a frame takes whole. A take trades the list's storage with the
// taker's, so the two storages take turns.
pub(super) struct TakenList<T> {
    items: Vec<T>,
}

impl<T> Default for TakenList<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<T> TakenList<T> {
    pub(super) fn push(&mut self, item: T) {
        self.items.push(item);
    }

    pub(super) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    // Moves the items into `output`, dropping what `output` held.
    pub(super) fn take_into(&mut self, output: &mut Vec<T>) {
        output.clear();
        std::mem::swap(&mut self.items, output);
    }

    // Gives back the room a burst of items took, keeping room for a few.
    pub(super) fn trim(&mut self) {
        self.items.shrink_to(ROOM_KEPT);
    }
}
