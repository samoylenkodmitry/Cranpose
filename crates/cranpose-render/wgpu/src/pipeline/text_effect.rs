use std::sync::Arc;

use cranpose_render_common::graph::TextPrimitiveNode;
use cranpose_ui::{TextStyle, text::TextDrawStyle};
use cranpose_ui_graphics::{BlendMode, Color, Rect, RenderEffect};

use super::{TextStyleDrawSink, expand_text_effect_rect};
use crate::scene::{DrawnText, EffectLayer, TextDraw};

// `style` with its glyphs filled in `color` and unpainted otherwise: the mask
// a text's effect paints through.
pub(super) fn mask_style(style: &TextStyle, color: Color) -> (Arc<TextStyle>, Color) {
    let mut mask = style.clone();
    mask.span_style.brush = None;
    mask.span_style.alpha = None;
    mask.span_style.color = Some(color);
    mask.span_style.draw_style = Some(TextDrawStyle::Fill);
    (Arc::new(mask), color)
}

// Pushes `mask_text`, the glyphs of `node` over `text_rect` in the mask
// style and its color, cut to `clip` grown by the effect's padding, then the
// layer of `effect` over `effect_rect`, which paints through them.
pub(super) fn push_text_effect<S: TextStyleDrawSink>(
    sink: &mut S,
    node: &TextPrimitiveNode,
    (text_rect, text_scale, clip): (Rect, f32, Option<Rect>),
    (mask_text, style, color): (&cranpose_ui::text::AnnotatedString, Arc<TextStyle>, Color),
    (effect, effect_rect): (RenderEffect, Rect),
) {
    let z_start = sink.current_z();
    sink.push_text(TextDraw {
        node_id: node.node_id,
        rect: text_rect,
        snap_anchor: None,
        text: DrawnText::of(
            Arc::clone(mask_text.render_string()),
            &style,
            node.paint().spread,
        ),
        layout: node.layout.clone(),
        color,
        style_hash: style.render_hash(),
        text_style: style,
        font_size: node.font_size,
        scale: text_scale,
        clip: clip.map(|clip| expand_text_effect_rect(clip, effect.input_padding())),
    });
    let z_end = sink.current_z();
    sink.push_effect_layer(EffectLayer {
        rect: effect_rect,
        clip,
        snap_anchor: None,
        effect: Some(effect),
        blend_mode: BlendMode::SrcOver,
        composite_alpha: 1.0,
        z_start,
        z_end,
    });
}
