//! The buffers a subcompose node's measure fills, kept between passes.

use std::rc::Rc;

use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_foundation::lazy::LazyItemRoots;
use cranpose_ui_layout::{MeasureResult, Placement};

use crate::{layout::MeasuredNode, modifier::Size};

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

// The measurements a subcompose policy made of its children, by child. The
// map holds an index into the list, so that it shares its table code with
// the other maps from a node to an index.
#[derive(Default)]
pub(crate) struct MeasurementsById {
    index: HashMap<NodeId, usize>,
    measured: Vec<MeasuredNode>,
}

impl MeasurementsById {
    pub(crate) fn insert(&mut self, child: NodeId, measured: MeasuredNode) {
        match self.index.get(&child) {
            Some(&at) => self.measured[at] = measured,
            None => {
                self.index.insert(child, self.measured.len());
                self.measured.push(measured);
            }
        }
    }

    pub(crate) fn get(&self, child: &NodeId) -> Option<&MeasuredNode> {
        self.index.get(child).map(|&at| &self.measured[at])
    }

    pub(crate) fn remove(&mut self, child: &NodeId) -> Option<MeasuredNode> {
        self.index.remove(child).map(|at| self.measured[at])
    }

    pub(crate) fn clear(&mut self) {
        self.index.clear();
        self.measured.clear();
    }
}
