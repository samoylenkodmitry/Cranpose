use std::convert::Infallible;

use cranpose_foundation::lazy::{BeyondItem, LazyItemSource, LazyListMeasuredItem};

use super::LazyListItems;

impl LazyItemSource for LazyListItems<'_, '_, '_> {
    type Refusal = Infallible;

    fn measure(&mut self, index: usize) -> Result<LazyListMeasuredItem, Infallible> {
        Ok(self.item(index))
    }

    fn key(&self, index: usize) -> u64 {
        self.inputs.content.get_key(index).to_slot_id()
    }

    fn keep_beyond(&mut self, index: usize, distance: f32) -> Result<BeyondItem, Infallible> {
        let composed = self.composed(index);
        if !self.admits(composed, distance) {
            return Ok(BeyondItem::Declined);
        }
        if self.place_beyond {
            return Ok(BeyondItem::Placed(self.item(index)));
        }
        self.keep(index, composed);
        Ok(BeyondItem::Kept)
    }
}
