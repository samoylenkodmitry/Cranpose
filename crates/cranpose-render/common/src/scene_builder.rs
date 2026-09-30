use std::{cell::Cell, rc::Rc};

use cranpose_core::{
    MemoryApplier, Node, NodeId,
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
use smallvec::SmallVec;

use crate::{
    graph::{
        CachePolicy, DrawCommandId, DrawRunNode, HitTestNode, IsolationReasons, LayerNode,
        PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph,
        RenderNode, TextPrimitiveNode,
    },
    layer_transform::{layer_scales_or_rotates, layer_transform_to_parent},
    raster_cache::LayerRasterCacheHashes,
    style_shared::{DrawPlacement, recording_for_placement_reusing},
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

struct SnapshotNodeData {
    layout_state: cranpose_ui::widgets::LayoutState,
    modifier_slices: Rc<ModifierNodeSlices>,
    children: SmallVec<[NodeId; 8]>,
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

pub fn build_graph_from_layout_tree(root: &LayoutBox, scale: f32) -> RenderGraph {
    bump_recording_generation();
    let root_snapshot = layout_box_to_snapshot(root, None);
    RenderGraph {
        root: build_layer_node(root_snapshot, scale, false),
    }
}

pub fn build_graph_from_applier(
    applier: &mut MemoryApplier,
    root: NodeId,
    scale: f32,
) -> Option<RenderGraph> {
    bump_recording_generation();
    Some(RenderGraph {
        root: build_layer_node_from_applier(applier, root, scale, false)?,
    })
}

/// Builds `root`'s graph again, its layers taking the allocations of
/// `previous`, the graph it replaces.
pub fn rebuild_graph_from_applier(
    applier: &mut MemoryApplier,
    root: NodeId,
    scale: f32,
    previous: Option<RenderGraph>,
) -> Option<RenderGraph> {
    if let Some(mut previous) = previous {
        crate::layer_recycling::recycle_children(&mut previous.root);
    }
    let graph = build_graph_from_applier(applier, root, scale);
    crate::layer_recycling::release();
    graph
}

pub fn update_graph_from_applier(
    applier: &mut MemoryApplier,
    graph: &mut RenderGraph,
    dirty_nodes: &[NodeId],
    scale: f32,
) -> bool {
    update_graph_from_applier_report(applier, graph, dirty_nodes, scale).applied()
}

pub fn update_graph_from_applier_report(
    applier: &mut MemoryApplier,
    graph: &mut RenderGraph,
    dirty_nodes: &[NodeId],
    scale: f32,
) -> GraphUpdateReport {
    let mut changed_nodes = Vec::new();
    update_graph_from_applier_report_into(applier, graph, dirty_nodes, scale, &mut changed_nodes)
}

pub fn update_graph_from_applier_report_into(
    applier: &mut MemoryApplier,
    graph: &mut RenderGraph,
    dirty_nodes: &[NodeId],
    scale: f32,
    changed_nodes: &mut Vec<NodeId>,
) -> GraphUpdateReport {
    let report = update_graph_from_applier_report_into_inner(
        applier,
        graph,
        dirty_nodes,
        scale,
        changed_nodes,
    );
    crate::layer_recycling::release();
    if let GraphUpdate::NeedsRebuild(reason) = report.update
        && cranpose_core::env_flag!("CRANPOSE_SCENE_UPDATE_DIAG")
    {
        eprintln!(
            "[scene-update-diag] scoped update abandoned, whole scene rebuilt: {reason:?} dirty={}",
            dirty_nodes.len()
        );
    }
    report
}

fn update_graph_from_applier_report_into_inner(
    applier: &mut MemoryApplier,
    graph: &mut RenderGraph,
    dirty_nodes: &[NodeId],
    scale: f32,
    changed_nodes: &mut Vec<NodeId>,
) -> GraphUpdateReport {
    if dirty_nodes.is_empty() {
        return GraphUpdateReport {
            update: GraphUpdate::Patched,
            hit_graph_dirty: false,
        };
    }
    bump_recording_generation();

    if cranpose_core::env_flag!("CRANPOSE_SCENE_UPDATE_DIAG") {
        eprintln!("[scene-update-diag] dirty={dirty_nodes:?}");
    }

    let mut remaining_dirty_nodes = dirty_nodes.iter().copied().collect::<HashSet<_>>();
    if let Some(root_id) = layer_identity(&graph.root)
        && remaining_dirty_nodes.contains(&root_id)
    {
        remaining_dirty_nodes.remove(&root_id);
        if try_translate_scrolled_layer(
            applier,
            &mut graph.root,
            &mut remaining_dirty_nodes,
            changed_nodes,
            TranslateAncestorContext {
                inherited_motion_context_animated: false,
                ancestor_hashed: false,
                inherited_translated_content_context: false,
                parent_content_offset: Point::default(),
                parent_abs: AbsOrigin::ROOT,
            },
        ) {
            if remaining_dirty_nodes.is_empty() {
                return GraphUpdateReport {
                    update: GraphUpdate::Patched,
                    hit_graph_dirty: true,
                };
            }
            let inherited = graph.root.translated_content_context;
            let root_children = AbsOrigin::ROOT.children_of(&graph.root);
            let walked = replace_dirty_layers_from_applier(
                applier,
                &mut graph.root,
                root_children,
                &mut remaining_dirty_nodes,
                inherited,
                false,
                changed_nodes,
            );
            return GraphUpdateReport {
                update: classify_walk(walked.is_some(), &remaining_dirty_nodes),
                hit_graph_dirty: true,
            };
        }
        collect_layer_node_ids(&graph.root, changed_nodes);
        crate::layer_recycling::recycle_children(&mut graph.root);
        let Some(root) = build_layer_node_from_applier(applier, root_id, scale, false) else {
            return GraphUpdateReport {
                update: GraphUpdate::NeedsRebuild(GraphRebuildReason::RootLayerUnavailable),
                hit_graph_dirty: true,
            };
        };
        let hit_graph_dirty = layer_hit_graph_state_dirty(&graph.root, &root);
        graph.root = root;
        graph.root.recompute_raster_cache_hashes();
        collect_layer_node_ids(&graph.root, changed_nodes);
        return GraphUpdateReport {
            update: GraphUpdate::Patched,
            hit_graph_dirty,
        };
    }

    let inherited_translated_content_context = graph.root.translated_content_context;
    let root_children = AbsOrigin::ROOT.children_of(&graph.root);
    let Some(report) = replace_dirty_layers_from_applier(
        applier,
        &mut graph.root,
        root_children,
        &mut remaining_dirty_nodes,
        inherited_translated_content_context,
        false,
        changed_nodes,
    ) else {
        return GraphUpdateReport {
            update: GraphUpdate::NeedsRebuild(GraphRebuildReason::DirtyLayerUnavailable),
            hit_graph_dirty: true,
        };
    };

    match classify_walk(true, &remaining_dirty_nodes) {
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

fn classify_walk(walked: bool, remaining_dirty_nodes: &HashSet<NodeId>) -> GraphUpdate {
    if !walked {
        GraphUpdate::NeedsRebuild(GraphRebuildReason::DirtyLayerUnavailable)
    } else if !remaining_dirty_nodes.is_empty() {
        GraphUpdate::NeedsRebuild(GraphRebuildReason::UnmatchedDirtyNodes(
            remaining_dirty_nodes.len(),
        ))
    } else {
        GraphUpdate::Patched
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ReplaceDirtyLayersReport {
    updated: bool,
    hit_graph_dirty: bool,
}

fn replace_dirty_layers_from_applier(
    applier: &mut MemoryApplier,
    parent: &mut LayerNode,
    parent_children: AbsOrigin,
    dirty_nodes: &mut HashSet<NodeId>,
    inherited_translated_content_context: bool,
    ancestor_hashed: bool,
    changed_nodes: &mut Vec<NodeId>,
) -> Option<ReplaceDirtyLayersReport> {
    if dirty_nodes.is_empty() {
        return Some(ReplaceDirtyLayersReport::default());
    }

    let child_inherited_translated_content_context =
        inherited_translated_content_context || parent.translated_content_context;
    let child_ancestor_hashed =
        crate::graph_hash::layer_children_ancestor_hashed(parent, ancestor_hashed);
    let mut report = ReplaceDirtyLayersReport::default();

    for child in &mut parent.children {
        let RenderNode::Layer(child_layer) = child else {
            continue;
        };

        if layer_identity(child_layer).is_some_and(|node_id| dirty_nodes.remove(&node_id)) {
            if try_translate_scrolled_layer(
                applier,
                child_layer,
                dirty_nodes,
                changed_nodes,
                TranslateAncestorContext {
                    inherited_motion_context_animated: parent.motion_context_animated,
                    ancestor_hashed: child_ancestor_hashed,
                    inherited_translated_content_context:
                        child_inherited_translated_content_context,
                    parent_content_offset: parent.content_offset,
                    parent_abs: parent_children,
                },
            ) {
                report.hit_graph_dirty = true;
                report.updated = true;
                let child_children = parent_children.children_of(child_layer);
                let child_report = replace_dirty_layers_from_applier(
                    applier,
                    child_layer,
                    child_children,
                    dirty_nodes,
                    child_inherited_translated_content_context,
                    child_ancestor_hashed,
                    changed_nodes,
                )?;
                report.hit_graph_dirty |= child_report.hit_graph_dirty;
                continue;
            }
            collect_layer_node_ids(child_layer, changed_nodes);
            crate::layer_recycling::recycle_children(child_layer);
            let mut replacement = build_layer_node_from_applier_internal(
                applier,
                layer_identity(child_layer).expect("dirty layer must have a node id"),
                parent.motion_context_animated,
                child_inherited_translated_content_context,
                Some(parent_children),
            )?;
            if parent.content_offset != Point::default() {
                replacement.transform_to_parent =
                    replacement
                        .transform_to_parent
                        .then(ProjectiveTransform::translation(
                            parent.content_offset.x,
                            parent.content_offset.y,
                        ));
            }
            report.hit_graph_dirty |= layer_hit_graph_state_dirty(child_layer, &replacement);
            remove_dirty_descendants(&replacement, dirty_nodes);
            **child_layer = replacement;
            collect_layer_node_ids(child_layer, changed_nodes);
            crate::graph_hash::recompute_layer_raster_cache_hashes_under(
                child_layer,
                child_ancestor_hashed,
            );
            report.updated = true;
            continue;
        }

        let child_children = parent_children.children_of(child_layer);
        let child_report = replace_dirty_layers_from_applier(
            applier,
            child_layer,
            child_children,
            dirty_nodes,
            child_inherited_translated_content_context,
            child_ancestor_hashed,
            changed_nodes,
        )?;
        report.updated |= child_report.updated;
        report.hit_graph_dirty |= child_report.hit_graph_dirty;
    }

    if report.updated {
        parent.draws_within_bounds = parent.content_draws_within_bounds();
        parent.has_hit_targets = parent.hit_test.is_some()
            || parent.children.iter().any(|child| match child {
                RenderNode::Layer(child_layer) => child_layer.has_hit_targets,
                RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
            });
        crate::graph_hash::refresh_layer_own_raster_cache_hashes(parent, ancestor_hashed);
        if let Some(node_id) = parent.node_id {
            changed_nodes.push(node_id);
        }
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
    ancestor_hashed: bool,
    inherited_translated_content_context: bool,
    parent_content_offset: Point,
    parent_abs: AbsOrigin,
}

fn try_translate_scrolled_layer(
    applier: &mut MemoryApplier,
    container: &mut LayerNode,
    dirty_nodes: &mut HashSet<NodeId>,
    changed_nodes: &mut Vec<NodeId>,
    ancestors: TranslateAncestorContext,
) -> bool {
    let Some(node_id) = layer_identity(container) else {
        return translate_bail("no node id");
    };
    let Some(data) = snapshot_node_data(applier, node_id) else {
        return translate_bail("container snapshot read failed");
    };
    if container.wraps.is_none() {
        return translate_layer_from_data(
            applier,
            container,
            dirty_nodes,
            changed_nodes,
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
    let inner_ancestors = TranslateAncestorContext {
        ancestor_hashed: crate::graph_hash::layer_children_ancestor_hashed(
            container,
            ancestors.ancestor_hashed,
        ),
        ..ancestors
    };
    let Some(inner) = container.children.iter_mut().find_map(|child| match child {
        RenderNode::Layer(layer) if layer.node_id == Some(node_id) => Some(layer),
        _ => None,
    }) else {
        return translate_bail("wrapped layer missing");
    };
    if !translate_layer_from_data(
        applier,
        inner,
        dirty_nodes,
        changed_nodes,
        inner_ancestors,
        data,
        true,
    ) {
        return false;
    }
    let layer = std::mem::take(inner.as_mut());
    let outer = outer_draws(node_id, slices.draw_commands(), outer_count, size)
        .expect("outer command count is nonzero");
    *container = wrap_layer_with_outer_draws(layer, placement, outer);
    if ancestors.parent_content_offset != Point::default() {
        container.transform_to_parent =
            container
                .transform_to_parent
                .then(ProjectiveTransform::translation(
                    ancestors.parent_content_offset.x,
                    ancestors.parent_content_offset.y,
                ));
    }
    for child in &mut container.children {
        if let RenderNode::Layer(layer) = child {
            crate::graph_hash::refresh_layer_own_raster_cache_hashes(
                layer,
                inner_ancestors.ancestor_hashed,
            );
        }
    }
    crate::graph_hash::refresh_layer_own_raster_cache_hashes(container, ancestors.ancestor_hashed);
    true
}

struct TranslatedContainer {
    node_id: NodeId,
    graphics_layer: GraphicsLayer,
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
    if container
        .children
        .iter()
        .any(|child| !matches!(child, RenderNode::Layer(_)))
    {
        return Err("container has own primitive children");
    }
    if !layout_state.is_placed()
        || Rect::from_size(layout_state.size()) != container.node_rect()
        || modifier_slices.layer_bounds(layout_state.size()) != container.local_bounds
    {
        return Err("container unplaced, resized or its layer moved");
    }
    let outer_count = modifier_slices.outer_draw_command_count();
    if (outer_count > 0 && !wrapped)
        || !modifier_slices.draw_commands()[outer_count..].is_empty()
        || (inherited_motion_context_animated || modifier_slices.motion_context_animated())
            != container.motion_context_animated
        || modifier_slices.annotated_text().is_some()
        || modifier_slices.translated_content_context() != container.translated_content_context
    {
        return Err("container draw/text/translated-context changed");
    }
    let clip_to_bounds = modifier_slices.clip_to_bounds();
    if clip_to_bounds != container.clip_to_bounds {
        return Err("container clip changed");
    }
    let graphics_layer = graphics_layer_with_shaped_clip(
        modifier_slices.graphics_layer().unwrap_or_default(),
        clip_to_bounds,
        modifier_slices.corner_shape(),
        container.local_bounds,
    );
    if graphics_layer != container.graphics_layer {
        return Err("container graphics layer changed");
    }
    Ok(TranslatedContainer {
        node_id,
        graphics_layer,
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
    applier: &mut MemoryApplier,
    container: &LayerNode,
    dirty_nodes: &HashSet<NodeId>,
    fresh_children: &[NodeId],
    scratch: &mut TranslateScratch,
) -> Result<bool, &'static str> {
    let placed_fresh = &mut scratch.placed_fresh;
    placed_fresh.clear();
    for child_id in fresh_children {
        let state = applier
            .with_node::<LayoutNode, _>(*child_id, |node| node.layout_state())
            .or_else(|_| {
                applier.with_node::<SubcomposeLayoutNode, _>(*child_id, |node| node.layout_state())
            });
        let Ok(state) = state else {
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

fn check_retained_children(
    container: &LayerNode,
    dirty_nodes: &HashSet<NodeId>,
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
        if dirty_nodes.contains(child_id) {
            continue;
        }
        if layer.has_origin_sinks {
            return Err("child subtree publishes window origins");
        }
        if Rect::from_size(state.size()) != layer.node_rect() {
            return Err("child resized");
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct TranslateGeometry {
    content_offset: Point,
    layer_translation: Point,
    window_origin: Point,
    child_origin: Point,
}

impl TranslateGeometry {
    fn new(
        layout_state: &cranpose_ui::widgets::LayoutState,
        graphics_layer: &GraphicsLayer,
        parent_abs: AbsOrigin,
    ) -> Self {
        let content_offset = layout_state.content_offset();
        let top_left = Point {
            x: parent_abs.content_origin.x + layout_state.position().x,
            y: parent_abs.content_origin.y + layout_state.position().y,
        };
        let layer_translation = Point {
            x: parent_abs.layer_translation.x + graphics_layer.translation_x,
            y: parent_abs.layer_translation.y + graphics_layer.translation_y,
        };
        Self {
            content_offset,
            layer_translation,
            window_origin: Point {
                x: top_left.x + layer_translation.x,
                y: top_left.y + layer_translation.y,
            },
            child_origin: Point {
                x: top_left.x + content_offset.x,
                y: top_left.y + content_offset.y,
            },
        }
    }
}

/// Moves `container`'s previous children that stay into `scratch.kept`, in
/// their new order, and the subtrees of those that leave into the layer
/// pool, so the entering children are built in their allocations.
fn recycle_leaving_children(
    container: &mut LayerNode,
    scratch: &mut TranslateScratch,
    changed_nodes: &mut Vec<NodeId>,
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
        collect_layer_node_ids(&leaving, changed_nodes);
        crate::layer_recycling::recycle(leaving);
    }
}

fn build_entering_children(
    applier: &mut MemoryApplier,
    container: &LayerNode,
    scratch: &mut TranslateScratch,
    geometry: TranslateGeometry,
    inherited: (bool, bool),
) {
    let (child_inherited_translated_content_context, children_ancestor_hashed) = inherited;
    let entering = &mut scratch.entering;
    entering.clear();
    for (child_id, _) in &scratch.placed_fresh {
        if scratch.old_index_by_id.contains_key(child_id) {
            continue;
        }
        let Some(mut lowered) = build_layer_node_from_applier_internal(
            applier,
            *child_id,
            container.motion_context_animated,
            child_inherited_translated_content_context,
            Some(AbsOrigin {
                content_origin: geometry.child_origin,
                layer_translation: geometry.layer_translation,
            }),
        ) else {
            continue;
        };
        if geometry.content_offset != Point::default() {
            lowered.transform_to_parent =
                lowered
                    .transform_to_parent
                    .then(ProjectiveTransform::translation(
                        geometry.content_offset.x,
                        geometry.content_offset.y,
                    ));
        }
        crate::graph_hash::recompute_layer_raster_cache_hashes_under(
            &mut lowered,
            children_ancestor_hashed,
        );
        entering.push((*child_id, crate::layer_recycling::boxed(lowered)));
    }
}

fn apply_translated_container_state(
    container: &mut LayerNode,
    modifier_slices: &ModifierNodeSlices,
    layout_state: &cranpose_ui::widgets::LayoutState,
    graphics_layer: &GraphicsLayer,
    parent_content_offset: Point,
    geometry: TranslateGeometry,
) {
    let mut transform = layer_transform_to_parent(
        container.local_bounds,
        layout_state.position(),
        graphics_layer,
    );
    if parent_content_offset != Point::default() {
        transform = transform.then(ProjectiveTransform::translation(
            parent_content_offset.x,
            parent_content_offset.y,
        ));
    }
    container.transform_to_parent = transform;
    container.content_offset = geometry.content_offset;
    if container.translated_content_context {
        container.translated_content_offset = modifier_slices
            .translated_content_offset()
            .unwrap_or(geometry.content_offset);
    }
    if let Some(sink) = modifier_slices.text_window_origin() {
        sink.set(geometry.window_origin);
    }
    if let Some(sink) = modifier_slices.viewport_window_rect() {
        sink.set(Rect {
            x: geometry.window_origin.x,
            y: geometry.window_origin.y,
            width: layout_state.size().width,
            height: layout_state.size().height,
        });
    }
    container.origin_in_parent = layout_state.position();
}

fn reconcile_translated_children(
    container: &mut LayerNode,
    dirty_nodes: &mut HashSet<NodeId>,
    changed_nodes: &mut Vec<NodeId>,
    scratch: &mut TranslateScratch,
    children_unchanged: bool,
    geometry: TranslateGeometry,
) {
    if children_unchanged {
        for (child, (child_id, state)) in container.children.iter_mut().zip(&scratch.placed_fresh) {
            let RenderNode::Layer(layer) = child else {
                unreachable!("retained child identities were checked");
            };
            if !dirty_nodes.contains(child_id) {
                translate_retained_child(layer, state, geometry.content_offset);
                changed_nodes.push(*child_id);
            }
        }
        return;
    }
    let mut entering = scratch.entering.drain(..).peekable();
    for ((child_id, state), kept) in scratch.placed_fresh.iter().zip(scratch.kept.drain(..)) {
        if let Some(mut layer) = kept {
            if !dirty_nodes.contains(child_id) {
                translate_retained_child(&mut layer, state, geometry.content_offset);
                changed_nodes.push(*child_id);
            }
            container.children.push(RenderNode::Layer(layer));
        } else if let Some((_, lowered)) = entering.next_if(|(id, _)| id == child_id) {
            dirty_nodes.remove(child_id);
            remove_dirty_descendants(&lowered, dirty_nodes);
            collect_layer_node_ids(&lowered, changed_nodes);
            container.children.push(RenderNode::Layer(lowered));
        }
    }
}

fn translate_layer_from_data(
    applier: &mut MemoryApplier,
    container: &mut LayerNode,
    dirty_nodes: &mut HashSet<NodeId>,
    changed_nodes: &mut Vec<NodeId>,
    ancestors: TranslateAncestorContext,
    data: SnapshotNodeData,
    wrapped: bool,
) -> bool {
    let TranslateAncestorContext {
        inherited_motion_context_animated,
        ancestor_hashed: container_ancestor_hashed,
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
    } = container_plan;
    let mut scratch = TRANSLATE_SCRATCH.take();
    let children_unchanged = match translated_children(
        applier,
        container,
        dirty_nodes,
        &fresh_children,
        &mut scratch,
    ) {
        Ok(unchanged) => unchanged,
        Err(reason) => {
            TRANSLATE_SCRATCH.set(scratch);
            return translate_bail(reason);
        }
    };

    let geometry = TranslateGeometry::new(&layout_state, &graphics_layer, parent_abs);
    let child_inherited_translated_content_context =
        inherited_translated_content_context || container.translated_content_context;
    let children_ancestor_hashed =
        crate::graph_hash::layer_children_ancestor_hashed(container, container_ancestor_hashed);
    if !children_unchanged {
        recycle_leaving_children(container, &mut scratch, changed_nodes);
        build_entering_children(
            applier,
            container,
            &mut scratch,
            geometry,
            (
                child_inherited_translated_content_context,
                children_ancestor_hashed,
            ),
        );
    }

    apply_translated_container_state(
        container,
        &modifier_slices,
        &layout_state,
        &graphics_layer,
        parent_content_offset,
        geometry,
    );

    reconcile_translated_children(
        container,
        dirty_nodes,
        changed_nodes,
        &mut scratch,
        children_unchanged,
        geometry,
    );
    TRANSLATE_SCRATCH.set(scratch);
    modifier_slices.publish_pointer_input_size(layout_state.size());
    container.hit_test = hit_test_from_slices(&modifier_slices);

    container.has_hit_targets = container.hit_test.is_some()
        || container.children.iter().any(|child| match child {
            RenderNode::Layer(child_layer) => child_layer.has_hit_targets,
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
        });
    container.has_origin_sinks = modifier_slices_have_origin_sinks(&modifier_slices)
        || container.children.iter().any(|child| match child {
            RenderNode::Layer(child_layer) => child_layer.has_origin_sinks,
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
        });

    container.draws_within_bounds = container.content_draws_within_bounds();
    crate::graph_hash::refresh_layer_own_raster_cache_hashes(container, container_ancestor_hashed);
    changed_nodes.push(node_id);
    true
}

fn translate_retained_child(
    layer: &mut LayerNode,
    state: &cranpose_ui::widgets::LayoutState,
    content_offset: Point,
) {
    let mut child_transform =
        layer_transform_to_parent(layer.local_bounds, state.position(), &layer.graphics_layer);
    if content_offset != Point::default() {
        child_transform = child_transform.then(ProjectiveTransform::translation(
            content_offset.x,
            content_offset.y,
        ));
    }
    layer.transform_to_parent = child_transform;
    layer.origin_in_parent = state.position();
}

fn layer_hit_graph_state_dirty(previous: &LayerNode, replacement: &LayerNode) -> bool {
    if previous.hit_test.is_some() || replacement.hit_test.is_some() {
        return true;
    }

    if !(previous.has_hit_targets || replacement.has_hit_targets) {
        return false;
    }

    previous.has_hit_targets != replacement.has_hit_targets
        || previous.local_bounds != replacement.local_bounds
        || previous.node_bounds != replacement.node_bounds
        || previous.transform_to_parent != replacement.transform_to_parent
        || previous.clip_rect() != replacement.clip_rect()
        || previous.graphics_layer.shape != replacement.graphics_layer.shape
}

fn collect_layer_node_ids(layer: &LayerNode, out: &mut Vec<NodeId>) {
    if let Some(node_id) = layer.node_id {
        out.push(node_id);
    }
    for child in &layer.children {
        if let RenderNode::Layer(child_layer) = child {
            collect_layer_node_ids(child_layer, out);
        }
    }
}

fn remove_dirty_descendants(layer: &LayerNode, dirty_nodes: &mut HashSet<NodeId>) {
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

fn build_layer_node(
    snapshot: BuildNodeSnapshot,
    _root_scale: f32,
    inherited_motion_context_animated: bool,
) -> LayerNode {
    build_layer_node_internal(snapshot, inherited_motion_context_animated, false)
}

fn build_layer_node_internal(
    snapshot: BuildNodeSnapshot,
    inherited_motion_context_animated: bool,
    inherited_translated_content_context: bool,
) -> LayerNode {
    let BuildNodeSnapshot {
        node_id,
        placement,
        size,
        content_offset,
        slices,
        graphics_layer,
        children: child_snapshots,
    } = snapshot;
    let motion_context_animated = slices.motion_context_animated();
    let translated_content_context = slices.translated_content_context();
    let has_own_origin_sinks = modifier_slices_have_origin_sinks(&slices);
    let measured_text_layout = slices.measured_text_layout();
    let draw_commands = slices.draw_commands();
    let outer_draw_command_count = slices.outer_draw_command_count();
    let clip_to_bounds = slices.clip_to_bounds();
    let text_style = slices.text_style();
    let text_layout_options = slices.text_layout_options();
    let text_pan = slices.text_pan_resolver();
    let outer = outer_draws(node_id, draw_commands, outer_draw_command_count, size);
    let layer_draw_commands = &draw_commands[outer_draw_command_count..];
    let (local_bounds, node_bounds) = layer_and_node_bounds(&slices, size);
    let graphics_layer = graphics_layer.unwrap_or_default();
    let transform_to_parent = layer_transform_to_parent(local_bounds, placement, &graphics_layer);
    let isolation = isolation_reasons(&graphics_layer);
    let cache_policy = layer_cache_policy(&graphics_layer, isolation);
    let shadow_clip = clip_to_bounds.then_some(local_bounds);
    let hit_test = hit_test_from_slices(&slices);

    let node_motion_context_animated = inherited_motion_context_animated || motion_context_animated;
    let child_translated_content_context =
        inherited_translated_content_context || translated_content_context;

    let mut children = Vec::with_capacity(layer_node_capacity(
        layer_draw_commands,
        child_snapshots.len(),
        measured_text_layout.is_some(),
    ));
    append_draw_nodes(
        &mut children,
        node_id,
        layer_draw_commands,
        outer_draw_command_count,
        DrawPlacement::Behind,
        size,
        PrimitivePhase::BeforeChildren,
    );
    if let Some(text) = text_node_from_parts(TextNodeParts {
        node_id,
        text_rect: slices.text_content_rect(size),
        text_style,
        text_layout_options,
        text_pan,
        measured_layout: measured_text_layout,
    }) {
        children.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(crate::layer_recycling::boxed_text(text)),
        }));
    }
    let child_motion_context_animated = node_motion_context_animated;
    for child in child_snapshots {
        let mut child_layer = build_layer_node_internal(
            child,
            child_motion_context_animated,
            child_translated_content_context,
        );
        if content_offset != Point::default() {
            child_layer.transform_to_parent =
                child_layer
                    .transform_to_parent
                    .then(ProjectiveTransform::translation(
                        content_offset.x,
                        content_offset.y,
                    ));
        }
        children.push(RenderNode::Layer(Box::new(child_layer)));
    }
    append_draw_nodes(
        &mut children,
        node_id,
        layer_draw_commands,
        outer_draw_command_count,
        DrawPlacement::Overlay,
        size,
        PrimitivePhase::AfterChildren,
    );
    let has_hit_targets = hit_test.is_some()
        || children.iter().any(|child| match child {
            RenderNode::Layer(child_layer) => child_layer.has_hit_targets,
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
        });
    let has_origin_sinks = has_own_origin_sinks
        || children.iter().any(|child| match child {
            RenderNode::Layer(child_layer) => child_layer.has_origin_sinks,
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
        });

    let layer = LayerNode {
        node_id: Some(node_id),
        wraps: None,
        local_bounds,
        node_bounds,
        transform_to_parent,
        content_offset,
        motion_context_animated: node_motion_context_animated,
        translated_content_context,
        translated_content_offset: if translated_content_context {
            content_offset
        } else {
            Point::default()
        },
        origin_in_parent: placement,
        graphics_layer,
        clip_to_bounds,
        shadow_clip,
        hit_test,
        has_hit_targets,
        has_origin_sinks,
        draws_within_bounds: false,
        isolation,
        cache_policy,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children,
    };
    finish_layer(layer, placement, outer)
}

#[derive(Clone, Copy)]
struct AbsOrigin {
    content_origin: Point,
    layer_translation: Point,
}

impl AbsOrigin {
    const ROOT: AbsOrigin = AbsOrigin {
        content_origin: Point { x: 0.0, y: 0.0 },
        layer_translation: Point { x: 0.0, y: 0.0 },
    };

    fn children_of(self, layer: &LayerNode) -> AbsOrigin {
        AbsOrigin {
            content_origin: Point {
                x: self.content_origin.x + layer.origin_in_parent.x + layer.content_offset.x,
                y: self.content_origin.y + layer.origin_in_parent.y + layer.content_offset.y,
            },
            layer_translation: Point {
                x: self.layer_translation.x + layer.graphics_layer.translation_x,
                y: self.layer_translation.y + layer.graphics_layer.translation_y,
            },
        }
    }
}

fn build_layer_node_from_applier(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    _root_scale: f32,
    inherited_motion_context_animated: bool,
) -> Option<LayerNode> {
    let mut data = snapshot_node_data(applier, node_id)?;
    if data.window_root {
        data.layout_state = data.layout_state.at_origin();
    }
    build_layer_node_from_data(
        applier,
        node_id,
        data,
        inherited_motion_context_animated,
        false,
        Some(AbsOrigin::ROOT),
    )
}

fn snapshot_node_data(applier: &mut MemoryApplier, node_id: NodeId) -> Option<SnapshotNodeData> {
    if let Ok(data) = applier.with_node::<LayoutNode, _>(node_id, |node| {
        let state = node.layout_state();
        let mut children = SmallVec::new();
        node.collect_children_into(&mut children);
        let modifier_slices = node.modifier_slices_snapshot();
        SnapshotNodeData {
            layout_state: state,
            modifier_slices,
            children,
            window_root: node.is_window_root(),
        }
    }) {
        return Some(data);
    }

    applier
        .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
            let state = node.layout_state();
            let mut children = SmallVec::new();
            node.collect_children_into(&mut children);
            let modifier_slices = node.modifier_slices_snapshot();
            SnapshotNodeData {
                layout_state: state,
                modifier_slices,
                children,
                window_root: false,
            }
        })
        .ok()
}

fn build_layer_node_from_applier_internal(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    inherited_motion_context_animated: bool,
    inherited_translated_content_context: bool,
    parent_abs: Option<AbsOrigin>,
) -> Option<LayerNode> {
    let data = snapshot_node_data(applier, node_id)?;
    if data.window_root {
        return None;
    }
    build_layer_node_from_data(
        applier,
        node_id,
        data,
        inherited_motion_context_animated,
        inherited_translated_content_context,
        parent_abs,
    )
}

fn hit_test_from_slices(slices: &Rc<ModifierNodeSlices>) -> Option<HitTestNode> {
    slices_hit_something(slices).then(|| HitTestNode {
        shape: None,
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

fn build_layer_node_from_data(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    data: SnapshotNodeData,
    inherited_motion_context_animated: bool,
    inherited_translated_content_context: bool,
    parent_abs: Option<AbsOrigin>,
) -> Option<LayerNode> {
    note_layer_lowered();
    let SnapshotNodeData {
        layout_state,
        modifier_slices,
        children,
        window_root: _,
    } = data;
    if !layout_state.is_placed() {
        return None;
    }

    let (local_bounds, node_bounds) = layer_and_node_bounds(&modifier_slices, layout_state.size());
    if cranpose_core::env_flag!("CRANPOSE_SCENE_UPDATE_DIAG") {
        eprintln!(
            "[scene-update-diag] build layer node={node_id:?} size=({:.2},{:.2}) pos=({:.2},{:.2})",
            layout_state.size().width,
            layout_state.size().height,
            layout_state.position().x,
            layout_state.position().y,
        );
    }
    let clip_to_bounds = modifier_slices.clip_to_bounds();
    let graphics_layer = graphics_layer_with_shaped_clip(
        modifier_slices.graphics_layer().unwrap_or_default(),
        clip_to_bounds,
        modifier_slices.corner_shape(),
        local_bounds,
    );
    let transform_to_parent =
        layer_transform_to_parent(local_bounds, layout_state.position(), &graphics_layer);
    let isolation = isolation_reasons(&graphics_layer);
    let cache_policy = layer_cache_policy(&graphics_layer, isolation);
    let shadow_clip = clip_to_bounds.then_some(local_bounds);
    let hit_test = hit_test_from_slices(&modifier_slices);

    modifier_slices.publish_pointer_input_size(layout_state.size());

    let node_motion_context_animated =
        inherited_motion_context_animated || modifier_slices.motion_context_animated();
    let local_translated_content_context = modifier_slices.translated_content_context();
    let content_offset = layout_state.content_offset();
    let local_translated_content_offset = modifier_slices
        .translated_content_offset()
        .unwrap_or(content_offset);
    let child_translated_content_context =
        inherited_translated_content_context || local_translated_content_context;

    let this_abs = parent_abs.map(|parent| {
        let top_left = Point {
            x: parent.content_origin.x + layout_state.position().x,
            y: parent.content_origin.y + layout_state.position().y,
        };
        let layer_translation = Point {
            x: parent.layer_translation.x + graphics_layer.translation_x,
            y: parent.layer_translation.y + graphics_layer.translation_y,
        };
        (top_left, layer_translation)
    });
    if let Some((top_left, layer_translation)) = this_abs {
        let window_origin = Point {
            x: top_left.x + layer_translation.x,
            y: top_left.y + layer_translation.y,
        };
        if let Some(sink) = modifier_slices.text_window_origin() {
            sink.set(window_origin);
        }
        if let Some(sink) = modifier_slices.viewport_window_rect() {
            sink.set(Rect {
                x: window_origin.x,
                y: window_origin.y,
                width: layout_state.size().width,
                height: layout_state.size().height,
            });
        }
    }
    let child_abs = this_abs.map(|(top_left, layer_translation)| AbsOrigin {
        content_origin: Point {
            x: top_left.x + layout_state.content_offset().x,
            y: top_left.y + layout_state.content_offset().y,
        },
        layer_translation,
    });

    let outer_draw_command_count = modifier_slices.outer_draw_command_count();
    let outer = outer_draws(
        node_id,
        modifier_slices.draw_commands(),
        outer_draw_command_count,
        layout_state.size(),
    );
    let layer_draw_commands = &modifier_slices.draw_commands()[outer_draw_command_count..];
    let mut render_children = crate::layer_recycling::child_list(layer_node_capacity(
        layer_draw_commands,
        children.len(),
        modifier_slices.annotated_text().is_some(),
    ));
    append_draw_nodes(
        &mut render_children,
        node_id,
        layer_draw_commands,
        outer_draw_command_count,
        DrawPlacement::Behind,
        layout_state.size(),
        PrimitivePhase::BeforeChildren,
    );
    if let Some(text) = text_node_from_parts(TextNodeParts {
        node_id,
        text_rect: modifier_slices.text_content_rect(layout_state.size()),
        text_style: modifier_slices.text_style(),
        text_layout_options: modifier_slices.text_layout_options(),
        text_pan: modifier_slices.text_pan_resolver(),
        measured_layout: modifier_slices.measured_text_layout(),
    }) {
        render_children.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(crate::layer_recycling::boxed_text(text)),
        }));
    }
    let child_motion_context_animated = node_motion_context_animated;
    for child_id in children {
        let Some(mut child_layer) = build_layer_node_from_applier_internal(
            applier,
            child_id,
            child_motion_context_animated,
            child_translated_content_context,
            child_abs,
        ) else {
            continue;
        };
        if layout_state.content_offset() != Point::default() {
            child_layer.transform_to_parent =
                child_layer
                    .transform_to_parent
                    .then(ProjectiveTransform::translation(
                        layout_state.content_offset().x,
                        layout_state.content_offset().y,
                    ));
        }
        render_children.push(RenderNode::Layer(crate::layer_recycling::boxed(
            child_layer,
        )));
    }
    append_draw_nodes(
        &mut render_children,
        node_id,
        layer_draw_commands,
        outer_draw_command_count,
        DrawPlacement::Overlay,
        layout_state.size(),
        PrimitivePhase::AfterChildren,
    );
    let has_hit_targets = hit_test.is_some()
        || render_children.iter().any(|child| match child {
            RenderNode::Layer(child_layer) => child_layer.has_hit_targets,
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
        });
    let has_origin_sinks = modifier_slices_have_origin_sinks(&modifier_slices)
        || render_children.iter().any(|child| match child {
            RenderNode::Layer(child_layer) => child_layer.has_origin_sinks,
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
        });

    let layer = LayerNode {
        node_id: Some(node_id),
        wraps: None,
        local_bounds,
        node_bounds,
        transform_to_parent,
        content_offset: layout_state.content_offset(),
        motion_context_animated: node_motion_context_animated,
        translated_content_context: local_translated_content_context,
        translated_content_offset: if local_translated_content_context {
            local_translated_content_offset
        } else {
            Point::default()
        },
        origin_in_parent: layout_state.position(),
        graphics_layer,
        clip_to_bounds,
        shadow_clip,
        hit_test,
        has_hit_targets,
        has_origin_sinks,
        draws_within_bounds: false,
        isolation,
        cache_policy,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children: render_children,
    };
    Some(finish_layer(layer, layout_state.position(), outer))
}

struct RecorderSlot {
    generation: u64,
    handles: [Option<Rc<CommandRecording>>; 2],
    /// The allocation of a handle whose recording `acquire_storage` took,
    /// which the next recording the slot publishes moves into.
    spare: Option<Rc<CommandRecording>>,
}

thread_local! {
    static COMMAND_RECORDINGS: std::cell::RefCell<
        std::collections::HashMap<DrawCommandId, RecorderSlot, cranpose_ui_graphics::FxBuildHasher>,
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
        let Some(slot) = map.get_mut(&id) else {
            return CommandRecording::default();
        };
        for handle in &mut slot.handles {
            let Some(recording) = handle.as_mut().and_then(Rc::get_mut) else {
                continue;
            };
            let storage = std::mem::take(recording);
            slot.spare = handle.take();
            return storage;
        }
        CommandRecording::default()
    })
}

fn publish_recording(id: DrawCommandId, recording: CommandRecording) -> Rc<CommandRecording> {
    COMMAND_RECORDINGS.with(|map| {
        let mut map = map.borrow_mut();
        let generation = RECORDING_GENERATION.with(Cell::get);
        let slot = map.entry(id).or_insert_with(|| RecorderSlot {
            generation,
            handles: [None, None],
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
        slot.handles[1] = slot.handles[0].take();
        slot.handles[0] = Some(Rc::clone(&shared));
        shared
    })
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
        placement,
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
    placement: DrawPlacement,
    size: Size,
    phase: PrimitivePhase,
) {
    for (command_index, command) in commands.iter().enumerate() {
        let id = DrawCommandId {
            node_id,
            command_index: (first_command_index + command_index) as u32,
            placement,
        };
        let Some((recording, segments)) =
            recording_for_placement_reusing(command, placement, size, || acquire_storage(id))
        else {
            retain_empty_draw_command(nodes, phase, id, placement, command);
            continue;
        };
        let shared = publish_recording(id, recording);
        if shared.is_empty_in(&segments) {
            retain_empty_draw_command(nodes, phase, id, placement, command);
            continue;
        }
        nodes.push(RenderNode::DrawRun(DrawRunNode::for_command_shared(
            phase,
            Some(id),
            shared,
            segments,
        )));
    }
}

fn retain_empty_draw_command(
    nodes: &mut Vec<RenderNode>,
    phase: PrimitivePhase,
    id: DrawCommandId,
    placement: DrawPlacement,
    command: &DrawCommand,
) {
    if matches!(
        (placement, command),
        (DrawPlacement::Behind, DrawCommand::Behind(_))
            | (DrawPlacement::Overlay, DrawCommand::Overlay(_))
            | (_, DrawCommand::WithContent(_))
    ) {
        nodes.push(RenderNode::DrawRun(DrawRunNode::for_command(
            phase,
            Some(id),
            Vec::new(),
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
        OuterDraws {
            behind: draw_nodes(
                node_id,
                commands,
                0,
                DrawPlacement::Behind,
                size,
                PrimitivePhase::BeforeChildren,
            ),
            overlay: draw_nodes(
                node_id,
                commands,
                0,
                DrawPlacement::Overlay,
                size,
                PrimitivePhase::AfterChildren,
            ),
        }
    })
}

fn finish_layer(mut layer: LayerNode, placement: Point, outer: Option<OuterDraws>) -> LayerNode {
    layer.draws_within_bounds = layer.content_draws_within_bounds();
    match outer {
        Some(outer) => wrap_layer_with_outer_draws(layer, placement, outer),
        None => layer,
    }
}

fn wrap_layer_with_outer_draws(
    mut layer: LayerNode,
    placement: Point,
    outer: OuterDraws,
) -> LayerNode {
    layer.transform_to_parent =
        layer_transform_to_parent(layer.local_bounds, Point::default(), &layer.graphics_layer);
    layer.origin_in_parent = Point::default();
    // The outer draws sit on the node's rect, outside its layer.
    let node_rect = layer.node_rect();
    let wrapper = LayerNode {
        wraps: layer.node_id,
        local_bounds: node_rect,
        transform_to_parent: layer_transform_to_parent(
            node_rect,
            placement,
            &GraphicsLayer::default(),
        ),
        origin_in_parent: placement,
        motion_context_animated: layer.motion_context_animated,
        has_hit_targets: layer.has_hit_targets,
        has_origin_sinks: layer.has_origin_sinks,
        ..Default::default()
    };
    let OuterDraws {
        behind: mut children,
        mut overlay,
    } = outer;
    children.reserve(1 + overlay.len());
    children.push(RenderNode::Layer(crate::layer_recycling::boxed(layer)));
    children.append(&mut overlay);
    crate::layer_recycling::recycle_list(overlay);
    let mut wrapper = LayerNode {
        children,
        ..wrapper
    };
    wrapper.draws_within_bounds = wrapper.content_draws_within_bounds();
    wrapper
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
    let graphics_layer = graphics_layer_with_shaped_clip(
        base_graphics_layer.clone().unwrap_or_default(),
        node.node_data.modifier_slices.clip_to_bounds(),
        node.node_data.modifier_slices.corner_shape(),
        Rect {
            x: 0.0,
            y: 0.0,
            width: node.rect.width,
            height: node.rect.height,
        },
    );
    let has_graphics_layer =
        base_graphics_layer.is_some() || graphics_layer.render_effect.is_some();

    BuildNodeSnapshot {
        node_id: node.node_id,
        placement,
        size: Size {
            width: node.rect.width,
            height: node.rect.height,
        },
        content_offset: node.content_offset,
        slices: Rc::clone(&node.node_data.modifier_slices),
        graphics_layer: has_graphics_layer.then_some(graphics_layer),
        children,
    }
}

fn modifier_slices_have_origin_sinks(slices: &ModifierNodeSlices) -> bool {
    slices.text_window_origin().is_some() || slices.viewport_window_rect().is_some()
}

fn graphics_layer_with_shaped_clip(
    mut graphics_layer: GraphicsLayer,
    clip_to_bounds: bool,
    corner_shape: Option<RoundedCornerShape>,
    local_bounds: Rect,
) -> GraphicsLayer {
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

    if let Some(existing) = graphics_layer.render_effect.take() {
        let rounded_clip = rounded_corner_alpha_mask_effect(
            local_bounds.width,
            local_bounds.height,
            radii,
            ROUNDED_CLIP_EDGE_FEATHER,
        );
        graphics_layer.render_effect = Some(existing.then(rounded_clip));
    } else {
        graphics_layer.shape = LayerShape::Rounded(corner_shape);
        graphics_layer.clip = true;
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
    remaining * cranpose_ui::text::text_align_fraction(text_style, text)
}

#[cfg(test)]
#[path = "tests/scene_builder_tests.rs"]
mod tests;
