//! Whether a child layer puts anything on the surface it composites onto.

use cranpose_ui_graphics::{BlendMode, RenderEffect};

use crate::collect::{ChildLayer, LayerScene};

// Whether the layer's content puts nothing on its surface.
pub(super) fn draws_nothing(content: &LayerScene) -> bool {
    content.scene.draw_ops.is_empty()
        && content.scene.shadow_draws.is_empty()
        && content.children.is_empty()
        && content.scene.backdrop_layers.is_empty()
        && content.scene.effect_layers.is_empty()
}

pub(super) fn composites_nothing(child: &ChildLayer) -> bool {
    child.blend_mode == BlendMode::SrcOver
        && (child.alpha == 0.0
            || (draws_nothing(&child.content)
                && child
                    .effect
                    .as_ref()
                    .is_none_or(RenderEffect::preserves_transparency)))
}
