//! Renderer-drawn dev overlay (fps counter): one shared graph builder so
//! every backend draws the identical overlay without composition impact.

use cranpose_ui_graphics::{Brush, Color, CornerRadii, DrawPrimitive, Point, Rect, Size};

use crate::{
    graph::{DrawPrimitiveNode, PrimitiveNode, TextPrimitiveNode},
    scene_store::OverlayLayer,
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

/// The frame counter's overlay: its layer, and the window it lies in.
#[derive(Clone, Debug)]
pub struct FpsOverlay {
    /// The size of the window the layer lies in, in logical pixels.
    pub window: Size,
    /// The pill that holds the stats text.
    pub layer: OverlayLayer,
}

/// Build the dev overlay: a translucent pill with the stats text, laid out
/// in `overlay`, in the top-right corner of `viewport`. The pill draws for
/// no layout node; `node_id` keys its text.
pub fn build_fps_overlay(
    overlay: &cranpose_ui::PreparedTextLayout,
    viewport: Size,
    node_id: cranpose_core::NodeId,
) -> FpsOverlay {
    let padding = 8.0;
    let Size {
        width: text_width,
        height: text_height,
    } = overlay.size();
    let x = (viewport.width - text_width - padding * 2.0).max(padding);
    let y = padding;

    let layer = OverlayLayer {
        size: Size::new(text_width + padding, text_height + padding / 2.0),
        origin: Point::new(x, y),
        primitives: vec![
            PrimitiveNode::Draw(Box::new(DrawPrimitiveNode {
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
            PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id,
                rect: Rect {
                    x: padding / 2.0,
                    y: padding / 4.0,
                    width: text_width,
                    height: text_height,
                },
                render_text: overlay.render_text(),
                layout: overlay.layout.clone(),
                text_style: std::sync::Arc::clone(&overlay.visual_style),
                font_size: overlay
                    .visual_style
                    .resolve_font_size(cranpose_ui_graphics::DEFAULT_FONT_SIZE),
                overflow: cranpose_ui::TextOverflow::Clip,
                clip: None,
                paint: Default::default(),
            })),
        ],
    };
    FpsOverlay {
        window: viewport,
        layer,
    }
}
