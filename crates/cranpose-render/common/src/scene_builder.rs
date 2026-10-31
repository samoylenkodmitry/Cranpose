use std::{any::Any, rc::Rc};

use cranpose_core::{MemoryApplier, NodeId, collections::map::HashMap};
use cranpose_ui::{LayoutBox, ModifierNodeSlices, Point, Rect, Size, SubcomposeLayoutNode};
use cranpose_ui_graphics::GraphicsLayer;

use crate::{
    NodeUpdate, SceneUpdates,
    graph::{LayerNode, ProjectiveTransform, RenderGraph, RenderNode},
    layer_transform::layer_transform_to_parent,
};

mod dirty_walk;
pub(crate) use dirty_walk::{DirtyAncestry, RetainedPath};
use dirty_walk::{update_along_retained_path, walk_dirty_layers};
mod draw_recording;
use draw_recording::{
    DrawPass, DrawTarget, append_draw_nodes, bump_recording_generation, draw_nodes,
    layer_node_capacity,
};
pub(crate) use draw_recording::{RecorderKey, RecorderSlot};
pub use draw_recording::{clear_command_recordings_for_tests, draw_command_nodes_for_tests};
mod hit_graph;
pub(crate) use hit_graph::HitGraphState;
use hit_graph::rebuild_tracking_hits;
mod inline_leaf;
use inline_leaf::{lower_child, update_dirty_inline};
mod layer_shape;
use layer_shape::shaped_graphics_layer;
mod text_node;
mod translate_geometry;
use text_node::{TextNodeParts, text_node_from_parts, translate_unwrapped};
use translate_geometry::{KeptChild, TranslateGeometry, move_kept_child, translate_retained_child};
mod node_layer;
use node_layer::{
    AbsOrigin, LayerHead, LowerContext, PlacedGeometry, assign_layer, children_have_origin_sinks,
    hit_test_from_slices, layer_and_node_bounds, layer_identity, layout_box_to_snapshot,
    lower_root_into, modifier_slices_have_origin_sinks, outer_draws, placed_child_geometry,
    placed_transform, prepare_node_layer, read_node_data, read_placed_node_data,
    slices_hit_something, take_wrapped_layer, write_node_content, write_node_layer,
    write_snapshot_layer, write_wrapper,
};
mod translate_layer;
pub(crate) use translate_layer::TranslateScratch;
use translate_layer::{
    TranslatedContainer, translate_tracking_hits, translated_container,
    write_own_content_around_children,
};

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
            let walked = walk_dirty_layers(applier, graph, remaining_dirty_nodes, update)
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

    let Some((report, path)) = walk_dirty_layers(applier, graph, remaining_dirty_nodes, update)
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

// Whether a node's layer lies in the wrapper that holds the draws chained
// before its graphics layer.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Wrapping {
    Wrapped,
    Bare,
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
        let kept = Position::Kept;
        if !retained_layer_matches_layout(container, &data, placement, Wrapping::Bare, kept) {
            return false;
        }
        let Ok(plan) = translated_container(
            container,
            &data.layout_state,
            &data.modifier_slices,
            ancestors,
            Wrapping::Bare,
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
    wrapping: Wrapping,
    position: Position,
) -> bool {
    let state = &data.layout_state;
    let slices = &data.modifier_slices;
    (position == Position::Moved || state.position() == placement)
        && Rect::from_size(state.size()) == layer.node_rect()
        && slices.layer_bounds(state.size()) == layer.local_bounds
        && state.content_offset() == layer.content_offset
        && (slices.outer_draw_command_count() > 0) == (wrapping == Wrapping::Wrapped)
}

fn update_layer_properties(
    applier: &MemoryApplier,
    container: &mut LayerNode,
    level: Level,
    position: Position,
    ancestors: TranslateAncestorContext,
) -> Option<bool> {
    let node_id = layer_identity(container)?;
    let wrapping = if container.wraps.is_some() {
        Wrapping::Wrapped
    } else {
        Wrapping::Bare
    };
    let wrapped = wrapping == Wrapping::Wrapped;
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
        if !retained_layer_matches_layout(target, &data, placement, wrapping, position) {
            return None;
        }
        let context = LowerContext::beneath(ancestors);
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

// Takes the node of `node`, a child of a layer, and every node beneath it
// out of `dirty_nodes`.
fn forget_dirty_node(node: &RenderNode, dirty_nodes: &mut HashMap<NodeId, NodeUpdate>) {
    match node {
        RenderNode::Layer(layer) => forget_dirty_subtree(layer, dirty_nodes),
        RenderNode::Inline(inline) => {
            dirty_nodes.remove(&inline.node_id);
        }
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) => {}
    }
}

fn remove_dirty_descendants(layer: &LayerNode, dirty_nodes: &mut HashMap<NodeId, NodeUpdate>) {
    for child in &layer.children {
        match child {
            RenderNode::Layer(child_layer) => {
                if let Some(node_id) = child_layer.node_id {
                    dirty_nodes.remove(&node_id);
                }
                remove_dirty_descendants(child_layer, dirty_nodes);
            }
            RenderNode::Inline(inline) => {
                dirty_nodes.remove(&inline.node_id);
            }
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => {}
        }
    }
}

// The node a child of a layer draws: a child layer's or an inline node's,
// `None` for the layer's own draws.
fn child_identity(node: &RenderNode) -> Option<NodeId> {
    match node {
        RenderNode::Layer(layer) => layer_identity(layer),
        RenderNode::Inline(inline) => Some(inline.node_id),
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
    }
}

#[cfg(test)]
#[path = "tests/scene_builder_tests.rs"]
mod tests;
