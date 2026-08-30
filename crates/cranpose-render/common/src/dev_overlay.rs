//! Renderer-drawn dev overlay (fps counter): one shared graph builder so
//! every backend draws the identical overlay without composition impact.

use cranpose_ui_graphics::{Brush, Color, CornerRadii, DrawPrimitive, Rect, Size};

use crate::graph::{
    DrawPrimitiveNode, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
    ProjectiveTransform, RenderGraph, RenderNode, TextPrimitiveNode,
};

/// Cache key for the last-built overlay; rebuilding only on change keeps the
/// overlay free when the text is stable.
#[derive(Clone, PartialEq, Eq)]
pub struct DevOverlayKey {
    pub text: String,
    pub viewport_width_bits: u32,
    pub viewport_height_bits: u32,
}

impl DevOverlayKey {
    pub fn new(text: &str, viewport: Size) -> Self {
        Self {
            text: text.to_string(),
            viewport_width_bits: viewport.width.to_bits(),
            viewport_height_bits: viewport.height.to_bits(),
        }
    }
}

/// Build the dev-overlay graph: a translucent pill with the stats text in
/// the top-right corner, sized with fixed monospace-ish metrics so the
/// overlay never depends on renderer text measurement.
pub fn build_dev_overlay_graph(
    text: &str,
    viewport: Size,
    node_id: cranpose_core::NodeId,
) -> RenderGraph {
    let padding = 8.0;
    let font_size = 14.0;
    let char_width = 7.0;
    let text_width = text.len() as f32 * char_width;
    let text_height = font_size * 1.4;
    let x = (viewport.width - text_width - padding * 2.0).max(padding);
    let y = padding;

    let mut overlay_layer = LayerNode {
        node_id: Some(node_id),
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: text_width + padding,
            height: text_height + padding / 2.0,
        },
        transform_to_parent: ProjectiveTransform::translation(x, y),
        children: vec![
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::RoundRect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: text_width + padding,
                            height: text_height + padding / 2.0,
                        },
                        brush: Brush::Solid(Color(0.0, 0.0, 0.0, 0.7)),
                        radii: CornerRadii::uniform(4.0),
                        stroke: None,
                    },
                    clip: None,
                }),
            }),
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::AfterChildren,
                node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                    node_id,
                    rect: Rect {
                        x: padding / 2.0,
                        y: padding / 4.0,
                        width: text_width,
                        height: text_height,
                    },
                    text: std::rc::Rc::new(cranpose_ui::text::AnnotatedString::from(text)),
                    text_style: cranpose_ui::TextStyle::default(),
                    font_size,
                    layout_options: cranpose_ui::TextLayoutOptions::default(),
                    clip: None,
                })),
            }),
        ],
        ..Default::default()
    };
    overlay_layer.recompute_raster_cache_hashes();

    let mut graph = RenderGraph::new(LayerNode {
        local_bounds: Rect::from_size(viewport),
        children: vec![RenderNode::Layer(Box::new(overlay_layer))],
        ..Default::default()
    });
    graph.root.recompute_raster_cache_hashes();
    graph
}
