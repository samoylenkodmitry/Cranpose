use std::{any::Any, rc::Rc};

use cranpose_core::{MemoryApplier, NodeId, RecordId, collections::map::HashMap};
use cranpose_ui::{ModifierNodeSlices, Point, Rect, SubcomposeLayoutNode};

use crate::{
    NodeUpdate, SceneUpdates,
    graph::ProjectiveTransform,
    layer_transform::layer_transform_to_parent,
    scene_store::{Item, LayerRef, RecordKind, SceneStore},
};

mod dirty_walk;
pub(crate) use dirty_walk::{DirtyAncestry, RetainedPath};
use dirty_walk::{update_along_retained_path, walk_dirty_layers};
mod draw_recording;
pub(crate) use draw_recording::draw_command_items;
use draw_recording::{
    DrawPass, DrawTarget, GroupPaint, NodeCommands, append_draw_nodes, draw_nodes,
    layer_node_capacity, redraw_in_place,
};
mod graph_changes;
use graph_changes::GraphChanges;
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
mod node_data;
use node_data::{PlacedGeometry, placed_child_geometry, read_node_data, read_placed_node_data};
mod node_layer;
use node_layer::{
    AbsOrigin, HeadKind, LayerHead, LowerContext, assign_layer, hit_test_from_slices,
    layer_and_node_bounds, layer_identity, lower_root_into, modifier_slices_have_origin_sinks,
    outer_commands, outer_draws, placing_transform, prepare_node_layer, set_bounds,
    set_origin_sinks, slices_hit_something, take_wrapped_layer, write_node_content,
    write_node_layer, write_wrapper,
};
mod translate_layer;
pub(crate) use translate_layer::TranslateScratch;
use translate_layer::{
    ItemParts, TranslatedContainer, translate_tracking_hits, translated_container,
    write_own_content_around_children,
};

struct SnapshotNodeData<'a> {
    layout_state: cranpose_ui::widgets::LayoutState,
    modifier_slices: Rc<ModifierNodeSlices>,
    children: &'a [NodeId],
    window_root: bool,
}

/// Why a scoped scene update could not be applied, forcing the caller to throw
/// the scene away and build it again from the applier.
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
    /// This many dirty nodes own no record in the scene, so the scoped walk
    /// never reached them. A dirty node that never made a record, or whose
    /// record left the scene this frame, cannot be patched in place.
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
    /// Whether the update moved a layer in its parent by the layer's own properties, as an
    /// animated graphics layer or offset moves it, which no layout pass reports.
    pub layers_moved: bool,
}

impl GraphUpdateReport {
    fn patched(changes: GraphChanges) -> Self {
        Self {
            update: GraphUpdate::Patched,
            hit_graph_dirty: changes.contains(GraphChanges::Hits),
            layers_moved: changes.contains(GraphChanges::Moves),
        }
    }

    fn needs_rebuild(reason: GraphRebuildReason) -> Self {
        Self {
            update: GraphUpdate::NeedsRebuild(reason),
            hit_graph_dirty: true,
            layers_moved: true,
        }
    }

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

// What the dirty walk keeps of one update for the next: the path to the
// layer it updated last and the marks of the nodes it walked through.
#[derive(Default)]
pub(crate) struct WalkMemory {
    retained_path: Option<RetainedPath>,
    dirty_ancestry: DirtyAncestry,
}

/// Builds the records of the layout tree below `root`, read from `applier`.
/// Returns `None` when `root` cannot be lowered to a layer.
pub fn build_from_applier(applier: &MemoryApplier, root: NodeId) -> Option<SceneStore> {
    let mut store = SceneStore::new();
    rebuild_from_applier(applier, root, &mut store, &mut WalkMemory::default()).then_some(store)
}

// Lowers the layout tree below `root` into `store` again, its records reusing
// the store's rows and arenas. Returns `false` and leaves the store empty
// when `root` cannot be lowered to a layer.
pub(crate) fn rebuild_from_applier(
    applier: &MemoryApplier,
    root: NodeId,
    store: &mut SceneStore,
    walk: &mut WalkMemory,
) -> bool {
    store.begin_update();
    if let Some(record) = store.root() {
        release_for_rebuild(store, record);
    }
    walk.retained_path = None;
    if !lower_root_into(applier, store, root) {
        store.clear();
        return false;
    }
    if let Some(record) = store.root() {
        store.note_content_change(record);
    }
    store.finish_update();
    true
}

// Applies `updates` to `store` in place, and reports whether the hit targets
// must be collected again or why the store must be built again.
#[expect(clippy::print_stderr, reason = "CRANPOSE_SCENE_UPDATE_DIAG asks")]
pub(crate) fn update_from_applier(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    walk: &mut WalkMemory,
    updates: SceneUpdates<'_>,
) -> GraphUpdateReport {
    let report = update_from_applier_inner(applier, store, walk, updates);
    if !updates.is_empty() && report.applied() {
        store.finish_update();
    }
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

// The dirty nodes the map keeps room for at least.
const DIRTY_NODES_KEPT: usize = 64;

// Returns a move when a node's layer change merged into new content: its
// layer is lowered again, which keeps no record of where it lay.
fn dirty_node_updates(
    updates: SceneUpdates<'_>,
    dirty: &mut HashMap<NodeId, NodeUpdate>,
) -> GraphChanges {
    dirty.clear();
    dirty.reserve(updates.len());
    let mut changes = GraphChanges::default();
    for &(id, update) in updates {
        dirty
            .entry(id)
            .and_modify(|kind| {
                let merged = kind.merge(update);
                if merged == NodeUpdate::Content && (changes_layer(*kind) || changes_layer(update))
                {
                    changes = GraphChanges::Moves;
                }
                *kind = merged;
            })
            .or_insert(update);
    }
    changes
}

fn changes_layer(update: NodeUpdate) -> bool {
    matches!(update, NodeUpdate::Layer | NodeUpdate::MovedLayer)
}

#[expect(clippy::print_stderr, reason = "CRANPOSE_SCENE_UPDATE_DIAG asks")]
fn update_from_applier_inner(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    walk: &mut WalkMemory,
    updates: SceneUpdates<'_>,
) -> GraphUpdateReport {
    if updates.is_empty() {
        return GraphUpdateReport::patched(GraphChanges::default());
    }
    store.begin_update();

    if cranpose_core::DIAGNOSTICS.scene_update_diag.is_set() {
        eprintln!("[scene-update-diag] dirty={updates:?}");
    }

    // The map of the frame before keeps its storage for this one.
    let (mut remaining_dirty_nodes, mut recent) =
        crate::thread_state::with(|state| state.dirty_node_scratch.take()).unwrap_or_default();
    let merged = dirty_node_updates(updates, &mut remaining_dirty_nodes);
    let named = remaining_dirty_nodes.len();
    let mut report = update_dirty_nodes(applier, store, walk, &mut remaining_dirty_nodes);
    report.layers_moved |= merged.contains(GraphChanges::Moves);
    remaining_dirty_nodes.clear();
    recent.settle_spent(&mut remaining_dirty_nodes, named, DIRTY_NODES_KEPT);
    crate::thread_state::with(|state| {
        state
            .dirty_node_scratch
            .set(Some((remaining_dirty_nodes, recent)));
    });
    report
}

fn update_dirty_nodes(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    walk: &mut WalkMemory,
    remaining_dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) -> GraphUpdateReport {
    let WalkMemory {
        retained_path,
        dirty_ancestry,
    } = walk;
    let Some(root) = store.root() else {
        return GraphUpdateReport::needs_rebuild(GraphRebuildReason::RootLayerUnavailable);
    };
    if let Some(root_id) = layer_identity(store.layer(root))
        && let Some(kind) = remaining_dirty_nodes.remove(&root_id)
    {
        return update_dirty_root(
            applier,
            store,
            dirty_ancestry,
            remaining_dirty_nodes,
            (root, root_id, kind),
        );
    }

    // An update that does not go along the retained path may move the
    // layers on it, so only one that does keeps it.
    if let Some(path) = retained_path.take()
        && let Some(report) =
            update_along_retained_path(applier, store, &path, remaining_dirty_nodes)
    {
        if report.is_some() {
            *retained_path = Some(RetainedPath {
                update: store.update(),
                ..path
            });
        }
        return match report {
            Some(report) => GraphUpdateReport::patched(report.changes),
            None => GraphUpdateReport::needs_rebuild(GraphRebuildReason::DirtyLayerUnavailable),
        };
    }

    let Some((report, path)) =
        walk_dirty_layers(applier, store, dirty_ancestry, remaining_dirty_nodes)
    else {
        return GraphUpdateReport::needs_rebuild(GraphRebuildReason::DirtyLayerUnavailable);
    };
    *retained_path = path;

    match classify_walk(applier, true, remaining_dirty_nodes) {
        GraphUpdate::Patched => GraphUpdateReport::patched(report.changes),
        GraphUpdate::NeedsRebuild(reason) => GraphUpdateReport::needs_rebuild(reason),
    }
}

// Updates the scene for a frame whose dirty nodes include the root, `kind`
// the root's update: a retained root layer takes it in place, any other
// root is lowered again. Out of line: most frames leave the root alone.
#[inline(never)]
fn update_dirty_root(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    dirty_ancestry: &mut DirtyAncestry,
    remaining_dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    (root, root_id, kind): (RecordId, NodeId, NodeUpdate),
) -> GraphUpdateReport {
    if let Some(mut changes) = try_update_retained_layer(
        applier,
        store,
        root,
        remaining_dirty_nodes,
        kind,
        Level::Root,
        TranslateAncestorContext {
            inherited_motion_context_animated: false,
            inherited_translated_content_context: false,
            parent_content_offset: Point::default(),
            parent_abs: AbsOrigin::ROOT,
        },
    ) {
        if remaining_dirty_nodes.is_empty() {
            return GraphUpdateReport::patched(changes);
        }
        let walked = walk_dirty_layers(applier, store, dirty_ancestry, remaining_dirty_nodes)
            .map(|(report, _)| report);
        changes |= walked.map_or(GraphChanges::HitsAndMoves, |report| report.changes);
        return GraphUpdateReport {
            update: classify_walk(applier, walked.is_some(), remaining_dirty_nodes),
            ..GraphUpdateReport::patched(changes)
        };
    }

    let Some(hit_graph_dirty) = rebuild_tracking_hits(store, root, |store, root| {
        release_for_rebuild(store, root);
        lower_root_into(applier, store, root_id)
    }) else {
        return GraphUpdateReport::needs_rebuild(GraphRebuildReason::RootLayerUnavailable);
    };
    store.note_content_change(root);
    GraphUpdateReport::patched(GraphChanges::hits(hit_graph_dirty))
}

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
    store: &mut SceneStore,
    layer: RecordId,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    kind: NodeUpdate,
    level: Level,
    ancestors: TranslateAncestorContext,
) -> Option<GraphChanges> {
    match kind {
        NodeUpdate::Moved if level == Level::Child => {
            move_retained_layer(applier, store, layer, ancestors)
                .or_else(|| translate_tracking_hits(applier, store, layer, dirty_nodes, ancestors))
        }
        NodeUpdate::Draws if redraw_retained_layer(applier, store, layer, level, ancestors) => {
            Some(GraphChanges::default())
        }
        // A translate keeps the layer's graphics layer: one that changed is
        // lowered again.
        NodeUpdate::Content | NodeUpdate::Draws | NodeUpdate::Moved => {
            translate_tracking_hits(applier, store, layer, dirty_nodes, ancestors)
        }
        NodeUpdate::Layer => {
            update_layer_properties(applier, store, layer, level, Position::Kept, ancestors)
        }
        NodeUpdate::MovedLayer => {
            update_layer_properties(applier, store, layer, level, Position::Moved, ancestors)
        }
    }
}

fn redraw_retained_layer(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    container: RecordId,
    level: Level,
    ancestors: TranslateAncestorContext,
) -> bool {
    let layer = store.layer(container);
    // The wrapper of a node's outer draws is written with its layer.
    if layer.kind() == RecordKind::Wrapper {
        return false;
    }
    let Some(node_id) = layer.node_id() else {
        return false;
    };
    read_placed_node_data(applier, node_id, level, |data| {
        let layer = store.layer(container);
        let placement = layer.origin_in_parent();
        let kept = Position::Kept;
        if !retained_layer_matches_layout(layer, &data, placement, Wrapping::Bare, kept) {
            return false;
        }
        if translated_container(
            store,
            container,
            &data.layout_state,
            &data.modifier_slices,
            ancestors,
            Wrapping::Bare,
        )
        .is_err()
        {
            return false;
        }
        let parts = ItemParts::of(store, container);
        let slices = &data.modifier_slices;
        let outer = slices.outer_draw_command_count();
        let commands = NodeCommands {
            node_id,
            commands: &slices.draw_commands()[outer..],
            first_index: outer,
        };
        let text = slices.has_text().then_some(&**slices);
        if redraw_in_place(store, parts.own(), commands, text, data.layout_state.size()) {
            parts.give_back(store);
            store.note_content_change(container);
            return true;
        }
        let mut list = store.lists.take();
        list.extend_from_slice(parts.children());
        parts.remove_own(store);
        write_own_content_around_children(
            store,
            container,
            &mut list,
            &data.modifier_slices,
            data.layout_state.size(),
        );
        store.set_items(container, &list);
        store.lists.give(list);
        parts.remove_gone(store);
        store.note_content_change(container);
        true
    })
    .unwrap_or(false)
}

fn move_retained_layer(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    layer: RecordId,
    ancestors: TranslateAncestorContext,
) -> Option<GraphChanges> {
    let placed = placed_child_geometry(applier, layer_identity(store.layer(layer))?)?;
    if Rect::from_size(placed.size) != store.layer(layer).node_rect() {
        return None;
    }
    let previous = HitGraphState::of(store.layer(layer));
    translate_retained_child(
        store,
        layer,
        placed.position,
        ancestors.parent_content_offset,
    );
    let moved = store.layer(layer);
    if moved.has_origin_sinks() {
        publish_retained_window_geometry(applier, moved, ancestors.parent_abs);
    }
    Some(previous.changes_against(&HitGraphState::of(moved)))
}

fn content_layer(store: &SceneStore, container: RecordId) -> Option<RecordId> {
    let layer = store.layer(container);
    let Some(node_id) = layer.wraps() else {
        return Some(container);
    };
    layer
        .children()
        .find(|child| child.kind() == RecordKind::Layer && child.node_id() == Some(node_id))
        .map(LayerRef::id)
}

fn retained_layer_matches_layout(
    layer: LayerRef<'_>,
    data: &SnapshotNodeData<'_>,
    placement: Point,
    wrapping: Wrapping,
    position: Position,
) -> bool {
    let state = &data.layout_state;
    let slices = &data.modifier_slices;
    (position == Position::Moved || state.position() == placement)
        && Rect::from_size(state.size()) == layer.node_rect()
        && slices.layer_bounds(state.size()) == layer.local_bounds()
        && state.content_offset() == layer.content_offset()
        && (slices.outer_draw_command_count() > 0) == (wrapping == Wrapping::Wrapped)
}

fn update_layer_properties(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    container: RecordId,
    level: Level,
    position: Position,
    ancestors: TranslateAncestorContext,
) -> Option<GraphChanges> {
    let layer = store.layer(container);
    let node_id = layer_identity(layer)?;
    let wrapping = if layer.kind() == RecordKind::Wrapper {
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
    let placement = layer.origin_in_parent();
    let changes = read_placed_node_data(applier, node_id, level, |data| {
        let target = content_layer(store, container)?;
        let slices = &data.modifier_slices;
        let state = &data.layout_state;
        if !retained_layer_matches_layout(store.layer(target), &data, placement, wrapping, position)
        {
            return None;
        }
        let context = LowerContext::beneath(ancestors);
        let previous = HitGraphState::of(store.layer(target));
        let (head, child_context) = prepare_node_layer(node_id, slices, state, context);
        assign_layer(store, target, head);
        if wrapped {
            let layer = store.layer(target);
            let transform = layer_transform_to_parent(
                layer.local_bounds(),
                Point::default(),
                layer.graphics_layer(),
            );
            store.set_placement(target, Point::default(), Some(transform));
        }
        let layer = store.layer(target);
        if layer.has_origin_sinks()
            && let Some(child_abs) = child_context.parent_abs
        {
            for child in layer.children() {
                publish_retained_window_geometry(applier, child, child_abs);
            }
        }
        Some(previous.changes_against(&HitGraphState::of(layer)))
    })??;
    if wrapped {
        store.note_property_change(container);
    }
    Some(changes)
}

pub(crate) fn publish_space(applier: &MemoryApplier, store: &SceneStore) {
    if !crate::thread_state::with(|state| state.space_changed.replace(false)) {
        return;
    }
    publish_space_now(applier, store);
}

#[cold]
#[inline(never)]
fn publish_space_now(applier: &MemoryApplier, store: &SceneStore) {
    let Some(root_layer) = store.root_layer() else {
        return;
    };
    let root = ProjectiveTransform::identity();
    crate::space::for_each_window_eye(root_layer, root, &mut |tree, view, to_window| {
        if let Some(eye) = &tree.cells[0].scene.eye {
            eye.world.rebuild(|world| view.build_world(tree, world));
        }
        for seen in view
            .surfaces
            .iter()
            .filter(|seen| seen.layer.has_origin_sinks())
        {
            if let Some(content) = view.content_to_window(seen, to_window) {
                publish_content_geometry(applier, seen.layer, content);
            }
        }
    });
}

fn publish_content_geometry(
    applier: &MemoryApplier,
    layer: LayerRef<'_>,
    to_window: ProjectiveTransform,
) {
    if let Some(node_id) = layer.node_id() {
        read_node_data(applier, node_id, |data| {
            data.modifier_slices.publish_window_geometry(
                Point::default(),
                to_window,
                data.layout_state.size(),
            );
        });
    }
    for child in layer.children() {
        if child.has_origin_sinks() && child.graphics_layer().space.is_none() {
            publish_content_geometry(applier, child, child.transform_to_parent().then(to_window));
        }
    }
}

fn publish_retained_window_geometry(
    applier: &MemoryApplier,
    layer: LayerRef<'_>,
    parent_abs: AbsOrigin,
) {
    if !layer.has_origin_sinks() {
        return;
    }
    let child_abs = parent_abs.children_of(layer);
    if let Some(node_id) = layer.node_id() {
        let origin = layer.origin_in_parent();
        read_node_data(applier, node_id, |data| {
            data.modifier_slices.publish_window_geometry(
                Point {
                    x: parent_abs.content_origin.x + origin.x,
                    y: parent_abs.content_origin.y + origin.y,
                },
                child_abs.window_transform,
                data.layout_state.size(),
            );
        });
    }
    for child in layer.children() {
        publish_retained_window_geometry(applier, child, child_abs);
    }
}

#[cold]
#[inline(never)]
fn note_space_change() {
    crate::thread_state::with(|state| state.space_changed.set(true));
}

fn release_for_rebuild(store: &mut SceneStore, record: RecordId) {
    store.clear_items(record);
    store.set_hit_test(record, None);
}

fn forget_dirty_subtree(
    store: &SceneStore,
    record: RecordId,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) {
    if let Some(node_id) = layer_identity(store.layer(record)) {
        dirty_nodes.remove(&node_id);
    }
    remove_dirty_descendants(store, record, dirty_nodes);
}

// Takes the node of `item`, an item of a record, and every node beneath it
// out of `dirty_nodes`.
fn forget_dirty_node(
    store: &SceneStore,
    item: Item,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) {
    if let Some(record) = item.as_child() {
        forget_dirty_subtree(store, record, dirty_nodes);
    }
}

fn remove_dirty_descendants(
    store: &SceneStore,
    record: RecordId,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) {
    for child in store.layer(record).children() {
        if let Some(node_id) = child.node_id() {
            dirty_nodes.remove(&node_id);
        }
        remove_dirty_descendants(store, child.id(), dirty_nodes);
    }
}

// The node an item of a record draws: a child record's, `None` for the
// record's own draws and draw groups.
#[inline]
fn child_identity(store: &SceneStore, item: Item) -> Option<NodeId> {
    item.as_child()
        .and_then(|child| layer_identity(store.layer(child)))
}

#[cfg(test)]
#[path = "tests/scene_builder_tests.rs"]
mod tests;
