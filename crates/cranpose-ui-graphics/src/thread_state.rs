use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

use crate::{
    FxBuildHasher,
    accessibility::AccessibilityOptions,
    geometry::VectorPathMaskCache,
    liquid_glass::{GLASS_SPECIALIZATION_CACHE_CAPACITY, GlassCacheKey},
    path_fill::PathSlicer,
    record::ShapeRecorder,
    render_effect::ShaderSpecializationCache,
};

pub(crate) struct GraphicsThreadState {
    pub(crate) accessibility_options: Cell<AccessibilityOptions>,
    pub(crate) vector_path_masks: RefCell<VectorPathMaskCache>,
    pub(crate) shared_texts: RefCell<HashMap<u64, Rc<str>, FxBuildHasher>>,
    pub(crate) path_slicer: RefCell<PathSlicer>,
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
            shared_texts: RefCell::new(HashMap::with_hasher(FxBuildHasher::new())),
            path_slicer: RefCell::new(PathSlicer::new()),
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
    let mut call = |state: &GraphicsThreadState| f.take().map(|f| f(state));
    let ran = STATE.try_with(&mut call).ok().flatten();
    ran.or_else(|| call(&GraphicsThreadState::new()))
        .expect("the closure runs on exactly one state")
}
