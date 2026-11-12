//! How a scene update finds the records of its dirty nodes: through the
//! store's table of node records, with what each record's parent passes to
//! its children worked out from the records above it.

use cranpose_core::{MemoryApplier, NodeId, RecordId, collections::map::HashMap};
use cranpose_ui::Point;

use super::{
    AbsOrigin, GraphChanges, Level, LowerContext, TranslateAncestorContext,
    inline_leaf::fold_kept_leaf, note_space_change, read_placed_node_data, rebuild_tracking_hits,
    release_for_rebuild, remove_dirty_descendants, try_update_retained_layer, update_dirty_inline,
    write_node_layer,
};
use crate::{
    NodeUpdate,
    scene_store::{RecordKind, SceneStore},
};

// A dirty node in the order its update takes: the depth of its outermost
// record, the record that holds that one, and the node.
pub(crate) type DirtyPlace = (u32, RecordId, NodeId);

// Updates the records of the nodes in `dirty` that the store draws, each
// with its update, and takes those nodes out of `dirty`: the nodes the store
// holds no record of stay. `order` is room for the nodes. `None` when the
// data of a node is gone.
pub(super) fn update_dirty_records(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    dirty: &mut HashMap<NodeId, NodeUpdate>,
    order: &mut Vec<DirtyPlace>,
) -> Option<GraphChanges> {
    order.clear();
    order.extend(dirty.keys().filter_map(|&node| {
        let record = outer_record(store, node)?;
        let parent = store.parent(record)?;
        Some((depth(store, parent), parent, node))
    }));
    // The update of a record may rewrite or drop the records beneath it, so
    // the records nearest the root go first. Siblings come together, and
    // share what the records above them pass down.
    if order.len() > 1 {
        order.sort_unstable();
    }
    let mut changes = GraphChanges::default();
    let mut last: Option<(RecordId, Lineage)> = None;
    for &(_, _, node) in order.iter() {
        let Some(record) = outer_record(store, node) else {
            continue;
        };
        let Some(parent) = store.parent(record) else {
            continue;
        };
        let Some(kind) = dirty.remove(&node) else {
            continue;
        };
        let lineage = match last {
            Some((above, lineage)) if above == parent => lineage,
            _ => Lineage::of(store, parent),
        };
        last = Some((parent, lineage));
        let pass = ParentPass::of(store, parent, lineage);
        changes |= update_dirty_record(applier, store, (record, node, kind), pass, dirty)?;
    }
    Some(changes)
}

// The outermost record of `node`: the wrapper around its layer, or its own
// record.
fn outer_record(store: &SceneStore, node: NodeId) -> Option<RecordId> {
    let mut record = store.record_of(node)?;
    while let Some(parent) = store.parent(record)
        && store.kind(parent) == RecordKind::Wrapper
        && store.node(parent) == Some(node)
    {
        record = parent;
    }
    Some(record)
}

// How many records hold `record`.
fn depth(store: &SceneStore, mut record: RecordId) -> u32 {
    let mut depth = 0;
    while let Some(parent) = store.parent(record) {
        depth += 1;
        record = parent;
    }
    depth
}

fn update_dirty_record(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    (record, node, kind): (RecordId, NodeId, NodeUpdate),
    pass: ParentPass,
    dirty: &mut HashMap<NodeId, NodeUpdate>,
) -> Option<GraphChanges> {
    // A dirty record in 3D, or one beneath a record in 3D, changes what lies
    // in 3D.
    if pass.in_space || store.layer(record).graphics_layer().space.is_some() {
        note_space_change();
    }
    if store.kind(record) == RecordKind::Leaf {
        let context = pass.lower_context();
        let hits = update_dirty_inline(applier, store, record, node, kind, context, dirty)?;
        return Some(GraphChanges::hits(hits));
    }
    let changes = update_dirty_layer(applier, store, record, node, kind, dirty, pass)?;
    fold_kept_leaf(applier, store, record, pass.content_offset);
    Some(changes)
}

// What the records from the root down to one record pass to its children.
#[derive(Clone, Copy)]
struct Lineage {
    // Where the children of the record lie.
    children: AbsOrigin,
    // Whether the record or one above it translates its content.
    translated: bool,
    // Whether the record or one above it lies in 3D.
    in_space: bool,
}

impl Lineage {
    // Worked out from the root down: a record's children lie where the
    // records above it put them.
    fn of(store: &SceneStore, record: RecordId) -> Self {
        let above = store.parent(record).map_or(
            Self {
                children: AbsOrigin::ROOT,
                translated: false,
                in_space: false,
            },
            |parent| Self::of(store, parent),
        );
        let layer = store.layer(record);
        Self {
            children: above.children.children_of(layer),
            translated: above.translated || layer.translated_content_context(),
            in_space: above.in_space || layer.graphics_layer().space.is_some(),
        }
    }
}

#[derive(Clone, Copy)]
struct ParentPass {
    motion_context_animated: bool,
    translated_content_context: bool,
    content_offset: Point,
    children: AbsOrigin,
    in_space: bool,
}

impl ParentPass {
    fn of(store: &SceneStore, parent: RecordId, lineage: Lineage) -> Self {
        let parent = store.layer(parent);
        Self {
            motion_context_animated: parent.motion_context_animated(),
            translated_content_context: lineage.translated,
            content_offset: parent.content_offset(),
            children: lineage.children,
            in_space: lineage.in_space,
        }
    }

    // What a child the parent lays out lowers with.
    fn lower_context(self) -> LowerContext {
        LowerContext {
            inherited_motion_context_animated: self.motion_context_animated,
            inherited_translated_content_context: self.translated_content_context,
            parent_abs: Some(self.children),
            parent_content_offset: self.content_offset,
        }
    }
}

fn update_dirty_layer(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    layer: RecordId,
    node_id: NodeId,
    kind: NodeUpdate,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    pass: ParentPass,
) -> Option<GraphChanges> {
    if let Some(changes) = try_update_retained_layer(
        applier,
        store,
        layer,
        dirty_nodes,
        kind,
        Level::Child,
        TranslateAncestorContext {
            inherited_motion_context_animated: pass.motion_context_animated,
            inherited_translated_content_context: pass.translated_content_context,
            parent_content_offset: pass.content_offset,
            parent_abs: pass.children,
        },
    ) {
        return Some(changes);
    }
    // Nodes that leave this subtree are gone from the scene: nothing
    // remains to update for them, and a dirty one would otherwise go
    // unmatched and rebuild the whole scene.
    remove_dirty_descendants(store, layer, dirty_nodes);
    let context = pass.lower_context();
    let hit_graph_dirty = rebuild_tracking_hits(store, layer, |store, layer| {
        release_for_rebuild(store, layer);
        read_placed_node_data(applier, node_id, Level::Child, |data| {
            write_node_layer(applier, store, node_id, data, context, layer);
        })
        .is_some()
    })?;
    remove_dirty_descendants(store, layer, dirty_nodes);
    store.note_content_change(layer);
    Some(GraphChanges::hits(hit_graph_dirty))
}
