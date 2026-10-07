use std::{any::Any, cell::Cell, rc::Rc};

use cranpose_core::{
    MemoryApplier, NodeId,
    collections::map::{HashMap, HashSet},
};
use cranpose_ui::{
    DrawCommand, LayoutBox, LayoutNode, ModifierNodeSlices, Point, PreparedTextLayout, Rect, Size,
    SubcomposeLayoutNode, TextLayoutOptions, TextOverflow, TextPanResolver, text::TextStyle,
};
use cranpose_ui_graphics::{
    CommandRecording, CompositingStrategy, GraphicsLayer, LayerShape, RoundedCornerShape,
    rounded_corner_alpha_mask_effect,
};

use crate::{
    SceneUpdates,
    graph::{
        CachePolicy, ContentChanges, DrawCommandId, DrawRunNode, HitTestNode, IsolationReasons,
        LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph,
        RenderNode, TextPrimitiveNode,
    },
    layer_transform::{
        layer_scales_or_rotates, layer_transform_to_parent, layer_transform_to_window,
    },
    style_shared::{DrawPlacement, recording_for_placement_reusing},
    text_paint::{TextPaint, TextPaintCache},
};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphUpdate {
    Patched,
    NeedsRebuild(GraphRebuildReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphUpdateReport {
    pub update: GraphUpdate,
    pub hit_graph_dirty: bool,
}

impl GraphUpdateReport {
    pub fn applied(self) -> bool {
        matches!(self.update, GraphUpdate::Patched)
    }

    pub fn rebuild_reason(self) -> Option<GraphRebuildReason> {
        match self.update {
            GraphUpdate::Patched => None,
            GraphUpdate::NeedsRebuild(reason) => Some(reason),
        }
    }
}

#[cfg(test)]
thread_local! {
    static LOWERED_LAYER_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn note_layer_lowered() {
    #[cfg(test)]
    LOWERED_LAYER_COUNT.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
fn reset_lowered_layer_count() {
    LOWERED_LAYER_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
fn lowered_layer_count() -> usize {
    LOWERED_LAYER_COUNT.with(Cell::get)
}

pub fn build_graph_from_layout_tree(root: &LayoutBox, _scale: f32) -> RenderGraph {
    bump_recording_generation();
    let root_snapshot = layout_box_to_snapshot(root, None);
    let mut graph = RenderGraph::new(LayerNode::default());
    write_snapshot_layer(root_snapshot, LowerContext::ROOT, &mut graph.root);
    graph
}

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
            graph.update += 1;
            graph.root.note_content_change(graph.update);
            lower_root_into(applier, root, &mut graph.root).then_some(graph)
        }
        None => build_graph_from_applier(applier, root, scale),
    };
    crate::layer_recycling::release();
    graph
}

pub fn update_graph_from_applier(
    applier: &MemoryApplier,
    graph: &mut RenderGraph,
    updates: SceneUpdates<'_>,
    scale: f32,
) -> bool {
    update_graph_from_applier_report(applier, graph, updates, scale).applied()
}

pub fn update_graph_from_applier_report(
    applier: &MemoryApplier,
    graph: &mut RenderGraph,
    updates: SceneUpdates<'_>,
    _scale: f32,
) -> GraphUpdateReport {
    let report = update_graph_from_applier_report_inner(applier, graph, updates);
    crate::layer_recycling::release();
    if let GraphUpdate::NeedsRebuild(reason) = report.update
        && cranpose_core::env_flag!("CRANPOSE_SCENE_UPDATE_DIAG")
    {
        eprintln!(
            "[scene-update-diag] scoped update abandoned, whole scene rebuilt: {reason:?} dirty={}",
            updates.content.len() + updates.layers.len() + updates.moved.len()
        );
    }
    report
}

#[derive(Clone, Copy)]
enum NodeUpdate {
    Content,
    Layer,
    /// The node moved in its parent and kept its size: its layer moves and
    /// keeps what it drew.
    Moved,
    /// The node moved and its layer properties changed: its layer moves,
    /// takes the new properties and keeps what it drew.
    MovedLayer,
}

/// Each dirty node with the update it needs: content wins over the others,
/// and a moved node whose layer properties changed too moves with them.
fn dirty_node_updates(updates: SceneUpdates<'_>) -> HashMap<NodeId, NodeUpdate> {
    let mut dirty = HashMap::with_capacity_and_hasher(
        updates.moved.len() + updates.layers.len() + updates.content.len(),
        Default::default(),
    );
    dirty.extend(updates.moved.iter().map(|&id| (id, NodeUpdate::Moved)));
    for &id in updates.layers {
        let kind = match dirty.get(&id) {
            Some(NodeUpdate::Moved) => NodeUpdate::MovedLayer,
            _ => NodeUpdate::Layer,
        };
        dirty.insert(id, kind);
    }
    dirty.extend(updates.content.iter().map(|&id| (id, NodeUpdate::Content)));
    dirty
}

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

    if cranpose_core::env_flag!("CRANPOSE_SCENE_UPDATE_DIAG") {
        eprintln!("[scene-update-diag] dirty={updates:?}");
    }

    let mut remaining_dirty_nodes = dirty_node_updates(updates);
    if let Some(root_id) = layer_identity(&graph.root)
        && let Some(kind) = remaining_dirty_nodes.remove(&root_id)
    {
        if let Some(hit_graph_dirty) = try_update_retained_layer(
            applier,
            &mut graph.root,
            &mut remaining_dirty_nodes,
            kind,
            true,
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
            let inherited = graph.root.translated_content_context;
            let root_children = AbsOrigin::ROOT.children_of(&graph.root);
            let ancestry = dirty_ancestry(applier, &remaining_dirty_nodes);
            let walked = replace_dirty_layers_from_applier(
                applier,
                &mut graph.root,
                root_children,
                DirtyWalk {
                    nodes: &mut remaining_dirty_nodes,
                    ancestry: &ancestry,
                },
                inherited,
                update,
            );
            return GraphUpdateReport {
                update: classify_walk(applier, walked.is_some(), &mut remaining_dirty_nodes),
                hit_graph_dirty: hit_graph_dirty || walked.is_none_or(|r| r.hit_graph_dirty),
            };
        }

        let previous = HitGraphState::of(&graph.root);
        release_for_rebuild(&mut graph.root);
        if !lower_root_into(applier, root_id, &mut graph.root) {
            return GraphUpdateReport {
                update: GraphUpdate::NeedsRebuild(GraphRebuildReason::RootLayerUnavailable),
                hit_graph_dirty: true,
            };
        }
        let hit_graph_dirty = previous.dirty_against(&HitGraphState::of(&graph.root));
        graph.root.note_content_change(update);

        return GraphUpdateReport {
            update: GraphUpdate::Patched,
            hit_graph_dirty,
        };
    }

    let inherited_translated_content_context = graph.root.translated_content_context;
    let root_children = AbsOrigin::ROOT.children_of(&graph.root);
    let ancestry = dirty_ancestry(applier, &remaining_dirty_nodes);
    let Some(report) = replace_dirty_layers_from_applier(
        applier,
        &mut graph.root,
        root_children,
        DirtyWalk {
            nodes: &mut remaining_dirty_nodes,
            ancestry: &ancestry,
        },
        inherited_translated_content_context,
        update,
    ) else {
        return GraphUpdateReport {
            update: GraphUpdate::NeedsRebuild(GraphRebuildReason::DirtyLayerUnavailable),
            hit_graph_dirty: true,
        };
    };

    match classify_walk(applier, true, &mut remaining_dirty_nodes) {
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
    ancestry: &'a HashSet<NodeId>,
}

impl DirtyWalk<'_> {
    fn reborrow(&mut self) -> DirtyWalk<'_> {
        DirtyWalk {
            nodes: self.nodes,
            ancestry: self.ancestry,
        }
    }

    /// Whether the subtree of a layer with `identity` may hold a dirty
    /// layer: always for a layer without a node.
    fn may_hold(&self, identity: Option<NodeId>) -> bool {
        identity.is_none_or(|id| self.ancestry.contains(&id))
    }
}

/// The applier ancestors of every node in `dirty`.
fn dirty_ancestry(applier: &MemoryApplier, dirty: &HashMap<NodeId, NodeUpdate>) -> HashSet<NodeId> {
    let mut ancestry = HashSet::default();
    for &node in dirty.keys() {
        let mut current = node;
        while let Some(parent) = applier
            .get_ref(current)
            .ok()
            .and_then(cranpose_core::Node::parent)
        {
            if !ancestry.insert(parent) {
                break;
            }
            current = parent;
        }
    }
    ancestry
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
    let mut report = ReplaceDirtyLayersReport::default();

    for child in &mut parent.children {
        if dirty.nodes.is_empty() {
            break;
        }
        let RenderNode::Layer(child_layer) = child else {
            continue;
        };

        let identity = layer_identity(child_layer);
        if let Some(kind) = identity.and_then(|id| dirty.nodes.remove(&id)) {
            if let Some(hit_graph_dirty) = try_update_retained_layer(
                applier,
                child_layer,
                dirty.nodes,
                kind,
                false,
                TranslateAncestorContext {
                    inherited_motion_context_animated: parent.motion_context_animated,
                    update,
                    inherited_translated_content_context:
                        child_inherited_translated_content_context,
                    parent_content_offset: parent.content_offset,
                    parent_abs: parent_children,
                },
            ) {
                report.hit_graph_dirty |= hit_graph_dirty;
                report.updated = true;
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
                continue;
            }
            let node_id = layer_identity(child_layer).expect("dirty layer must have a node id");

            let previous = HitGraphState::of(child_layer);
            // Nodes that leave this subtree are gone from the scene: nothing
            // remains to update for them, and a dirty one would otherwise go
            // unmatched and rebuild the whole scene.
            remove_dirty_descendants(child_layer, dirty.nodes);
            release_for_rebuild(child_layer);
            let context = LowerContext {
                inherited_motion_context_animated: parent.motion_context_animated,
                inherited_translated_content_context: child_inherited_translated_content_context,
                parent_abs: Some(parent_children),
                parent_content_offset: parent.content_offset,
            };
            read_placed_node_data(applier, node_id, false, |data| {
                write_node_layer(applier, node_id, data, context, child_layer);
            })?;
            report.hit_graph_dirty |= previous.dirty_against(&HitGraphState::of(child_layer));
            remove_dirty_descendants(child_layer, dirty.nodes);

            child_layer.note_content_change(update);
            report.updated = true;
            continue;
        }

        if !dirty.may_hold(identity) {
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
        parent.refresh_child_facts();
        // A layer's own sinks are not kept apart from its children's, so a
        // child that stopped publishing leaves the flag set, which only keeps
        // the scroll fast path off; a child that started must set it.
        parent.has_origin_sinks |= children_have_origin_sinks(&parent.children);
        parent.note_content_change(update);
    }

    Some(report)
}

fn translate_bail(reason: &str) -> bool {
    if cranpose_core::env_flag!("CRANPOSE_SCENE_UPDATE_DIAG") {
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

fn try_update_retained_layer(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    kind: NodeUpdate,
    root: bool,
    ancestors: TranslateAncestorContext,
) -> Option<bool> {
    match kind {
        NodeUpdate::Moved if !root => {
            move_retained_layer(applier, layer, ancestors.parent_content_offset).or_else(|| {
                try_translate_scrolled_layer(applier, layer, dirty_nodes, ancestors).then_some(true)
            })
        }
        NodeUpdate::Content | NodeUpdate::Moved => {
            try_translate_scrolled_layer(applier, layer, dirty_nodes, ancestors).then_some(true)
        }
        NodeUpdate::Layer => update_layer_properties(applier, layer, root, false, ancestors),
        NodeUpdate::MovedLayer => update_layer_properties(applier, layer, root, true, ancestors),
    }
}

/// Moves the layer of a node that only moved in its parent, keeping what it
/// drew, and returns whether hit geometry changed. `None` when the layer
/// cannot keep its content: its size changed, or its subtree publishes
/// window origins.
fn move_retained_layer(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    parent_content_offset: Point,
) -> Option<bool> {
    if layer.has_origin_sinks {
        return None;
    }
    let state = scene_layout_state(applier, layer_identity(layer)?)?;
    if !state.is_placed() || Rect::from_size(state.size()) != layer.node_rect() {
        return None;
    }
    let previous = HitGraphState::of(layer);
    translate_retained_child(layer, &state, parent_content_offset);
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
/// can update it in place: at `placement`, unless the node `moved`.
fn retained_layer_matches_layout(
    layer: &LayerNode,
    data: &SnapshotNodeData<'_>,
    placement: Point,
    wrapped: bool,
    moved: bool,
) -> bool {
    let state = &data.layout_state;
    let slices = &data.modifier_slices;
    (moved || state.position() == placement)
        && Rect::from_size(state.size()) == layer.node_rect()
        && slices.layer_bounds(state.size()) == layer.local_bounds
        && state.content_offset() == layer.content_offset
        && (slices.outer_draw_command_count() > 0) == wrapped
}

fn update_layer_properties(
    applier: &MemoryApplier,
    container: &mut LayerNode,
    root: bool,
    moved: bool,
    ancestors: TranslateAncestorContext,
) -> Option<bool> {
    let node_id = layer_identity(container)?;
    let wrapped = container.wraps.is_some();
    // The wrapper of a node's outer draws holds its placement; a moved
    // wrapped node is drawn again.
    if moved && wrapped {
        return None;
    }
    let placement = container.origin_in_parent;
    let hit_graph_dirty = read_placed_node_data(applier, node_id, root, |data| {
        let target = content_layer_mut(container)?;
        let slices = &data.modifier_slices;
        let state = &data.layout_state;
        if !retained_layer_matches_layout(target, &data, placement, wrapped, moved) {
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
        let outer = outer_draws(node_id, slices.draw_commands(), outer_count, size)
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

/// What a container keeps its children under: its node, its graphics layer
/// and its bounds at its current size.
struct TranslatedContainer {
    node_id: NodeId,
    graphics_layer: Option<GraphicsLayer>,
    local_bounds: Rect,
    node_bounds: Option<Rect>,
}

fn translated_container(
    container: &LayerNode,
    layout_state: &cranpose_ui::widgets::LayoutState,
    modifier_slices: &ModifierNodeSlices,
    inherited_motion_context_animated: bool,
    wrapped: bool,
) -> Result<TranslatedContainer, &'static str> {
    if cranpose_core::env_flag!("CRANPOSE_DISABLE_SCROLL_TRANSLATE") {
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
    let shaped = |bounds: Rect| {
        graphics_layer_with_shaped_clip(
            modifier_slices.graphics_layer(),
            clip_to_bounds,
            modifier_slices.corner_shape(),
            bounds,
        )
    };
    if shaped(container.local_bounds)
        .as_ref()
        .unwrap_or(&GraphicsLayer::DEFAULT)
        != &*container.graphics_layer
    {
        return Err("container graphics layer changed");
    }
    let (local_bounds, node_bounds) = layer_and_node_bounds(modifier_slices, layout_state.size());
    Ok(TranslatedContainer {
        node_id,
        graphics_layer: shaped(local_bounds),
        local_bounds,
        node_bounds,
    })
}

/// What a scroll step reconciles a container's children in, kept between
/// steps so a step allocates nothing of its own. Every list is empty
/// between steps, so it holds no layer.
#[derive(Default)]
struct TranslateScratch {
    /// The placed children, in their new order.
    placed_fresh: Vec<(NodeId, cranpose_ui::widgets::LayoutState)>,
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

thread_local! {
    static TRANSLATE_SCRATCH: Cell<TranslateScratch> = Cell::new(TranslateScratch::default());
}

/// Fills `scratch` with `container`'s placed children and the index of its
/// previous ones, and returns whether they are the same children in the
/// same order.
fn translated_children(
    applier: &MemoryApplier,
    container: &LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    fresh_children: &[NodeId],
    scratch: &mut TranslateScratch,
) -> Result<bool, &'static str> {
    let placed_fresh = &mut scratch.placed_fresh;
    placed_fresh.clear();
    for child_id in fresh_children {
        let Some(state) = scene_layout_state(applier, *child_id) else {
            continue;
        };
        if !state.is_placed() {
            continue;
        }
        placed_fresh.push((*child_id, state));
    }
    let children_unchanged = container.children.len() == placed_fresh.len()
        && container
        .children
        .iter()
        .zip(placed_fresh.iter())
        .all(|(child, (id, _))| {
            matches!(child, RenderNode::Layer(layer) if layer_identity(layer) == Some(*id))
        });
    let old_index_by_id = &mut scratch.old_index_by_id;
    old_index_by_id.clear();
    if !children_unchanged {
        for (index, child) in container.children.iter().enumerate() {
            let RenderNode::Layer(layer) = child else {
                return Err("child without node id");
            };
            let Some(id) = layer_identity(layer) else {
                return Err("child without node id");
            };
            old_index_by_id.insert(id, index);
        }
    }
    check_retained_children(
        container,
        dirty_nodes,
        &scratch.placed_fresh,
        children_unchanged,
        &scratch.old_index_by_id,
    )?;
    Ok(children_unchanged)
}

/// Checks that every child kept as it is still fits its layer. A moved child
/// that no longer fits is drawn again: it becomes a content update.
fn check_retained_children(
    container: &LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    placed_fresh: &[(NodeId, cranpose_ui::widgets::LayoutState)],
    children_unchanged: bool,
    old_index_by_id: &HashMap<NodeId, usize>,
) -> Result<(), &'static str> {
    for (fresh_index, (child_id, state)) in placed_fresh.iter().enumerate() {
        let old_index = if children_unchanged {
            fresh_index
        } else if let Some(index) = old_index_by_id.get(child_id) {
            *index
        } else {
            continue;
        };
        let RenderNode::Layer(layer) = &container.children[old_index] else {
            return Err("retained child slot is not a layer");
        };
        let fits = !layer.has_origin_sinks && Rect::from_size(state.size()) == layer.node_rect();
        match dirty_nodes.get_mut(child_id) {
            Some(NodeUpdate::Content | NodeUpdate::Layer) => {}
            Some(kind @ (NodeUpdate::Moved | NodeUpdate::MovedLayer)) => {
                if !fits {
                    *kind = NodeUpdate::Content;
                }
            }
            None if layer.has_origin_sinks => {
                return Err("child subtree publishes window origins");
            }
            None if !fits => return Err("child resized"),
            None => {}
        }
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
        old_index_by_id,
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
    kept.extend(placed_fresh.iter().map(|(child_id, _)| {
        old_index_by_id
            .get(child_id)
            .and_then(|index| retained[*index].take())
    }));
    for leaving in retained.drain(..).flatten() {
        forget_dirty_subtree(&leaving, dirty_nodes);
        crate::layer_recycling::recycle(leaving);
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
    for (child_id, _) in &scratch.placed_fresh {
        if scratch.old_index_by_id.contains_key(child_id) {
            continue;
        }
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
        && modifier_slices.annotated_text().is_none()
    {
        return;
    }
    let mut layers = std::mem::replace(
        &mut container.children,
        crate::layer_recycling::child_list(0),
    );
    write_node_content(
        &mut container.children,
        node_id,
        modifier_slices,
        size,
        layers.len(),
        |list| list.append(&mut layers),
    );
    crate::layer_recycling::recycle_list(layers);
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
        &container.graphics_layer,
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
    container: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    scratch: &mut TranslateScratch,
    children_unchanged: bool,
    geometry: TranslateGeometry,
) {
    if children_unchanged {
        for (child, (child_id, state)) in container.children.iter_mut().zip(&scratch.placed_fresh) {
            let RenderNode::Layer(layer) = child else {
                unreachable!("retained child identities were checked");
            };
            if keeps_retained_child(dirty_nodes, *child_id) {
                translate_retained_child(layer, state, geometry.content_offset);
            }
        }
        return;
    }
    let mut entering = scratch.entering.drain(..).peekable();
    for ((child_id, state), kept) in scratch.placed_fresh.iter().zip(scratch.kept.drain(..)) {
        if let Some(mut layer) = kept {
            if keeps_retained_child(dirty_nodes, *child_id) {
                translate_retained_child(&mut layer, state, geometry.content_offset);
            }
            container.children.push(RenderNode::Layer(layer));
        } else if let Some((_, lowered)) = entering.next_if(|(id, _)| id == child_id) {
            dirty_nodes.remove(child_id);
            remove_dirty_descendants(&lowered, dirty_nodes);

            container.children.push(RenderNode::Layer(lowered));
        }
    }
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
        graphics_layer,
        local_bounds,
        node_bounds,
    } = container_plan;
    if container.local_bounds != local_bounds {
        container.local_bounds = local_bounds;
        container.graphics_layer.replace(graphics_layer);
    }
    container.node_bounds = node_bounds;
    // The container's own draws and text are recorded again around the
    // children it keeps: an unchanged draw reuses its recording, while
    // building the container anew lowered every child layer beneath it too.
    crate::layer_recycling::recycle_primitives(container);
    let mut scratch = TRANSLATE_SCRATCH.take();
    let children_unchanged = match translated_children(
        applier,
        container,
        dirty_nodes,
        fresh_children,
        &mut scratch,
    ) {
        Ok(unchanged) => unchanged,
        Err(reason) => {
            TRANSLATE_SCRATCH.set(scratch);
            return translate_bail(reason);
        }
    };

    let geometry = TranslateGeometry::new(
        &layout_state,
        &container.graphics_layer,
        container.local_bounds,
        parent_abs,
    );
    let child_inherited_translated_content_context =
        inherited_translated_content_context || container.translated_content_context;
    if !children_unchanged {
        recycle_leaving_children(container, &mut scratch, dirty_nodes);
        build_entering_children(
            applier,
            container,
            &mut scratch,
            dirty_nodes,
            geometry,
            (child_inherited_translated_content_context, update),
        );
    }

    apply_translated_container_state(
        container,
        &modifier_slices,
        &layout_state,
        parent_content_offset,
        geometry,
    );

    reconcile_translated_children(
        container,
        dirty_nodes,
        &mut scratch,
        children_unchanged,
        geometry,
    );
    TRANSLATE_SCRATCH.set(scratch);
    write_own_content_around_children(container, node_id, &modifier_slices, layout_state.size());
    container.hit_test = hit_test_from_slices(&modifier_slices);

    container.has_origin_sinks = modifier_slices_have_origin_sinks(&modifier_slices)
        || children_have_origin_sinks(&container.children);
    container.refresh_child_facts();
    container.note_content_change(update);

    true
}

/// Whether a retained child keeps its layer as it is and only follows its
/// placement: it is not dirty, or it only moved, which this placement
/// settles.
fn keeps_retained_child(dirty_nodes: &mut HashMap<NodeId, NodeUpdate>, child_id: NodeId) -> bool {
    match dirty_nodes.get(&child_id) {
        None => true,
        Some(NodeUpdate::Moved) => {
            dirty_nodes.remove(&child_id);
            true
        }
        Some(NodeUpdate::Content | NodeUpdate::Layer | NodeUpdate::MovedLayer) => false,
    }
}

fn translate_retained_child(
    layer: &mut LayerNode,
    state: &cranpose_ui::widgets::LayoutState,
    content_offset: Point,
) {
    layer.transform_to_parent = placed_transform(
        layer.local_bounds,
        state.position(),
        &layer.graphics_layer,
        content_offset,
    );
    layer.origin_in_parent = state.position();
}

#[derive(PartialEq)]
struct HitGraphState {
    hit_test: bool,
    has_hit_targets: bool,
    local_bounds: Rect,
    node_bounds: Option<Rect>,
    transform_to_parent: ProjectiveTransform,
    clip_rect: Option<Rect>,
    shape: LayerShape,
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
        }
    }

    fn dirty_against(&self, replacement: &Self) -> bool {
        self.hit_test
            || replacement.hit_test
            || ((self.has_hit_targets || replacement.has_hit_targets) && self != replacement)
    }
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
            properties,
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
        has_origin_sinks,
        draws_within_bounds,
        isolation,
        cache_policy,
        cache_hashes,
        content_changes,
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
    *clip_to_bounds = head.clip_to_bounds;
    *hit_test = head.hit_test;
    *has_hit_targets = false;
    *has_origin_sinks = head.has_origin_sinks || children_have_origin_sinks(children);
    *draws_within_bounds = false;
    *isolation = head.isolation;
    *cache_policy = head.cache_policy;
    cache_hashes.set(None);
    layer.refresh_child_facts();
}

fn children_have_origin_sinks(children: &[RenderNode]) -> bool {
    children.iter().any(|child| match child {
        RenderNode::Layer(child_layer) => child_layer.has_origin_sinks,
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
    })
}

fn placed_transform(
    local_bounds: Rect,
    placement: Point,
    graphics_layer: &GraphicsLayer,
    parent_content_offset: Point,
) -> ProjectiveTransform {
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

fn write_node_content(
    list: &mut Vec<RenderNode>,
    node_id: NodeId,
    slices: &ModifierNodeSlices,
    size: Size,
    child_count: usize,
    write_children: impl FnOnce(&mut Vec<RenderNode>),
) {
    let outer_count = slices.outer_draw_command_count();
    let commands = &slices.draw_commands()[outer_count..];
    list.reserve(layer_node_capacity(
        commands,
        child_count,
        slices.annotated_text().is_some(),
    ));
    let first_draw = list.len();
    append_draw_nodes(
        list,
        node_id,
        commands,
        outer_count,
        DrawPass::Record(DrawPlacement::Behind),
        size,
        PrimitivePhase::BeforeChildren,
    );
    if let Some(text) = text_node_from_parts(TextNodeParts {
        node_id,
        text_rect: slices.text_content_rect(size),
        text_style: slices.text_style(),
        text_layout_options: slices.text_layout_options(),
        text_pan: slices.text_pan_resolver(),
        measured_layout: slices.measured_text_layout(),
    }) {
        list.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(crate::layer_recycling::boxed_text(text)),
        }));
    }
    write_children(list);
    append_draw_nodes(
        list,
        node_id,
        commands,
        outer_count,
        DrawPass::OverlayFromOutput(first_draw),
        size,
        PrimitivePhase::AfterChildren,
    );
}

fn write_layer_with_outer(
    target: &mut LayerNode,
    head: LayerHead,
    outer: Option<OuterDraws>,
    placement: Point,
    parent_content_offset: Point,
    write_children: impl FnOnce(&mut Vec<RenderNode>),
) {
    debug_assert!(target.children.is_empty(), "a layer is written emptied");
    let Some(outer) = outer else {
        write_children(&mut target.children);
        assign_layer(target, head);
        return;
    };
    let mut layer = crate::layer_recycling::layer_box();
    write_children(&mut layer.children);
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
    let outer = outer_draws(
        node_id,
        slices.draw_commands(),
        slices.outer_draw_command_count(),
        size,
    );
    write_layer_with_outer(
        target,
        head,
        outer,
        placement,
        context.parent_content_offset,
        |list| {
            write_node_content(list, node_id, &slices, size, children.len(), |list| {
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
    const ROOT: AbsOrigin = AbsOrigin {
        content_origin: Point { x: 0.0, y: 0.0 },
        window_transform: ProjectiveTransform::identity(),
    };

    fn children_of(self, layer: &LayerNode) -> AbsOrigin {
        let top_left = Point {
            x: self.content_origin.x + layer.origin_in_parent.x,
            y: self.content_origin.y + layer.origin_in_parent.y,
        };
        AbsOrigin {
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
    root: bool,
    read: impl FnOnce(SnapshotNodeData<'_>) -> R,
) -> Option<R> {
    read_node_data(applier, node_id, |mut data| {
        if data.window_root {
            if !root {
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
    read_placed_node_data(applier, node_id, true, |data| {
        write_node_layer(applier, node_id, data, LowerContext::ROOT, target);
    })
    .is_some()
}

fn lower_child(
    applier: &MemoryApplier,
    node_id: NodeId,
    context: LowerContext,
) -> Option<Box<LayerNode>> {
    read_placed_node_data(applier, node_id, false, |data| {
        let mut layer = crate::layer_recycling::layer_box();
        write_node_layer(applier, node_id, data, context, &mut layer);
        layer
    })
}

fn scene_layout_state(
    applier: &MemoryApplier,
    node_id: NodeId,
) -> Option<cranpose_ui::widgets::LayoutState> {
    let node: &dyn Any = applier.get_ref(node_id).ok()?;
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        return Some(node.layout_state());
    }
    node.downcast_ref::<SubcomposeLayoutNode>()
        .map(SubcomposeLayoutNode::layout_state)
}

fn read_node_data<R>(
    applier: &MemoryApplier,
    node_id: NodeId,
    read: impl FnOnce(SnapshotNodeData<'_>) -> R,
) -> Option<R> {
    let node: &dyn Any = applier.get_ref(node_id).ok()?;
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        return Some(read(SnapshotNodeData {
            layout_state: node.layout_state(),
            modifier_slices: node.modifier_slices_snapshot(),
            children: &node.children,
            window_root: node.is_window_root(),
        }));
    }
    let node = node.downcast_ref::<SubcomposeLayoutNode>()?;
    let layout_state = node.layout_state();
    let modifier_slices = node.modifier_slices_snapshot();
    Some(node.with_active_children(|children| {
        read(SnapshotNodeData {
            layout_state,
            modifier_slices,
            children,
            window_root: false,
        })
    }))
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
    let graphics_layer = graphics_layer_with_shaped_clip(
        slices.graphics_layer(),
        slices.clip_to_bounds(),
        slices.corner_shape(),
        local_bounds,
    );
    let geometry = context.parent_abs.map(|parent_abs| {
        TranslateGeometry::new(
            layout_state,
            graphics_layer.as_ref().unwrap_or(&GraphicsLayer::DEFAULT),
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
        graphics_layer,
        context,
    );
    (head, child_context)
}

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
    if cranpose_core::env_flag!("CRANPOSE_SCENE_UPDATE_DIAG") {
        eprintln!(
            "[scene-update-diag] build layer node={node_id:?} size=({:.2},{:.2}) pos=({:.2},{:.2})",
            size.width, size.height, placement.x, placement.y,
        );
    }
    let (head, child_context) = prepare_node_layer(node_id, &slices, &layout_state, context);
    let outer = outer_draws(
        node_id,
        slices.draw_commands(),
        slices.outer_draw_command_count(),
        size,
    );
    write_layer_with_outer(
        target,
        head,
        outer,
        placement,
        context.parent_content_offset,
        |list| {
            write_node_content(list, node_id, &slices, size, children.len(), |list| {
                for &child_id in children {
                    if let Some(child) = lower_child(applier, child_id, child_context) {
                        list.push(RenderNode::Layer(child));
                    }
                }
            });
        },
    );
}

struct RecorderSlot {
    generation: u64,
    handles: [Option<Rc<CommandRecording>>; 3],
    /// The allocation of a handle whose recording `acquire_storage` took,
    /// which the next recording the slot publishes moves into.
    spare: Option<Rc<CommandRecording>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct RecorderKey {
    node_id: NodeId,
    command_index: u32,
}

impl From<DrawCommandId> for RecorderKey {
    fn from(id: DrawCommandId) -> Self {
        Self {
            node_id: id.node_id,
            command_index: id.command_index,
        }
    }
}

thread_local! {
    static COMMAND_RECORDINGS: std::cell::RefCell<
        std::collections::HashMap<RecorderKey, RecorderSlot, cranpose_ui_graphics::FxBuildHasher>,
    > = std::cell::RefCell::new(std::collections::HashMap::default());
    static RECORDING_GENERATION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[doc(hidden)]
pub fn clear_command_recordings_for_tests() {
    COMMAND_RECORDINGS.with(|map| map.borrow_mut().clear());
}

fn bump_recording_generation() {
    let generation = RECORDING_GENERATION.with(|cell| {
        let next = cell.get().wrapping_add(1);
        cell.set(next);
        next
    });
    if generation.is_multiple_of(512) {
        COMMAND_RECORDINGS.with(|map| {
            map.borrow_mut()
                .retain(|_, slot| generation.wrapping_sub(slot.generation) <= 64);
        });
    }
}

fn acquire_storage(id: DrawCommandId) -> CommandRecording {
    COMMAND_RECORDINGS.with(|map| {
        let mut map = map.borrow_mut();
        let Some(slot) = map.get_mut(&RecorderKey::from(id)) else {
            return CommandRecording::default();
        };
        for handle in &mut slot.handles {
            let Some(recording) = handle.as_mut().and_then(Rc::get_mut) else {
                continue;
            };
            let Some(storage) = recording.try_take_reusable() else {
                continue;
            };
            slot.spare = handle.take();
            return storage;
        }
        if slot.handles.iter().all(Option::is_some) {
            let storage = slot.handles[2]
                .as_mut()
                .and_then(Rc::get_mut)
                .map(std::mem::take);
            if let Some(storage) = storage {
                slot.spare = slot.handles[2].take();
                return storage;
            }
        }
        CommandRecording::default()
    })
}

fn publish_recording(id: DrawCommandId, recording: CommandRecording) -> Rc<CommandRecording> {
    COMMAND_RECORDINGS.with(|map| {
        let mut map = map.borrow_mut();
        let generation = RECORDING_GENERATION.with(Cell::get);
        let slot = map
            .entry(RecorderKey::from(id))
            .or_insert_with(|| RecorderSlot {
                generation,
                handles: [None, None, None],
                spare: None,
            });
        let shared = match slot.spare.take() {
            Some(mut spare) => match Rc::get_mut(&mut spare) {
                Some(storage) => {
                    *storage = recording;
                    spare
                }
                None => Rc::new(recording),
            },
            None => Rc::new(recording),
        };
        slot.generation = generation;
        slot.handles[2] = slot.handles[1].take();
        slot.handles[1] = slot.handles[0].take();
        slot.handles[0] = Some(Rc::clone(&shared));
        shared
    })
}

enum DrawPass<'a> {
    Record(DrawPlacement),
    OverlayFrom(std::slice::Iter<'a, RenderNode>),
    OverlayFromOutput(usize),
}

impl DrawPass<'_> {
    fn placement(&self) -> DrawPlacement {
        match self {
            Self::Record(placement) => *placement,
            Self::OverlayFrom(_) | Self::OverlayFromOutput(_) => DrawPlacement::Overlay,
        }
    }

    fn take_recording(
        &mut self,
        command: &DrawCommand,
        output: &[RenderNode],
    ) -> Option<Rc<CommandRecording>> {
        if matches!(command, DrawCommand::Overlay(_)) {
            return None;
        }
        let node = match self {
            Self::Record(_) => return None,
            Self::OverlayFrom(nodes) => nodes.next().expect("each behind command has a draw run"),
            Self::OverlayFromOutput(index) => {
                let node = &output[*index];
                *index += 1;
                node
            }
        };
        let RenderNode::DrawRun(run) = node else {
            unreachable!("behind commands produce draw runs");
        };
        matches!(command, DrawCommand::WithContent(_)).then(|| Rc::clone(&run.recording))
    }
}

fn layer_node_capacity(commands: &[DrawCommand], children: usize, has_text: bool) -> usize {
    children
        + usize::from(has_text)
        + commands.len()
        + commands
            .iter()
            .filter(|command| matches!(command, DrawCommand::WithContent(_)))
            .count()
}

fn draw_nodes(
    node_id: NodeId,
    commands: &[DrawCommand],
    first_command_index: usize,
    placement: DrawPlacement,
    size: Size,
    phase: PrimitivePhase,
) -> Vec<RenderNode> {
    let mut nodes = crate::layer_recycling::child_list(commands.len());
    append_draw_nodes(
        &mut nodes,
        node_id,
        commands,
        first_command_index,
        DrawPass::Record(placement),
        size,
        phase,
    );
    nodes
}

fn append_draw_nodes(
    nodes: &mut Vec<RenderNode>,
    node_id: NodeId,
    commands: &[DrawCommand],
    first_command_index: usize,
    mut pass: DrawPass<'_>,
    size: Size,
    phase: PrimitivePhase,
) {
    let placement = pass.placement();
    for (command_index, command) in commands.iter().enumerate() {
        let id = DrawCommandId {
            node_id,
            command_index: (first_command_index + command_index) as u32,
            placement,
        };
        let (shared, segments) = if let Some(shared) = pass.take_recording(command, nodes) {
            let segments = shared.content_split(false);
            (shared, segments)
        } else {
            let Some((recording, segments)) =
                recording_for_placement_reusing(command, placement, size, || acquire_storage(id))
            else {
                continue;
            };
            (publish_recording(id, recording), segments)
        };
        nodes.push(RenderNode::DrawRun(DrawRunNode::for_command_shared(
            phase,
            Some(id),
            shared,
            segments,
        )));
    }
}

#[doc(hidden)]
pub fn draw_command_nodes_for_tests(
    node_id: NodeId,
    commands: &[DrawCommand],
    placement: DrawPlacement,
    size: Size,
    phase: PrimitivePhase,
) -> Vec<RenderNode> {
    bump_recording_generation();
    draw_nodes(node_id, commands, 0, placement, size, phase)
}

struct OuterDraws {
    behind: Vec<RenderNode>,
    overlay: Vec<RenderNode>,
}

fn outer_draws(
    node_id: NodeId,
    draw_commands: &[DrawCommand],
    outer_draw_command_count: usize,
    size: Size,
) -> Option<OuterDraws> {
    (outer_draw_command_count > 0).then(|| {
        let commands = &draw_commands[..outer_draw_command_count];
        let behind = draw_nodes(
            node_id,
            commands,
            0,
            DrawPlacement::Behind,
            size,
            PrimitivePhase::BeforeChildren,
        );
        let mut overlay = crate::layer_recycling::child_list(commands.len());
        append_draw_nodes(
            &mut overlay,
            node_id,
            commands,
            0,
            DrawPass::OverlayFrom(behind.iter()),
            size,
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
        transform_to_parent: placed_transform(
            node_rect,
            placement,
            &GraphicsLayer::DEFAULT,
            parent_content_offset,
        ),
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
    };
    let OuterDraws {
        mut behind,
        mut overlay,
    } = outer;
    let children = &mut wrapper.children;
    children.reserve(behind.len() + 1 + overlay.len());
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

struct TextNodeParts<'a> {
    node_id: NodeId,
    /// Where the text node was placed in its layout node.
    text_rect: Rect,
    text_style: Option<&'a TextStyle>,
    text_layout_options: Option<TextLayoutOptions>,
    text_pan: Option<TextPanResolver>,
    measured_layout: Option<Rc<PreparedTextLayout>>,
}

fn text_node_from_parts(parts: TextNodeParts<'_>) -> Option<TextPrimitiveNode> {
    let TextNodeParts {
        node_id,
        text_rect,
        text_style,
        text_layout_options,
        text_pan,
        measured_layout,
    } = parts;
    let prepared = measured_layout?;
    let default_text_style;
    let text_style = match text_style {
        Some(style) => style,
        None => {
            default_text_style = TextStyle::default();
            &default_text_style
        }
    };
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
    let measured_draw_width = prepared.metrics.width.max(0.0);
    let draw_width = if options.overflow == TextOverflow::Visible || pans_horizontally {
        measured_draw_width
    } else {
        measured_draw_width.min(content_width)
    };
    let alignment_offset = resolve_text_horizontal_offset(
        text_style,
        prepared.text.text.as_str(),
        content_width,
        prepared.metrics.width,
    );
    let rect = Rect {
        x: text_rect.x + alignment_offset - pan_offset,
        y: text_rect.y,
        width: draw_width,
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
        text_style: std::sync::Arc::clone(visual_style),
        font_size,
        layout_options: options,
        clip,
        paint: TextPaintCache::holding(TextPaint::of(
            visual_style,
            &prepared.text.span_styles,
            font_size,
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
    let base_graphics_layer = node.node_data.modifier_slices.graphics_layer();
    let has_graphics_layer = base_graphics_layer.is_some();
    let graphics_layer = graphics_layer_with_shaped_clip(
        base_graphics_layer,
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
        graphics_layer: graphics_layer.filter(|_| has_graphics_layer),
        children,
    }
}

fn modifier_slices_have_origin_sinks(slices: &ModifierNodeSlices) -> bool {
    slices.text_window_transform().is_some() || slices.viewport_window_rect().is_some()
}

fn graphics_layer_with_shaped_clip(
    mut graphics_layer: Option<GraphicsLayer>,
    clip_to_bounds: bool,
    corner_shape: Option<RoundedCornerShape>,
    local_bounds: Rect,
) -> Option<GraphicsLayer> {
    if !clip_to_bounds {
        return graphics_layer;
    }

    let Some(corner_shape) = corner_shape else {
        return graphics_layer;
    };
    let radii = corner_shape.resolve(local_bounds.width, local_bounds.height);
    if radii.top_left <= f32::EPSILON
        && radii.top_right <= f32::EPSILON
        && radii.bottom_right <= f32::EPSILON
        && radii.bottom_left <= f32::EPSILON
    {
        return graphics_layer;
    }

    let properties = graphics_layer.get_or_insert_default();
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
    graphics_layer
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

fn resolve_text_horizontal_offset(
    text_style: &TextStyle,
    text: &str,
    content_width: f32,
    measured_width: f32,
) -> f32 {
    let remaining = (content_width - measured_width).max(0.0);
    if remaining == 0.0 {
        return 0.0;
    }
    remaining * cranpose_ui::text::text_align_fraction(text_style, text)
}

#[cfg(test)]
#[path = "tests/scene_builder_tests.rs"]
mod tests;
