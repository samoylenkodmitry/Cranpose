// The most entries a reused collection held at a frame lately, falling by an
// eighth a frame: the room a burst took, such as a first build's, goes back
// once later frames hold far fewer, while a collection that fills alike
// every frame keeps its room.

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RecentPeak(usize);

impl RecentPeak {
    // Notes a frame at which the collection held `len` entries.
    pub(crate) fn note(&mut self, len: usize) {
        self.0 = len.max(self.0 - self.0 / 8);
    }

    // The room to shrink a collection of `capacity` to: twice the peak, at
    // least `floor`, once the capacity passes four times that.
    pub(crate) fn shrink_to(self, capacity: usize, floor: usize) -> Option<usize> {
        let bound = self.0.max(floor);
        (capacity > 4 * bound).then_some(2 * bound)
    }
}
