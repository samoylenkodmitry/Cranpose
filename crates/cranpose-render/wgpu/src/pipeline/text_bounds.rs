// The area a text node's draws cover, worked out by running the same
// emission into a sink that only keeps bounds.

use cranpose_render_common::{
    geometry::{expand_blurred_rect, union_rect},
    graph::TextPrimitiveNode,
};
use cranpose_ui_graphics::{DrawPrimitive, Rect};

use super::{TextPlacement, TextStyleDrawSink, emit_text_style_draws};
use crate::scene::{EffectLayer, SnapAnchor, TextDraw};

pub(crate) fn estimate_text_style_draw_bounds(
    node: &TextPrimitiveNode,
    placement: TextPlacement<'_>,
) -> Option<Rect> {
    let mut collector = TextBoundsCollector::default();
    emit_text_style_draws(&mut collector, node, placement);
    collector.bounds
}

#[derive(Default)]
struct TextBoundsCollector {
    bounds: Option<Rect>,
    next_z: usize,
}

impl TextStyleDrawSink for TextBoundsCollector {
    fn current_z(&mut self) -> usize {
        self.next_z
    }

    fn push_shape(
        &mut self,
        primitive: DrawPrimitive,
        _clip: Option<Rect>,
        _anchor: Option<SnapAnchor>,
    ) {
        let (DrawPrimitive::Rect { rect, .. } | DrawPrimitive::RoundRect { rect, .. }) = primitive
        else {
            unreachable!("text decorations are rects");
        };
        self.bounds = union_rect(self.bounds, rect);
        self.next_z += 1;
    }

    fn push_text(&mut self, draw: TextDraw) {
        self.bounds = union_rect(self.bounds, draw.rect);
        self.next_z += 1;
    }

    fn push_shadow_text(&mut self, draw: TextDraw, blur_radius: f32) {
        let shadow_bounds = expand_blurred_rect(draw.rect, blur_radius, draw.scale, draw.clip);
        if let Some(shadow_bounds) = shadow_bounds {
            self.bounds = union_rect(self.bounds, shadow_bounds);
        }
        self.next_z += 1;
    }

    fn push_effect_layer(&mut self, layer: EffectLayer) {
        self.bounds = union_rect(self.bounds, layer.rect);
    }
}
