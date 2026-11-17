// The measurements a lazy policy placed its items' roots with.

use std::rc::Rc;

use cranpose_foundation::lazy::LazyItemRoots;

use super::MeasuredNode;

/// The measurement of each placement a lazy policy made, in order: see
/// [`crate::subcompose_layout::PlacedRootsScope::layout_with_placed_roots`].
pub(super) struct PlacedRootMeasurements<'a> {
    items: std::slice::Iter<'a, Rc<LazyItemRoots>>,
    /// The measurements of the item being placed, one a root; empty for an
    /// item that holds none.
    measured: &'a [Rc<MeasuredNode>],
    /// The item's next root, and how many roots it has.
    root: usize,
    roots: usize,
}

impl<'a> PlacedRootMeasurements<'a> {
    pub(super) fn new(items: &'a [Rc<LazyItemRoots>]) -> Self {
        Self {
            items: items.iter(),
            measured: &[],
            root: 0,
            roots: 0,
        }
    }

    /// The measurement of the next placement; `None` for a root the item
    /// holds none for, and once every item is placed.
    pub(super) fn next_measurement(&mut self) -> Option<&'a MeasuredNode> {
        while self.root == self.roots {
            let item = self.items.next()?;
            self.measured = crate::widgets::lazy_list::root_measurements(item)
                .map_or(&[], smallvec::SmallVec::as_slice);
            self.root = 0;
            self.roots = item.node_ids.len();
        }
        let measured = self.measured.get(self.root);
        self.root += 1;
        measured.map(|measured| &**measured)
    }
}
