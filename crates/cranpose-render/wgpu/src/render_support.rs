use cranpose_core::NodeId;
use cranpose_ui::text::AnnotatedString;
use cranpose_ui::{TextLayoutOptions, TextStyle};
use cranpose_ui_graphics::{BlendMode, DrawPrimitive, GraphicsLayer, Rect};

use crate::pipeline;
use crate::scene::Scene;

pub(crate) fn effective_layer_isolation(layer: &GraphicsLayer) -> Option<pipeline::LayerIsolation> {
    pipeline::effective_layer_isolation(layer)
}

pub(crate) fn layer_for_content(
    layer: &GraphicsLayer,
    isolation: Option<&pipeline::LayerIsolation>,
) -> GraphicsLayer {
    pipeline::layer_for_content(layer, isolation)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn push_text_style_draws(
    scene: &mut Scene,
    node_id: NodeId,
    rect: Rect,
    text_rect: Rect,
    content_layer: &GraphicsLayer,
    text: &AnnotatedString,
    text_style: &TextStyle,
    font_size: f32,
    options: TextLayoutOptions,
    text_clip: Option<Rect>,
) {
    pipeline::push_text_style_draws(
        scene,
        node_id,
        rect,
        text_rect,
        content_layer,
        text,
        text_style,
        font_size,
        options,
        text_clip,
    );
}

pub(crate) fn resolve_clip(
    parent_clip: Option<Rect>,
    requested_clip: Option<Rect>,
) -> Option<Rect> {
    pipeline::resolve_clip(parent_clip, requested_clip)
}

pub(crate) fn push_draw_primitive(
    primitive: DrawPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    scene: &mut Scene,
    blend_mode: Option<BlendMode>,
) {
    pipeline::push_draw_primitive(primitive, layer_bounds, layer, clip, scene, blend_mode);
}

pub(crate) fn push_layer_shadow(
    scene: &mut Scene,
    layer: &GraphicsLayer,
    layer_bounds: Rect,
    transformed_bounds: Rect,
    clip: Option<Rect>,
) {
    pipeline::push_layer_shadow(scene, layer, layer_bounds, transformed_bounds, clip);
}
