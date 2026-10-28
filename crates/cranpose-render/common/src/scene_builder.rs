use std::{
    any::Any,
    borrow::Cow,
    ops::{Range, RangeInclusive},
    rc::Rc,
};

use cranpose_core::{MemoryApplier, NodeId, NodeMarks, collections::map::HashMap};
use cranpose_ui::{
    DrawCommand, LayoutBox, LayoutNode, ModifierNodeSlices, Point, PreparedTextLayout, Rect, Size,
    SubcomposeLayoutNode, TextLayoutOptions, TextOverflow, TextPanResolver, text::TextStyle,
};
use cranpose_ui_graphics::{
    CompositingStrategy, CornerRadii, DrawPlacement, GraphicsLayer, LayerShape, RoundedCornerShape,
    rounded_corner_alpha_mask_effect,
};

use crate::{
    NodeUpdate, SceneUpdates,
    graph::{
        CachePolicy, ContentChanges, HitTestNode, IsolationReasons, LayerNode, PrimitiveEntry,
        PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
        TextPrimitiveNode,
    },
    layer_transform::{
        layer_scales_or_rotates, layer_transform_to_parent, layer_transform_to_window,
    },
    text_paint::{TextPaint, TextPaintCache},
};

mod draw_recording;
use draw_recording::{
    DrawPass, DrawTarget, append_draw_nodes, bump_recording_generation, draw_nodes,
    layer_node_capacity,
};
pub(crate) use draw_recording::{RecorderKey, RecorderSlot};
pub use draw_recording::{clear_command_recordings_for_tests, draw_command_nodes_for_tests};

const TEXT_CLIP_PAD: f32 = 1.0;
const ROUNDED_CLIP_EDGE_FEATHER: f32 = 1.0;

#[derive(Clone, Default)]
struct BuildNodeSnapshot {
    node_id: NodeId,
    placement: Point,
    size: Size,
    content_offset: Point,
    /// The node's modifier slices, shared: its draw commands, handlers and
    /// text are read from them rather than copied out.
    slices: Rc<ModifierNodeSlices>,
    graphics_layer: Option<GraphicsLayer>,
    children: Vec<Self>,
}

struct SnapshotNodeData<'a> {
    layout_state: cranpose_ui::widgets::LayoutState,
    modifier_slices: Rc<ModifierNodeSlices>,
    children: &'a [NodeId],
    window_root: bool,
}

/// Why a scoped scene update could not be applied, forcing the caller to throw
/// the render graph away and build it again from the applier.
///
/// The reason has to travel with the outcome because the shell picks the
/// scoped path from the shape of the dirty set, before this code runs, and
/// logs that choice. A frame that chose the scoped path and then rebuilt the
/// whole scene is the expensive case, and in the log it reads exactly like a
/// cheap patch -- so the fallback says so itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphRebuildReason {
    /// The root was dirty and its replacement layer could not be built.
    RootLayerUnavailable,
    /// A dirty subtree's replacement layer could not be built.
    DirtyLayerUnavailable,
    /// This many dirty nodes own no layer in the render graph, so the scoped
    /// walk never reached them. A node enters the graph only as a `LayerNode`;
    /// a dirty node that never produced one -- or whose layer left the graph
    /// this frame -- cannot be patched in place.
    UnmatchedDirtyNodes(usize),
}

/// The outcome of a scoped scene update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphUpdate {
    /// The dirty nodes were updated in place.
    Patched,
    /// The update could not be applied and the graph must be built again, for the given reason.
    NeedsRebuild(GraphRebuildReason),
}

/// The outcome of a scoped update together with its effect on hit testing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphUpdateReport {
    /// Whether the graph was patched or must be rebuilt.
    pub update: GraphUpdate,
    /// Whether the update changed hit geometry or handlers, so the hit targets must be collected
    /// again.
    pub hit_graph_dirty: bool,
}

impl GraphUpdateReport {
    /// Whether the graph was patched in place.
    pub fn applied(self) -> bool {
        matches!(self.update, GraphUpdate::Patched)
    }

    /// Why the graph must be rebuilt, or `None` when it was patched.
    pub fn rebuild_reason(self) -> Option<GraphRebuildReason> {
        match self.update {
            GraphUpdate::Patched => None,
            GraphUpdate::NeedsRebuild(reason) => Some(reason),
        }
    }
}

fn note_layer_lowered() {
    #[cfg(test)]
    crate::thread_state::with(|state| {
        let count = &state.lowered_layer_count;
        count.set(count.get() + 1);
    });
}

#[cfg(test)]
fn reset_lowered_layer_count() {
    crate::thread_state::with(|state| state.lowered_layer_count.set(0));
}

#[cfg(test)]
fn lowered_layer_count() -> usize {
    crate::thread_state::with(|state| state.lowered_layer_count.get())
}

/// Builds the render graph of the layout box tree rooted at `root`.
pub fn build_graph_from_layout_tree(root: &LayoutBox, _scale: f32) -> RenderGraph {
    bump_recording_generation();
    let root_snapshot = layout_box_to_snapshot(root, None);
    let mut graph = RenderGraph::new(LayerNode::default());
    write_snapshot_layer(root_snapshot, LowerContext::ROOT, &mut graph.root);
    graph
}

/// Builds the render graph of the layout tree below `root` by reading nodes from `applier`.
/// Returns `None` when `root` cannot be lowered to a layer.
pub fn build_graph_from_applier(
    applier: &MemoryApplier,
    root: NodeId,
    _scale: f32,
) -> Option<RenderGraph> {
    bump_recording_generation();
    let mut graph = RenderGraph::new(LayerNode::default());
    lower_root_into(applier, root, &mut graph.root).then_some(graph)
}

/// Builds `root`'s graph again, its layers taking the allocations of
/// `previous`, the graph it replaces.
pub fn rebuild_graph_from_applier(
    applier: &MemoryApplier,
    root: NodeId,
    scale: f32,
    previous: Option<RenderGraph>,
) -> Option<RenderGraph> {
    let graph = match previous {
        Some(mut graph) => {
            release_for_rebuild(&mut graph.root);
            bump_recording_generation();
            graph.retained_path = None;
            graph.update += 1;
            graph.root.note_content_change(graph.update);
            lower_root_into(applier, root, &mut graph.root).then_some(graph)
        }
        None => build_graph_from_applier(applier, root, scale),
    };
    crate::layer_recycling::release();
    graph
}

/// Applies `updates` to `graph` in place. Returns `false` when the update could not be applied
/// and the graph must be built again.
pub fn update_graph_from_applier(
    applier: &MemoryApplier,
    graph: &mut RenderGraph,
    updates: SceneUpdates<'_>,
    scale: f32,
) -> bool {
    update_graph_from_applier_report(applier, graph, updates, scale).applied()
}

/// Like `update_graph_from_applier`, and also reports whether the hit targets must be collected
/// again or why the graph must be rebuilt.
#[expect(clippy::print_stderr, reason = "CRANPOSE_SCENE_UPDATE_DIAG asks")]
pub fn update_graph_from_applier_report(
    applier: &MemoryApplier,
    graph: &mut RenderGraph,
    updates: SceneUpdates<'_>,
    _scale: f32,
) -> GraphUpdateReport {
    let report = update_graph_from_applier_report_inner(applier, graph, updates);
    crate::layer_recycling::release();
    if let GraphUpdate::NeedsRebuild(reason) = report.update
        && cranpose_core::DIAGNOSTICS.scene_update_diag.is_set()
    {
        eprintln!(
            "[scene-update-diag] scoped update abandoned, whole scene rebuilt: {reason:?} dirty={}",
            updates.len()
        );
    }
    report
}

/// Puts into `dirty` each dirty node with the update it needs, the updates
/// of a node named more than once merged.
fn dirty_node_updates(updates: SceneUpdates<'_>, dirty: &mut HashMap<NodeId, NodeUpdate>) {
    dirty.clear();
    dirty.reserve(updates.len());
    for &(id, update) in updates {
        dirty
            .entry(id)
            .and_modify(|kind| *kind = kind.merge(update))
            .or_insert(update);
    }
}

#[expect(clippy::print_stderr, reason = "CRANPOSE_SCENE_UPDATE_DIAG asks")]
fn update_graph_from_applier_report_inner(
    applier: &MemoryApplier,
    graph: &mut RenderGraph,
    updates: SceneUpdates<'_>,
) -> GraphUpdateReport {
    if updates.is_empty() {
        return GraphUpdateReport {
            update: GraphUpdate::Patched,
            hit_graph_dirty: false,
        };
    }
    bump_recording_generation();
    graph.update += 1;
    let update = graph.update;

    if cranpose_core::DIAGNOSTICS.scene_update_diag.is_set() {
        eprintln!("[scene-update-diag] dirty={updates:?}");
    }

    // The map of the frame before keeps its storage for this one.
    let mut remaining_dirty_nodes =
        crate::thread_state::with(|state| state.dirty_node_scratch.take()).unwrap_or_default();
    dirty_node_updates(updates, &mut remaining_dirty_nodes);
    let report = update_dirty_nodes(applier, graph, &mut remaining_dirty_nodes, update);
    remaining_dirty_nodes.clear();
    crate::thread_state::with(|state| state.dirty_node_scratch.set(Some(remaining_dirty_nodes)));
    report
}

/// Updates `graph` for `remaining_dirty_nodes`, removing each node it
/// updates from the map.
fn update_dirty_nodes(
    applier: &MemoryApplier,
    graph: &mut RenderGraph,
    remaining_dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    update: u64,
) -> GraphUpdateReport {
    if let Some(root_id) = layer_identity(&graph.root)
        && let Some(kind) = remaining_dirty_nodes.remove(&root_id)
    {
        if let Some(hit_graph_dirty) = try_update_retained_layer(
            applier,
            &mut graph.root,
            remaining_dirty_nodes,
            kind,
            Level::Root,
            TranslateAncestorContext {
                inherited_motion_context_animated: false,
                update,
                inherited_translated_content_context: false,
                parent_content_offset: Point::default(),
                parent_abs: AbsOrigin::ROOT,
            },
        ) {
            if remaining_dirty_nodes.is_empty() {
                return GraphUpdateReport {
                    update: GraphUpdate::Patched,
                    hit_graph_dirty,
                };
            }
            let walked = walk_dirty_layers(applier, &mut graph.root, remaining_dirty_nodes, update)
                .map(|(report, _)| report);
            return GraphUpdateReport {
                update: classify_walk(applier, walked.is_some(), remaining_dirty_nodes),
                hit_graph_dirty: hit_graph_dirty || walked.is_none_or(|r| r.hit_graph_dirty),
            };
        }

        let Some(hit_graph_dirty) = rebuild_tracking_hits(&mut graph.root, |root| {
            release_for_rebuild(root);
            lower_root_into(applier, root_id, root)
        }) else {
            return GraphUpdateReport {
                update: GraphUpdate::NeedsRebuild(GraphRebuildReason::RootLayerUnavailable),
                hit_graph_dirty: true,
            };
        };
        graph.root.note_content_change(update);

        return GraphUpdateReport {
            update: GraphUpdate::Patched,
            hit_graph_dirty,
        };
    }

    // An update that does not go along the retained path may move the
    // layers on it, so only one that does keeps it.
    if let Some(path) = graph.retained_path.take()
        && let Some(report) = update_along_retained_path(
            applier,
            &mut graph.root,
            &path,
            remaining_dirty_nodes,
            update,
        )
    {
        if report.is_some() {
            graph.retained_path = Some(RetainedPath { update, ..path });
        }
        return match report {
            Some(report) => GraphUpdateReport {
                update: GraphUpdate::Patched,
                hit_graph_dirty: report.hit_graph_dirty,
            },
            None => GraphUpdateReport {
                update: GraphUpdate::NeedsRebuild(GraphRebuildReason::DirtyLayerUnavailable),
                hit_graph_dirty: true,
            },
        };
    }

    let Some((report, path)) =
        walk_dirty_layers(applier, &mut graph.root, remaining_dirty_nodes, update)
    else {
        return GraphUpdateReport {
            update: GraphUpdate::NeedsRebuild(GraphRebuildReason::DirtyLayerUnavailable),
            hit_graph_dirty: true,
        };
    };
    graph.retained_path = path;

    match classify_walk(applier, true, remaining_dirty_nodes) {
        GraphUpdate::Patched => GraphUpdateReport {
            update: GraphUpdate::Patched,
            hit_graph_dirty: report.hit_graph_dirty,
        },
        update => GraphUpdateReport {
            update,
            hit_graph_dirty: true,
        },
    }
}

/// The update a walk made. A dirty node the walk did not find needs the whole
/// scene built again, unless no layer draws it.
fn classify_walk(
    applier: &MemoryApplier,
    walked: bool,
    remaining_dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) -> GraphUpdate {
    if !walked {
        return GraphUpdate::NeedsRebuild(GraphRebuildReason::DirtyLayerUnavailable);
    }
    remaining_dirty_nodes.retain(|&node, _| is_drawn(applier, node));
    if !remaining_dirty_nodes.is_empty() {
        GraphUpdate::NeedsRebuild(GraphRebuildReason::UnmatchedDirtyNodes(
            remaining_dirty_nodes.len(),
        ))
    } else {
        GraphUpdate::Patched
    }
}

/// Whether the scene draws `node`: each subcompose node above it draws the
/// slot `node` is in. A slot a lazy list keeps for reuse, or composes before
/// it shows it, stays in the applier's tree without being drawn, and its
/// nodes still change.
fn is_drawn(applier: &MemoryApplier, node: NodeId) -> bool {
    let mut child = node;
    let mut current = node;
    while let Some(parent) = applier
        .get_ref(current)
        .ok()
        .and_then(cranpose_core::Node::parent)
    {
        let Ok(parent_node) = applier.get_ref(parent) else {
            return true;
        };
        current = parent;
        if parent_node.is_virtual() {
            continue;
        }
        let parent_node: &dyn Any = parent_node;
        if let Some(subcompose) = parent_node.downcast_ref::<SubcomposeLayoutNode>()
            && !subcompose.with_active_children(|children| children.contains(&child))
        {
            return false;
        }
        child = parent;
    }
    true
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ReplaceDirtyLayersReport {
    updated: bool,
    hit_graph_dirty: bool,
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
fn walk_dirty_layers(
    applier: &MemoryApplier,
    root: &mut LayerNode,
    dirty: &mut HashMap<NodeId, NodeUpdate>,
    update: u64,
) -> Option<(ReplaceDirtyLayersReport, Option<RetainedPath>)> {
    let inherited = root.translated_content_context;
    let root_children = AbsOrigin::ROOT.children_of(root);
    let lone = match dirty.keys().next() {
        Some(&node) if dirty.len() == 1 => Some(node),
        _ => None,
    };
    let mut ancestry = crate::thread_state::with(|state| state.dirty_ancestry.take());
    mark_dirty_ancestry(applier, dirty, &mut ancestry);
    let mut walk = DirtyWalk {
        nodes: dirty,
        ancestry: &ancestry,
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
    crate::thread_state::with(|state| state.dirty_ancestry.set(ancestry));
    walked.map(|report| (report, path))
}

/// The way from the root to the layer of a node that the last scene
/// updates changed alone, as an animation changes one layer each frame,
/// and what the layer's parent passes it. The next update that changes that
/// node alone goes along it instead of walking down from the root: no
/// other update ran since, so the layers on the way are where they were.
#[derive(Clone)]
pub(crate) struct RetainedPath {
    node: NodeId,
    /// The index of each layer on the way in its parent's children and the
    /// identity it had there, from a child of the root to the node's layer.
    steps: Vec<(usize, Option<NodeId>)>,
    pass: ParentPass,
    /// The update that went along the path last.
    update: u64,
    /// Whether a layer on the way is a scene, a surface or a 3D transform,
    /// so a change at the end of the path changes what lies in 3D.
    in_space: bool,
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
fn update_along_retained_path(
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

fn mark_dirty_ancestry(
    applier: &MemoryApplier,
    dirty: &HashMap<NodeId, NodeUpdate>,
    ancestry: &mut NodeMarks,
) {
    ancestry.reset(applier);
    for &node in dirty.keys() {
        ancestry.add(applier, node, DIRTY_NODE);
        let mut current = node;
        while let Some(parent) = applier
            .get_ref(current)
            .ok()
            .and_then(cranpose_core::Node::parent)
        {
            if ancestry.add(applier, parent, DIRTY_ANCESTOR) & DIRTY_ANCESTOR != 0 {
                break;
            }
            current = parent;
        }
    }
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

#[expect(clippy::print_stderr, reason = "CRANPOSE_SCENE_UPDATE_DIAG asks")]
fn translate_bail(reason: &str) -> bool {
    if cranpose_core::DIAGNOSTICS.scene_update_diag.is_set() {
        eprintln!("[scene-update-diag] translate bail: {reason}");
    }
    false
}

#[derive(Clone, Copy)]
struct TranslateAncestorContext {
    inherited_motion_context_animated: bool,
    /// The scene update in progress.
    update: u64,
    inherited_translated_content_context: bool,
    parent_content_offset: Point,
    parent_abs: AbsOrigin,
}

// Whether a node lowers as the graph's root, the one node that may be a
// window root, which then draws at the origin.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Root,
    Child,
}

// Whether a retained layer's node kept its place in its parent.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Kept,
    Moved,
}

fn try_update_retained_layer(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    kind: NodeUpdate,
    level: Level,
    ancestors: TranslateAncestorContext,
) -> Option<bool> {
    match kind {
        NodeUpdate::Moved if level == Level::Child => {
            move_retained_layer(applier, layer, ancestors)
                .or_else(|| translate_tracking_hits(applier, layer, dirty_nodes, ancestors))
        }
        NodeUpdate::Draws if redraw_retained_layer(applier, layer, level, ancestors) => Some(false),
        NodeUpdate::Content | NodeUpdate::Draws | NodeUpdate::Moved => {
            translate_tracking_hits(applier, layer, dirty_nodes, ancestors)
        }
        NodeUpdate::Layer => {
            update_layer_properties(applier, layer, level, Position::Kept, ancestors)
        }
        NodeUpdate::MovedLayer => {
            update_layer_properties(applier, layer, level, Position::Moved, ancestors)
        }
    }
}

/// Records the own draws of the node of `container` again around the child
/// layers it keeps, for a node whose draws read state that changed while its
/// layout, modifiers and children stayed. False when the layer no longer
/// fits the node: the content update then takes it.
fn redraw_retained_layer(
    applier: &MemoryApplier,
    container: &mut LayerNode,
    level: Level,
    ancestors: TranslateAncestorContext,
) -> bool {
    // The wrapper of a node's outer draws is written with its layer.
    if container.wraps.is_some() {
        return false;
    }
    let Some(node_id) = container.node_id else {
        return false;
    };
    read_placed_node_data(applier, node_id, level, |data| {
        let placement = container.origin_in_parent;
        if !retained_layer_matches_layout(container, &data, placement, false, Position::Kept) {
            return false;
        }
        let Ok(plan) = translated_container(
            container,
            &data.layout_state,
            &data.modifier_slices,
            ancestors.inherited_motion_context_animated,
            false,
        ) else {
            return false;
        };
        container.node_bounds = plan.node_bounds;
        crate::layer_recycling::recycle_primitives(container);
        write_own_content_around_children(
            container,
            node_id,
            &data.modifier_slices,
            data.layout_state.size(),
        );
        container.refresh_child_facts();
        container.note_content_change(ancestors.update);
        true
    })
    .unwrap_or(false)
}

/// Moves the layer of a node that only moved in its parent, keeping what it
/// drew, and returns whether hit geometry changed. A subtree that publishes
/// window origins publishes them again from where it now lies. `None` when
/// the layer cannot keep its content: its size changed.
fn move_retained_layer(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    ancestors: TranslateAncestorContext,
) -> Option<bool> {
    let placed = placed_child_geometry(applier, layer_identity(layer)?)?;
    if Rect::from_size(placed.size) != layer.node_rect() {
        return None;
    }
    let previous = HitGraphState::of(layer);
    translate_retained_child(layer, placed.position, ancestors.parent_content_offset);
    if layer.has_origin_sinks {
        publish_retained_window_geometry(applier, layer, ancestors.parent_abs);
    }
    Some(previous.dirty_against(&HitGraphState::of(layer)))
}

fn content_layer_mut(container: &mut LayerNode) -> Option<&mut LayerNode> {
    let Some(node_id) = container.wraps else {
        return Some(container);
    };
    container.children.iter_mut().find_map(|child| match child {
        RenderNode::Layer(layer) if layer.node_id == Some(node_id) => Some(&mut **layer),
        _ => None,
    })
}

/// Whether `layer` still fits its node's layout, so new layer properties
/// can update it in place: at `placement`, unless the node moved.
fn retained_layer_matches_layout(
    layer: &LayerNode,
    data: &SnapshotNodeData<'_>,
    placement: Point,
    wrapped: bool,
    position: Position,
) -> bool {
    let state = &data.layout_state;
    let slices = &data.modifier_slices;
    (position == Position::Moved || state.position() == placement)
        && Rect::from_size(state.size()) == layer.node_rect()
        && slices.layer_bounds(state.size()) == layer.local_bounds
        && state.content_offset() == layer.content_offset
        && (slices.outer_draw_command_count() > 0) == wrapped
}

fn update_layer_properties(
    applier: &MemoryApplier,
    container: &mut LayerNode,
    level: Level,
    position: Position,
    ancestors: TranslateAncestorContext,
) -> Option<bool> {
    let node_id = layer_identity(container)?;
    let wrapped = container.wraps.is_some();
    // The wrapper of a node's outer draws holds its placement; a moved
    // wrapped node is drawn again.
    if position == Position::Moved && wrapped {
        return None;
    }
    let placement = container.origin_in_parent;
    let hit_graph_dirty = read_placed_node_data(applier, node_id, level, |data| {
        let target = content_layer_mut(container)?;
        let slices = &data.modifier_slices;
        let state = &data.layout_state;
        if !retained_layer_matches_layout(target, &data, placement, wrapped, position) {
            return None;
        }
        let context = LowerContext {
            inherited_motion_context_animated: ancestors.inherited_motion_context_animated,
            inherited_translated_content_context: ancestors.inherited_translated_content_context,
            parent_abs: Some(ancestors.parent_abs),
            parent_content_offset: ancestors.parent_content_offset,
        };
        let previous = HitGraphState::of(target);
        let (head, child_context) = prepare_node_layer(node_id, slices, state, context);
        assign_layer(target, head);
        if wrapped {
            target.transform_to_parent = layer_transform_to_parent(
                target.local_bounds,
                Point::default(),
                &target.graphics_layer,
            );
            target.origin_in_parent = Point::default();
        }
        if target.has_origin_sinks
            && let Some(child_abs) = child_context.parent_abs
        {
            for child in &target.children {
                if let RenderNode::Layer(child) = child {
                    publish_retained_window_geometry(applier, child, child_abs);
                }
            }
        }
        Some(previous.dirty_against(&HitGraphState::of(target)))
    })??;
    if wrapped {
        container.refresh_child_facts();
        container.has_origin_sinks = children_have_origin_sinks(&container.children);
        container.forget_own_raster_cache_hashes();
    }
    Some(hit_graph_dirty)
}

/// After a scene build or update that changed a layer in 3D, publishes
/// where the window's scenes show the layers on their surfaces that publish
/// window origins, as the eye sees them, and leaves each scene's eye the
/// world it walks in. A flat window asks nothing of its layers.
pub(crate) fn publish_space(applier: &MemoryApplier, graph: &RenderGraph) {
    if !crate::thread_state::with(|state| state.space_changed.replace(false)) {
        return;
    }
    publish_space_now(applier, graph);
}

#[cold]
#[inline(never)]
fn publish_space_now(applier: &MemoryApplier, graph: &RenderGraph) {
    let root = ProjectiveTransform::identity();
    crate::space::for_each_window_eye(&graph.root, root, &mut |tree, view, to_window| {
        if let Some(eye) = &tree.cells[0].scene.eye {
            eye.world.rebuild(|world| view.build_world(tree, world));
        }
        for seen in view
            .surfaces
            .iter()
            .filter(|seen| seen.layer.has_origin_sinks)
        {
            if let Some(content) = view.content_to_window(seen, to_window) {
                publish_content_geometry(applier, seen.layer, content);
            }
        }
    });
}

/// Publishes the window geometry of `layer`, whose local space `to_window`
/// maps to the window, and of the layers on it that publish window origins,
/// as far as the next layer in 3D.
fn publish_content_geometry(
    applier: &MemoryApplier,
    layer: &LayerNode,
    to_window: ProjectiveTransform,
) {
    if let Some(node_id) = layer.node_id {
        read_node_data(applier, node_id, |data| {
            data.modifier_slices.publish_window_geometry(
                Point::default(),
                to_window,
                data.layout_state.size(),
            );
        });
    }
    for child in &layer.children {
        if let RenderNode::Layer(child) = child
            && child.has_origin_sinks
            && child.graphics_layer.space.is_none()
        {
            publish_content_geometry(applier, child, child.transform_to_parent.then(to_window));
        }
    }
}

fn publish_retained_window_geometry(
    applier: &MemoryApplier,
    layer: &LayerNode,
    parent_abs: AbsOrigin,
) {
    if !layer.has_origin_sinks {
        return;
    }
    let child_abs = parent_abs.children_of(layer);
    if let Some(node_id) = layer.node_id {
        read_node_data(applier, node_id, |data| {
            data.modifier_slices.publish_window_geometry(
                Point {
                    x: parent_abs.content_origin.x + layer.origin_in_parent.x,
                    y: parent_abs.content_origin.y + layer.origin_in_parent.y,
                },
                child_abs.window_transform,
                data.layout_state.size(),
            );
        });
    }
    for child in &layer.children {
        if let RenderNode::Layer(child) = child {
            publish_retained_window_geometry(applier, child, child_abs);
        }
    }
}

/// [`try_translate_scrolled_layer`], returning whether hit geometry may
/// have changed: a layer that neither held nor holds a hit target and lies
/// in no 3D space leaves the hit graph as it was, as
/// [`rebuild_tracking_hits`] would find. `None` when the layer cannot be
/// translated.
fn translate_tracking_hits(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    ancestors: TranslateAncestorContext,
) -> Option<bool> {
    let held = holds_hit_geometry(layer);
    try_translate_scrolled_layer(applier, layer, dirty_nodes, ancestors)
        .then(|| held || holds_hit_geometry(layer))
}

/// Whether the hit graph reads `layer`: it is a hit target, holds one or is
/// a 3D space, the states [`HitGraphState::dirty_against`] compares.
fn holds_hit_geometry(layer: &LayerNode) -> bool {
    layer.hit_test.is_some() || layer.has_hit_targets || layer.graphics_layer.space.is_some()
}

fn try_translate_scrolled_layer(
    applier: &MemoryApplier,
    container: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    ancestors: TranslateAncestorContext,
) -> bool {
    let Some(node_id) = layer_identity(container) else {
        return translate_bail("no node id");
    };
    read_node_data(applier, node_id, |data| {
        if container.wraps.is_none() {
            return translate_layer_from_data(
                applier,
                container,
                dirty_nodes,
                ancestors,
                data,
                false,
            );
        }
        let outer_count = data.modifier_slices.outer_draw_command_count();
        if outer_count == 0 {
            return translate_bail("outer draws removed");
        }
        let size = data.layout_state.size();
        let placement = data.layout_state.position();
        let slices = Rc::clone(&data.modifier_slices);
        let Some(inner) = content_layer_mut(container) else {
            return translate_bail("wrapped layer missing");
        };
        if !translate_layer_from_data(applier, inner, dirty_nodes, ancestors, data, true) {
            return false;
        }
        let draws = DrawTarget {
            size,
            motion_context_animated: inner.motion_context_animated,
            layer: None,
        };
        let outer = outer_draws(node_id, slices.draw_commands(), outer_count, draws)
            .expect("outer command count is nonzero");
        let layer =
            take_wrapped_layer(container, node_id).expect("the wrapped layer was found above");
        write_wrapper(
            container,
            layer,
            placement,
            outer,
            ancestors.parent_content_offset,
        );
        container.note_content_change(ancestors.update);
        true
    })
    .unwrap_or_else(|| translate_bail("container snapshot read failed"))
}

/// The node of a container that keeps its children, and its node bounds at
/// its current size.
struct TranslatedContainer {
    node_id: NodeId,
    node_bounds: Option<Rect>,
}

/// Checks that `container` can keep its children and brings its bounds and
/// graphics layer to its current size: a container that kept its size keeps
/// its layer as it is.
fn translated_container(
    container: &mut LayerNode,
    layout_state: &cranpose_ui::widgets::LayoutState,
    modifier_slices: &ModifierNodeSlices,
    inherited_motion_context_animated: bool,
    wrapped: bool,
) -> Result<TranslatedContainer, &'static str> {
    if cranpose_core::DIAGNOSTICS.disable_scroll_translate.is_set() {
        return Err("fast path disabled by ablation switch");
    }
    let Some(node_id) = container.node_id else {
        return Err("no node id");
    };
    if !layout_state.is_placed() {
        return Err("container unplaced");
    }
    let outer_count = modifier_slices.outer_draw_command_count();
    if (outer_count > 0 && !wrapped)
        || (inherited_motion_context_animated || modifier_slices.motion_context_animated())
            != container.motion_context_animated
        || modifier_slices.translated_content_context() != container.translated_content_context
    {
        return Err("container outer draws or translated context changed");
    }
    let clip_to_bounds = modifier_slices.clip_to_bounds();
    if clip_to_bounds != container.clip_to_bounds {
        return Err("container clip changed");
    }
    // A resized container keeps its children: only its bounds and the clip
    // shaped to them follow its size. Any other change to its graphics layer
    // may change what its children inherit.
    if !container
        .graphics_layer
        .reads_as(shaped_graphics_layer(modifier_slices, container.local_bounds).as_deref())
    {
        return Err("container graphics layer changed");
    }
    let (local_bounds, node_bounds) = layer_and_node_bounds(modifier_slices, layout_state.size());
    if local_bounds != container.local_bounds {
        container.local_bounds = local_bounds;
        container
            .graphics_layer
            .replace(shaped_graphics_layer(modifier_slices, local_bounds).map(Cow::into_owned));
    }
    Ok(TranslatedContainer {
        node_id,
        node_bounds,
    })
}

/// What a scroll step reconciles a container's children in, kept between
/// steps so a step allocates nothing of its own. Every list is empty
/// between steps, so it holds no layer.
#[derive(Default)]
pub(crate) struct TranslateScratch {
    /// The placed children, in their new order.
    placed_fresh: Vec<PlacedChild>,
    /// The ids of the container's previous children, in their order.
    old_ids: Vec<NodeId>,
    old_index_by_id: HashMap<NodeId, usize>,
    /// The container's previous children by their index, until each is
    /// kept or recycled.
    retained: Vec<Option<Box<LayerNode>>>,
    /// The previous children that stay, in their new order; `None` where a
    /// child enters.
    kept: Vec<Option<Box<LayerNode>>>,
    /// The children built for the step, in their new order.
    entering: Vec<(NodeId, Box<LayerNode>)>,
}

/// A placed child of a container a scroll step translates.
struct PlacedChild {
    id: NodeId,
    geometry: PlacedGeometry,
    /// The child's index among the container's previous children; `None`
    /// for a child that enters.
    old_index: Option<usize>,
    /// Whether the child keeps its layer and follows its placement: it was
    /// not dirty, it only moved, which this step settles, or its layer
    /// properties changed, which the walk beneath the container settles.
    keeps_layer: bool,
}

/// How a translated container's placed children stand to its previous ones.
enum ChildOrder {
    /// The kept children lie together and in their order, at `kept` among
    /// the previous children, and the entering ones come before or after
    /// them: `entering_before` of the placed children, then the kept ones,
    /// then the rest. A scroll leaves its children so, and so do unchanged
    /// children.
    Shifted {
        kept: Range<usize>,
        entering_before: usize,
    },
    /// The kept children changed their order, or children enter between
    /// them.
    Mixed,
}

/// Fills `scratch` with `container`'s placed children, each with its index
/// among the previous ones, and returns how they stand to those.
fn translated_children(
    applier: &MemoryApplier,
    container: &LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    fresh_children: &[NodeId],
    scratch: &mut TranslateScratch,
) -> Result<ChildOrder, &'static str> {
    let placed_fresh = &mut scratch.placed_fresh;
    placed_fresh.clear();
    for &id in fresh_children {
        let Some(geometry) = placed_child_geometry(applier, id) else {
            continue;
        };
        placed_fresh.push(PlacedChild {
            id,
            geometry,
            old_index: None,
            keeps_layer: false,
        });
    }
    let children_unchanged = container.children.len() == placed_fresh.len()
        && container
            .children
            .iter()
            .zip(placed_fresh.iter())
            .all(|(child, placed)| {
                matches!(child, RenderNode::Layer(layer) if layer_identity(layer) == Some(placed.id))
            });
    let order = if children_unchanged {
        for (index, placed) in placed_fresh.iter_mut().enumerate() {
            placed.old_index = Some(index);
        }
        ChildOrder::Shifted {
            kept: 0..placed_fresh.len(),
            entering_before: 0,
        }
    } else {
        let old_ids = &mut scratch.old_ids;
        old_ids.clear();
        let (mut lowest, mut highest) = (NodeId(usize::MAX), NodeId(0));
        for child in &container.children {
            let RenderNode::Layer(layer) = child else {
                return Err("child without node id");
            };
            let Some(id) = layer_identity(layer) else {
                return Err("child without node id");
            };
            lowest = lowest.min(id);
            highest = highest.max(id);
            old_ids.push(id);
        }
        match_old_children(
            old_ids,
            lowest..=highest,
            placed_fresh,
            &mut scratch.old_index_by_id,
        )
    };
    check_retained_children(container, dirty_nodes, placed_fresh).map(|()| order)
}

/// The searches of the previous children a match makes before it maps them:
/// a search reads each id once, a map hashes each of them in.
const MATCH_SEARCHES: usize = 16;

/// Notes each placed child's index among the previous children `old_ids`,
/// whose ids lie in `old_id_range`.
/// Children that kept their order match in turn, as a scroll that brings
/// rows in at one end and lets them go at the other leaves them; a child
/// past a gap is searched for, and the match continues after it. A child
/// whose id lies outside the range enters without a search: a node made
/// since the previous children takes an id past all of theirs. Once the
/// order is mixed past [`MATCH_SEARCHES`] searches, `old_index_by_id` maps
/// the previous children for the rest. An id is a node's, so it is once
/// among the previous children. Returns how the placed children stand to
/// the previous ones.
fn match_old_children(
    old_ids: &[NodeId],
    old_id_range: RangeInclusive<NodeId>,
    placed_fresh: &mut [PlacedChild],
    old_index_by_id: &mut HashMap<NodeId, usize>,
) -> ChildOrder {
    let mut cursor = 0;
    let mut searches = 0;
    let mut mapped = false;
    let mut kept: Option<Range<usize>> = None;
    let mut entering_before = 0;
    let mut entering_after = 0;
    let mut shifted = true;
    for placed in placed_fresh {
        let old_index = if old_ids.get(cursor) == Some(&placed.id) {
            Some(cursor)
        } else if !old_id_range.contains(&placed.id) {
            None
        } else if searches < MATCH_SEARCHES {
            searches += 1;
            old_ids.iter().position(|&id| id == placed.id)
        } else {
            if !mapped {
                old_index_by_id.clear();
                old_index_by_id.extend(old_ids.iter().enumerate().map(|(index, &id)| (id, index)));
                mapped = true;
            }
            old_index_by_id.get(&placed.id).copied()
        };
        match (old_index, kept.as_mut()) {
            (Some(index), Some(kept)) => {
                shifted &= entering_after == 0 && index == kept.end;
                kept.end = index + 1;
            }
            (Some(index), None) => kept = Some(index..index + 1),
            (None, Some(_)) => entering_after += 1,
            (None, None) => entering_before += 1,
        }
        if let Some(index) = old_index {
            cursor = index + 1;
        }
        placed.old_index = old_index;
    }
    if shifted {
        ChildOrder::Shifted {
            kept: kept.unwrap_or(0..0),
            entering_before,
        }
    } else {
        ChildOrder::Mixed
    }
}

/// Checks that every child kept as it is still fits its layer, and notes
/// which children keep their layers. A moved child that no longer fits is
/// drawn again: it becomes a content update. A moved child that fits leaves
/// the dirty set, which this step settles.
fn check_retained_children(
    container: &LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    placed_fresh: &mut [PlacedChild],
) -> Result<(), &'static str> {
    for placed in placed_fresh {
        let Some(old_index) = placed.old_index else {
            continue;
        };
        let RenderNode::Layer(layer) = &container.children[old_index] else {
            return Err("retained child slot is not a layer");
        };
        let fits = Rect::from_size(placed.geometry.size) == layer.node_rect();
        placed.keeps_layer = match dirty_nodes.get_mut(&placed.id) {
            None if !fits => return Err("child resized"),
            None => true,
            Some(kind @ (NodeUpdate::Moved | NodeUpdate::MovedLayer)) if !fits => {
                *kind = NodeUpdate::Content;
                false
            }
            Some(NodeUpdate::Moved) => {
                dirty_nodes.remove(&placed.id);
                true
            }
            // The child moves with the others; the walk beneath the
            // container then gives it its new layer properties or draws
            // where it now lies.
            Some(NodeUpdate::Layer | NodeUpdate::Draws) => fits,
            Some(NodeUpdate::Content | NodeUpdate::MovedLayer) => false,
        };
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct TranslateGeometry {
    content_offset: Point,
    window_transform: ProjectiveTransform,
    top_left: Point,
    child_origin: Point,
}

impl TranslateGeometry {
    fn new(
        layout_state: &cranpose_ui::widgets::LayoutState,
        graphics_layer: &GraphicsLayer,
        local_bounds: Rect,
        parent_abs: AbsOrigin,
    ) -> Self {
        let content_offset = layout_state.content_offset();
        let top_left = Point {
            x: parent_abs.content_origin.x + layout_state.position().x,
            y: parent_abs.content_origin.y + layout_state.position().y,
        };
        let window_transform = layer_transform_to_window(
            parent_abs.window_transform,
            top_left,
            local_bounds,
            graphics_layer,
        );
        Self {
            content_offset,
            window_transform,
            top_left,
            child_origin: Point {
                x: top_left.x + content_offset.x,
                y: top_left.y + content_offset.y,
            },
        }
    }

    fn child_abs(self) -> AbsOrigin {
        AbsOrigin {
            content_origin: self.child_origin,
            window_transform: self.window_transform,
        }
    }
}

/// Moves `container`'s previous children that stay into `scratch.kept`, in
/// their new order, and the subtrees of those that leave into the layer
/// pool, so the entering children are built in their allocations.
/// Recycles the children that leave `container`. Their nodes leave the
/// dirty set with them: nothing remains to update for a node the scene no
/// longer shows, and one left dirty would go unmatched and rebuild the whole
/// scene.
fn recycle_leaving_children(
    container: &mut LayerNode,
    scratch: &mut TranslateScratch,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) {
    let TranslateScratch {
        placed_fresh,
        retained,
        kept,
        ..
    } = scratch;
    retained.clear();
    retained.extend(container.children.drain(..).map(|child| match child {
        RenderNode::Layer(layer) => Some(layer),
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
    }));
    kept.clear();
    kept.extend(
        placed_fresh
            .iter()
            .map(|placed| placed.old_index.and_then(|index| retained[index].take())),
    );
    for leaving in retained.drain(..).flatten() {
        forget_dirty_subtree(&leaving, dirty_nodes);
        crate::layer_recycling::recycle(leaving);
    }
}

/// Recycles the children that leave `container` around the `kept` ones,
/// which stay where they lie, already in their new order. Their nodes
/// leave the dirty set with them, as in [`recycle_leaving_children`].
fn recycle_children_around(
    container: &mut LayerNode,
    kept: Range<usize>,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) {
    let mut recycle = |leaving| {
        if let RenderNode::Layer(leaving) = leaving {
            forget_dirty_subtree(&leaving, dirty_nodes);
            crate::layer_recycling::recycle(leaving);
        }
    };
    // Most steps keep every child.
    if kept.end < container.children.len() {
        container.children.drain(kept.end..).for_each(&mut recycle);
    }
    if kept.start > 0 {
        container.children.drain(..kept.start).for_each(recycle);
    }
}

/// Builds the children that enter `container`. They are built from the
/// applier's current state, so their nodes leave the dirty set: the walk
/// beneath would otherwise build each of them a second time.
fn build_entering_children(
    applier: &MemoryApplier,
    container: &LayerNode,
    scratch: &mut TranslateScratch,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    geometry: TranslateGeometry,
    inherited: (bool, u64),
) {
    let (child_inherited_translated_content_context, update) = inherited;
    let entering = &mut scratch.entering;
    entering.clear();
    for placed in &scratch.placed_fresh {
        if placed.old_index.is_some() {
            continue;
        }
        let child_id = &placed.id;
        let context = LowerContext {
            inherited_motion_context_animated: container.motion_context_animated,
            inherited_translated_content_context: child_inherited_translated_content_context,
            parent_abs: Some(geometry.child_abs()),
            parent_content_offset: geometry.content_offset,
        };
        let Some(mut lowered) = lower_child(applier, *child_id, context) else {
            continue;
        };
        lowered.note_content_change(update);
        forget_dirty_subtree(&lowered, dirty_nodes);
        entering.push((*child_id, lowered));
    }
}

/// Records `container`'s own draws and text around the child layers its list
/// holds, in the order [`write_node_content`] writes a layer.
fn write_own_content_around_children(
    container: &mut LayerNode,
    node_id: NodeId,
    modifier_slices: &ModifierNodeSlices,
    size: Size,
) {
    if modifier_slices.draw_commands()[modifier_slices.outer_draw_command_count()..].is_empty()
        && !modifier_slices.has_text()
    {
        return;
    }
    let kept = container.children.len();
    let draws = DrawTarget {
        size,
        motion_context_animated: container.motion_context_animated,
        layer: container.graphics_layer.get(),
    };
    write_node_content(
        &mut container.children,
        node_id,
        modifier_slices,
        draws,
        (kept, 0),
        |_| {},
    );
}

fn apply_translated_container_state(
    container: &mut LayerNode,
    modifier_slices: &ModifierNodeSlices,
    layout_state: &cranpose_ui::widgets::LayoutState,
    parent_content_offset: Point,
    geometry: TranslateGeometry,
) {
    container.transform_to_parent = placed_transform(
        container.local_bounds,
        layout_state.position(),
        container.graphics_layer.get(),
        parent_content_offset,
    );
    container.content_offset = geometry.content_offset;
    if container.translated_content_context {
        container.translated_content_offset = modifier_slices
            .translated_content_offset()
            .unwrap_or(geometry.content_offset);
    }
    modifier_slices.publish_window_geometry(
        geometry.top_left,
        geometry.window_transform,
        layout_state.size(),
    );
    container.origin_in_parent = layout_state.position();
}

fn reconcile_translated_children(
    applier: &MemoryApplier,
    container: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    scratch: &mut TranslateScratch,
    order: ChildOrder,
    geometry: TranslateGeometry,
) {
    if let ChildOrder::Shifted {
        entering_before, ..
    } = order
    {
        reconcile_shifted_children(
            applier,
            container,
            dirty_nodes,
            scratch,
            entering_before,
            geometry,
        );
        return;
    }
    let mut entering = scratch.entering.drain(..).peekable();
    for (placed, kept) in scratch.placed_fresh.iter().zip(scratch.kept.drain(..)) {
        if let Some(mut layer) = kept {
            if placed.keeps_layer {
                move_kept_child(applier, &mut layer, placed.geometry.position, geometry);
            }
            container.children.push(RenderNode::Layer(layer));
        } else if let Some((_, lowered)) = entering.next_if(|(id, _)| *id == placed.id) {
            dirty_nodes.remove(&placed.id);
            remove_dirty_descendants(&lowered, dirty_nodes);

            container.children.push(RenderNode::Layer(lowered));
        }
    }
}

/// Moves the kept children, which lie in `container` in their new order, to
/// their placements, and puts the entering children before and after them:
/// those of the first `entering_before` placed children before.
fn reconcile_shifted_children(
    applier: &MemoryApplier,
    container: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    scratch: &mut TranslateScratch,
    entering_before: usize,
    geometry: TranslateGeometry,
) {
    let TranslateScratch {
        placed_fresh,
        entering,
        ..
    } = scratch;
    for (child, placed) in container
        .children
        .iter_mut()
        .zip(&placed_fresh[entering_before..])
    {
        let RenderNode::Layer(layer) = child else {
            unreachable!("retained child identities were checked");
        };
        if placed.keeps_layer {
            move_kept_child(applier, layer, placed.geometry.position, geometry);
        }
    }
    if entering.is_empty() {
        return;
    }
    // The children built for the step lie in their placed order, and those
    // placed before the kept ones come first.
    let mut built = entering.iter().peekable();
    let built_before = placed_fresh[..entering_before]
        .iter()
        .filter(|placed| built.next_if(|(id, _)| *id == placed.id).is_some())
        .count();
    for (id, lowered) in entering.iter() {
        dirty_nodes.remove(id);
        remove_dirty_descendants(lowered, dirty_nodes);
    }
    let layer = |(_, lowered): (NodeId, Box<LayerNode>)| RenderNode::Layer(lowered);
    if built_before > 0 {
        container
            .children
            .splice(0..0, entering.drain(..built_before).map(layer));
    }
    container.children.extend(entering.drain(..).map(layer));
}

/// Runs `f` on the thread's translate scratch where it lies, or on a new one
/// while another translate holds it.
fn with_translate_scratch<R>(f: impl FnOnce(&mut TranslateScratch) -> R) -> R {
    crate::thread_state::with(|state| match state.translate_scratch.try_borrow_mut() {
        Ok(mut scratch) => f(&mut scratch),
        Err(_) => f(&mut TranslateScratch::default()),
    })
}

fn translate_layer_from_data(
    applier: &MemoryApplier,
    container: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    ancestors: TranslateAncestorContext,
    data: SnapshotNodeData<'_>,
    wrapped: bool,
) -> bool {
    let TranslateAncestorContext {
        inherited_motion_context_animated,
        update,
        inherited_translated_content_context,
        parent_content_offset,
        parent_abs,
    } = ancestors;
    let SnapshotNodeData {
        layout_state,
        modifier_slices,
        children: fresh_children,
        window_root,
    } = data;
    let layout_state = if window_root {
        layout_state.at_origin()
    } else {
        layout_state
    };
    let container_plan = match translated_container(
        container,
        &layout_state,
        &modifier_slices,
        inherited_motion_context_animated,
        wrapped,
    ) {
        Ok(plan) => plan,
        Err(reason) => return translate_bail(reason),
    };
    let TranslatedContainer {
        node_id,
        node_bounds,
    } = container_plan;
    container.node_bounds = node_bounds;
    // The container's own draws and text are recorded again around the
    // children it keeps: an unchanged draw reuses its recording, while
    // building the container anew lowered every child layer beneath it too.
    crate::layer_recycling::recycle_primitives(container);
    let translated = with_translate_scratch(|scratch| {
        let order = translated_children(applier, container, dirty_nodes, fresh_children, scratch)?;
        let geometry = TranslateGeometry::new(
            &layout_state,
            &container.graphics_layer,
            container.local_bounds,
            parent_abs,
        );
        let child_inherited_translated_content_context =
            inherited_translated_content_context || container.translated_content_context;
        match &order {
            ChildOrder::Shifted { kept, .. } => {
                recycle_children_around(container, kept.clone(), dirty_nodes);
            }
            ChildOrder::Mixed => recycle_leaving_children(container, scratch, dirty_nodes),
        }
        build_entering_children(
            applier,
            container,
            scratch,
            dirty_nodes,
            geometry,
            (child_inherited_translated_content_context, update),
        );

        apply_translated_container_state(
            container,
            &modifier_slices,
            &layout_state,
            parent_content_offset,
            geometry,
        );

        reconcile_translated_children(applier, container, dirty_nodes, scratch, order, geometry);
        Ok(())
    });
    if let Err(reason) = translated {
        return translate_bail(reason);
    }
    write_own_content_around_children(container, node_id, &modifier_slices, layout_state.size());
    container.hit_test = hit_test_from_slices(&modifier_slices);

    container.has_origin_sinks = modifier_slices_have_origin_sinks(&modifier_slices)
        || children_have_origin_sinks(&container.children);
    container.refresh_child_facts();
    container.note_content_change(update);

    true
}

fn translate_retained_child(layer: &mut LayerNode, position: Point, content_offset: Point) {
    layer.transform_to_parent = placed_transform(
        layer.local_bounds,
        position,
        layer.graphics_layer.get(),
        content_offset,
    );
    layer.origin_in_parent = position;
}

/// Moves a child a translated container keeps to its placement in the
/// container laid out with `geometry`. A subtree that publishes window
/// origins publishes them again from where it now lies.
fn move_kept_child(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    position: Point,
    geometry: TranslateGeometry,
) {
    translate_retained_child(layer, position, geometry.content_offset);
    if layer.has_origin_sinks {
        publish_retained_window_geometry(applier, layer, geometry.child_abs());
    }
}

/// Rebuilds `layer` with `rebuild`, which reports whether it succeeded, and
/// returns whether the hit geometry of the layer or of a layer under it
/// changed; `None` when the rebuild failed. A rebuild takes in the moves and
/// layer changes of the nodes under the layer, which its own state does not
/// show: a sheet that slides inside a container that redraws keeps the
/// container's state.
fn rebuild_tracking_hits(
    layer: &mut LayerNode,
    rebuild: impl FnOnce(&mut LayerNode) -> bool,
) -> Option<bool> {
    let previous = HitGraphState::of(layer);
    let mut descendants = crate::thread_state::with(|state| state.hit_geometry_scratch.take());
    descendants.clear();
    visit_hit_layers(layer, &mut |child| {
        descendants.push(HitGraphState::of(child));
        true
    });
    let dirty = rebuild(layer).then(|| {
        let mut expected = descendants.iter();
        previous.dirty_against(&HitGraphState::of(layer))
            || !visit_hit_layers(layer, &mut |child| {
                expected.next() == Some(&HitGraphState::of(child))
            })
            || expected.next().is_some()
    });
    crate::thread_state::with(|state| state.hit_geometry_scratch.set(descendants));
    dirty
}

/// Visits the layers under `layer` that are hit targets or hold one, in draw
/// order, while `visit` returns true; returns whether every visit did.
fn visit_hit_layers(layer: &LayerNode, visit: &mut impl FnMut(&LayerNode) -> bool) -> bool {
    for child in &layer.children {
        let RenderNode::Layer(child) = child else {
            continue;
        };
        if child.hit_test.is_none() && !child.has_hit_targets {
            continue;
        }
        if !visit(child) || !visit_hit_layers(child, visit) {
            return false;
        }
    }
    true
}

#[derive(PartialEq)]
pub(crate) struct HitGraphState {
    hit_test: bool,
    has_hit_targets: bool,
    local_bounds: Rect,
    node_bounds: Option<Rect>,
    transform_to_parent: ProjectiveTransform,
    clip_rect: Option<Rect>,
    shape: LayerShape,
    /// Whether the layer is a scene, a mesh surface or a 3D transform. Each
    /// update of such a layer runs its closure again, which may move where
    /// the surfaces of its scene take input or hide one another.
    space: bool,
}

impl HitGraphState {
    fn of(layer: &LayerNode) -> Self {
        Self {
            hit_test: layer.hit_test.is_some(),
            has_hit_targets: layer.has_hit_targets,
            local_bounds: layer.local_bounds,
            node_bounds: layer.node_bounds,
            transform_to_parent: layer.transform_to_parent,
            clip_rect: layer.clip_rect(),
            shape: layer.graphics_layer.shape,
            space: layer.graphics_layer.space.is_some(),
        }
    }

    fn dirty_against(&self, replacement: &Self) -> bool {
        self.hit_test
            || replacement.hit_test
            || self.space
            || replacement.space
            || ((self.has_hit_targets || replacement.has_hit_targets) && self != replacement)
    }
}

/// Notes that the scene update changes a layer that is a scene, a surface or
/// a 3D transform, or lies beneath one: the window's scenes publish where
/// their layers show and their eyes' worlds again.
#[cold]
#[inline(never)]
fn note_space_change() {
    crate::thread_state::with(|state| state.space_changed.set(true));
}

fn release_for_rebuild(layer: &mut LayerNode) {
    layer.hit_test = None;
    crate::layer_recycling::recycle_children(layer);
}

/// Takes `layer`'s node and every node beneath it out of `dirty_nodes`.
fn forget_dirty_subtree(layer: &LayerNode, dirty_nodes: &mut HashMap<NodeId, NodeUpdate>) {
    if let Some(node_id) = layer_identity(layer) {
        dirty_nodes.remove(&node_id);
    }
    remove_dirty_descendants(layer, dirty_nodes);
}

fn remove_dirty_descendants(layer: &LayerNode, dirty_nodes: &mut HashMap<NodeId, NodeUpdate>) {
    for child in &layer.children {
        let RenderNode::Layer(child_layer) = child else {
            continue;
        };
        if let Some(node_id) = child_layer.node_id {
            dirty_nodes.remove(&node_id);
        }
        remove_dirty_descendants(child_layer, dirty_nodes);
    }
}

#[derive(Clone, Copy)]
struct LowerContext {
    inherited_motion_context_animated: bool,
    inherited_translated_content_context: bool,
    parent_abs: Option<AbsOrigin>,
    parent_content_offset: Point,
}

impl LowerContext {
    const ROOT: Self = Self {
        inherited_motion_context_animated: false,
        inherited_translated_content_context: false,
        parent_abs: Some(AbsOrigin::ROOT),
        parent_content_offset: Point { x: 0.0, y: 0.0 },
    };

    fn for_children(
        self,
        slices: &ModifierNodeSlices,
        content_offset: Point,
        parent_abs: Option<AbsOrigin>,
    ) -> Self {
        Self {
            inherited_motion_context_animated: self.inherited_motion_context_animated
                || slices.motion_context_animated(),
            inherited_translated_content_context: self.inherited_translated_content_context
                || slices.translated_content_context(),
            parent_abs,
            parent_content_offset: content_offset,
        }
    }
}

struct LayerHead {
    node_id: Option<NodeId>,
    wraps: Option<NodeId>,
    local_bounds: Rect,
    node_bounds: Option<Rect>,
    transform_to_parent: ProjectiveTransform,
    content_offset: Point,
    motion_context_animated: bool,
    translated_content_context: bool,
    translated_content_offset: Point,
    origin_in_parent: Point,
    graphics_layer: Option<GraphicsLayer>,
    clip_to_bounds: bool,
    hit_test: Option<HitTestNode>,
    has_origin_sinks: bool,
    isolation: IsolationReasons,
    cache_policy: CachePolicy,
    draw_phase: Option<PrimitivePhase>,
}

struct NodeFrame {
    local_bounds: Rect,
    node_bounds: Option<Rect>,
    placement: Point,
    content_offset: Point,
    translated_content_offset: Point,
}

fn node_layer_head(
    node_id: NodeId,
    slices: &Rc<ModifierNodeSlices>,
    frame: NodeFrame,
    graphics_layer: Option<GraphicsLayer>,
    context: LowerContext,
) -> LayerHead {
    let clip_to_bounds = slices.clip_to_bounds();
    let translated_content_context = slices.translated_content_context();
    let properties = graphics_layer.as_ref().unwrap_or(&GraphicsLayer::DEFAULT);
    let isolation = isolation_reasons(properties);
    LayerHead {
        node_id: Some(node_id),
        wraps: None,
        local_bounds: frame.local_bounds,
        node_bounds: frame.node_bounds,
        transform_to_parent: placed_transform(
            frame.local_bounds,
            frame.placement,
            graphics_layer.as_ref(),
            context.parent_content_offset,
        ),
        content_offset: frame.content_offset,
        motion_context_animated: context.inherited_motion_context_animated
            || slices.motion_context_animated(),
        translated_content_context,
        translated_content_offset: if translated_content_context {
            frame.translated_content_offset
        } else {
            Point::default()
        },
        origin_in_parent: frame.placement,
        cache_policy: layer_cache_policy(properties, isolation),
        isolation,
        graphics_layer,
        clip_to_bounds,
        hit_test: hit_test_from_slices(slices),
        has_origin_sinks: modifier_slices_have_origin_sinks(slices),
        draw_phase: None,
    }
}

fn assign_layer(layer: &mut LayerNode, head: LayerHead) {
    let LayerNode {
        node_id,
        wraps,
        local_bounds,
        node_bounds,
        transform_to_parent,
        content_offset,
        motion_context_animated,
        translated_content_context,
        translated_content_offset,
        origin_in_parent,
        graphics_layer,
        clip_to_bounds,
        hit_test,
        has_hit_targets,
        space,
        has_origin_sinks,
        contained,
        isolation,
        draw_phase,
        cache_policy,
        cache_hashes: _,
        content_changes,
        change: _,
        own_change: _,
        children,
    } = layer;
    if *node_id != head.node_id {
        *content_changes = ContentChanges::default();
    }
    *node_id = head.node_id;
    *wraps = head.wraps;
    *local_bounds = head.local_bounds;
    *node_bounds = head.node_bounds;
    *transform_to_parent = head.transform_to_parent;
    *content_offset = head.content_offset;
    *motion_context_animated = head.motion_context_animated;
    *translated_content_context = head.translated_content_context;
    *translated_content_offset = head.translated_content_offset;
    *origin_in_parent = head.origin_in_parent;
    graphics_layer.replace(head.graphics_layer);
    if graphics_layer.space.is_some() {
        note_space_change();
    }
    *clip_to_bounds = head.clip_to_bounds;
    *hit_test = head.hit_test;
    *has_hit_targets = false;
    space.set(None);
    *has_origin_sinks = head.has_origin_sinks || children_have_origin_sinks(children);
    contained.set(None);
    *isolation = head.isolation;
    *draw_phase = head.draw_phase;
    *cache_policy = head.cache_policy;
    layer.forget_own_raster_cache_hashes();
    layer.refresh_child_facts();
}

fn children_have_origin_sinks(children: &[RenderNode]) -> bool {
    children.iter().any(|child| match child {
        RenderNode::Layer(child_layer) => child_layer.has_origin_sinks,
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
    })
}

/// The transform that places a layer with `graphics_layer`, `None` for the
/// default properties, at `placement` in a parent whose content is offset by
/// `parent_content_offset`.
fn placed_transform(
    local_bounds: Rect,
    placement: Point,
    graphics_layer: Option<&GraphicsLayer>,
    parent_content_offset: Point,
) -> ProjectiveTransform {
    // Most layers only move: one translation places them.
    let graphics_layer = match graphics_layer {
        Some(layer) if layer_scales_or_rotates(layer) => layer,
        moved => {
            let offset = moved.map_or_else(Point::default, |layer| Point {
                x: layer.translation_x,
                y: layer.translation_y,
            });
            return ProjectiveTransform::translation(
                placement.x + offset.x + parent_content_offset.x,
                placement.y + offset.y + parent_content_offset.y,
            );
        }
    };
    let transform = layer_transform_to_parent(local_bounds, placement, graphics_layer);
    if parent_content_offset == Point::default() {
        transform
    } else {
        transform.then(ProjectiveTransform::translation(
            parent_content_offset.x,
            parent_content_offset.y,
        ))
    }
}

/// Writes a node's own draws and text around its child layers: `kept`
/// layers already end `list`, and `write_children` appends `written` more.
fn write_node_content(
    list: &mut Vec<RenderNode>,
    node_id: NodeId,
    slices: &ModifierNodeSlices,
    draws: DrawTarget,
    (kept, written): (usize, usize),
    write_children: impl FnOnce(&mut Vec<RenderNode>),
) {
    let size = draws.size;
    let outer_count = slices.outer_draw_command_count();
    let commands = &slices.draw_commands()[outer_count..];
    crate::layer_recycling::reserve_nodes(
        list,
        layer_node_capacity(commands, written, slices.has_text()),
    );
    let first_kept = list.len() - kept;
    let first_draw = list.len();
    append_draw_nodes(
        list,
        node_id,
        commands,
        outer_count,
        DrawPass::Record(DrawPlacement::Behind),
        draws,
        PrimitivePhase::BeforeChildren,
    );
    if let Some(text) = text_node_from_parts(TextNodeParts {
        node_id,
        text_rect: slices.text_content_rect(size),
        text_layout_options: slices.text_layout_options(),
        text_pan: slices.text_pan_resolver(),
        measured_layout: slices.measured_text_layout(),
    }) {
        list.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(crate::layer_recycling::boxed_text(text)),
        }));
    }
    // The kept layers go after the draws and text just written.
    list[first_kept..].rotate_left(kept);
    let first_draw = first_draw - kept;
    write_children(list);
    append_draw_nodes(
        list,
        node_id,
        commands,
        outer_count,
        DrawPass::OverlayFromOutput(first_draw),
        draws,
        PrimitivePhase::AfterChildren,
    );
}

fn write_layer_with_outer(
    target: &mut LayerNode,
    head: LayerHead,
    outer: Option<OuterDraws>,
    placement: Point,
    parent_content_offset: Point,
    write_children: impl FnOnce(&mut Vec<RenderNode>, Option<&GraphicsLayer>),
) {
    debug_assert!(target.children.is_empty(), "a layer is written emptied");
    let Some(outer) = outer else {
        write_children(&mut target.children, head.graphics_layer.as_ref());
        assign_layer(target, head);
        return;
    };
    let mut layer = crate::layer_recycling::layer_box();
    write_children(&mut layer.children, head.graphics_layer.as_ref());
    assign_layer(&mut layer, head);
    write_wrapper(target, layer, placement, outer, parent_content_offset);
}

fn write_snapshot_layer(
    snapshot: BuildNodeSnapshot,
    context: LowerContext,
    target: &mut LayerNode,
) {
    let BuildNodeSnapshot {
        node_id,
        placement,
        size,
        content_offset,
        slices,
        graphics_layer,
        children,
    } = snapshot;
    let (local_bounds, node_bounds) = layer_and_node_bounds(&slices, size);
    let child_context = context.for_children(&slices, content_offset, None);
    let head = node_layer_head(
        node_id,
        &slices,
        NodeFrame {
            local_bounds,
            node_bounds,
            placement,
            content_offset,
            translated_content_offset: content_offset,
        },
        graphics_layer,
        context,
    );
    let draws = DrawTarget {
        size,
        motion_context_animated: head.motion_context_animated,
        layer: None,
    };
    let outer = outer_draws(
        node_id,
        slices.draw_commands(),
        slices.outer_draw_command_count(),
        draws,
    );
    write_layer_with_outer(
        target,
        head,
        outer,
        placement,
        context.parent_content_offset,
        |list, layer| {
            let draws = DrawTarget { layer, ..draws };
            write_node_content(list, node_id, &slices, draws, (0, children.len()), |list| {
                for child in children {
                    let mut layer = crate::layer_recycling::layer_box();
                    write_snapshot_layer(child, child_context, &mut layer);
                    list.push(RenderNode::Layer(layer));
                }
            });
        },
    );
}

#[derive(Clone, Copy)]
struct AbsOrigin {
    content_origin: Point,
    window_transform: ProjectiveTransform,
}

impl AbsOrigin {
    const ROOT: Self = Self {
        content_origin: Point { x: 0.0, y: 0.0 },
        window_transform: ProjectiveTransform::identity(),
    };

    fn children_of(self, layer: &LayerNode) -> Self {
        let top_left = Point {
            x: self.content_origin.x + layer.origin_in_parent.x,
            y: self.content_origin.y + layer.origin_in_parent.y,
        };
        Self {
            content_origin: Point {
                x: self.content_origin.x + layer.origin_in_parent.x + layer.content_offset.x,
                y: self.content_origin.y + layer.origin_in_parent.y + layer.content_offset.y,
            },
            window_transform: layer_transform_to_window(
                self.window_transform,
                top_left,
                layer.local_bounds,
                &layer.graphics_layer,
            ),
        }
    }
}

fn read_placed_node_data<R>(
    applier: &MemoryApplier,
    node_id: NodeId,
    level: Level,
    read: impl FnOnce(SnapshotNodeData<'_>) -> R,
) -> Option<R> {
    read_node_data(applier, node_id, |mut data| {
        if data.window_root {
            if level == Level::Child {
                return None;
            }
            data.layout_state = data.layout_state.at_origin();
        }
        note_layer_lowered();
        if !data.layout_state.is_placed() {
            return None;
        }
        Some(read(data))
    })?
}

fn lower_root_into(applier: &MemoryApplier, node_id: NodeId, target: &mut LayerNode) -> bool {
    read_placed_node_data(applier, node_id, Level::Root, |data| {
        write_node_layer(applier, node_id, data, LowerContext::ROOT, target);
    })
    .is_some()
}

fn lower_child(
    applier: &MemoryApplier,
    node_id: NodeId,
    context: LowerContext,
) -> Option<Box<LayerNode>> {
    read_placed_node_data(applier, node_id, Level::Child, |data| {
        let mut layer = crate::layer_recycling::layer_box();
        write_node_layer(applier, node_id, data, context, &mut layer);
        layer
    })
}

/// Where a placed child sits in its parent and how large it is.
#[derive(Clone, Copy)]
struct PlacedGeometry {
    position: Point,
    size: Size,
}

impl PlacedGeometry {
    fn of_placed(state: &cranpose_ui::widgets::LayoutState) -> Option<Self> {
        state.is_placed().then(|| Self {
            position: state.position(),
            size: state.size(),
        })
    }
}

/// The geometry of the placed child `node_id`; `None` for a child that is
/// not placed or is a window root, drawn in a surface of its own.
fn placed_child_geometry(applier: &MemoryApplier, node_id: NodeId) -> Option<PlacedGeometry> {
    let node: &dyn Any = applier.get_ref(node_id).ok()?;
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        if node.is_window_root() {
            return None;
        }
        return node.with_layout_state(PlacedGeometry::of_placed);
    }
    PlacedGeometry::of_placed(&node.downcast_ref::<SubcomposeLayoutNode>()?.layout_state())
}

fn read_node_data<R>(
    applier: &MemoryApplier,
    node_id: NodeId,
    read: impl FnOnce(SnapshotNodeData<'_>) -> R,
) -> Option<R> {
    let node: &dyn Any = applier.get_ref(node_id).ok()?;
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        let z_order = z_ordered_children(applier, &node.children);
        return Some(read(SnapshotNodeData {
            layout_state: node.layout_state(),
            modifier_slices: node.modifier_slices_snapshot(),
            children: z_order.as_deref().unwrap_or(&node.children),
            window_root: node.is_window_root(),
        }));
    }
    let node = node.downcast_ref::<SubcomposeLayoutNode>()?;
    let layout_state = node.layout_state();
    let modifier_slices = node.modifier_slices_snapshot();
    Some(node.with_active_children(|children| {
        let z_order = z_ordered_children(applier, children);
        read(SnapshotNodeData {
            layout_state,
            modifier_slices,
            children: z_order.as_deref().unwrap_or(children),
            window_root: false,
        })
    }))
}

/// `children` in the order they draw and take taps, when a `z_index`
/// modifier moves one of them: by z index, and in composition order among
/// equal ones, as Compose's `zSortedChildren`. `None` while the composition
/// order already is that order, which it always is while no `z_index`
/// modifier exists.
#[inline]
fn z_ordered_children(applier: &MemoryApplier, children: &[NodeId]) -> Option<Vec<NodeId>> {
    if children.len() < 2 || !cranpose_ui::z_index_in_use() {
        return None;
    }
    z_sorted_children(applier, children)
}

#[inline(never)]
fn z_sorted_children(applier: &MemoryApplier, children: &[NodeId]) -> Option<Vec<NodeId>> {
    let mut previous = f32::NEG_INFINITY;
    let in_order = children.iter().all(|&child| {
        let z_index = node_z_index(applier, child);
        let ordered = z_index >= previous;
        previous = z_index;
        ordered
    });
    if in_order {
        return None;
    }
    let mut ordered: Vec<(f32, NodeId)> = children
        .iter()
        .map(|&child| (node_z_index(applier, child), child))
        .collect();
    ordered.sort_by(|a, b| a.0.total_cmp(&b.0));
    Some(ordered.into_iter().map(|(_, child)| child).collect())
}

fn node_z_index(applier: &MemoryApplier, node_id: NodeId) -> f32 {
    let Ok(node) = applier.get_ref(node_id) else {
        return 0.0;
    };
    let node: &dyn Any = node;
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        return node.modifier_slices_snapshot().z_index();
    }
    node.downcast_ref::<SubcomposeLayoutNode>()
        .map_or(0.0, |node| node.modifier_slices_snapshot().z_index())
}

fn hit_test_from_slices(slices: &Rc<ModifierNodeSlices>) -> Option<HitTestNode> {
    slices_hit_something(slices).then(|| HitTestNode {
        handlers: Rc::clone(slices),
    })
}

/// A node's layer bounds and, when they differ from it, its own rect (see
/// [`LayerNode::node_bounds`]).
fn layer_and_node_bounds(slices: &ModifierNodeSlices, size: Size) -> (Rect, Option<Rect>) {
    let node_bounds = Rect::from_size(size);
    let layer_bounds = slices.layer_bounds(size);
    (
        layer_bounds,
        (layer_bounds != node_bounds).then_some(node_bounds),
    )
}

/// Whether a node's slices make it a hit target: a pointer input or a
/// pointer icon.
fn slices_hit_something(slices: &ModifierNodeSlices) -> bool {
    !slices.pointer_inputs().is_empty() || slices.pointer_icon().is_some()
}

fn prepare_node_layer(
    node_id: NodeId,
    slices: &Rc<ModifierNodeSlices>,
    layout_state: &cranpose_ui::widgets::LayoutState,
    context: LowerContext,
) -> (LayerHead, LowerContext) {
    let size = layout_state.size();
    let placement = layout_state.position();
    let (local_bounds, node_bounds) = layer_and_node_bounds(slices, size);
    let graphics_layer = shaped_graphics_layer(slices, local_bounds);
    let geometry = context.parent_abs.map(|parent_abs| {
        TranslateGeometry::new(
            layout_state,
            graphics_layer.as_deref().unwrap_or(&GraphicsLayer::DEFAULT),
            local_bounds,
            parent_abs,
        )
    });
    if let Some(geometry) = geometry {
        slices.publish_window_geometry(geometry.top_left, geometry.window_transform, size);
    } else {
        slices.publish_pointer_input_size(size);
    }
    let content_offset = layout_state.content_offset();
    let child_context = context.for_children(
        slices,
        content_offset,
        geometry.map(TranslateGeometry::child_abs),
    );
    let head = node_layer_head(
        node_id,
        slices,
        NodeFrame {
            local_bounds,
            node_bounds,
            placement,
            content_offset,
            translated_content_offset: slices.translated_content_offset().unwrap_or(content_offset),
        },
        graphics_layer.map(Cow::into_owned),
        context,
    );
    (head, child_context)
}

#[expect(clippy::print_stderr, reason = "CRANPOSE_SCENE_UPDATE_DIAG asks")]
fn write_node_layer(
    applier: &MemoryApplier,
    node_id: NodeId,
    data: SnapshotNodeData<'_>,
    context: LowerContext,
    target: &mut LayerNode,
) {
    let SnapshotNodeData {
        layout_state,
        modifier_slices: slices,
        children,
        window_root: _,
    } = data;
    let size = layout_state.size();
    let placement = layout_state.position();
    if cranpose_core::DIAGNOSTICS.scene_update_diag.is_set() {
        eprintln!(
            "[scene-update-diag] build layer node={node_id:?} size=({:.2},{:.2}) pos=({:.2},{:.2})",
            size.width, size.height, placement.x, placement.y,
        );
    }
    let (head, child_context) = prepare_node_layer(node_id, &slices, &layout_state, context);
    let draws = DrawTarget {
        size,
        motion_context_animated: head.motion_context_animated,
        layer: None,
    };
    let outer = outer_draws(
        node_id,
        slices.draw_commands(),
        slices.outer_draw_command_count(),
        draws,
    );
    write_layer_with_outer(
        target,
        head,
        outer,
        placement,
        context.parent_content_offset,
        |list, layer| {
            let draws = DrawTarget { layer, ..draws };
            write_node_content(list, node_id, &slices, draws, (0, children.len()), |list| {
                for &child_id in children {
                    if let Some(child) = lower_child(applier, child_id, child_context) {
                        list.push(RenderNode::Layer(child));
                    }
                }
            });
        },
    );
}

struct OuterDraws {
    behind: Vec<RenderNode>,
    overlay: Vec<RenderNode>,
}

fn outer_draws(
    node_id: NodeId,
    draw_commands: &[DrawCommand],
    outer_draw_command_count: usize,
    draws: DrawTarget,
) -> Option<OuterDraws> {
    (outer_draw_command_count > 0).then(|| {
        let commands = &draw_commands[..outer_draw_command_count];
        let behind = draw_nodes(
            node_id,
            commands,
            0,
            DrawPlacement::Behind,
            draws,
            PrimitivePhase::BeforeChildren,
        );
        let mut overlay = crate::layer_recycling::child_list(commands.len());
        append_draw_nodes(
            &mut overlay,
            node_id,
            commands,
            0,
            DrawPass::OverlayFrom(behind.iter()),
            draws,
            PrimitivePhase::AfterChildren,
        );
        OuterDraws { behind, overlay }
    })
}

fn write_wrapper(
    wrapper: &mut LayerNode,
    mut layer: Box<LayerNode>,
    placement: Point,
    outer: OuterDraws,
    parent_content_offset: Point,
) {
    debug_assert!(wrapper.children.is_empty(), "a wrapper is written emptied");
    layer.transform_to_parent =
        layer_transform_to_parent(layer.local_bounds, Point::default(), &layer.graphics_layer);
    layer.origin_in_parent = Point::default();
    let node_rect = layer.node_rect();
    let graphics_layer = None;
    let head = LayerHead {
        node_id: None,
        wraps: layer.node_id,
        local_bounds: node_rect,
        node_bounds: None,
        transform_to_parent: placed_transform(node_rect, placement, None, parent_content_offset),
        content_offset: Point::default(),
        motion_context_animated: layer.motion_context_animated,
        translated_content_context: false,
        translated_content_offset: Point::default(),
        origin_in_parent: placement,
        graphics_layer,
        clip_to_bounds: false,
        hit_test: None,
        has_origin_sinks: false,
        isolation: IsolationReasons::default(),
        cache_policy: CachePolicy::None,
        draw_phase: None,
    };
    let OuterDraws {
        mut behind,
        mut overlay,
    } = outer;
    let children = &mut wrapper.children;
    crate::layer_recycling::reserve_nodes(children, behind.len() + 1 + overlay.len());
    children.append(&mut behind);
    children.push(RenderNode::Layer(layer));
    children.append(&mut overlay);
    crate::layer_recycling::recycle_list(behind);
    crate::layer_recycling::recycle_list(overlay);
    assign_layer(wrapper, head);
}

fn take_wrapped_layer(wrapper: &mut LayerNode, node_id: NodeId) -> Option<Box<LayerNode>> {
    let mut wrapped = None;
    for child in wrapper.children.drain(..) {
        if let RenderNode::Layer(layer) = child
            && layer.node_id == Some(node_id)
        {
            wrapped = Some(layer);
        }
    }
    wrapped
}

fn layer_identity(layer: &LayerNode) -> Option<NodeId> {
    layer.node_id.or(layer.wraps)
}

struct TextNodeParts {
    node_id: NodeId,
    /// Where the text node was placed in its layout node.
    text_rect: Rect,
    text_layout_options: Option<TextLayoutOptions>,
    text_pan: Option<TextPanResolver>,
    measured_layout: Option<Rc<PreparedTextLayout>>,
}

fn text_node_from_parts(parts: TextNodeParts) -> Option<TextPrimitiveNode> {
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
        height: prepared.metrics.height,
    };
    let text_bounds = Rect {
        width: content_width,
        height: text_rect.height.max(0.0),
        ..text_rect
    };
    let font_size = visual_style.resolve_font_size(14.0);
    let expanded_bounds =
        expand_text_bounds_for_baseline_shift(text_bounds, visual_style, font_size);
    let clip = if options.overflow == TextOverflow::Visible && !pans_horizontally {
        None
    } else {
        Some(pad_clip_rect(expanded_bounds))
    };

    Some(TextPrimitiveNode {
        node_id,
        rect,
        text: Rc::clone(&prepared.text),
        render_text: prepared.render_text(),
        layout: std::sync::Arc::clone(&prepared.layout),
        text_style: std::sync::Arc::clone(visual_style),
        font_size,
        layout_options: options,
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

fn layout_box_to_snapshot(node: &LayoutBox, parent: Option<&LayoutBox>) -> BuildNodeSnapshot {
    let placement = parent
        .map(|parent_box| Point {
            x: node.rect.x - parent_box.rect.x - parent_box.content_offset.x,
            y: node.rect.y - parent_box.rect.y - parent_box.content_offset.y,
        })
        .unwrap_or_default();
    let mut children = Vec::with_capacity(node.children.len());
    for child in &node.children {
        children.push(layout_box_to_snapshot(child, Some(node)));
    }
    if cranpose_ui::z_index_in_use() {
        // Stable: equal z indices keep the composition order.
        children.sort_by(|a, b| a.slices.z_index().total_cmp(&b.slices.z_index()));
    }
    let mut graphics_layer = node.node_data.modifier_slices.graphics_layer();
    let has_graphics_layer = graphics_layer.is_some();
    shape_clip(
        &mut graphics_layer,
        node.node_data.modifier_slices.clip_to_bounds(),
        node.node_data.modifier_slices.corner_shape(),
        Rect {
            x: 0.0,
            y: 0.0,
            width: node.rect.width,
            height: node.rect.height,
        },
    );
    let has_graphics_layer = has_graphics_layer
        || graphics_layer
            .as_ref()
            .is_some_and(|layer| layer.render_effect.is_some());

    BuildNodeSnapshot {
        node_id: node.node_id,
        placement,
        size: Size {
            width: node.rect.width,
            height: node.rect.height,
        },
        content_offset: node.content_offset,
        slices: Rc::clone(&node.node_data.modifier_slices),
        graphics_layer: graphics_layer
            .filter(|_| has_graphics_layer)
            .map(Cow::into_owned),
        children,
    }
}

fn modifier_slices_have_origin_sinks(slices: &ModifierNodeSlices) -> bool {
    slices.text_window().is_some() || slices.viewport_window_rect().is_some()
}

/// The graphics layer of a node with `slices` and bounds `local_bounds`:
/// the chain's own, its clip shaped as [`shape_clip`] shapes it. Lent by
/// the slices unless the shaping changes it, and returned straight from
/// them when nothing shapes it: a layer kept in a local first is copied out.
#[inline]
fn shaped_graphics_layer(
    slices: &ModifierNodeSlices,
    local_bounds: Rect,
) -> Option<Cow<'_, GraphicsLayer>> {
    match clip_corners(slices.clip_to_bounds(), slices.corner_shape(), local_bounds) {
        None => slices.graphics_layer(),
        Some(corners) => rounded_graphics_layer(slices, corners, local_bounds),
    }
}

#[inline(never)]
fn rounded_graphics_layer(
    slices: &ModifierNodeSlices,
    corners: (RoundedCornerShape, CornerRadii),
    local_bounds: Rect,
) -> Option<Cow<'_, GraphicsLayer>> {
    let mut graphics_layer = slices.graphics_layer();
    round_clip(&mut graphics_layer, corners, local_bounds);
    graphics_layer
}

/// Shapes the clip of `graphics_layer`, a layer's own, to the corner shape
/// of a layer that clips to its bounds `local_bounds`. In place: the layer
/// moved in and out of a call is hundreds of bytes copied each way, and a
/// lent layer is copied only when the shaping changes it.
fn shape_clip(
    graphics_layer: &mut Option<Cow<'_, GraphicsLayer>>,
    clip_to_bounds: bool,
    corner_shape: Option<RoundedCornerShape>,
    local_bounds: Rect,
) {
    if let Some(corners) = clip_corners(clip_to_bounds, corner_shape, local_bounds) {
        round_clip(graphics_layer, corners, local_bounds);
    }
}

/// The corner shape a layer that clips to its bounds `local_bounds` rounds
/// its clip with, and its radii there: `None` when it does not clip to its
/// bounds or its corners are square.
fn clip_corners(
    clip_to_bounds: bool,
    corner_shape: Option<RoundedCornerShape>,
    local_bounds: Rect,
) -> Option<(RoundedCornerShape, CornerRadii)> {
    if !clip_to_bounds {
        return None;
    }
    let corner_shape = corner_shape?;
    let radii = corner_shape.resolve(local_bounds.width, local_bounds.height);
    let square = radii.top_left <= f32::EPSILON
        && radii.top_right <= f32::EPSILON
        && radii.bottom_right <= f32::EPSILON
        && radii.bottom_left <= f32::EPSILON;
    (!square).then_some((corner_shape, radii))
}

/// Rounds the clip of `graphics_layer` with `corners`, as [`clip_corners`]
/// found them for a layer at `local_bounds`.
fn round_clip(
    graphics_layer: &mut Option<Cow<'_, GraphicsLayer>>,
    (corner_shape, radii): (RoundedCornerShape, CornerRadii),
    local_bounds: Rect,
) {
    let properties = graphics_layer.get_or_insert_default().to_mut();
    if let Some(existing) = properties.render_effect.take() {
        let rounded_clip = rounded_corner_alpha_mask_effect(
            local_bounds.width,
            local_bounds.height,
            radii,
            ROUNDED_CLIP_EDGE_FEATHER,
        );
        properties.render_effect = Some(existing.then(rounded_clip));
    } else {
        properties.shape = LayerShape::Rounded(corner_shape);
        properties.clip = true;
    }
}

fn layer_cache_policy(layer: &GraphicsLayer, isolation: IsolationReasons) -> CachePolicy {
    if isolation.has_any() || layer_scales_or_rotates(layer) {
        CachePolicy::Auto
    } else {
        CachePolicy::None
    }
}

fn isolation_reasons(layer: &GraphicsLayer) -> IsolationReasons {
    IsolationReasons {
        explicit_offscreen: layer.compositing_strategy == CompositingStrategy::Offscreen,
        shape_clip: layer.clip && !matches!(layer.shape, LayerShape::Rectangle),
        effect: layer.render_effect.is_some(),
        backdrop: layer.backdrop_effect.is_some(),
        group_opacity: layer.compositing_strategy != CompositingStrategy::ModulateAlpha
            && layer.alpha < 1.0,
        blend_mode: layer.blend_mode != cranpose_ui::BlendMode::SrcOver,
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

/// Grows `text_bounds` in the direction of the style's baseline shift, by the shift in pixels at
/// `font_size`, so a clip does not cut the shifted glyphs. Returns the bounds unchanged when
/// there is no baseline shift.
pub fn expand_text_bounds_for_baseline_shift(
    text_bounds: Rect,
    text_style: &TextStyle,
    font_size: f32,
) -> Rect {
    let baseline_shift_px = text_style
        .span_style
        .baseline_shift
        .filter(|shift| shift.is_specified())
        .map_or(0.0, |shift| -(shift.0 * font_size));
    if baseline_shift_px == 0.0 {
        return text_bounds;
    }

    if baseline_shift_px < 0.0 {
        Rect {
            x: text_bounds.x,
            y: text_bounds.y + baseline_shift_px,
            width: text_bounds.width,
            height: (text_bounds.height - baseline_shift_px).max(0.0),
        }
    } else {
        Rect {
            x: text_bounds.x,
            y: text_bounds.y,
            width: text_bounds.width,
            height: (text_bounds.height + baseline_shift_px).max(0.0),
        }
    }
}

#[cfg(test)]
#[path = "tests/scene_builder_tests.rs"]
mod tests;
