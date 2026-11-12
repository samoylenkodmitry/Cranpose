use std::{rc::Rc, sync::Arc};

use cranpose_ui_graphics::{BlendMode, FrontPolygon, RuntimeShader};

use crate::{
    effect_renderer::{CompositeSampleMode, RoundedCompositeMask, SubstrateRegions},
    offscreen::OffscreenTarget,
};

/// What a composite's texture holds beyond this frame: a retained texture
/// keeps the pixels its cache key names for as long as it lives, so the key's
/// hash identifies them; a transient one is drawn anew every frame and
/// identifies nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceContent {
    Retained(u64),
    Transient,
}

impl SourceContent {
    pub(crate) fn retained(key: &impl std::hash::Hash) -> Self {
        let mut hasher = cranpose_ui_graphics::FxHasher::default();
        key.hash(&mut hasher);
        Self::Retained(std::hash::Hasher::finish(&hasher))
    }

    /// The hash naming a retained texture's pixels; none for a transient.
    pub(crate) fn retained_hash(self) -> Option<u64> {
        match self {
            Self::Retained(hash) => Some(hash),
            Self::Transient => None,
        }
    }

    /// The content of a texture derived from this one by `step`, retained
    /// exactly when this one is.
    pub(crate) fn derived(self, step: &impl std::hash::Hash) -> Self {
        match self {
            Self::Retained(hash) => Self::retained(&(hash, step)),
            Self::Transient => Self::Transient,
        }
    }
}

/// A resolved texture drawn into the pass at its z, described in the scene's
/// device space so one description serves every target the scene is drawn
/// into.
#[derive(Clone)]
pub(crate) struct ResolvedComposite {
    pub(crate) z_index: usize,
    pub(crate) source: Rc<OffscreenTarget>,
    pub(crate) content: SourceContent,
    pub(crate) dest: (f32, f32, f32, f32),
    pub(crate) scissor: Option<(f32, f32, f32, f32)>,
    pub(crate) kind: ResolvedCompositeKind,
}

#[derive(Clone)]
pub(crate) enum ResolvedCompositeKind {
    Blit {
        alpha: f32,
        blend_mode: BlendMode,
        rounded_mask: Option<RoundedCompositeMask>,
        sample_mode: CompositeSampleMode,
        source_viewport: Option<(f32, f32, f32, f32)>,
    },
    Shader {
        shader: Arc<RuntimeShader>,
        layer_pixel_rect: [f32; 4],
        source_region: Option<(f32, f32, f32, f32)>,
        source_logical_size: Option<(f32, f32)>,
        substrate_regions: SubstrateRegions,
        rounded_mask: Option<RoundedCompositeMask>,
        alpha: f32,
    },
    Projective {
        dest: FrontPolygon,
        inverse: [[f32; 3]; 3],
        alpha: f32,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
        source_region: Option<(f32, f32, f32, f32)>,
    },
}
