use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_ui::{PreparedTextLayout, Rect, TextLayoutOptions, TextOverflow, TextPanResolver};

use crate::{
    graph::TextPrimitiveNode,
    text_paint::{TextPaint, TextPaintCache},
};

pub(super) const TEXT_CLIP_PAD: f32 = 1.0;

pub(super) struct TextNodeParts {
    pub(super) node_id: NodeId,
    // Where the text node was placed in its layout node.
    pub(super) text_rect: Rect,
    pub(super) text_layout_options: Option<TextLayoutOptions>,
    pub(super) text_pan: Option<TextPanResolver>,
    pub(super) measured_layout: Option<Rc<PreparedTextLayout>>,
}

pub(super) fn text_node_from_parts(parts: TextNodeParts) -> Option<TextPrimitiveNode> {
    let TextNodeParts {
        node_id,
        text_rect,
        text_layout_options,
        text_pan,
        measured_layout,
    } = parts;
    let prepared = measured_layout?;
    let options = text_layout_options.unwrap_or_default().normalized();
    let content_width = text_rect.width.max(0.0);
    if content_width <= 0.0 {
        return None;
    }

    let pan_offset = text_pan
        .as_ref()
        .map_or(0.0, |resolve| resolve(content_width));
    let pans_horizontally = text_pan.is_some();

    let visual_style = &prepared.visual_style;
    // The layout aligns its lines across its own width; it draws from the
    // content's corner.
    let rect = Rect {
        x: text_rect.x - pan_offset,
        y: text_rect.y,
        width: prepared.layout.width(),
        height: prepared.size().height,
    };
    let text_bounds = Rect {
        width: content_width,
        height: text_rect.height.max(0.0),
        ..text_rect
    };
    let font_size = visual_style.resolve_font_size(14.0);
    let clip = if options.overflow == TextOverflow::Visible && !pans_horizontally {
        None
    } else {
        Some(pad_clip_rect(text_bounds))
    };

    Some(TextPrimitiveNode {
        node_id,
        rect,
        text: Rc::clone(&prepared.text),
        render_text: prepared.render_text(),
        layout: prepared.layout.clone(),
        text_style: std::sync::Arc::clone(visual_style),
        font_size,
        overflow: options.overflow,
        clip,
        paint: TextPaintCache::holding(TextPaint::of(
            visual_style,
            prepared.visual_style_hash(),
            &prepared.text.span_styles,
            font_size,
            (prepared.render_text_hash(), prepared.text.text.is_empty()),
        )),
    })
}

fn pad_clip_rect(rect: Rect) -> Rect {
    Rect {
        x: rect.x - TEXT_CLIP_PAD,
        y: rect.y - TEXT_CLIP_PAD,
        width: (rect.width + TEXT_CLIP_PAD * 2.0).max(0.0),
        height: (rect.height + TEXT_CLIP_PAD * 2.0).max(0.0),
    }
}
