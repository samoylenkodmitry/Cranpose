use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

use cranpose_core::NodeMarks;
use cranpose_ui_graphics::FxBuildHasher;

use crate::{
    layer_recycling::Pool,
    scene_builder::{RecorderKey, RecorderSlot, TranslateScratch},
};

pub(crate) struct RenderThreadState {
    pub(crate) layer_pool: RefCell<Pool>,
    pub(crate) dirty_ancestry: Cell<NodeMarks>,
    pub(crate) translate_scratch: Cell<TranslateScratch>,
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
            translate_scratch: Cell::new(TranslateScratch::default()),
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
    STATE.with(f)
}
