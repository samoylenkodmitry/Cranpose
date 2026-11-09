/// The most entries a collection reused every frame held lately, falling by
/// a sixteenth a frame. The room a burst took, such as a first build's, goes
/// back once later frames hold far fewer, while a collection that fills
/// alike every frame keeps its room.
#[derive(Clone, Copy, Debug, Default)]
pub struct RecentPeak(usize);

impl RecentPeak {
    /// Notes a frame at which the collection held `len` entries.
    pub fn note(&mut self, len: usize) {
        self.0 = len.max(self.0 - self.0 / 16);
    }

    /// The room to shrink a collection of `capacity` to: twice the peak, at
    /// least `floor`, once the capacity passes four times that.
    #[must_use]
    pub fn shrink_to(self, capacity: usize, floor: usize) -> Option<usize> {
        let bound = self.0.max(floor);
        (capacity > 4 * bound).then_some(2 * bound)
    }

    /// Notes the frame `list` held, empties it and gives back the room the
    /// recent frames left unused, keeping room for at least `floor`.
    pub fn clear<T>(&mut self, list: &mut Vec<T>, floor: usize) {
        self.note(list.len());
        list.clear();
        if let Some(room) = self.shrink_to(list.capacity(), floor) {
            list.shrink_to(room);
        }
    }
}
