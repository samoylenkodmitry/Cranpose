//! The heights a scaling list measured so far.

/// Every item height the list has measured so far, and a stand-in for the rest.
///
/// A virtualised list cannot know the height of an item it has never composed,
/// and the scroll indicator's total extent is a sum over all of them. This is
/// the same bargain `LazyListState` makes with `average_item_size`: remember
/// what has been measured, and let the mean of it speak for what has not.
///
/// The cache is cleared when the item **count** changes and not otherwise. A
/// row whose content changed is re-measured the next time it is composed, so a
/// stale entry can only ever bias the estimate for rows that are off screen —
/// never a placed row's geometry.
#[derive(Default, Debug)]
pub(super) struct ItemHeights {
    known: Vec<Option<f32>>,
    sum: f32,
    count: usize,
}

impl ItemHeights {
    pub(super) fn len(&self) -> usize {
        self.known.len()
    }

    pub(super) fn resize(&mut self, len: usize) {
        if self.known.len() == len {
            return;
        }
        self.known.clear();
        self.known.resize(len, None);
        self.sum = 0.0;
        self.count = 0;
    }

    pub(super) fn record(&mut self, index: usize, height: f32) {
        let Some(slot) = self.known.get_mut(index) else {
            return;
        };
        match slot.replace(height) {
            Some(previous) => self.sum += height - previous,
            None => {
                self.sum += height;
                self.count += 1;
            }
        }
    }

    fn estimate(&self) -> f32 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / self.count as f32
        }
    }

    pub(super) fn height_of(&self, index: usize) -> f32 {
        self.known
            .get(index)
            .copied()
            .flatten()
            .unwrap_or_else(|| self.estimate())
    }
}
