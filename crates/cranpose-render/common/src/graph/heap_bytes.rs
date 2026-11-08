// Estimates of the heap memory a render graph holds, for memory reports.

use std::mem::size_of;

use cranpose_ui::text::AnnotatedString;
use cranpose_ui_graphics::{DrawPrimitive, GraphicsLayer, ShadowPrimitive};

use super::{
    DrawPrimitiveNode, InlineNode, LayerNode, PrimitiveEntry, PrimitiveNode, RenderNode,
    TextPrimitiveNode,
};

pub(super) fn layer_heap_bytes(layer: &LayerNode) -> usize {
    size_of::<RenderNode>() * layer.children.capacity()
        + layer
            .graphics_layer
            .0
            .as_ref()
            .map_or(0, |_| size_of::<GraphicsLayer>())
        + layer
            .children
            .iter()
            .map(render_node_heap_bytes)
            .sum::<usize>()
}

fn render_node_heap_bytes(node: &RenderNode) -> usize {
    match node {
        RenderNode::Primitive(entry) => primitive_entry_heap_bytes(entry),
        RenderNode::DrawRun(run) => {
            run.recording.pod_heap_bytes()
                + size_of_val(run.recording.others())
                + run
                    .recording
                    .others()
                    .iter()
                    .map(draw_primitive_heap_bytes)
                    .sum::<usize>()
        }
        RenderNode::Layer(layer) => size_of::<LayerNode>() + layer_heap_bytes(layer),
        RenderNode::Inline(inline) => {
            size_of::<InlineNode>()
                + size_of::<RenderNode>() * inline.content.capacity()
                + inline
                    .content
                    .iter()
                    .map(render_node_heap_bytes)
                    .sum::<usize>()
        }
    }
}

fn primitive_entry_heap_bytes(entry: &PrimitiveEntry) -> usize {
    match &entry.node {
        PrimitiveNode::Draw(draw) => {
            size_of::<DrawPrimitiveNode>() + draw_primitive_heap_bytes(&draw.primitive)
        }
        PrimitiveNode::Text(text) => {
            size_of::<TextPrimitiveNode>() + annotated_string_heap_bytes(&text.text)
        }
    }
}

fn draw_primitive_heap_bytes(primitive: &DrawPrimitive) -> usize {
    match primitive {
        DrawPrimitive::Content
        | DrawPrimitive::Group(_)
        | DrawPrimitive::Rect { .. }
        | DrawPrimitive::RoundRect { .. }
        | DrawPrimitive::Arc { .. }
        | DrawPrimitive::Line { .. }
        | DrawPrimitive::Trapezoid { .. } => 0,
        DrawPrimitive::Blend { primitive, .. } => {
            size_of::<DrawPrimitive>() + draw_primitive_heap_bytes(primitive)
        }
        DrawPrimitive::Image { .. } => 0,
        DrawPrimitive::Text(text) => {
            size_of::<cranpose_ui_graphics::TextPrimitive>()
                + text.text.len()
                + text.style.font_family.as_ref().map_or(0, String::capacity)
        }
        DrawPrimitive::Shadow(shadow) => shadow_primitive_heap_bytes(shadow),
    }
}

fn shadow_primitive_heap_bytes(shadow: &ShadowPrimitive) -> usize {
    match shadow {
        ShadowPrimitive::Drop { shape, .. } => {
            size_of::<DrawPrimitive>() + draw_primitive_heap_bytes(shape)
        }
        ShadowPrimitive::Inner { fill, cutout, .. } => {
            size_of::<DrawPrimitive>() * 2
                + draw_primitive_heap_bytes(fill)
                + draw_primitive_heap_bytes(cutout)
        }
    }
}

fn annotated_string_heap_bytes(text: &AnnotatedString) -> usize {
    let content = text.render_string();
    let ranges = content.span_styles().len()
        + content.paragraph_styles().len()
        + content.string_annotations().len()
        + content.links().len();
    content.len()
        + ranges * size_of::<usize>() * 2
        + content
            .string_annotations()
            .iter()
            .map(|annotation| annotation.item.tag.len() + annotation.item.annotation.len())
            .sum::<usize>()
        + content
            .links()
            .iter()
            .map(|link| match &link.item {
                cranpose_ui::text::LinkKey::Url(url) => url.len(),
                cranpose_ui::text::LinkKey::Clickable(tag) => tag.len(),
            })
            .sum::<usize>()
}
