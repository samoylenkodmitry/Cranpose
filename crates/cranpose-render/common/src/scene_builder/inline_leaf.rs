// Leaf nodes that draw in their parent's layer: which nodes lower so, how
// they lower, and how a scene update brings one to its node's state.

use cranpose_core::{MemoryApplier, NodeId, collections::map::HashMap};
use cranpose_ui::{ModifierNodeSlices, Point, Rect, Size};

use super::{
    BuildNodeSnapshot, DrawTarget, Level, LowerContext, SnapshotNodeData, assign_layer,
    modifier_slices_have_origin_sinks, note_layer_lowered, prepare_node_layer, read_node_data,
    read_placed_node_data, remove_dirty_descendants, slices_hit_something,
    text_node::{TextNodeParts, drawn_text, refresh_text_node},
    translate_layer::holds_hit_geometry,
    write_node_content, write_node_layer,
};
use crate::{
    NodeUpdate,
    graph::{InlineNode, LayerNode, RenderNode},
};

// Whether a node with `slices`, `size` and no children draws in its
// parent's layer: it sets no graphics layer and no clip, chains no draws
// before a layer, handles no pointer, publishes no window geometry, does
// not translate content, and its layer would be its own rect.
fn slices_lower_inline(slices: &ModifierNodeSlices, size: Size) -> bool {
    slices.outer_draw_command_count() == 0
        && !slices.clip_to_bounds()
        && !slices.has_graphics_layer()
        && !slices.translated_content_context()
        && !slices_hit_something(slices)
        && slices.pointer_input_size_sinks().is_empty()
        && !modifier_slices_have_origin_sinks(slices)
        && slices.layer_bounds(size) == Rect::from_size(size)
}

// Whether the child node of `data` draws in its parent's layer.
pub(super) fn lowers_inline(data: &SnapshotNodeData<'_>) -> bool {
    data.children.is_empty()
        && !data.window_root
        && slices_lower_inline(&data.modifier_slices, data.layout_state.size())
}

// Whether `snapshot`, a child in a layout tree snapshot, draws in its
// parent's layer.
pub(super) fn snapshot_lowers_inline(snapshot: &BuildNodeSnapshot) -> bool {
    snapshot.graphics_layer.is_none()
        && snapshot.children.is_empty()
        && slices_lower_inline(&snapshot.slices, snapshot.size)
}

// Lowers the placed child `node_id` into the node its parent holds: inline
// when it needs no layer of its own, else its layer.
pub(super) fn lower_child(
    applier: &MemoryApplier,
    node_id: NodeId,
    context: LowerContext,
) -> Option<RenderNode> {
    read_placed_node_data(applier, node_id, Level::Child, |data| {
        lower_placed_child(applier, node_id, data, context)
    })
}

fn lower_placed_child(
    applier: &MemoryApplier,
    node_id: NodeId,
    data: SnapshotNodeData<'_>,
    context: LowerContext,
) -> RenderNode {
    if !lowers_inline(&data) {
        let mut layer = crate::layer_recycling::layer_box();
        write_node_layer(applier, node_id, data, context, &mut layer);
        return RenderNode::Layer(layer);
    }
    let mut inline = crate::layer_recycling::inline_box().unwrap_or_default();
    write_inline(&mut inline, node_id, &data, context);
    if draws_inline(&inline) {
        return RenderNode::Inline(inline);
    }
    let layer = layer_from_content(&mut inline, node_id, &data, context);
    crate::layer_recycling::recycle_node(RenderNode::Inline(inline));
    RenderNode::Layer(layer)
}

// Writes the node `node_id` of `data`, placed under `context`, into
// `inline`, whose content list is empty.
fn write_inline(
    inline: &mut InlineNode,
    node_id: NodeId,
    data: &SnapshotNodeData<'_>,
    context: LowerContext,
) {
    let slices = &data.modifier_slices;
    let size = data.layout_state.size();
    place_inline(inline, data, context);
    inline.node_id = node_id;
    inline.size = size;
    let draws = DrawTarget {
        size,
        motion_context_animated: inline.motion_context_animated,
        layer: None,
    };
    write_node_content(&mut inline.content, node_id, slices, draws, (0, 0), |_| {});
    inline.note_change();
}

// Puts `inline` where its node of `data` lies under `context`.
fn place_inline(inline: &mut InlineNode, data: &SnapshotNodeData<'_>, context: LowerContext) {
    let placement = data.layout_state.position();
    inline.origin = Point::new(
        placement.x + context.parent_content_offset.x,
        placement.y + context.parent_content_offset.y,
    );
    inline.motion_context_animated =
        context.inherited_motion_context_animated || data.modifier_slices.motion_context_animated();
}

// Whether the content just written into `inline` draws in its parent's
// layer: a transform or clip in its draws groups some of them into a layer,
// which only a layer of the node's own holds.
fn draws_inline(inline: &InlineNode) -> bool {
    inline
        .content
        .iter()
        .all(|node| !matches!(node, RenderNode::Layer(_)))
}

// The layer of the node of `data` around the content written into
// `inline`, which it takes.
fn layer_from_content(
    inline: &mut InlineNode,
    node_id: NodeId,
    data: &SnapshotNodeData<'_>,
    context: LowerContext,
) -> Box<LayerNode> {
    let mut layer = crate::layer_recycling::layer_box();
    std::mem::swap(&mut layer.children, &mut inline.content);
    let (head, _) = prepare_node_layer(node_id, &data.modifier_slices, &data.layout_state, context);
    assign_layer(&mut layer, head);
    layer
}

// Updates `slot`, the inline node of the dirty node `node_id`, for the
// change `kind` under `context`: in place while the node draws inline, else
// as its new layer. Returns whether hit geometry changed: only a node that
// takes a layer with a hit target changes it. `None` when the node's data
// is gone.
pub(super) fn update_dirty_inline(
    applier: &MemoryApplier,
    slot: &mut RenderNode,
    node_id: NodeId,
    kind: NodeUpdate,
    context: LowerContext,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) -> Option<bool> {
    read_child_data(applier, node_id, |data| {
        let RenderNode::Inline(inline) = slot else {
            unreachable!("the slot holds an inline node");
        };
        if lowers_inline(&data) {
            // Without a layer the node's layer properties draw nothing:
            // a node that kept its size only moves.
            let moved = matches!(
                kind,
                NodeUpdate::Moved | NodeUpdate::Layer | NodeUpdate::MovedLayer
            );
            if moved && inline.size == data.layout_state.size() {
                place_inline(inline, &data, context);
                return false;
            }
            if retext_inline(inline, &data, context) {
                return false;
            }
            note_layer_lowered();
            crate::layer_recycling::recycle_content(inline);
            write_inline(inline, node_id, &data, context);
            if draws_inline(inline) {
                return false;
            }
            let layer = layer_from_content(inline, node_id, &data, context);
            crate::layer_recycling::recycle_node(std::mem::replace(slot, RenderNode::Layer(layer)));
            return false;
        }
        note_layer_lowered();
        let lowered = lower_placed_child(applier, node_id, data, context);
        crate::layer_recycling::recycle_node(std::mem::replace(slot, lowered));
        let RenderNode::Layer(layer) = slot else {
            unreachable!("a node that does not lower inline lowers as a layer");
        };
        remove_dirty_descendants(layer, dirty_nodes);
        holds_hit_geometry(layer)
    })
}

// Writes the text of the node of `data`, a label that drew its text and
// nothing else, into the text `inline` drew, placed under `context`: a text
// and style that stay keep their paint. False when either drew more.
fn retext_inline(
    inline: &mut InlineNode,
    data: &SnapshotNodeData<'_>,
    context: LowerContext,
) -> bool {
    let slices = &data.modifier_slices;
    if !slices.draw_commands().is_empty() || !slices.has_text() {
        return false;
    }
    let size = data.layout_state.size();
    let Some(text) = drawn_text(&mut inline.content) else {
        return false;
    };
    if !refresh_text_node(text, &TextNodeParts::of(inline.node_id, slices, size)) {
        return false;
    }
    place_inline(inline, data, context);
    inline.size = size;
    inline.note_change();
    true
}

// Writes the text of the inline node in `slot`, a label a translated
// container keeps for a content change, where it now lies under `context`.
// False when it no longer draws its text alone inline, which leaves it to
// the walk beneath the container.
pub(super) fn retext_kept_inline(
    applier: &MemoryApplier,
    inline: &mut InlineNode,
    context: LowerContext,
) -> bool {
    read_child_data(applier, inline.node_id, |data| {
        lowers_inline(&data) && retext_inline(inline, &data, context)
    })
    .unwrap_or(false)
}

// Reads the data of `node_id`, a placed child drawn in its parent's layer,
// as `read_placed_node_data` does without counting a lowering: an update
// that only moves or retexts the node lowers nothing. `None` for a node
// that is gone, not placed or a window root.
fn read_child_data<R>(
    applier: &MemoryApplier,
    node_id: NodeId,
    read: impl FnOnce(SnapshotNodeData<'_>) -> R,
) -> Option<R> {
    read_node_data(applier, node_id, |data| {
        (!data.window_root && data.layout_state.is_placed()).then(|| read(data))
    })?
}

// Turns `slot`, the layer of a dirty node a scene update kept, into the
// node drawn in its parent's layer, placed in a parent whose content is
// offset by `parent_content_offset`, once the node lowers inline, as a fresh
// build lowers it: a leaf whose graphics layer, clip or pointer handling
// went away. Every other layer keeps its layer after a check of its own
// fields, inline here: most kept layers have properties or children.
#[inline]
pub(super) fn fold_kept_leaf(
    applier: &MemoryApplier,
    slot: &mut RenderNode,
    parent_content_offset: Point,
) {
    if let RenderNode::Layer(layer) = slot
        && layer.graphics_layer.get().is_none()
        && layer.node_id.is_some()
        && layer.wraps.is_none()
        && !layer.clip_to_bounds
        && layer.hit_test.is_none()
        && !layer.has_origin_sinks
        && !layer.translated_content_context
        && layer.node_bounds.is_none()
        && layer.draw_phase.is_none()
        && !layer
            .children
            .iter()
            .any(|child| matches!(child, RenderNode::Layer(_) | RenderNode::Inline(_)))
    {
        fold_leaf_layer(applier, slot, parent_content_offset);
    }
}

// Turns `slot`, a leaf's layer without properties, into the node drawn in
// its parent's layer when its node lowers inline. A leaf's own draws
// without a group draw alike in either node, so the content moves over as
// it is.
#[inline(never)]
fn fold_leaf_layer(applier: &MemoryApplier, slot: &mut RenderNode, parent_content_offset: Point) {
    let RenderNode::Layer(layer) = slot else {
        return;
    };
    let Some(node_id) = layer.node_id else {
        return;
    };
    if !read_child_data(applier, node_id, |data| lowers_inline(&data)).unwrap_or(false) {
        return;
    }
    let mut inline = crate::layer_recycling::inline_box().unwrap_or_default();
    inline.node_id = node_id;
    inline.origin = Point::new(
        layer.origin_in_parent.x + parent_content_offset.x,
        layer.origin_in_parent.y + parent_content_offset.y,
    );
    let rect = layer.node_rect();
    inline.size = Size::new(rect.width, rect.height);
    inline.motion_context_animated = layer.motion_context_animated;
    std::mem::swap(&mut inline.content, &mut layer.children);
    inline.note_change();
    crate::layer_recycling::recycle_node(std::mem::replace(slot, RenderNode::Inline(inline)));
}
