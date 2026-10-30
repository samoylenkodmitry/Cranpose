use std::{rc::Rc, sync::Arc};

use cranpose_core::NodeId;
use cranpose_render_common::{
    graph::{
        LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, RenderGraph, RenderNode,
        TextPrimitiveNode,
    },
    text_fonts::TextFonts,
};
use cranpose_ui::text::{AnnotatedString, RangeStyle, SpanStyle, TextStyle, TextUnit};
use cranpose_ui_graphics::{Color, Rect};

pub(crate) fn mixed_size_pairs() -> AnnotatedString {
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

// A scene of `text` in `style` laid out with the fonts a renderer without
// app fonts hands its app, in `text_rect` of `viewport`.
pub(crate) fn text_graph(
    text: AnnotatedString,
    style: TextStyle,
    text_rect: Rect,
    viewport: Rect,
) -> RenderGraph {
    let prepared = cranpose_ui::AppContext::new().enter(|| {
        cranpose_ui::text::set_text_fonts(TextFonts::from_fonts_or_default(&[]).resolver());
        cranpose_ui::text::prepare_text_layout(
            &text,
            &style,
            Default::default(),
            Some(text_rect.width),
        )
    });
    RenderGraph::new(LayerNode {
        local_bounds: viewport,
        children: vec![RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::AfterChildren,
            node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id: NodeId(1),
                rect: text_rect,
                render_text: prepared.render_text(),
                layout: prepared.layout.clone(),
                text: Rc::new(text),
                font_size: style.resolve_font_size(10.0),
                text_style: Arc::new(style),
                overflow: cranpose_ui::TextLayoutOptions::default().overflow,
                clip: Some(viewport),
                paint: Default::default(),
            })),
        })],
        ..Default::default()
    })
}
