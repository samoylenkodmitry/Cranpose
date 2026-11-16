use std::rc::Rc;

// The longest slice a pool keeps, and the most it keeps of one length.
const LONGEST: usize = 128;
const PER_LENGTH: usize = 32;

// Slices the glyph caches let go of, by length, kept for the next slices of
// the same length: a text that shows a new value every frame, as a ticker's
// does, keeps its length from one value to the next.
pub(crate) struct RcSlicePool<T> {
    by_length: Vec<Vec<Rc<[T]>>>,
}

impl<T> Default for RcSlicePool<T> {
    fn default() -> Self {
        Self {
            by_length: Vec::new(),
        }
    }
}

impl<T: Copy> RcSlicePool<T> {
    // Keeps `slice` for a later slice of its length when nothing else holds
    // it, it is short and its length has room.
    pub(crate) fn give(&mut self, mut slice: Rc<[T]>) {
        let length = slice.len();
        if length == 0 || length > LONGEST || Rc::get_mut(&mut slice).is_none() {
            return;
        }
        if self.by_length.len() <= length {
            self.by_length.resize_with(length + 1, Vec::new);
        }
        let kept = &mut self.by_length[length];
        if kept.len() < PER_LENGTH {
            kept.push(slice);
        }
    }

    // `items` in a kept slice of their length, or in a new one.
    pub(crate) fn filled(&mut self, items: &[T]) -> Rc<[T]> {
        if let Some(mut spare) = self.by_length.get_mut(items.len()).and_then(Vec::pop)
            && let Some(slots) = Rc::get_mut(&mut spare)
        {
            slots.copy_from_slice(items);
            return spare;
        }
        Rc::from(items)
    }
}
