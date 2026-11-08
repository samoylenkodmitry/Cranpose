// Hashes of what a record draws, for keying the pixels a renderer keeps.

use std::hash::{Hash, Hasher};

use cranpose_ui_graphics::{
    BlendMode, ColorFilter, FxHasher, GraphicsLayer, Point, Rect, RenderEffect, RenderHash,
};

use super::{ItemRef, LayerRef};
use crate::{
    graph::{DrawPrimitiveNode, DrawRunNode, ProjectiveTransform, TextPrimitiveNode},
    layer_composition::{layer_composite_params, local_content_layer_for},
    raster_cache::LayerRasterCacheHashes,
};

pub(super) fn raster_cache_hashes(layer: LayerRef<'_>) -> LayerRasterCacheHashes {
    LayerRasterCacheHashes {
        target_content: finish_hash(|state| hash_content(layer, state, true)),
        effect: hash_optional_render_effect(layer.effect()),
    }
}

pub(super) fn motion_source_content_hash(layer: LayerRef<'_>) -> u64 {
    finish_hash(|state| hash_content(layer, state, false))
}

// A hash of what an own draw of a record draws; `None` for a child.
pub(super) fn own_draw_print(item: ItemRef<'_>) -> Option<u64> {
    let print = match item {
        ItemRef::Draw(draw) => finish_hash(|state| hash_draw(draw, state)),
        ItemRef::Text(text) => finish_hash(|state| hash_text(text, state)),
        ItemRef::Run(run) => finish_hash(|state| hash_run(run, state)),
        ItemRef::Layer(_) | ItemRef::Leaf(_) => return None,
    };
    Some(print)
}

fn hash_content<H: Hasher>(layer: LayerRef<'_>, state: &mut H, with_translated_offset: bool) {
    layer.local_bounds().render_hash().hash(state);
    let translated = layer.translated_content_context();
    translated.hash(state);
    hash_optional_rect(layer.visual_clip_rect(), state);
    let local_layer = local_content_layer_for(layer.graphics_layer());
    hash_f32_bits(local_layer.alpha, state);
    hash_optional_color_filter(local_layer.color_filter.as_ref(), state);
    layer.motion_context_animated().hash(state);
    if with_translated_offset && translated {
        hash_point(layer.translated_content_offset(), state);
    }
    hash_items(
        layer,
        translated.then(|| layer.translated_content_offset()),
        state,
    );
}

fn hash_items<H: Hasher>(layer: LayerRef<'_>, translated_offset: Option<Point>, state: &mut H) {
    layer.item_count().hash(state);
    for item in layer.items() {
        match item {
            ItemRef::Draw(draw) => {
                0u8.hash(state);
                hash_draw(draw, state);
            }
            ItemRef::Text(text) => {
                1u8.hash(state);
                hash_text(text, state);
            }
            ItemRef::Run(run) => {
                2u8.hash(state);
                hash_run(run, state);
            }
            ItemRef::Layer(child) => {
                3u8.hash(state);
                hash_child_layer(child, translated_offset, state);
            }
            ItemRef::Leaf(leaf) => {
                4u8.hash(state);
                let offset = translated_offset.unwrap_or_default();
                let origin = leaf.origin();
                hash_point(Point::new(origin.x - offset.x, origin.y - offset.y), state);
                leaf.node_rect().render_hash().hash(state);
                leaf.motion_context_animated().hash(state);
                hash_items(leaf, None, state);
            }
        }
    }
}

fn hash_child_layer<H: Hasher>(
    layer: LayerRef<'_>,
    parent_translated_offset: Option<Point>,
    state: &mut H,
) {
    let transform = match parent_translated_offset {
        Some(offset) if offset != Point::default() => layer
            .transform_to_parent()
            .then(ProjectiveTransform::translation(-offset.x, -offset.y)),
        _ => layer.transform_to_parent(),
    };
    hash_projective_transform(transform, state);
    layer.translated_content_context().hash(state);
    hash_optional_rect(layer.shadow_clip(), state);
    hash_child_composite(layer.graphics_layer(), state);
    layer.target_content_hash().hash(state);
}

fn finish_hash(write: impl FnOnce(&mut FxHasher)) -> u64 {
    let mut hasher = FxHasher::default();
    write(&mut hasher);
    hasher.finish()
}

// What a child layer's graphics layer adds to its parent's pixels: its
// shadow, clip, effects and how it composites.
fn hash_child_composite<H: Hasher>(layer: &GraphicsLayer, state: &mut H) {
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

fn hash_run<H: Hasher>(run: &DrawRunNode, state: &mut H) {
    run.recording.fingerprint().hash(state);
    run.segments.start.hash(state);
    run.segments.end.hash(state);
}

fn hash_draw<H: Hasher>(draw: &DrawPrimitiveNode, state: &mut H) {
    hash_optional_rect(draw.clip, state);
    draw.primitive.render_hash().hash(state);
}

fn hash_text<H: Hasher>(text: &TextPrimitiveNode, state: &mut H) {
    text.rect.render_hash().hash(state);
    text.text.render_hash().hash(state);
    text.text_style.render_hash().hash(state);
    hash_f32_bits(text.font_size, state);
    text.overflow.hash(state);
    text.layout.id().hash(state);
    hash_optional_rect(text.clip, state);
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

fn hash_optional_color_filter<H: Hasher>(filter: Option<&ColorFilter>, state: &mut H) {
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
