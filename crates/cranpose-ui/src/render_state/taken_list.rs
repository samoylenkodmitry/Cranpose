use cranpose_core::collections::recent_peak::RecentPeak;

// The room a taken list keeps whatever recent takes held.
const ROOM_KEPT: usize = 64;

// A list a frame takes whole. A take trades the list's storage with the
// taker's, so the two storages take turns. The take gives back the room
// recent takes left unused in the storage the list fills next: a screen's
// first frame takes every node, while later frames take a few.
pub(super) struct TakenList<T> {
    items: Vec<T>,
    recent: RecentPeak,
}

impl<T> Default for TakenList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            recent: RecentPeak::default(),
        }
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
        self.recent.settle(&mut self.items, output.len(), ROOM_KEPT);
    }
}
