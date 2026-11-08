use std::hash::{Hash, Hasher};

use cranpose_ui_graphics::{
    BlendMode, ColorFilter, FxHasher, GraphicsLayer, Point, Rect, RenderEffect, RenderHash,
};

use crate::{
    graph::{
        DrawPrimitiveNode, DrawRunNode, InlineNode, LayerNode, PrimitiveEntry, PrimitiveNode,
        PrimitivePhase, ProjectiveTransform, RenderNode, TextPrimitiveNode,
    },
    layer_composition::{layer_composite_params, local_content_layer_for},
    raster_cache::LayerRasterCacheHashes,
};

pub(crate) fn layer_raster_cache_hashes(layer: &LayerNode) -> LayerRasterCacheHashes {
    LayerRasterCacheHashes {
        target_content: finish_hash(|state| hash_layer_target_content(layer, state)),
        effect: hash_optional_render_effect(layer.effect()),
    }
}

// The hash of what `inline` draws, in its own space.
pub(crate) fn inline_content_hash(inline: &InlineNode) -> u64 {
    finish_hash(|state| {
        inline.content.len().hash(state);
        for node in &inline.content {
            hash_child(node, None, state);
        }
    })
}

pub(crate) fn layer_motion_source_content_hash(layer: &LayerNode) -> u64 {
    finish_hash(|state| hash_layer_content(layer, state, false))
}

pub(crate) fn finish_hash(write: impl FnOnce(&mut FxHasher)) -> u64 {
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
    hash_optional_rect(layer.visual_clip_rect(), state);
    let local_layer = local_content_layer_for(&layer.graphics_layer);
    hash_f32_bits(local_layer.alpha, state);
    hash_optional_color_filter(local_layer.color_filter.as_ref(), state);
    layer.motion_context_animated.hash(state);
    if include_translated_content_offset && layer.translated_content_context {
        hash_point(layer.translated_content_offset, state);
    }
    layer.children.len().hash(state);
    let translated_content_offset = layer
        .translated_content_context
        .then_some(layer.translated_content_offset);
    for child in &layer.children {
        hash_child(child, translated_content_offset, state);
    }
}

fn hash_child<H: Hasher>(
    child: &RenderNode,
    parent_translated_content_offset: Option<Point>,
    state: &mut H,
) {
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
            hash_run(run, state);
        }
        RenderNode::Layer(child_layer) => {
            1u8.hash(state);
            hash_child_layer_contribution(child_layer, parent_translated_content_offset, state);
        }
        RenderNode::Inline(inline) => {
            3u8.hash(state);
            let offset = parent_translated_content_offset.unwrap_or_default();
            hash_point(
                Point::new(inline.origin.x - offset.x, inline.origin.y - offset.y),
                state,
            );
            inline.node_rect().render_hash().hash(state);
            inline.motion_context_animated.hash(state);
            inline.content_hash().hash(state);
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
    hash_optional_rect(layer.shadow_clip(), state);
    hash_child_composite(&layer.graphics_layer, state);
    layer.target_content_hash().hash(state);
}

// What a child layer's graphics layer adds to its parent's pixels: its
// shadow, clip, effects and how it composites.
pub(crate) fn hash_child_composite<H: Hasher>(layer: &GraphicsLayer, state: &mut H) {
    hash_f32_bits(layer.shadow_elevation, state);
    layer.ambient_shadow_color.render_hash().hash(state);
    layer.spot_shadow_color.render_hash().hash(state);
    hash_f32_bits(layer.scale, state);
    hash_f32_bits(layer.scale_x, state);
    hash_f32_bits(layer.scale_y, state);
    layer.shape.render_hash().hash(state);
    layer.clip.hash(state);
    hash_optional_render_effect_to(layer.render_effect.as_ref(), state);
    hash_optional_render_effect_to(layer.backdrop_effect.as_ref(), state);
    let (composite_alpha, blend_mode) =
        layer_composite_params(layer).unwrap_or((1.0, BlendMode::SrcOver));
    hash_f32_bits(composite_alpha, state);
    blend_mode.hash(state);
}

pub(crate) fn hash_run<H: Hasher>(run: &DrawRunNode, state: &mut H) {
    run.recording.fingerprint().hash(state);
    run.segments.start.hash(state);
    run.segments.end.hash(state);
}

fn hash_primitive_entry<H: Hasher>(primitive: &PrimitiveEntry, state: &mut H) {
    match primitive.phase {
        PrimitivePhase::BeforeChildren => 0u8.hash(state),
        PrimitivePhase::AfterChildren => 1u8.hash(state),
    }
    match &primitive.node {
        PrimitiveNode::Draw(draw) => {
            0u8.hash(state);
            hash_draw(draw, state);
        }
        PrimitiveNode::Text(text) => {
            1u8.hash(state);
            hash_text(text, state);
        }
    }
}

pub(crate) fn hash_draw<H: Hasher>(draw: &DrawPrimitiveNode, state: &mut H) {
    hash_optional_rect(draw.clip, state);
    draw.primitive.render_hash().hash(state);
}

pub(crate) fn hash_text<H: Hasher>(text: &TextPrimitiveNode, state: &mut H) {
    text.rect.render_hash().hash(state);
    text.text.render_hash().hash(state);
    text.text_style.render_hash().hash(state);
    hash_f32_bits(text.font_size, state);
    text.overflow.hash(state);
    text.layout.id().hash(state);
    hash_optional_rect(text.clip, state);
}

pub(crate) fn hash_projective_transform<H: Hasher>(transform: ProjectiveTransform, state: &mut H) {
    for row in transform.matrix() {
        for value in row {
            hash_f32_bits(value, state);
        }
    }
}

pub(crate) fn hash_optional_render_effect(effect: Option<&RenderEffect>) -> u64 {
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

pub(crate) fn hash_optional_color_filter<H: Hasher>(filter: Option<&ColorFilter>, state: &mut H) {
    match filter {
        Some(filter) => {
            1u8.hash(state);
            filter.render_hash().hash(state);
        }
        None => 0u8.hash(state),
    }
}

pub(crate) fn hash_optional_rect<H: Hasher>(rect: Option<Rect>, state: &mut H) {
    match rect {
        Some(rect) => {
            1u8.hash(state);
            rect.render_hash().hash(state);
        }
        None => 0u8.hash(state),
    }
}

pub(crate) fn hash_point<H: Hasher>(point: Point, state: &mut H) {
    hash_f32_bits(point.x, state);
    hash_f32_bits(point.y, state);
}

pub(crate) fn hash_f32_bits<H: Hasher>(value: f32, state: &mut H) {
    value.to_bits().hash(state);
}
