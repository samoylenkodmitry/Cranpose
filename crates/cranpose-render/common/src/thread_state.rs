use std::{
    cell::{Cell, RefCell},
    sync::Arc,
};

use cranpose_ui_graphics::ContentPool;

use crate::{
    scene_builder::{DirtyRoom, HitGraphState, TranslateScratch},
    text_paint::TextPaint,
};

pub(crate) struct RenderThreadState {
    pub(crate) translate_scratch: RefCell<TranslateScratch>,
    pub(crate) hit_geometry_scratch: Cell<Vec<HitGraphState>>,
    pub(crate) dirty_node_scratch: Cell<Option<DirtyRoom>>,
    pub(crate) space_changed: Cell<bool>,
    // The paints of the texts the thread made, shared by those that paint
    // the same.
    pub(crate) text_paints: RefCell<ContentPool<Arc<TextPaint>>>,
}

impl RenderThreadState {
    fn new() -> Self {
        Self {
            translate_scratch: RefCell::new(TranslateScratch::default()),
            hit_geometry_scratch: Cell::new(Vec::new()),
            dirty_node_scratch: Cell::new(None),
            space_changed: Cell::new(false),
            text_paints: RefCell::new(ContentPool::new()),
        }
    }
}

thread_local! {
    static STATE: RenderThreadState = RenderThreadState::new();
}

pub(crate) fn with<R>(f: impl FnOnce(&RenderThreadState) -> R) -> R {
    cranpose_core::with_or_fresh(&STATE, RenderThreadState::new, f)
}
