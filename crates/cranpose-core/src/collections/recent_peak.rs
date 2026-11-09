// The frames a window of the peak spans: the peak is the most of the window
// under way and the window before it, the last 8 to 16 frames.
const WINDOW_FRAMES: u8 = 8;

/// The most entries a collection reused every frame held in the last 8 to 16
/// frames. The room a burst took, such as a first build's, goes back once
/// 16 later frames hold far fewer, while a collection that fills alike every
/// frame keeps its room.
#[derive(Clone, Copy, Debug, Default)]
pub struct RecentPeak {
    current: usize,
    previous: usize,
    frames: u8,
}

impl RecentPeak {
    /// Notes a frame at which the collection held `len` entries.
    pub fn note(&mut self, len: usize) {
        self.current = self.current.max(len);
        self.frames += 1;
        if self.frames == WINDOW_FRAMES {
            self.previous = std::mem::take(&mut self.current);
            self.frames = 0;
        }
    }

    /// The room to shrink a collection of `capacity` to: twice the peak, at
    /// least `floor`, once the capacity passes four times that.
    #[must_use]
    pub fn shrink_to(self, capacity: usize, floor: usize) -> Option<usize> {
        let bound = self.current.max(self.previous).max(floor);
        (capacity > 4 * bound).then_some(2 * bound)
    }

    /// Gives back the room the recent frames left unused in `list`, a
    /// collection that fills like the one the peak notes, keeping its
    /// entries and room for at least `floor`.
    pub fn shrink<T>(self, list: &mut Vec<T>, floor: usize) {
        if let Some(room) = self.shrink_to(list.capacity(), floor) {
            list.shrink_to(room);
        }
    }

    /// Notes the frame `list` holds and gives back the room the recent
    /// frames left unused, keeping its entries and room for at least
    /// `floor`.
    pub fn trim<T>(&mut self, list: &mut Vec<T>, floor: usize) {
        self.note(list.len());
        self.shrink(list, floor);
    }

    /// Notes the frame `list` held, empties it and gives back the room the
    /// recent frames left unused, keeping room for at least `floor`.
    pub fn clear<T>(&mut self, list: &mut Vec<T>, floor: usize) {
        self.note(list.len());
        list.clear();
        self.shrink(list, floor);
    }
}
