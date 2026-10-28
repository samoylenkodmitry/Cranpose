use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::Rc,
    sync::Arc,
};

use crate::{
    ContentPool,
    accessibility::AccessibilityOptions,
    liquid_glass::{GLASS_SPECIALIZATION_CACHE_CAPACITY, GlassCacheKey},
    path::PathBuffers,
    path_fill::PathSlicer,
    record::ShapeRecorder,
    render_effect::ShaderSpecializationCache,
    vector_path_mask::VectorPathMaskCache,
};

pub(crate) struct GraphicsThreadState {
    pub(crate) accessibility_options: Cell<AccessibilityOptions>,
    pub(crate) vector_path_masks: RefCell<VectorPathMaskCache>,
    pub(crate) shared_texts: RefCell<ContentPool<Rc<str>>>,
    pub(crate) path_slicer: RefCell<PathSlicer>,
    pub(crate) path_buffers: RefCell<PathBuffers>,
    pub(crate) glass_specializations:
        RefCell<ShaderSpecializationCache<GlassCacheKey, GLASS_SPECIALIZATION_CACHE_CAPACITY>>,
    pub(crate) no_shapes: OnceCell<Arc<ShapeRecorder>>,
    #[cfg(test)]
    pub(crate) override_hash_computations: Cell<usize>,
}

impl GraphicsThreadState {
    const fn new() -> Self {
        Self {
            accessibility_options: Cell::new(AccessibilityOptions::DEFAULT),
            vector_path_masks: RefCell::new(VectorPathMaskCache::new()),
            shared_texts: RefCell::new(ContentPool::new()),
            path_slicer: RefCell::new(PathSlicer::new()),
            path_buffers: RefCell::new(PathBuffers::new()),
            glass_specializations: RefCell::new(ShaderSpecializationCache::new()),
            no_shapes: OnceCell::new(),
            #[cfg(test)]
            override_hash_computations: Cell::new(0),
        }
    }
}

thread_local! {
    static STATE: GraphicsThreadState = const { GraphicsThreadState::new() };
}

pub(crate) fn with<R>(f: impl FnOnce(&GraphicsThreadState) -> R) -> R {
    let mut f = Some(f);
    if let Ok(Some(result)) = STATE.try_with(|state| f.take().map(|f| f(state))) {
        return result;
    }
    with_fresh(f.expect("the closure runs on exactly one state"))
}

// Out of line, so a caller keeps no room on its stack for the fresh state,
// which only a late destructor at thread exit needs.
#[inline(never)]
fn with_fresh<R>(f: impl FnOnce(&GraphicsThreadState) -> R) -> R {
    f(&GraphicsThreadState::new())
}
