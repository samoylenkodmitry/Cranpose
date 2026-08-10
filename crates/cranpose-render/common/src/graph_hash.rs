use std::hash::{Hash, Hasher};

use cranpose_ui_graphics::{
    BlendMode, ColorFilter, FxHasher, Point, Rect, RenderEffect, RenderHash,
};

use crate::graph::{
    CachePolicy, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform,
    RenderNode,
};
use crate::layer_composition::{layer_composite_params, local_content_layer_for};
use crate::raster_cache::LayerRasterCacheHashes;

pub(crate) fn recompute_layer_raster_cache_hashes(layer: &mut LayerNode) {
    recompute_layer_raster_cache_hashes_inner(layer, false);
}

/// True when some consumer might read this layer's stored hashes this frame:
/// the raster-cache candidate checks and the child-surface composite paths all
/// key off cache policy or an isolating property. A plain content layer (the
/// common case for a full-screen game canvas) has no reader, and eagerly
/// hashing its whole subtree is a pure walk over every primitive per frame.
fn layer_hashes_have_consumers(layer: &LayerNode) -> bool {
    layer.cache_policy != CachePolicy::None
        || layer.isolation.has_any()
        || layer.effect().is_some()
        || layer.backdrop().is_some()
        || layer.blend_mode() != BlendMode::SrcOver
        || layer.opacity() < 1.0
}

fn recompute_layer_raster_cache_hashes_inner(layer: &mut LayerNode, ancestor_hashed: bool) {
    // A hashed parent folds `child.target_content_hash()` into its own hash,
    // so every descendant of a hashed layer must stay eagerly hashed or each
    // parent recompute would re-walk the child subtree through the lazy path.
    let eager = ancestor_hashed || layer_hashes_have_consumers(layer);
    for child in &mut layer.children {
        if let RenderNode::Layer(child_layer) = child {
            recompute_layer_raster_cache_hashes_inner(child_layer, eager);
        }
    }
    if eager {
        layer.cache_hashes = layer_raster_cache_hashes(layer);
        layer.cache_hashes_valid = true;
    } else {
        // Readers fall back to computing on demand (`target_content_hash`),
        // which keeps correctness if a consumer shows up unexpectedly.
        layer.cache_hashes_valid = false;
    }
}

pub(crate) fn layer_raster_cache_hashes(layer: &LayerNode) -> LayerRasterCacheHashes {
    LayerRasterCacheHashes {
        target_content: finish_hash(|state| hash_layer_target_content(layer, state)),
        effect: hash_optional_render_effect(layer.effect()),
    }
}

pub(crate) fn layer_motion_source_content_hash(layer: &LayerNode) -> u64 {
    finish_hash(|state| hash_layer_content(layer, state, false))
}

fn finish_hash(write: impl FnOnce(&mut FxHasher)) -> u64 {
    let mut hasher = FxHasher::default();
    write(&mut hasher);
    hasher.finish()
}

fn hash_layer_target_content<H: Hasher>(layer: &LayerNode, state: &mut H) {
    hash_layer_content(layer, state, true);
}

fn hash_layer_content<H: Hasher>(
    layer: &LayerNode,
    state: &mut H,
    include_translated_content_offset: bool,
) {
    layer.local_bounds.render_hash().hash(state);
    layer.translated_content_context.hash(state);
    hash_optional_rect(layer.clip_rect(), state);
    let local_layer = local_content_layer_for(&layer.graphics_layer);
    hash_f32_bits(local_layer.alpha, state);
    hash_optional_color_filter(local_layer.color_filter, state);
    layer.motion_context_animated.hash(state);
    if include_translated_content_offset && layer.translated_content_context {
        hash_point(layer.translated_content_offset, state);
    }
    layer.children.len().hash(state);
    let translated_content_offset = layer
        .translated_content_context
        .then_some(layer.translated_content_offset);
    for child in &layer.children {
        match child {
            RenderNode::Primitive(primitive) => {
                0u8.hash(state);
                hash_primitive_entry(primitive, state);
            }
            RenderNode::DrawRun(run) => {
                2u8.hash(state);
                match run.phase {
                    PrimitivePhase::BeforeChildren => 0u8.hash(state),
                    PrimitivePhase::AfterChildren => 1u8.hash(state),
                }
                run.primitives.len().hash(state);
                for primitive in run.primitives.iter() {
                    primitive.render_hash().hash(state);
                }
                // Bypassed retained spans carry frame content the primitive
                // vector no longer shows; without them two frames differing
                // only in retained motion would hash identical.
                if let Some(frame) = &run.replay {
                    frame.spans.len().hash(state);
                    frame.center.x.to_bits().hash(state);
                    frame.center.y.to_bits().hash(state);
                    for span in &frame.spans {
                        match span {
                            cranpose_ui_graphics::FrameSpan::Dynamic { range } => {
                                0u8.hash(state);
                                range.hash(state);
                            }
                            cranpose_ui_graphics::FrameSpan::Retained {
                                slot,
                                capture,
                                slot_offset,
                                range,
                                tape_range,
                                transform,
                                recolors,
                                bounds,
                            } => {
                                1u8.hash(state);
                                slot.hash(state);
                                capture.hash(state);
                                slot_offset.hash(state);
                                range.hash(state);
                                tape_range.hash(state);
                                transform.scale.to_bits().hash(state);
                                transform.angle.to_bits().hash(state);
                                recolors.len().hash(state);
                                for (offset, color) in recolors {
                                    offset.hash(state);
                                    color.0.to_bits().hash(state);
                                    color.1.to_bits().hash(state);
                                    color.2.to_bits().hash(state);
                                    color.3.to_bits().hash(state);
                                }
                                bounds.x.to_bits().hash(state);
                                bounds.y.to_bits().hash(state);
                                bounds.width.to_bits().hash(state);
                                bounds.height.to_bits().hash(state);
                            }
                        }
                    }
                }
            }
            RenderNode::Layer(child_layer) => {
                1u8.hash(state);
                hash_child_layer_contribution(child_layer, translated_content_offset, state);
            }
        }
    }
}

fn hash_child_layer_contribution<H: Hasher>(
    layer: &LayerNode,
    parent_translated_content_offset: Option<Point>,
    state: &mut H,
) {
    let transform = match parent_translated_content_offset {
        Some(offset) if offset != Point::default() => layer
            .transform_to_parent
            .then(ProjectiveTransform::translation(-offset.x, -offset.y)),
        _ => layer.transform_to_parent,
    };
    hash_projective_transform(transform, state);
    layer.translated_content_context.hash(state);
    hash_optional_rect(layer.shadow_clip, state);
    hash_child_shadow_state(layer, state);
    layer.graphics_layer.clip.hash(state);
    hash_optional_render_effect_to(layer.effect(), state);
    hash_optional_render_effect_to(layer.backdrop(), state);
    let (composite_alpha, blend_mode) =
        layer_composite_params(&layer.graphics_layer).unwrap_or((1.0, BlendMode::SrcOver));
    hash_f32_bits(composite_alpha, state);
    blend_mode.hash(state);
    layer.target_content_hash().hash(state);
}

fn hash_child_shadow_state<H: Hasher>(layer: &LayerNode, state: &mut H) {
    hash_f32_bits(layer.graphics_layer.shadow_elevation, state);
    layer
        .graphics_layer
        .ambient_shadow_color
        .render_hash()
        .hash(state);
    layer
        .graphics_layer
        .spot_shadow_color
        .render_hash()
        .hash(state);
    hash_f32_bits(layer.graphics_layer.scale, state);
    hash_f32_bits(layer.graphics_layer.scale_x, state);
    hash_f32_bits(layer.graphics_layer.scale_y, state);
    layer.graphics_layer.shape.render_hash().hash(state);
}

fn hash_primitive_entry<H: Hasher>(primitive: &PrimitiveEntry, state: &mut H) {
    match primitive.phase {
        PrimitivePhase::BeforeChildren => 0u8.hash(state),
        PrimitivePhase::AfterChildren => 1u8.hash(state),
    }
    match &primitive.node {
        PrimitiveNode::Draw(draw) => {
            0u8.hash(state);
            hash_optional_rect(draw.clip, state);
            draw.primitive.render_hash().hash(state);
        }
        PrimitiveNode::Text(text) => {
            1u8.hash(state);
            text.rect.render_hash().hash(state);
            text.text.render_hash().hash(state);
            text.text_style.render_hash().hash(state);
            hash_f32_bits(text.font_size, state);
            text.layout_options.hash(state);
            hash_optional_rect(text.clip, state);
        }
    }
}

fn hash_projective_transform<H: Hasher>(transform: ProjectiveTransform, state: &mut H) {
    for row in transform.matrix() {
        for value in row {
            hash_f32_bits(value, state);
        }
    }
}

fn hash_optional_render_effect(effect: Option<&RenderEffect>) -> u64 {
    finish_hash(|state| hash_optional_render_effect_to(effect, state))
}

fn hash_optional_render_effect_to<H: Hasher>(effect: Option<&RenderEffect>, state: &mut H) {
    match effect {
        Some(effect) => {
            1u8.hash(state);
            effect.render_hash().hash(state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_optional_color_filter<H: Hasher>(filter: Option<ColorFilter>, state: &mut H) {
    match filter {
        Some(filter) => {
            1u8.hash(state);
            filter.render_hash().hash(state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_optional_rect<H: Hasher>(rect: Option<Rect>, state: &mut H) {
    match rect {
        Some(rect) => {
            1u8.hash(state);
            rect.render_hash().hash(state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_point<H: Hasher>(point: Point, state: &mut H) {
    hash_f32_bits(point.x, state);
    hash_f32_bits(point.y, state);
}

fn hash_f32_bits<H: Hasher>(value: f32, state: &mut H) {
    value.to_bits().hash(state);
}
