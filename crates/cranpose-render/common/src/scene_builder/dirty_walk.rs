//! How a scene update finds the layers of its dirty nodes: down from the
//! root through the layers above them, or along the way the last update
//! took.

use cranpose_core::{
    MemoryApplier, NodeId, NodeMarks, RecordId, SceneUpdate, collections::map::HashMap,
};
use cranpose_ui::Point;

use super::{
    AbsOrigin, GraphChanges, Level, LowerContext, TranslateAncestorContext,
    inline_leaf::fold_kept_leaf, layer_identity, note_space_change, read_placed_node_data,
    rebuild_tracking_hits, release_for_rebuild, remove_dirty_descendants,
    try_update_retained_layer, update_dirty_inline, write_node_layer,
};
use crate::{
    NodeUpdate,
    scene_store::{ItemRef, LayerRef, RecordKind, SceneStore},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ReplaceDirtyLayersReport {
    updated: bool,
    pub(super) changes: GraphChanges,
}

struct DirtyWalk<'a> {
    nodes: &'a mut HashMap<NodeId, NodeUpdate>,
    ancestry: &'a NodeMarks,
    applier: &'a MemoryApplier,
}

impl DirtyWalk<'_> {
    fn reborrow(&mut self) -> DirtyWalk<'_> {
        DirtyWalk {
            nodes: self.nodes,
            ancestry: self.ancestry,
            applier: self.applier,
        }
    }

    #[inline]
    fn marks(&self, identity: Option<NodeId>) -> u8 {
        identity.map_or(DIRTY_NODE | DIRTY_ANCESTOR, |id| {
            self.ancestry.get(self.applier, id)
        })
    }
}

const DIRTY_ANCESTOR: u8 = 1;
const DIRTY_NODE: u8 = 2;

pub(super) fn walk_dirty_layers(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    ancestry: &mut DirtyAncestry,
    dirty: &mut HashMap<NodeId, NodeUpdate>,
) -> Option<(ReplaceDirtyLayersReport, Option<RetainedPath>)> {
    let root = store.root()?;
    let update = store.update();
    let root_layer = store.layer(root);
    let inherited = root_layer.translated_content_context();
    let root_children = AbsOrigin::ROOT.children_of(root_layer);
    let lone = match dirty.keys().next() {
        Some(&node) if dirty.len() == 1 => Some(node),
        _ => None,
    };
    ancestry.mark(applier, dirty, update);
    let mut walk = DirtyWalk {
        nodes: dirty,
        ancestry: &ancestry.marks,
        applier,
    };
    let walked = replace_dirty_layers_from_applier(
        applier,
        store,
        root,
        root_children,
        walk.reborrow(),
        inherited,
    );
    let path = match (&walked, lone) {
        (Some(report), Some(node)) if report.updated && walk.nodes.is_empty() => {
            RetainedPath::to(store.layer(root), &walk, node, update)
        }
        _ => None,
    };
    walked.map(|report| (report, path))
}

// The marks a walk set on its dirty nodes and the nodes above them, kept
// for the next update of the graph when it changes the same nodes: as a
// node that moves to another parent makes both parents dirty, the nodes
// above them are where they were.
#[derive(Clone, Default)]
pub(crate) struct DirtyAncestry {
    marks: NodeMarks,
    // How many dirty nodes the marks hold.
    nodes: usize,
    update: SceneUpdate,
}

impl DirtyAncestry {
    // Marks the `dirty` nodes of update `update` and the nodes above them,
    // unless the update before marked the same nodes: as many, each marked
    // dirty.
    #[inline]
    fn mark(
        &mut self,
        applier: &MemoryApplier,
        dirty: &HashMap<NodeId, NodeUpdate>,
        update: SceneUpdate,
    ) {
        let kept = self.update.next() == update
            && self.nodes == dirty.len()
            && dirty
                .keys()
                .all(|&node| self.marks.get(applier, node) & DIRTY_NODE != 0);
        self.update = update;
        if kept {
            return;
        }
        self.nodes = dirty.len();
        mark_ancestry(&mut self.marks, applier, dirty);
    }
}

// Marks the `dirty` nodes and the nodes above them afresh. The marks walk
// as a local: the frame holds their vector's fields, where a reference
// would be loaded again after each virtual `parent` call.
#[inline(never)]
fn mark_ancestry(
    marks: &mut NodeMarks,
    applier: &MemoryApplier,
    dirty: &HashMap<NodeId, NodeUpdate>,
) {
    let mut fresh = std::mem::take(marks);
    fresh.reset(applier);
    for &node in dirty.keys() {
        fresh.add_with_ancestors(applier, node, DIRTY_NODE, DIRTY_ANCESTOR);
    }
    *marks = fresh;
}

#[derive(Clone)]
pub(crate) struct RetainedPath {
    pub(super) node: NodeId,
    pub(super) steps: Vec<(usize, Option<NodeId>)>,
    pub(super) pass: ParentPass,
    pub(super) update: SceneUpdate,
    pub(super) in_space: bool,
}

impl RetainedPath {
    fn to(
        root: LayerRef<'_>,
        walk: &DirtyWalk<'_>,
        node: NodeId,
        update: SceneUpdate,
    ) -> Option<Self> {
        let mut steps = Vec::new();
        let pass = path_steps(
            root,
            AbsOrigin::ROOT.children_of(root),
            root.translated_content_context(),
            walk,
            node,
            &mut steps,
        )?;
        let in_space = root.graphics_layer().space.is_some() || {
            let mut layer = root;
            steps
                .iter()
                .any(|&(index, _)| match layer.items().nth(index) {
                    Some(ItemRef::Layer(child)) => {
                        layer = child;
                        child.graphics_layer().space.is_some()
                    }
                    _ => false,
                })
        };
        Some(Self {
            node,
            steps,
            pass,
            update,
            in_space,
        })
    }
}

fn path_steps(
    parent: LayerRef<'_>,
    parent_children: AbsOrigin,
    inherited_translated_content_context: bool,
    walk: &DirtyWalk<'_>,
    node: NodeId,
    steps: &mut Vec<(usize, Option<NodeId>)>,
) -> Option<ParentPass> {
    let translated = inherited_translated_content_context || parent.translated_content_context();
    for (index, child) in parent.items().enumerate() {
        let layer = match child {
            ItemRef::Layer(layer) => layer,
            ItemRef::Leaf(leaf) if leaf.owner() == Some(node) => {
                steps.push((index, Some(node)));
                return Some(ParentPass::of(parent, parent_children, translated));
            }
            ItemRef::Leaf(_)
            | ItemRef::Draw(_)
            | ItemRef::Text(_)
            | ItemRef::Run(_)
            | ItemRef::Shape(_) => continue,
        };
        let identity = layer_identity(layer);
        if identity == Some(node) {
            steps.push((index, identity));
            return Some(ParentPass::of(parent, parent_children, translated));
        }
        if walk.marks(identity) & DIRTY_ANCESTOR == 0 {
            continue;
        }
        steps.push((index, identity));
        let children = parent_children.children_of(layer);
        if let Some(pass) = path_steps(layer, children, translated, walk, node, steps) {
            return Some(pass);
        }
        steps.pop();
    }
    None
}

pub(super) fn update_along_retained_path(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    path: &RetainedPath,
    dirty: &mut HashMap<NodeId, NodeUpdate>,
) -> Option<Option<ReplaceDirtyLayersReport>> {
    let root = store.root()?;
    if dirty.len() != 1
        || path.update.next() != store.update()
        || !leads_to(store.layer(root), &path.steps)
    {
        return None;
    }
    let kind = dirty.remove(&path.node)?;
    if path.in_space {
        note_space_change();
    }
    Some(update_at_step(
        applier,
        store,
        root,
        path,
        &path.steps,
        kind,
        dirty,
    ))
}

fn leads_to(mut layer: LayerRef<'_>, steps: &[(usize, Option<NodeId>)]) -> bool {
    let Some((&(last, identity), way)) = steps.split_last() else {
        return false;
    };
    for &(index, identity) in way {
        match layer.items().nth(index) {
            Some(ItemRef::Layer(child)) if layer_identity(child) == identity => layer = child,
            _ => return false,
        }
    }
    match layer.items().nth(last) {
        Some(ItemRef::Layer(child) | ItemRef::Leaf(child)) => layer_identity(child) == identity,
        Some(ItemRef::Draw(_) | ItemRef::Text(_) | ItemRef::Run(_) | ItemRef::Shape(_)) | None => {
            false
        }
    }
}

fn update_at_step(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    layer: RecordId,
    path: &RetainedPath,
    steps: &[(usize, Option<NodeId>)],
    kind: NodeUpdate,
    dirty: &mut HashMap<NodeId, NodeUpdate>,
) -> Option<ReplaceDirtyLayersReport> {
    let (&(index, _), rest) = steps.split_first()?;
    let child = store.items(layer).get(index)?.as_child()?;
    let leaf = store.layer(child).kind() == RecordKind::Leaf;
    let changes = match (leaf, rest.is_empty()) {
        (false, true) => {
            let changes =
                update_dirty_layer(applier, store, child, path.node, kind, dirty, path.pass)?.0;
            fold_kept_leaf(applier, store, child, path.pass.content_offset);
            changes
        }
        (false, false) => update_at_step(applier, store, child, path, rest, kind, dirty)?.changes,
        (true, true) => {
            let context = path.pass.lower_context();
            GraphChanges::hits(update_dirty_inline(
                applier, store, child, path.node, kind, context, dirty,
            )?)
        }
        (true, false) => return None,
    };
    Some(ReplaceDirtyLayersReport {
        updated: true,
        changes,
    })
}

fn replace_dirty_layers_from_applier(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    parent: RecordId,
    parent_children: AbsOrigin,
    mut dirty: DirtyWalk<'_>,
    inherited_translated_content_context: bool,
) -> Option<ReplaceDirtyLayersReport> {
    if dirty.nodes.is_empty() {
        return Some(ReplaceDirtyLayersReport::default());
    }

    let parent_layer = store.layer(parent);
    let child_inherited_translated_content_context =
        inherited_translated_content_context || parent_layer.translated_content_context();
    let pass = ParentPass::of(
        parent_layer,
        parent_children,
        child_inherited_translated_content_context,
    );
    let mut report = ReplaceDirtyLayersReport::default();

    for position in 0..store.items(parent).len() {
        if dirty.nodes.is_empty() {
            break;
        }
        let Some(child) = store.items(parent)[position].as_child() else {
            continue;
        };
        let child_layer = store.layer(child);
        if child_layer.kind() == RecordKind::Leaf {
            update_inline_child(applier, store, child, &mut dirty, pass, &mut report)?;
            continue;
        }

        let identity = layer_identity(child_layer);
        let marks = dirty.marks(identity);
        if marks == 0 {
            continue;
        }
        // A dirty layer in 3D, or one above a dirty layer, changes what lies
        // in 3D.
        if child_layer.graphics_layer().space.is_some() {
            note_space_change();
        }
        if marks & DIRTY_NODE != 0
            && let Some(node_id) = identity
            && let Some(kind) = dirty.nodes.remove(&node_id)
        {
            let (changes, retained) =
                update_dirty_layer(applier, store, child, node_id, kind, dirty.nodes, pass)?;
            report.changes |= changes;
            report.updated = true;
            if retained {
                let child_children = parent_children.children_of(store.layer(child));
                let child_report = replace_dirty_layers_from_applier(
                    applier,
                    store,
                    child,
                    child_children,
                    dirty.reborrow(),
                    child_inherited_translated_content_context,
                )?;
                report.changes |= child_report.changes;
            }
            fold_kept_leaf(applier, store, child, pass.content_offset);
            continue;
        }

        if marks & DIRTY_ANCESTOR == 0 {
            continue;
        }
        let child_children = parent_children.children_of(child_layer);
        let child_report = replace_dirty_layers_from_applier(
            applier,
            store,
            child,
            child_children,
            dirty.reborrow(),
            child_inherited_translated_content_context,
        )?;
        report.updated |= child_report.updated;
        report.changes |= child_report.changes;
    }

    Some(report)
}

// Updates `child`, a leaf, when its node is dirty, noting in `report` what
// changed. `None` when the node's data is gone. Inline: most leaves a walk
// passes are clean.
#[inline]
fn update_inline_child(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    child: RecordId,
    dirty: &mut DirtyWalk<'_>,
    pass: ParentPass,
    report: &mut ReplaceDirtyLayersReport,
) -> Option<()> {
    let Some(node_id) = store.layer(child).owner() else {
        return Some(());
    };
    if dirty.marks(Some(node_id)) & DIRTY_NODE == 0 {
        return Some(());
    }
    let Some(kind) = dirty.nodes.remove(&node_id) else {
        return Some(());
    };
    let context = pass.lower_context();
    report.changes |= GraphChanges::hits(update_dirty_inline(
        applier,
        store,
        child,
        node_id,
        kind,
        context,
        dirty.nodes,
    )?);
    report.updated = true;
    Some(())
}

#[derive(Clone, Copy)]
pub(crate) struct ParentPass {
    motion_context_animated: bool,
    translated_content_context: bool,
    content_offset: Point,
    children: AbsOrigin,
}

impl ParentPass {
    fn of(parent: LayerRef<'_>, children: AbsOrigin, translated_content_context: bool) -> Self {
        Self {
            motion_context_animated: parent.motion_context_animated(),
            translated_content_context,
            content_offset: parent.content_offset(),
            children,
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
) -> Option<(GraphChanges, bool)> {
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
        return Some((changes, true));
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
    Some((GraphChanges::hits(hit_graph_dirty), false))
}
