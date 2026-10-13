use std::cell::{Cell, OnceCell};

use cranpose_core::CompositionLocal;

use crate::{
    appearance::GlassTintAmount,
    theme::{LiquidColors, LiquidSegmentedStyle, LiquidTypography},
};

pub(crate) struct LiquidThreadState {
    pub(crate) colors: OnceCell<CompositionLocal<LiquidColors>>,
    pub(crate) typography: OnceCell<CompositionLocal<LiquidTypography>>,
    pub(crate) glass_tint: OnceCell<CompositionLocal<GlassTintAmount>>,
    pub(crate) segmented: OnceCell<CompositionLocal<Option<LiquidSegmentedStyle>>>,
    pub(crate) glass_light_return: Cell<(f32, f32)>,
}

impl LiquidThreadState {
    const fn new() -> Self {
        Self {
            colors: OnceCell::new(),
            typography: OnceCell::new(),
            glass_tint: OnceCell::new(),
            segmented: OnceCell::new(),
            glass_light_return: Cell::new((0.0, 1.0)),
        }
    }
}

thread_local! {
    static STATE: LiquidThreadState = const { LiquidThreadState::new() };
}

pub(crate) fn with<R>(f: impl FnOnce(&LiquidThreadState) -> R) -> R {
    cranpose_core::with_or_fresh(&STATE, LiquidThreadState::new, f)
}
