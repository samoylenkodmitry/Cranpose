//! How a scene update finds the layers of its dirty nodes: down from the
//! root through the layers above them, or along the way the last update
//! took.

use cranpose_core::{MemoryApplier, NodeId, NodeMarks, collections::map::HashMap};
use cranpose_ui::Point;

use super::{
    AbsOrigin, Level, LowerContext, TranslateAncestorContext, children_have_origin_sinks,
    layer_identity, note_space_change, read_placed_node_data, rebuild_tracking_hits,
    release_for_rebuild, remove_dirty_descendants, try_update_retained_layer, write_node_layer,
};
use crate::{
    NodeUpdate,
    graph::{LayerNode, RenderGraph, RenderNode},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ReplaceDirtyLayersReport {
    updated: bool,
    pub(super) hit_graph_dirty: bool,
}

/// The dirty nodes a scene update has yet to find, and every node above
/// them in the applier: a layer is lowered from its node's subtree, so a
/// layer whose node is in neither holds no dirty layer.
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

    /// The marks of the layer with `identity`: whether its node is dirty and
    /// whether its subtree may hold a dirty layer. Both for a layer without
    /// a node.
    #[inline]
    fn marks(&self, identity: Option<NodeId>) -> u8 {
        identity.map_or(DIRTY_NODE | DIRTY_ANCESTOR, |id| {
            self.ancestry.get(self.applier, id)
        })
    }
}

/// The mark of a node above a dirty node.
const DIRTY_ANCESTOR: u8 = 1;
/// The mark of a dirty node: a layer whose node has no mark needs no map
/// lookup.
const DIRTY_NODE: u8 = 2;

/// Walks from `root` down to the layers of the `dirty` nodes and updates
/// them. With the report comes the way to the layer of a lone dirty node
/// the walk updated, for the next update that changes that node alone.
pub(super) fn walk_dirty_layers(
    applier: &MemoryApplier,
    graph: &mut RenderGraph,
    dirty: &mut HashMap<NodeId, NodeUpdate>,
    update: u64,
) -> Option<(ReplaceDirtyLayersReport, Option<RetainedPath>)> {
    let RenderGraph {
        root,
        dirty_ancestry: ancestry,
        ..
    } = graph;
    let inherited = root.translated_content_context;
    let root_children = AbsOrigin::ROOT.children_of(root);
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
        root,
        root_children,
        walk.reborrow(),
        inherited,
        update,
    );
    let path = match (&walked, lone) {
        (Some(report), Some(node)) if report.updated && walk.nodes.is_empty() => {
            RetainedPath::to(root, &walk, node, update)
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
    update: u64,
}

impl DirtyAncestry {
    // Marks the `dirty` nodes of update `update` and the nodes above them,
    // unless the update before marked the same nodes: as many, each marked
    // dirty.
    #[inline]
    fn mark(&mut self, applier: &MemoryApplier, dirty: &HashMap<NodeId, NodeUpdate>, update: u64) {
        let kept = self.update + 1 == update
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

/// The way from the root to the layer of a node that the last scene
/// updates changed alone, as an animation changes one layer each frame,
/// and what the layer's parent passes it. The next update that changes that
/// node alone goes along it instead of walking down from the root: no
/// other update ran since, so the layers on the way are where they were.
#[derive(Clone)]
pub(crate) struct RetainedPath {
    pub(super) node: NodeId,
    /// The index of each layer on the way in its parent's children and the
    /// identity it had there, from a child of the root to the node's layer.
    pub(super) steps: Vec<(usize, Option<NodeId>)>,
    pub(super) pass: ParentPass,
    /// The update that went along the path last.
    pub(super) update: u64,
    /// Whether a layer on the way is a scene, a surface or a 3D transform,
    /// so a change at the end of the path changes what lies in 3D.
    pub(super) in_space: bool,
}

impl RetainedPath {
    /// The way to the layer of `node` through the layers `walk` marks, as
    /// the walk of update `update` went.
    fn to(root: &LayerNode, walk: &DirtyWalk<'_>, node: NodeId, update: u64) -> Option<Self> {
        let mut steps = Vec::new();
        let pass = path_steps(
            root,
            AbsOrigin::ROOT.children_of(root),
            root.translated_content_context,
            walk,
            node,
            &mut steps,
        )?;
        let in_space = root.graphics_layer.space.is_some() || {
            let mut layer = root;
            steps
                .iter()
                .any(|&(index, _)| match layer.children.get(index) {
                    Some(RenderNode::Layer(child)) => {
                        layer = child;
                        child.graphics_layer.space.is_some()
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

/// Appends to `steps` the way from `parent`, whose children sit at
/// `parent_children`, to the layer of `node`, searching the children as the
/// dirty walk does, and returns what that layer's parent passes it.
fn path_steps(
    parent: &LayerNode,
    parent_children: AbsOrigin,
    inherited_translated_content_context: bool,
    walk: &DirtyWalk<'_>,
    node: NodeId,
    steps: &mut Vec<(usize, Option<NodeId>)>,
) -> Option<ParentPass> {
    let translated = inherited_translated_content_context || parent.translated_content_context;
    for (index, child) in parent.children.iter().enumerate() {
        let RenderNode::Layer(layer) = child else {
            continue;
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

/// Updates the lone dirty node in `dirty` along `path` when the path leads
/// to it: `None` when it does not, and nothing changed. Otherwise the
/// update's report, `None` when the node's data is gone.
pub(super) fn update_along_retained_path(
    applier: &MemoryApplier,
    root: &mut LayerNode,
    path: &RetainedPath,
    dirty: &mut HashMap<NodeId, NodeUpdate>,
    update: u64,
) -> Option<Option<ReplaceDirtyLayersReport>> {
    if dirty.len() != 1 || path.update + 1 != update || !leads_to(root, &path.steps) {
        return None;
    }
    let kind = dirty.remove(&path.node)?;
    if path.in_space {
        note_space_change();
    }
    Some(update_at_step(
        applier,
        root,
        path,
        &path.steps,
        kind,
        dirty,
        update,
    ))
}

/// Whether each step of `steps` from `layer` finds the layer it names.
fn leads_to(mut layer: &LayerNode, steps: &[(usize, Option<NodeId>)]) -> bool {
    for &(index, identity) in steps {
        match layer.children.get(index) {
            Some(RenderNode::Layer(child)) if layer_identity(child) == identity => layer = child,
            _ => return false,
        }
    }
    !steps.is_empty()
}

/// Goes down `steps` from `layer` to the path's layer, updates it for
/// `kind`, and brings the facts of each layer on the way up to date.
fn update_at_step(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    path: &RetainedPath,
    steps: &[(usize, Option<NodeId>)],
    kind: NodeUpdate,
    dirty: &mut HashMap<NodeId, NodeUpdate>,
    update: u64,
) -> Option<ReplaceDirtyLayersReport> {
    let (&(index, _), rest) = steps.split_first()?;
    let Some(RenderNode::Layer(child)) = layer.children.get_mut(index) else {
        return None;
    };
    let hit_graph_dirty = if rest.is_empty() {
        update_dirty_layer(applier, child, path.node, kind, dirty, path.pass, update)?.0
    } else {
        update_at_step(applier, child, path, rest, kind, dirty, update)?.hit_graph_dirty
    };
    note_updated_children(layer, update);
    Some(ReplaceDirtyLayersReport {
        updated: true,
        hit_graph_dirty,
    })
}

fn replace_dirty_layers_from_applier(
    applier: &MemoryApplier,
    parent: &mut LayerNode,
    parent_children: AbsOrigin,
    mut dirty: DirtyWalk<'_>,
    inherited_translated_content_context: bool,
    update: u64,
) -> Option<ReplaceDirtyLayersReport> {
    if dirty.nodes.is_empty() {
        return Some(ReplaceDirtyLayersReport::default());
    }

    let child_inherited_translated_content_context =
        inherited_translated_content_context || parent.translated_content_context;
    let pass = ParentPass::of(
        parent,
        parent_children,
        child_inherited_translated_content_context,
    );
    let mut report = ReplaceDirtyLayersReport::default();

    for child in &mut parent.children {
        if dirty.nodes.is_empty() {
            break;
        }
        let RenderNode::Layer(child_layer) = child else {
            continue;
        };

        let identity = layer_identity(child_layer);
        let marks = dirty.marks(identity);
        if marks == 0 {
            continue;
        }
        // A dirty layer in 3D, or one above a dirty layer, changes what lies
        // in 3D.
        if child_layer.graphics_layer.space.is_some() {
            note_space_change();
        }
        if marks & DIRTY_NODE != 0
            && let Some(node_id) = identity
            && let Some(kind) = dirty.nodes.remove(&node_id)
        {
            let (hit_graph_dirty, retained) = update_dirty_layer(
                applier,
                child_layer,
                node_id,
                kind,
                dirty.nodes,
                pass,
                update,
            )?;
            report.hit_graph_dirty |= hit_graph_dirty;
            report.updated = true;
            if retained {
                let child_children = parent_children.children_of(child_layer);
                let child_report = replace_dirty_layers_from_applier(
                    applier,
                    child_layer,
                    child_children,
                    dirty.reborrow(),
                    child_inherited_translated_content_context,
                    update,
                )?;
                report.hit_graph_dirty |= child_report.hit_graph_dirty;
            }
            continue;
        }

        if marks & DIRTY_ANCESTOR == 0 {
            continue;
        }
        let child_children = parent_children.children_of(child_layer);
        let child_report = replace_dirty_layers_from_applier(
            applier,
            child_layer,
            child_children,
            dirty.reborrow(),
            child_inherited_translated_content_context,
            update,
        )?;
        report.updated |= child_report.updated;
        report.hit_graph_dirty |= child_report.hit_graph_dirty;
    }

    if report.updated {
        note_updated_children(parent, update);
    }

    Some(report)
}

/// Brings `parent`'s facts about its children up to date after scene update
/// `update` changed a layer beneath it.
fn note_updated_children(parent: &mut LayerNode, update: u64) {
    parent.refresh_child_facts();
    // A layer's own sinks are not kept apart from its children's, so a
    // child that stopped publishing leaves the flag set, which only keeps
    // the scroll fast path off; a child that started must set it.
    parent.has_origin_sinks |= children_have_origin_sinks(&parent.children);
    parent.note_descendant_change(update);
}

/// What a layer passes the children it lays out to a scene update that
/// changes one of them.
#[derive(Clone, Copy)]
pub(crate) struct ParentPass {
    motion_context_animated: bool,
    translated_content_context: bool,
    content_offset: Point,
    children: AbsOrigin,
}

impl ParentPass {
    /// What `parent`, whose children sit at `children`, passes them, where
    /// the layers above it set `translated_content_context`.
    fn of(parent: &LayerNode, children: AbsOrigin, translated_content_context: bool) -> Self {
        Self {
            motion_context_animated: parent.motion_context_animated,
            translated_content_context,
            content_offset: parent.content_offset,
            children,
        }
    }
}

/// Updates `layer`, the layer of the dirty node `node_id`, for the change
/// `kind`: in place when the layer can keep what it drew, else lowered
/// again from the node. Returns whether hit geometry changed and whether
/// the layer was kept, whose children may hold dirty layers yet; a layer
/// lowered again drops the dirty nodes beneath it from `dirty_nodes`.
/// `None` when the node's data is gone.
fn update_dirty_layer(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    node_id: NodeId,
    kind: NodeUpdate,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    pass: ParentPass,
    update: u64,
) -> Option<(bool, bool)> {
    if let Some(hit_graph_dirty) = try_update_retained_layer(
        applier,
        layer,
        dirty_nodes,
        kind,
        Level::Child,
        TranslateAncestorContext {
            inherited_motion_context_animated: pass.motion_context_animated,
            update,
            inherited_translated_content_context: pass.translated_content_context,
            parent_content_offset: pass.content_offset,
            parent_abs: pass.children,
        },
    ) {
        return Some((hit_graph_dirty, true));
    }
    // Nodes that leave this subtree are gone from the scene: nothing
    // remains to update for them, and a dirty one would otherwise go
    // unmatched and rebuild the whole scene.
    remove_dirty_descendants(layer, dirty_nodes);
    let context = LowerContext {
        inherited_motion_context_animated: pass.motion_context_animated,
        inherited_translated_content_context: pass.translated_content_context,
        parent_abs: Some(pass.children),
        parent_content_offset: pass.content_offset,
    };
    let hit_graph_dirty = rebuild_tracking_hits(layer, |layer| {
        release_for_rebuild(layer);
        read_placed_node_data(applier, node_id, Level::Child, |data| {
            write_node_layer(applier, node_id, data, context, layer);
        })
        .is_some()
    })?;
    remove_dirty_descendants(layer, dirty_nodes);
    layer.note_content_change(update);
    Some((hit_graph_dirty, false))
}
