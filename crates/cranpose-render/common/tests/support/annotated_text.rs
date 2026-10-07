use std::{rc::Rc, sync::Arc};

use cranpose_render_common::graph::{
    LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, RenderGraph, RenderNode,
    TextPrimitiveNode,
};
use cranpose_ui::text::{AnnotatedString, RangeStyle, SpanStyle, TextStyle, TextUnit};
use cranpose_ui_graphics::{Color, Rect};

pub fn mixed_size_pairs() -> AnnotatedString {
    let mut text = AnnotatedString::from("HH\nHH");
    for (range, size, color) in [
        (0..1, 10.0, Color::RED),
        (1..2, 20.0, Color::GREEN),
        (3..4, 10.0, Color::RED),
        (4..5, 30.0, Color::GREEN),
    ] {
        text.span_styles.push(RangeStyle {
            range,
            item: SpanStyle {
                font_size: TextUnit::Sp(size),
                color: Some(color),
                ..Default::default()
            },
        });
    }
    text
}

pub fn text_graph(
    text: AnnotatedString,
    style: TextStyle,
    text_rect: Rect,
    viewport: Rect,
) -> RenderGraph {
    RenderGraph::new(LayerNode {
        local_bounds: viewport,
        children: vec![RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::AfterChildren,
            node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id: 1,
                rect: text_rect,
                render_text: Arc::new(text.render_string()),
                text: Rc::new(text),
                font_size: style.resolve_font_size(10.0),
                text_style: Arc::new(style),
                layout_options: Default::default(),
                clip: Some(viewport),
                paint: Default::default(),
            })),
        })],
        ..Default::default()
    })
}
