use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use cranpose_core::collections::map::HashMap as NodeMap;
use cranpose_ui_graphics::{CommandRecording, FxBuildHasher};

use crate::{
    NodeUpdate,
    layer_recycling::Pool,
    scene_builder::{HitGraphState, RecorderKey, RecorderSlot, RedrawnBuffers, TranslateScratch},
};

pub(crate) struct RenderThreadState {
    pub(crate) layer_pool: RefCell<Pool>,
    pub(crate) translate_scratch: RefCell<TranslateScratch>,
    /// The hit geometry a subtree had before its rebuild, kept from one
    /// rebuild to the next so no frame allocates it.
    pub(crate) hit_geometry_scratch: Cell<Vec<HitGraphState>>,
    /// The dirty nodes of a scene update, kept empty between updates so no
    /// frame makes or allocates the map.
    pub(crate) dirty_node_scratch: Cell<Option<NodeMap<cranpose_core::NodeId, NodeUpdate>>>,
    pub(crate) command_recordings: RefCell<HashMap<RecorderKey, RecorderSlot, FxBuildHasher>>,
    pub(crate) recording_generation: Cell<u64>,
    // What the commands of `RecordingStore::Redrawn` keep to record into:
    // the recordings of those that drew in two updates in a row, the room of
    // the others', and the update each command drew in last; and the update
    // that draws now.
    pub(crate) redrawn_recordings:
        RefCell<HashMap<RecorderKey, (Option<RedrawnBuffers>, u64), FxBuildHasher>>,
    pub(crate) redrawn_update: Cell<u64>,
    // The recordings the commands of the update that runs gave back, for its
    // next commands.
    pub(crate) spare_recordings: RefCell<Vec<Rc<CommandRecording>>>,
    /// Whether a scene update or build changed a layer in 3D since the
    /// window's scenes last published where their layers show and their
    /// eyes' worlds.
    pub(crate) space_changed: Cell<bool>,
    #[cfg(test)]
    pub(crate) lowered_layer_count: Cell<usize>,
}

impl RenderThreadState {
    fn new() -> Self {
        Self {
            layer_pool: RefCell::new(Pool::default()),
            translate_scratch: RefCell::new(TranslateScratch::default()),
            hit_geometry_scratch: Cell::new(Vec::new()),
            dirty_node_scratch: Cell::new(None),
            command_recordings: RefCell::new(HashMap::default()),
            recording_generation: Cell::new(0),
            redrawn_recordings: RefCell::new(HashMap::default()),
            redrawn_update: Cell::new(0),
            spare_recordings: RefCell::new(Vec::new()),
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
