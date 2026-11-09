// Leaf nodes that draw in their parent's layer: which nodes lower so, how
// they lower, and how a scene update brings one to its node's state.

use cranpose_core::{MemoryApplier, NodeId, RecordId, collections::map::HashMap};
use cranpose_ui::{ModifierNodeSlices, Point, Rect, Size};

use super::{
    DrawTarget, Level, LowerContext, SnapshotNodeData, assign_layer,
    modifier_slices_have_origin_sinks, note_layer_lowered, prepare_node_layer, read_node_data,
    read_placed_node_data, remove_dirty_descendants, slices_hit_something,
    text_node::{TextNodeParts, drawn_text, refresh_text_node},
    translate_layer::holds_hit_geometry,
    write_node_content, write_node_layer,
};
use crate::{
    NodeUpdate,
    graph::ProjectiveTransform,
    scene_store::{Item, ItemKind, Leaf, RecordFlags, RecordKind, RecordSeed, SceneStore},
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

// Lowers the placed child `node_id` into a record under `parent`: a leaf
// when it needs no layer of its own, else its layer.
pub(super) fn lower_child(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    parent: RecordId,
    node_id: NodeId,
    context: LowerContext,
) -> Option<Item> {
    read_placed_node_data(applier, node_id, Level::Child, |data| {
        let record = store.insert(RecordSeed {
            kind: RecordKind::Layer,
            node: Some(node_id),
            parent: Some(parent),
        });
        write_placed_child(applier, store, record, node_id, data, context);
        Item::child(record)
    })
}

// Writes the placed child `node_id` of `data` into `record`, whose items
// are empty: as a leaf when it needs no layer of its own, else its layer.
fn write_placed_child(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    record: RecordId,
    node_id: NodeId,
    data: SnapshotNodeData<'_>,
    context: LowerContext,
) {
    if !lowers_inline(&data) {
        write_node_layer(applier, store, node_id, data, context, record);
        return;
    }
    write_inline(store, record, node_id, &data, context);
    if !draws_inline(store, record) {
        layer_from_content(store, record, node_id, &data, context);
    }
}

// Writes the node `node_id` of `data`, placed under `context`, into
// `record` as a leaf; its items are empty.
fn write_inline(
    store: &mut SceneStore,
    record: RecordId,
    node_id: NodeId,
    data: &SnapshotNodeData<'_>,
    context: LowerContext,
) {
    let slices = &data.modifier_slices;
    let size = data.layout_state.size();
    let flags = leaf_flags(data, context);
    let draws = DrawTarget {
        size,
        motion_context_animated: flags.contains(RecordFlags::MOTION_ANIMATED),
        paint: None,
    };
    let mut list = store.lists.take();
    write_node_content(store, record, &mut list, slices, draws, (0, 0), |_, _| {});
    store.write_leaf(
        record,
        Leaf {
            node: node_id,
            size,
            content_offset: data.layout_state.content_offset(),
            origin_in_parent: data.layout_state.position(),
            flags,
            items: &list,
        },
    );
    store.lists.give(list);
}

// Puts `record`, the leaf of the node of `data`, where the node lies under
// `context`.
fn place_inline(
    store: &mut SceneStore,
    record: RecordId,
    data: &SnapshotNodeData<'_>,
    context: LowerContext,
) {
    store.place_leaf(
        record,
        data.layout_state.position(),
        leaf_flags(data, context),
    );
}

// The flags of the leaf of the node of `data` under `context`: whether it
// lies in a motion context.
fn leaf_flags(data: &SnapshotNodeData<'_>, context: LowerContext) -> RecordFlags {
    if context.inherited_motion_context_animated || data.modifier_slices.motion_context_animated() {
        RecordFlags::MOTION_ANIMATED
    } else {
        RecordFlags::default()
    }
}

// Whether the content just written into `record` draws in its parent's
// layer: a transform or clip in its draws groups some of them into a
// record, which only a layer of the node's own holds.
fn draws_inline(store: &SceneStore, record: RecordId) -> bool {
    store
        .items(record)
        .iter()
        .all(|item| item.kind() != ItemKind::Child)
}

// Makes `record`, which holds the content of the node of `data`, the
// node's layer.
fn layer_from_content(
    store: &mut SceneStore,
    record: RecordId,
    node_id: NodeId,
    data: &SnapshotNodeData<'_>,
    context: LowerContext,
) {
    let (head, _) = prepare_node_layer(node_id, &data.modifier_slices, &data.layout_state, context);
    assign_layer(store, record, head);
}

// Updates `record`, the leaf of the dirty node `node_id`, for the change
// `kind` under `context`: in place while the node draws inline, else as
// its new layer. Returns whether hit geometry changed: only a node that
// takes a layer with a hit target changes it. `None` when the node's data
// is gone.
pub(super) fn update_dirty_inline(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    record: RecordId,
    node_id: NodeId,
    kind: NodeUpdate,
    context: LowerContext,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) -> Option<bool> {
    read_child_data(applier, node_id, |data| {
        if lowers_inline(&data) {
            // Without a layer the node's layer properties draw nothing:
            // a node that kept its size only moves.
            let moved = matches!(
                kind,
                NodeUpdate::Moved | NodeUpdate::Layer | NodeUpdate::MovedLayer
            );
            if moved && store.layer(record).size() == data.layout_state.size() {
                place_inline(store, record, &data, context);
                return false;
            }
            if retext_inline(store, record, &data, context) {
                return false;
            }
            note_layer_lowered();
            store.clear_items(record);
            write_inline(store, record, node_id, &data, context);
            if !draws_inline(store, record) {
                layer_from_content(store, record, node_id, &data, context);
            }
            return false;
        }
        note_layer_lowered();
        store.clear_items(record);
        write_placed_child(applier, store, record, node_id, data, context);
        remove_dirty_descendants(store, record, dirty_nodes);
        holds_hit_geometry(store.layer(record))
    })
}

// Writes the text of the node of `data`, a label that drew its text and
// nothing else, into the text `record` drew, placed under `context`: a
// text and style that stay keep their paint. False when either drew more.
fn retext_inline(
    store: &mut SceneStore,
    record: RecordId,
    data: &SnapshotNodeData<'_>,
    context: LowerContext,
) -> bool {
    let slices = &data.modifier_slices;
    if !slices.draw_commands().is_empty() || !slices.has_text() {
        return false;
    }
    let size = data.layout_state.size();
    let Some(node_id) = store.layer(record).owner() else {
        return false;
    };
    let Some(text) = drawn_text(store, record) else {
        return false;
    };
    if !refresh_text_node(text, &TextNodeParts::of(node_id, slices, size)) {
        return false;
    }
    place_inline(store, record, data, context);
    store.set_size(record, size.width, size.height);
    store.note_content_change(record);
    true
}

// Writes the text of `record`, the leaf of a label a translated container
// keeps for a content change, where it now lies under `context`. False
// when it no longer draws its text alone inline, which leaves it to the
// walk beneath the container.
pub(super) fn retext_kept_inline(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    record: RecordId,
    context: LowerContext,
) -> bool {
    let Some(node_id) = store.layer(record).owner() else {
        return false;
    };
    read_child_data(applier, node_id, |data| {
        lowers_inline(&data) && retext_inline(store, record, &data, context)
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

// Turns `record`, the layer of a dirty node a scene update kept, into the
// leaf drawn in its parent's layer, placed in a parent whose content is
// offset by `parent_content_offset`, once the node lowers inline, as a fresh
// build lowers it: a leaf whose graphics layer, clip or pointer handling
// went away. Every other layer keeps its layer after a check of its own
// fields, inline here: most kept layers have properties or children.
#[inline]
pub(super) fn fold_kept_leaf(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    record: RecordId,
    parent_content_offset: Point,
) {
    let layer = store.layer(record);
    if layer.kind() == RecordKind::Layer
        && layer.props().is_none()
        && layer.node_id().is_some()
        && !layer.clip_to_bounds()
        && !layer.translated_content_context()
        && layer.children().next().is_none()
        && !layer.has_origin_sinks()
    {
        fold_leaf_layer(applier, store, record, parent_content_offset);
    }
}

// Turns `record`, a leaf's layer without properties, into the leaf drawn in
// its parent's layer when its node lowers inline. A leaf's own draws
// without a group draw alike in either record, so the content stays as it
// is.
#[inline(never)]
fn fold_leaf_layer(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    record: RecordId,
    parent_content_offset: Point,
) {
    let layer = store.layer(record);
    let Some(node_id) = layer.node_id() else {
        return;
    };
    if !read_child_data(applier, node_id, |data| lowers_inline(&data)).unwrap_or(false) {
        return;
    }
    let origin = layer.origin_in_parent();
    let animated = layer.motion_context_animated();
    store.set_kind(record, RecordKind::Leaf, Some(node_id));
    store.set_placement(
        record,
        origin,
        ProjectiveTransform::translation(
            origin.x + parent_content_offset.x,
            origin.y + parent_content_offset.y,
        ),
    );
    store.set_flags(
        record,
        if animated {
            RecordFlags::MOTION_ANIMATED
        } else {
            RecordFlags::default()
        },
    );
    store.note_content_change(record);
}
