//! The buffers a subcompose node's measure fills, kept between passes.

use std::rc::Rc;

use cranpose_foundation::lazy::LazyItemRoots;
use cranpose_ui_layout::{MeasureResult, Placement};

use crate::modifier::Size;

/// The buffers a measure pass of a subcompose node fills, kept on the node
/// between passes.
#[derive(Default)]
pub(crate) struct MeasureScratch {
    pub(super) placements: Vec<Placement>,
    /// The roots the policy placed, each with its measurement, in the order
    /// of the placements, where it placed lazy items: empty otherwise.
    pub(super) placed_roots: Vec<Rc<LazyItemRoots>>,
}

impl MeasureScratch {
    pub(super) fn layout_with_placed_roots(
        &mut self,
        width: f32,
        height: f32,
        build: impl FnOnce(&mut Vec<Placement>, &mut Vec<Rc<LazyItemRoots>>),
    ) -> MeasureResult {
        self.placements.clear();
        self.placed_roots.clear();
        build(&mut self.placements, &mut self.placed_roots);
        MeasureResult::new(Size { width, height }, std::mem::take(&mut self.placements))
    }
}
