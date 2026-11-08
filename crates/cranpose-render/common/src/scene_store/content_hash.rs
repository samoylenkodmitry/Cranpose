// Hashes of what a record draws, for keying the pixels a renderer keeps.

use std::hash::{Hash, Hasher};

use cranpose_ui_graphics::{Point, RenderHash};

use super::{ItemRef, LayerRef};
use crate::{
    graph::ProjectiveTransform,
    graph_hash::{
        finish_hash, hash_child_composite, hash_draw, hash_f32_bits, hash_optional_color_filter,
        hash_optional_rect, hash_optional_render_effect, hash_point, hash_projective_transform,
        hash_run, hash_text,
    },
    layer_composition::local_content_layer_for,
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
