use std::cell::{Cell, RefCell};

use cranpose_core::collections::{map::HashMap as NodeMap, recent_peak::RecentPeak};

use crate::{
    NodeUpdate,
    scene_builder::{HitGraphState, TranslateScratch},
};

pub(crate) struct RenderThreadState {
    pub(crate) translate_scratch: RefCell<TranslateScratch>,
    pub(crate) hit_geometry_scratch: Cell<Vec<HitGraphState>>,
    pub(crate) dirty_node_scratch:
        Cell<Option<(NodeMap<cranpose_core::NodeId, NodeUpdate>, RecentPeak)>>,
    pub(crate) space_changed: Cell<bool>,
    #[cfg(test)]
    pub(crate) lowered_layer_count: Cell<usize>,
}

impl RenderThreadState {
    fn new() -> Self {
        Self {
            translate_scratch: RefCell::new(TranslateScratch::default()),
            hit_geometry_scratch: Cell::new(Vec::new()),
            dirty_node_scratch: Cell::new(None),
            space_changed: Cell::new(false),
            #[cfg(test)]
            lowered_layer_count: Cell::new(0),
        }
    }
}

thread_local! {
    static STATE: RenderThreadState = RenderThreadState::new();
}

pub(crate) fn with<R>(f: impl FnOnce(&RenderThreadState) -> R) -> R {
    cranpose_core::with_or_fresh(&STATE, RenderThreadState::new, f)
}
