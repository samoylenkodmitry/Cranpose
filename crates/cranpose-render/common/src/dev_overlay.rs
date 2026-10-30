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
    /// The overlay text, for example the fps line.
    pub text: String,
    /// Bit pattern of the viewport width in logical pixels, so the key can be compared exactly.
    pub viewport_width_bits: u32,
    /// Bit pattern of the viewport height in logical pixels, so the key can be compared exactly.
    pub viewport_height_bits: u32,
}

impl DevOverlayKey {
    /// Builds the key for `text` drawn in a viewport of the given size in logical pixels.
    pub fn new(text: &str, viewport: Size) -> Self {
        Self {
            text: text.to_string(),
            viewport_width_bits: viewport.width.to_bits(),
            viewport_height_bits: viewport.height.to_bits(),
        }
    }
}

/// Build the dev-overlay graph: a translucent pill with the stats text, laid
/// out in `overlay`, in the top-right corner.
pub fn build_dev_overlay_graph(
    overlay: &cranpose_ui::PreparedTextLayout,
    viewport: Size,
    node_id: cranpose_core::NodeId,
) -> RenderGraph {
    let padding = 8.0;
    let text_width = overlay.metrics.width;
    let text_height = overlay.metrics.height;
    let x = (viewport.width - text_width - padding * 2.0).max(padding);
    let y = padding;

    let overlay_layer = LayerNode {
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
                node: PrimitiveNode::Draw(Box::new(DrawPrimitiveNode {
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
                })),
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
                    text: std::rc::Rc::clone(&overlay.text),
                    render_text: overlay.render_text(),
                    layout: overlay.layout.clone(),
                    text_style: std::sync::Arc::clone(&overlay.visual_style),
                    font_size: overlay.visual_style.resolve_font_size(14.0),
                    overflow: cranpose_ui::TextOverflow::Clip,
                    clip: None,
                    paint: Default::default(),
                })),
            }),
        ],
        ..Default::default()
    };
    RenderGraph::new(LayerNode {
        local_bounds: Rect::from_size(viewport),
        children: vec![RenderNode::Layer(Box::new(overlay_layer))],
        ..Default::default()
    })
}
