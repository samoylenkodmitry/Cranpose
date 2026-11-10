use std::{rc::Rc, sync::Arc};

use cranpose_core::{MemoryApplier, NodeId, RecordId, collections::map::HashMap};
use cranpose_ui::{
    ModifierNodeSlices, PreparedTextLayout, Rect, Size, TextLayoutOptions, TextOverflow,
    TextPanResolver,
};

use super::{
    SnapshotNodeData, TranslateAncestorContext, TranslateGeometry, TranslatedContainer, Wrapping,
    hit_test_from_slices, read_node_data, set_origin_sinks, translate_bail,
    translate_layer::{apply_translated_container_state, translate_layer_from_data},
    translated_container,
};
use crate::{
    NodeUpdate,
    graph::TextPrimitiveNode,
    scene_store::{Item, ItemKind, RecordKind, SceneStore},
    text_paint::TextPaintCache,
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

// Brings `container`, a layer no wrapper holds, to its node's layout in
// `data`: a label that drew its text alone takes its text and place where
// it is, any other layer the full update.
pub(super) fn translate_unwrapped(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    container: RecordId,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    ancestors: TranslateAncestorContext,
    data: SnapshotNodeData<'_>,
) -> bool {
    if draws_text_alone(store, container)
        && let Some(done) = translate_leaf_text(store, container, &data, ancestors)
    {
        return done;
    }
    translate_layer_from_data(
        applier,
        store,
        container,
        dirty_nodes,
        ancestors,
        data,
        Wrapping::Bare,
    )
}

// Brings `container`, the layer of a leaf that drew its node's text and
// nothing else, to the node's layout in `data`, as a label a resize moves:
// writes the text into the text the layer drew and places the layer,
// recording nothing. `None` for any other layer, which the full update
// takes.
pub(super) fn translate_leaf_text(
    store: &mut SceneStore,
    container: RecordId,
    data: &SnapshotNodeData<'_>,
    ancestors: TranslateAncestorContext,
) -> Option<bool> {
    let slices = &data.modifier_slices;
    let leaf = data.children.is_empty() && !data.window_root && slices.draw_commands().is_empty();
    if !leaf || !slices.has_text() || drawn_text(store, container).is_none() {
        return None;
    }
    let layout_state = &data.layout_state;
    let TranslatedContainer { node_id } = match translated_container(
        store,
        container,
        layout_state,
        slices,
        ancestors,
        Wrapping::Bare,
    ) {
        Ok(plan) => plan,
        Err(reason) => return Some(translate_bail(reason)),
    };
    let parts = TextNodeParts::of(node_id, slices, layout_state.size());
    if !refresh_text_node(drawn_text(store, container)?, &parts) {
        return None;
    }
    let layer = store.layer(container);
    let geometry = TranslateGeometry::new(
        layout_state,
        layer.graphics_layer(),
        layer.local_bounds(),
        ancestors.parent_abs,
    );
    let offset = ancestors.parent_content_offset;
    apply_translated_container_state(store, container, slices, layout_state, offset, geometry);
    let hit_test = hit_test_from_slices(slices);
    store.set_hit_test(container, hit_test);
    set_origin_sinks(store, container, slices);
    store.note_content_change(container);
    Some(true)
}

// Writes the text of the node of `layer`, a label a translated container
// keeps for a content change, where the label lies. `None` when the label
// no longer draws text alone, which leaves it to the walk beneath the
// container.
pub(super) fn retext_kept_leaf(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    layer: RecordId,
    children: TranslateAncestorContext,
) -> Option<bool> {
    let node_id = store.layer(layer).node_id()?;
    read_node_data(applier, node_id, |data| {
        translate_leaf_text(store, layer, &data, children)
    })
    .flatten()
}

// Whether `layer` drew its node's text and nothing else.
pub(super) fn draws_text_alone(store: &SceneStore, layer: RecordId) -> bool {
    store.layer(layer).kind() != RecordKind::Wrapper && text_alone(store.items(layer))
}

// Whether `items`, what a node drew, are its text and nothing else.
pub(super) fn text_alone(items: &[Item]) -> bool {
    matches!(items, [item] if item.kind() == ItemKind::Text)
}

// The text `record` drew, when it drew that and nothing else.
pub(super) fn drawn_text(
    store: &mut SceneStore,
    record: RecordId,
) -> Option<&mut TextPrimitiveNode> {
    let [item] = *store.items(record) else {
        return None;
    };
    (item.kind() == ItemKind::Text).then(|| store.text_mut(item))
}

pub(super) fn text_node_from_parts(parts: &TextNodeParts) -> Option<TextPrimitiveNode> {
    let prepared = parts.measured_layout.as_ref()?;
    let placement = TextPlacement::of(parts, prepared)?;
    let visual_style = &prepared.visual_style;
    let font_size = visual_style.resolve_font_size(cranpose_ui_graphics::DEFAULT_FONT_SIZE);
    Some(TextPrimitiveNode {
        node_id: parts.node_id,
        rect: placement.rect,
        render_text: prepared.render_text(),
        layout: prepared.layout.clone(),
        text_style: Arc::clone(visual_style),
        font_size,
        overflow: placement.overflow,
        clip: placement.clip,
        paint: TextPaintCache::of_text(
            visual_style,
            prepared.visual_style_hash(),
            prepared.text.span_styles(),
            (font_size, &prepared.layout),
        ),
    })
}

// Writes the text of `parts` into `drawn`, the text its node drew before:
// a text and style that stay keep their paint, and only the box, the clip
// and the lines change. False, leaving `drawn` as it was, when the node
// draws no text now.
pub(super) fn refresh_text_node(drawn: &mut TextPrimitiveNode, parts: &TextNodeParts) -> bool {
    let Some(prepared) = parts.measured_layout.as_ref() else {
        return false;
    };
    let Some(placement) = TextPlacement::of(parts, prepared) else {
        return false;
    };
    if !Arc::ptr_eq(&drawn.render_text, &prepared.text)
        || !Arc::ptr_eq(&drawn.text_style, &prepared.visual_style)
    {
        let Some(text) = text_node_from_parts(parts) else {
            return false;
        };
        *drawn = text;
        return true;
    }
    drawn.rect = placement.rect;
    drawn.clip = placement.clip;
    drawn.overflow = placement.overflow;
    drawn.layout = prepared.layout.clone();
    true
}

// Where a node's text draws, and the rectangle that cuts it.
struct TextPlacement {
    rect: Rect,
    clip: Option<Rect>,
    overflow: TextOverflow,
}

impl TextPlacement {
    // Where the text of `parts`, laid out as `prepared`, draws; `None` when
    // its node has no width to draw it in. Inline: every text node places
    // its text each time the scene takes it.
    #[inline]
    fn of(parts: &TextNodeParts, prepared: &PreparedTextLayout) -> Option<Self> {
        let text_rect = parts.text_rect;
        let options = parts.text_layout_options.unwrap_or_default().normalized();
        let content_width = text_rect.width.max(0.0);
        if content_width <= 0.0 {
            return None;
        }
        let text_pan = parts.text_pan.as_ref();
        let pan_offset = text_pan.map_or(0.0, |resolve| resolve(content_width));
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
        // As Compose cuts a text at its box only when the layout overflows
        // it, glyph ink past a fitting layout draws; a text that pans cuts.
        let overflows = || {
            rect.width > text_bounds.width
                || rect.height > text_bounds.height
                || prepared.layout.did_overflow_width()
                || prepared.layout.did_overflow_height()
        };
        let clip = (text_pan.is_some()
            || (options.overflow != TextOverflow::Visible && overflows()))
        .then(|| pad_clip_rect(text_bounds));
        Some(Self {
            rect,
            clip,
            overflow: options.overflow,
        })
    }
}

fn pad_clip_rect(rect: Rect) -> Rect {
    Rect {
        x: rect.x - TEXT_CLIP_PAD,
        y: rect.y - TEXT_CLIP_PAD,
        width: (rect.width + TEXT_CLIP_PAD * 2.0).max(0.0),
        height: (rect.height + TEXT_CLIP_PAD * 2.0).max(0.0),
    }
}
