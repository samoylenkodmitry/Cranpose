// Estimates of the heap memory a render graph holds, for memory reports.

use std::mem::size_of;

use cranpose_ui::text::AnnotatedString;
use cranpose_ui_graphics::{DrawPrimitive, GraphicsLayer, ShadowPrimitive};

use super::{
    DrawPrimitiveNode, LayerNode, PrimitiveEntry, PrimitiveNode, RenderNode, TextPrimitiveNode,
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
    text.text.capacity()
        + text.span_styles.capacity() * size_of::<usize>() * 2
        + text.paragraph_styles.capacity() * size_of::<usize>() * 2
        + text.string_annotations.capacity() * size_of::<usize>() * 2
        + text.link_annotations.capacity() * size_of::<usize>() * 2
        + text
            .string_annotations
            .iter()
            .map(|annotation| {
                annotation.item.tag.capacity() + annotation.item.annotation.capacity()
            })
            .sum::<usize>()
        + text
            .link_annotations
            .iter()
            .map(|annotation| match &annotation.item {
                cranpose_ui::text::LinkAnnotation::Url(url) => url.capacity(),
                cranpose_ui::text::LinkAnnotation::Clickable { tag, .. } => tag.capacity(),
            })
            .sum::<usize>()
}
