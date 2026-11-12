//! How a scene update finds the records of its dirty nodes: through the
//! store's table of node records, with what each record's parent passes to
//! its children worked out from the records above it.

use cranpose_core::{MemoryApplier, NodeId, RecordId, collections::map::HashMap};

use super::{
    AbsOrigin, GraphChanges, Inherited, Level, LowerContext, TranslateAncestorContext,
    inline_leaf::fold_kept_leaf, note_space_change, read_placed_node_data, rebuild_tracking_hits,
    redraw_retained_layer, release_for_rebuild, remove_dirty_descendants,
    try_update_retained_layer, update_dirty_inline, write_node_layer,
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
        Some((0, store.parent(record)?, node))
    }));
    // The update of a record may rewrite or drop the records beneath it, so
    // the records nearest the root go first. Siblings come together, and
    // share what the records above them pass down.
    if order.len() > 1 {
        for (depth_of, parent, _) in order.iter_mut() {
            *depth_of = depth(store, *parent);
        }
        order.sort_unstable();
    }
    let mut changes = GraphChanges::default();
    let mut last: Option<Lineage> = None;
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
        if last
            .as_ref()
            .is_some_and(|lineage| lineage.parent != parent)
        {
            last = None;
        }
        let lineage = last.get_or_insert_with(|| Lineage::of(store, parent));
        changes |= update_dirty_record(applier, store, (record, node, kind), lineage, dirty)?;
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
    lineage: &mut Lineage,
    dirty: &mut HashMap<NodeId, NodeUpdate>,
) -> Option<GraphChanges> {
    let pass = ParentPass::of(store, lineage);
    // A dirty record in 3D, or one beneath a record in 3D, changes what lies
    // in 3D.
    if pass.in_space || store.layer(record).graphics_layer().space.is_some() {
        note_space_change();
    }
    if store.kind(record) == RecordKind::Leaf {
        let context = pass.lower_context(None);
        let window = |store: &SceneStore| lineage.children(store);
        let hits =
            update_dirty_inline(applier, store, (record, node, kind), context, window, dirty)?;
        return Some(GraphChanges::hits(hits));
    }
    let changes = update_dirty_layer(applier, store, (record, node, kind), dirty, (pass, lineage))?;
    fold_kept_leaf(applier, store, record, pass.inherited.parent_content_offset);
    Some(changes)
}

// What the records from the root down to one parent record pass to its
// children, shared by the dirty records beneath that parent.
struct Lineage {
    parent: RecordId,
    // Whether the parent or a record above it translates its content.
    translated: bool,
    // Whether the parent or a record above it lies in 3D.
    in_space: bool,
    // Where the children of the parent lie. Absent until an update reads
    // it: a redraw never does.
    children: Option<AbsOrigin>,
}

impl Lineage {
    fn of(store: &SceneStore, parent: RecordId) -> Self {
        let mut lineage = Self {
            parent,
            translated: false,
            in_space: false,
            children: None,
        };
        let mut record = Some(parent);
        while let Some(current) = record
            && !(lineage.translated && lineage.in_space)
        {
            let layer = store.layer(current);
            lineage.translated |= layer.translated_content_context();
            lineage.in_space |= layer.graphics_layer().space.is_some();
            record = store.parent(current);
        }
        lineage
    }

    // Worked out from the root down at the first read, then kept for the
    // siblings: a record's children lie where the records above it put them.
    fn children(&mut self, store: &SceneStore) -> AbsOrigin {
        *self
            .children
            .get_or_insert_with(|| children_origin(store, self.parent))
    }
}

fn children_origin(store: &SceneStore, record: RecordId) -> AbsOrigin {
    let above = store
        .parent(record)
        .map_or(AbsOrigin::ROOT, |parent| children_origin(store, parent));
    above.children_of(store.layer(record))
}

#[derive(Clone, Copy)]
struct ParentPass {
    inherited: Inherited,
    in_space: bool,
}

impl ParentPass {
    fn of(store: &SceneStore, lineage: &Lineage) -> Self {
        let parent = store.layer(lineage.parent);
        Self {
            inherited: Inherited {
                motion_context_animated: parent.motion_context_animated(),
                translated_content_context: lineage.translated,
                parent_content_offset: parent.content_offset(),
            },
            in_space: lineage.in_space,
        }
    }

    // What a child the parent lays out lowers with, `children` being where
    // the parent's children lie: `None` for a child that publishes no window
    // geometry.
    fn lower_context(self, children: Option<AbsOrigin>) -> LowerContext {
        LowerContext {
            inherited: self.inherited,
            parent_abs: children,
        }
    }
}

fn update_dirty_layer(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    (layer, node_id, kind): (RecordId, NodeId, NodeUpdate),
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    (pass, lineage): (ParentPass, &mut Lineage),
) -> Option<GraphChanges> {
    if kind == NodeUpdate::Draws
        && redraw_retained_layer(applier, store, layer, Level::Child, pass.inherited)
    {
        return Some(GraphChanges::default());
    }
    let children = lineage.children(store);
    if let Some(changes) = try_update_retained_layer(
        applier,
        store,
        layer,
        dirty_nodes,
        kind,
        Level::Child,
        TranslateAncestorContext {
            inherited: pass.inherited,
            parent_abs: children,
        },
    ) {
        return Some(changes);
    }
    // Nodes that leave this subtree are gone from the scene: nothing
    // remains to update for them, and a dirty one would otherwise go
    // unmatched and rebuild the whole scene.
    remove_dirty_descendants(store, layer, dirty_nodes);
    let context = pass.lower_context(Some(children));
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
