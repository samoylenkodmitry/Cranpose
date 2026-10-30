use std::{rc::Rc, sync::Arc};

use cranpose_core::NodeId;
use cranpose_ui::{
    ModifierNodeSlices, PreparedTextLayout, Rect, Size, TextLayoutOptions, TextOverflow,
    TextPanResolver,
};

use super::{
    SnapshotNodeData, TranslateAncestorContext, TranslateGeometry, TranslatedContainer,
    apply_translated_container_state, hit_test_from_slices, modifier_slices_have_origin_sinks,
    translate_bail, translated_container,
};
use crate::{
    graph::{
        LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, RenderNode, TextPrimitiveNode,
    },
    text_paint::{TextPaint, TextPaintCache},
};

pub(super) const TEXT_CLIP_PAD: f32 = 1.0;

pub(super) struct TextNodeParts {
    node_id: NodeId,
    // Where the text node was placed in its layout node.
    text_rect: Rect,
    text_layout_options: Option<TextLayoutOptions>,
    text_pan: Option<TextPanResolver>,
    measured_layout: Option<Rc<PreparedTextLayout>>,
}

impl TextNodeParts {
    // The text `slices` of node `node_id` draw at `size`.
    pub(super) fn of(node_id: NodeId, slices: &ModifierNodeSlices, size: Size) -> Self {
        Self {
            node_id,
            text_rect: slices.text_content_rect(size),
            text_layout_options: slices.text_layout_options(),
            text_pan: slices.text_pan_resolver(),
            measured_layout: slices.measured_text_layout(),
        }
    }
}

// Brings `container`, the layer of a leaf that drew its node's text and
// nothing else, to the node's layout in `data`, as a label a resize moves:
// writes the text into the text the layer drew and places the layer,
// recording nothing. `None` for any other layer, which the full update
// takes.
pub(super) fn translate_leaf_text(
    container: &mut LayerNode,
    data: &SnapshotNodeData<'_>,
    ancestors: TranslateAncestorContext,
) -> Option<bool> {
    let slices = &data.modifier_slices;
    let leaf = data.children.is_empty() && !data.window_root && slices.draw_commands().is_empty();
    if !leaf || !slices.has_text() || drawn_text(container).is_none() {
        return None;
    }
    let layout_state = &data.layout_state;
    let TranslatedContainer {
        node_id,
        node_bounds,
    } = match translated_container(
        container,
        layout_state,
        slices,
        ancestors.inherited_motion_context_animated,
        false,
    ) {
        Ok(plan) => plan,
        Err(reason) => return Some(translate_bail(reason)),
    };
    let parts = TextNodeParts::of(node_id, slices, layout_state.size());
    if !refresh_text_node(drawn_text(container)?, parts) {
        return None;
    }
    container.node_bounds = node_bounds;
    let geometry = TranslateGeometry::new(
        layout_state,
        &container.graphics_layer,
        container.local_bounds,
        ancestors.parent_abs,
    );
    let offset = ancestors.parent_content_offset;
    apply_translated_container_state(container, slices, layout_state, offset, geometry);
    container.hit_test = hit_test_from_slices(slices);
    container.has_origin_sinks = modifier_slices_have_origin_sinks(slices);
    container.refresh_child_facts();
    container.note_content_change(ancestors.update);
    Some(true)
}

// The text `layer` drew, when it drew that and nothing else.
fn drawn_text(layer: &mut LayerNode) -> Option<&mut TextPrimitiveNode> {
    match layer.children.as_mut_slice() {
        [
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Text(drawn),
            }),
        ] => Some(drawn),
        _ => None,
    }
}

pub(super) fn text_node_from_parts(parts: TextNodeParts) -> Option<TextPrimitiveNode> {
    text_node(parts, None)
}

// Writes the text of `parts` into `drawn`, the text its node drew before,
// which keeps its paint while its text and style stay. False, leaving
// `drawn` as it was, when the node draws no text now.
fn refresh_text_node(drawn: &mut TextPrimitiveNode, parts: TextNodeParts) -> bool {
    let Some(text) = text_node(parts, Some(drawn)) else {
        return false;
    };
    *drawn = text;
    true
}

fn text_node(parts: TextNodeParts, drawn: Option<&TextPrimitiveNode>) -> Option<TextPrimitiveNode> {
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
        text_style: Arc::clone(visual_style),
        font_size,
        overflow: options.overflow,
        clip,
        paint: match drawn {
            Some(drawn)
                if Rc::ptr_eq(&drawn.text, &prepared.text)
                    && Arc::ptr_eq(&drawn.text_style, visual_style)
                    && drawn.font_size == font_size =>
            {
                drawn.paint.clone()
            }
            _ => TextPaintCache::holding(TextPaint::of(
                visual_style,
                prepared.visual_style_hash(),
                &prepared.text.span_styles,
                font_size,
                (prepared.render_text_hash(), prepared.text.text.is_empty()),
            )),
        },
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
