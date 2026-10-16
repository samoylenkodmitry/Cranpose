use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

use cranpose_core::NodeMarks;
use cranpose_ui_graphics::FxBuildHasher;

use crate::{
    layer_recycling::Pool,
    scene_builder::{HitGraphState, NodeUpdate, RecorderKey, RecorderSlot, TranslateScratch},
};

pub(crate) struct RenderThreadState {
    pub(crate) layer_pool: RefCell<Pool>,
    pub(crate) dirty_ancestry: Cell<NodeMarks>,
    pub(crate) translate_scratch: RefCell<TranslateScratch>,
    /// The hit geometry a subtree had before its rebuild, kept from one
    /// rebuild to the next so no frame allocates it.
    pub(crate) hit_geometry_scratch: Cell<Vec<HitGraphState>>,
    /// The dirty nodes of a scene update, kept empty between updates so no
    /// frame makes or allocates the map.
    pub(crate) dirty_node_scratch:
        Cell<Option<cranpose_core::collections::map::HashMap<cranpose_core::NodeId, NodeUpdate>>>,
    pub(crate) command_recordings: RefCell<HashMap<RecorderKey, RecorderSlot, FxBuildHasher>>,
    pub(crate) recording_generation: Cell<u64>,
    #[cfg(test)]
    pub(crate) lowered_layer_count: Cell<usize>,
}

impl RenderThreadState {
    fn new() -> Self {
        Self {
            layer_pool: RefCell::new(Pool::default()),
            dirty_ancestry: Cell::new(NodeMarks::default()),
            translate_scratch: RefCell::new(TranslateScratch::default()),
            hit_geometry_scratch: Cell::new(Vec::new()),
            dirty_node_scratch: Cell::new(None),
            command_recordings: RefCell::new(HashMap::default()),
            recording_generation: Cell::new(0),
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
