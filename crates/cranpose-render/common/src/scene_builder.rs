use std::{collections::HashSet, rc::Rc};

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui::{
    DrawCommand, LayoutBox, LayoutNode, ModifierNodeSlices, Point, Rect, ResolvedModifiers, Size,
    SubcomposeLayoutNode, TextLayoutOptions, TextOverflow, TextPanResolver, prepare_text_layout,
    text::{AnnotatedString, TextAlign, TextStyle, resolve_text_direction},
};
use cranpose_ui_graphics::{
    CompositingStrategy, GraphicsLayer, LayerShape, RoundedCornerShape,
    rounded_corner_alpha_mask_effect,
};

use crate::{
    graph::{
        CachePolicy, DrawCommandId, DrawRunNode, HitTestNode, IsolationReasons, LayerNode,
        PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph,
        RenderNode, TextPrimitiveNode,
    },
    layer_transform::layer_transform_to_parent,
    raster_cache::LayerRasterCacheHashes,
    style_shared::{DrawPlacement, primitives_for_placement_verified},
};

const TEXT_CLIP_PAD: f32 = 1.0;
const ROUNDED_CLIP_EDGE_FEATHER: f32 = 1.0;

#[derive(Clone)]
struct BuildNodeSnapshot {
    node_id: NodeId,
    placement: Point,
    size: Size,
    content_offset: Point,
    motion_context_animated: bool,
    translated_content_context: bool,
    has_own_origin_sinks: bool,
    measured_max_width: Option<f32>,
    resolved_modifiers: ResolvedModifiers,
    draw_commands: Vec<DrawCommand>,
    click_actions: Vec<Rc<dyn Fn(Point)>>,
    pointer_inputs: Vec<Rc<dyn Fn(cranpose_foundation::PointerEvent)>>,
    clip_to_bounds: bool,
    annotated_text: Option<AnnotatedString>,
    text_style: Option<TextStyle>,
    text_layout_options: Option<TextLayoutOptions>,
    text_pan: Option<TextPanResolver>,
    graphics_layer: Option<GraphicsLayer>,
    children: Vec<Self>,
}

struct SnapshotNodeData {
    layout_state: cranpose_ui::widgets::LayoutState,
    modifier_slices: Rc<ModifierNodeSlices>,
    resolved_modifiers: ResolvedModifiers,
    children: Vec<NodeId>,
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
    LOWERED_LAYER_COUNT.with(std::cell::Cell::get)
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
    if let Some(root_id) = graph.root.node_id
        && remaining_dirty_nodes.contains(&root_id)
    {
        remaining_dirty_nodes.remove(&root_id);
        if try_translate_scrolled_layer(
            applier,
            &mut graph.root,
            &mut remaining_dirty_nodes,
            changed_nodes,
            TranslateAncestorContext {
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
            let walked = replace_dirty_layers_from_applier(
                applier,
                &mut graph.root,
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
        let Some(root) = build_layer_node_from_applier(applier, root_id, scale, false) else {
            return GraphUpdateReport {
                update: GraphUpdate::NeedsRebuild(GraphRebuildReason::RootLayerUnavailable),
                hit_graph_dirty: true,
            };
        };
        let hit_graph_dirty = layer_hit_graph_state_dirty(&graph.root, &root);
        collect_layer_node_ids(&graph.root, changed_nodes);
        graph.root = root;
        graph.root.recompute_raster_cache_hashes();
        collect_layer_node_ids(&graph.root, changed_nodes);
        return GraphUpdateReport {
            update: GraphUpdate::Patched,
            hit_graph_dirty,
        };
    }

    let inherited_translated_content_context = graph.root.translated_content_context;
    let report = match replace_dirty_layers_from_applier(
        applier,
        &mut graph.root,
        &mut remaining_dirty_nodes,
        inherited_translated_content_context,
        false,
        changed_nodes,
    ) {
        Some(report) => report,
        None => {
            return GraphUpdateReport {
                update: GraphUpdate::NeedsRebuild(GraphRebuildReason::DirtyLayerUnavailable),
                hit_graph_dirty: true,
            };
        }
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

        if child_layer
            .node_id
            .is_some_and(|node_id| dirty_nodes.remove(&node_id))
        {
            if try_translate_scrolled_layer(
                applier,
                child_layer,
                dirty_nodes,
                changed_nodes,
                TranslateAncestorContext {
                    ancestor_hashed: child_ancestor_hashed,
                    inherited_translated_content_context:
                        child_inherited_translated_content_context,
                    parent_content_offset: parent.content_offset,
                    parent_abs: AbsOrigin {
                        content_origin: parent.scene_children_origin,
                        layer_translation: parent.scene_children_layer_translation,
                    },
                },
            ) {
                report.hit_graph_dirty = true;
                report.updated = true;
                let child_report = replace_dirty_layers_from_applier(
                    applier,
                    child_layer,
                    dirty_nodes,
                    child_inherited_translated_content_context,
                    child_ancestor_hashed,
                    changed_nodes,
                )?;
                report.hit_graph_dirty |= child_report.hit_graph_dirty;
                continue;
            }
            let mut replacement = build_layer_node_from_applier_internal(
                applier,
                child_layer
                    .node_id
                    .expect("dirty layer must have a node id"),
                parent.motion_context_animated,
                child_inherited_translated_content_context,
                Some(AbsOrigin {
                    content_origin: parent.scene_children_origin,
                    layer_translation: parent.scene_children_layer_translation,
                }),
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
            collect_layer_node_ids(child_layer, changed_nodes);
            **child_layer = replacement;
            collect_layer_node_ids(child_layer, changed_nodes);
            crate::graph_hash::recompute_layer_raster_cache_hashes_under(
                child_layer,
                child_ancestor_hashed,
            );
            report.updated = true;
            continue;
        }

        let child_report = replace_dirty_layers_from_applier(
            applier,
            child_layer,
            dirty_nodes,
            child_inherited_translated_content_context,
            child_ancestor_hashed,
            changed_nodes,
        )?;
        report.updated |= child_report.updated;
        report.hit_graph_dirty |= child_report.hit_graph_dirty;
    }

    if report.updated {
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
    let TranslateAncestorContext {
        ancestor_hashed: container_ancestor_hashed,
        inherited_translated_content_context,
        parent_content_offset,
        parent_abs,
    } = ancestors;
    if cranpose_core::env_flag!("CRANPOSE_DISABLE_SCROLL_TRANSLATE") {
        return translate_bail("fast path disabled by ablation switch");
    }
    let Some(node_id) = container.node_id else {
        return translate_bail("no node id");
    };
    if container
        .children
        .iter()
        .any(|child| !matches!(child, RenderNode::Layer(_)))
    {
        return translate_bail("container has own primitive children");
    }
    let Some(data) = snapshot_node_data(applier, node_id) else {
        return translate_bail("container snapshot read failed");
    };
    let SnapshotNodeData {
        layout_state,
        modifier_slices,
        resolved_modifiers: _,
        children: fresh_children,
    } = data;
    if !layout_state.is_placed()
        || layout_state.size().width != container.local_bounds.width
        || layout_state.size().height != container.local_bounds.height
    {
        return translate_bail("container unplaced or resized");
    }
    if !modifier_slices.draw_commands().is_empty()
        || modifier_slices.annotated_text().is_some()
        || modifier_slices.translated_content_context() != container.translated_content_context
    {
        return translate_bail("container draw/text/translated-context changed");
    }
    let clip_to_bounds = modifier_slices.clip_to_bounds();
    if clip_to_bounds != container.clip_to_bounds {
        return translate_bail("container clip changed");
    }
    let graphics_layer = graphics_layer_with_shaped_clip(
        modifier_slices.graphics_layer().unwrap_or_default(),
        clip_to_bounds,
        modifier_slices.corner_shape(),
        container.local_bounds,
    );
    if graphics_layer != container.graphics_layer {
        return translate_bail("container graphics layer changed");
    }
    let mut old_ids = Vec::with_capacity(container.children.len());
    for child in &container.children {
        let RenderNode::Layer(layer) = child else {
            return translate_bail("non-layer child");
        };
        let Some(child_id) = layer.node_id else {
            return translate_bail("child without node id");
        };
        old_ids.push(child_id);
    }
    let old_index_by_id: std::collections::HashMap<NodeId, usize> = old_ids
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect();
    let mut placed_fresh = Vec::with_capacity(fresh_children.len());
    for child_id in &fresh_children {
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
    let fresh_id_set: HashSet<NodeId> = placed_fresh.iter().map(|(id, _)| *id).collect();
    for (child_id, state) in &placed_fresh {
        let Some(&old_index) = old_index_by_id.get(child_id) else {
            continue;
        };
        let RenderNode::Layer(layer) = &container.children[old_index] else {
            return false;
        };
        if dirty_nodes.contains(child_id) {
            continue;
        }
        if layer.has_origin_sinks {
            return translate_bail("child subtree publishes window origins");
        }
        if state.size().width != layer.local_bounds.width
            || state.size().height != layer.local_bounds.height
        {
            return translate_bail("child resized");
        }
    }

    let content_offset = layout_state.content_offset;
    let (translation_x, translation_y) = modifier_slices
        .graphics_layer()
        .map(|layer| (layer.translation_x, layer.translation_y))
        .unwrap_or((0.0, 0.0));
    let top_left = Point {
        x: parent_abs.content_origin.x + layout_state.position().x,
        y: parent_abs.content_origin.y + layout_state.position().y,
    };
    let layer_translation = Point {
        x: parent_abs.layer_translation.x + translation_x,
        y: parent_abs.layer_translation.y + translation_y,
    };
    let window_origin = Point {
        x: top_left.x + layer_translation.x,
        y: top_left.y + layer_translation.y,
    };
    let child_origin = Point {
        x: top_left.x + content_offset.x,
        y: top_left.y + content_offset.y,
    };
    let translation_delta = Point {
        x: layer_translation.x - container.scene_children_layer_translation.x,
        y: layer_translation.y - container.scene_children_layer_translation.y,
    };

    let child_inherited_translated_content_context =
        inherited_translated_content_context || container.translated_content_context;
    let children_ancestor_hashed =
        crate::graph_hash::layer_children_ancestor_hashed(container, container_ancestor_hashed);
    let mut entering: std::collections::HashMap<NodeId, LayerNode> =
        std::collections::HashMap::new();
    for (child_id, _) in &placed_fresh {
        if old_index_by_id.contains_key(child_id) {
            continue;
        }
        let Some(mut lowered) = build_layer_node_from_applier_internal(
            applier,
            *child_id,
            container.motion_context_animated,
            child_inherited_translated_content_context,
            Some(AbsOrigin {
                content_origin: child_origin,
                layer_translation,
            }),
        ) else {
            continue;
        };
        if content_offset != Point::default() {
            lowered.transform_to_parent =
                lowered
                    .transform_to_parent
                    .then(ProjectiveTransform::translation(
                        content_offset.x,
                        content_offset.y,
                    ));
        }
        crate::graph_hash::recompute_layer_raster_cache_hashes_under(
            &mut lowered,
            children_ancestor_hashed,
        );
        entering.insert(*child_id, lowered);
    }

    let mut transform = layer_transform_to_parent(
        container.local_bounds,
        layout_state.position(),
        &graphics_layer,
    );
    if parent_content_offset != Point::default() {
        transform = transform.then(ProjectiveTransform::translation(
            parent_content_offset.x,
            parent_content_offset.y,
        ));
    }
    container.transform_to_parent = transform;
    container.content_offset = content_offset;
    if container.translated_content_context {
        container.translated_content_offset = modifier_slices
            .translated_content_offset()
            .unwrap_or(content_offset);
    }

    if let Some(sink) = modifier_slices.text_field_window_origin() {
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
    container.scene_children_origin = child_origin;
    container.scene_children_layer_translation = layer_translation;

    let mut old_by_id: std::collections::HashMap<NodeId, Box<LayerNode>> =
        std::collections::HashMap::new();
    for child in container.children.drain(..) {
        let RenderNode::Layer(layer) = child else {
            continue;
        };
        let child_id = layer.node_id.expect("checked above");
        if fresh_id_set.contains(&child_id) {
            old_by_id.insert(child_id, layer);
        } else {
            collect_layer_node_ids(&layer, changed_nodes);
        }
    }
    let mut new_children = Vec::with_capacity(placed_fresh.len());
    for (child_id, state) in &placed_fresh {
        if let Some(mut layer) = old_by_id.remove(child_id) {
            if !dirty_nodes.contains(child_id) {
                let mut child_transform = layer_transform_to_parent(
                    layer.local_bounds,
                    state.position(),
                    &layer.graphics_layer,
                );
                if content_offset != Point::default() {
                    child_transform = child_transform.then(ProjectiveTransform::translation(
                        content_offset.x,
                        content_offset.y,
                    ));
                }
                layer.transform_to_parent = child_transform;
                let new_children_origin = Point {
                    x: child_origin.x + state.position().x + layer.content_offset.x,
                    y: child_origin.y + state.position().y + layer.content_offset.y,
                };
                let origin_delta = Point {
                    x: new_children_origin.x - layer.scene_children_origin.x,
                    y: new_children_origin.y - layer.scene_children_origin.y,
                };
                offset_scene_origins(&mut layer, origin_delta, translation_delta);
                if let Some(moved_id) = layer.node_id {
                    changed_nodes.push(moved_id);
                }
            }
            new_children.push(RenderNode::Layer(layer));
        } else if let Some(lowered) = entering.remove(child_id) {
            dirty_nodes.remove(child_id);
            remove_dirty_descendants(&lowered, dirty_nodes);
            collect_layer_node_ids(&lowered, changed_nodes);
            new_children.push(RenderNode::Layer(Box::new(lowered)));
        }
    }
    container.children = new_children;

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

    crate::graph_hash::refresh_layer_own_raster_cache_hashes(container, container_ancestor_hashed);
    changed_nodes.push(node_id);
    true
}

fn offset_scene_origins(layer: &mut LayerNode, origin_delta: Point, translation_delta: Point) {
    layer.scene_children_origin.x += origin_delta.x;
    layer.scene_children_origin.y += origin_delta.y;
    layer.scene_children_layer_translation.x += translation_delta.x;
    layer.scene_children_layer_translation.y += translation_delta.y;
    for child in &mut layer.children {
        if let RenderNode::Layer(child_layer) = child {
            offset_scene_origins(child_layer, origin_delta, translation_delta);
        }
    }
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
        motion_context_animated,
        translated_content_context,
        has_own_origin_sinks,
        measured_max_width,
        resolved_modifiers,
        draw_commands,
        click_actions,
        pointer_inputs,
        clip_to_bounds,
        annotated_text,
        text_style,
        text_layout_options,
        text_pan,
        graphics_layer,
        children: child_snapshots,
    } = snapshot;
    let local_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: size.width,
        height: size.height,
    };
    let graphics_layer = graphics_layer.unwrap_or_default();
    let transform_to_parent = layer_transform_to_parent(local_bounds, placement, &graphics_layer);
    let isolation = isolation_reasons(&graphics_layer);
    let cache_policy = if isolation.has_any() {
        CachePolicy::Auto
    } else {
        CachePolicy::None
    };
    let shadow_clip = clip_to_bounds.then_some(local_bounds);
    let hit_test = (!click_actions.is_empty() || !pointer_inputs.is_empty()).then(|| HitTestNode {
        shape: None,
        click_actions,
        pointer_inputs,
        clip: (clip_to_bounds || graphics_layer.clip).then_some(local_bounds),
    });

    let node_motion_context_animated = inherited_motion_context_animated || motion_context_animated;
    let child_translated_content_context =
        inherited_translated_content_context || translated_content_context;

    let mut children = draw_nodes(
        node_id,
        &draw_commands,
        DrawPlacement::Behind,
        size,
        PrimitivePhase::BeforeChildren,
    );
    if let Some(text) = text_node_from_parts(TextNodeParts {
        node_id,
        local_bounds,
        measured_max_width,
        resolved_modifiers: &resolved_modifiers,
        annotated_text: annotated_text.as_ref(),
        text_style: text_style.as_ref(),
        text_layout_options,
        text_pan,
        modifier_slices: None,
    }) {
        children.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(text)),
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
    children.extend(draw_nodes(
        node_id,
        &draw_commands,
        DrawPlacement::Overlay,
        size,
        PrimitivePhase::AfterChildren,
    ));
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

    LayerNode {
        node_id: Some(node_id),
        local_bounds,
        transform_to_parent,
        content_offset,
        motion_context_animated: node_motion_context_animated,
        translated_content_context,
        translated_content_offset: if translated_content_context {
            content_offset
        } else {
            Point::default()
        },
        scene_children_origin: Point::default(),
        scene_children_layer_translation: Point::default(),
        graphics_layer,
        clip_to_bounds,
        shadow_clip,
        hit_test,
        has_hit_targets,
        has_origin_sinks,
        isolation,
        cache_policy,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children,
    }
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
}

fn build_layer_node_from_applier(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    _root_scale: f32,
    inherited_motion_context_animated: bool,
) -> Option<LayerNode> {
    build_layer_node_from_applier_internal(
        applier,
        node_id,
        inherited_motion_context_animated,
        false,
        Some(AbsOrigin::ROOT),
    )
}

fn snapshot_node_data(applier: &mut MemoryApplier, node_id: NodeId) -> Option<SnapshotNodeData> {
    if let Ok(data) = applier.with_node::<LayoutNode, _>(node_id, |node| {
        let state = node.layout_state();
        let children = node.children.clone();
        let modifier_slices = node.modifier_slices_snapshot();
        SnapshotNodeData {
            layout_state: state,
            modifier_slices,
            resolved_modifiers: node.resolved_modifiers(),
            children,
        }
    }) {
        return Some(data);
    }

    applier
        .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
            let state = node.layout_state();
            let children = node.active_children();
            let modifier_slices = node.modifier_slices_snapshot();
            SnapshotNodeData {
                layout_state: state,
                modifier_slices,
                resolved_modifiers: node.resolved_modifiers(),
                children,
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
    build_layer_node_from_data(
        applier,
        node_id,
        data,
        inherited_motion_context_animated,
        inherited_translated_content_context,
        parent_abs,
    )
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
        resolved_modifiers,
        children,
    } = data;
    if !layout_state.is_placed() {
        return None;
    }

    let local_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: layout_state.size().width,
        height: layout_state.size().height,
    };
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
    let cache_policy = if isolation.has_any() {
        CachePolicy::Auto
    } else {
        CachePolicy::None
    };
    let click_actions = modifier_slices.click_handlers();
    let pointer_inputs = modifier_slices.pointer_inputs();
    let shadow_clip = clip_to_bounds.then_some(local_bounds);
    let hit_test = (!click_actions.is_empty() || !pointer_inputs.is_empty()).then(|| HitTestNode {
        shape: None,
        click_actions: click_actions.to_vec(),
        pointer_inputs: pointer_inputs.to_vec(),
        clip: (clip_to_bounds || graphics_layer.clip).then_some(local_bounds),
    });

    modifier_slices.publish_pointer_input_size(layout_state.size());

    let node_motion_context_animated =
        inherited_motion_context_animated || modifier_slices.motion_context_animated();
    let local_translated_content_context = modifier_slices.translated_content_context();
    let local_translated_content_offset = modifier_slices
        .translated_content_offset()
        .unwrap_or(layout_state.content_offset);
    let child_translated_content_context =
        inherited_translated_content_context || local_translated_content_context;

    let this_abs = parent_abs.map(|parent| {
        let (tx, ty) = modifier_slices
            .graphics_layer()
            .map(|layer| (layer.translation_x, layer.translation_y))
            .unwrap_or((0.0, 0.0));
        let top_left = Point {
            x: parent.content_origin.x + layout_state.position().x,
            y: parent.content_origin.y + layout_state.position().y,
        };
        let layer_translation = Point {
            x: parent.layer_translation.x + tx,
            y: parent.layer_translation.y + ty,
        };
        (top_left, layer_translation)
    });
    if let Some((top_left, layer_translation)) = this_abs {
        let window_origin = Point {
            x: top_left.x + layer_translation.x,
            y: top_left.y + layer_translation.y,
        };
        if let Some(sink) = modifier_slices.text_field_window_origin() {
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
            x: top_left.x + layout_state.content_offset.x,
            y: top_left.y + layout_state.content_offset.y,
        },
        layer_translation,
    });

    let mut render_children = draw_nodes(
        node_id,
        modifier_slices.draw_commands(),
        DrawPlacement::Behind,
        layout_state.size(),
        PrimitivePhase::BeforeChildren,
    );
    if let Some(text) = text_node_from_parts(TextNodeParts {
        node_id,
        local_bounds,
        measured_max_width: layout_state
            .measurement_constraints
            .max_width
            .is_finite()
            .then_some(layout_state.measurement_constraints.max_width),
        resolved_modifiers: &resolved_modifiers,
        annotated_text: modifier_slices.annotated_text(),
        text_style: modifier_slices.text_style(),
        text_layout_options: modifier_slices.text_layout_options(),
        text_pan: modifier_slices.text_pan_resolver(),
        modifier_slices: Some(modifier_slices.as_ref()),
    }) {
        render_children.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(text)),
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
        if layout_state.content_offset != Point::default() {
            child_layer.transform_to_parent =
                child_layer
                    .transform_to_parent
                    .then(ProjectiveTransform::translation(
                        layout_state.content_offset.x,
                        layout_state.content_offset.y,
                    ));
        }
        render_children.push(RenderNode::Layer(Box::new(child_layer)));
    }
    render_children.extend(draw_nodes(
        node_id,
        modifier_slices.draw_commands(),
        DrawPlacement::Overlay,
        layout_state.size(),
        PrimitivePhase::AfterChildren,
    ));
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
        local_bounds,
        transform_to_parent,
        content_offset: layout_state.content_offset,
        motion_context_animated: node_motion_context_animated,
        translated_content_context: local_translated_content_context,
        translated_content_offset: if local_translated_content_context {
            local_translated_content_offset
        } else {
            Point::default()
        },
        scene_children_origin: child_abs.map(|c| c.content_origin).unwrap_or_default(),
        scene_children_layer_translation: child_abs
            .map(|c| c.layer_translation)
            .unwrap_or_default(),
        graphics_layer,
        clip_to_bounds,
        shadow_clip,
        hit_test,
        has_hit_targets,
        has_origin_sinks,
        isolation,
        cache_policy,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children: render_children,
    };
    Some(layer)
}

struct RecorderSlot {
    generation: u64,
    handles: [Option<Rc<Vec<cranpose_ui_graphics::DrawPrimitive>>>; 2],
    recordings: [Option<Rc<cranpose_ui_graphics::CommandRecording>>; 2],
    replay: cranpose_ui_graphics::CommandReplayState,
    replay_epoch: Option<u64>,
    saved_emission: Option<SavedReplayEmission>,
}

struct SavedReplayEmission {
    spans: Vec<cranpose_ui_graphics::FrameSpan>,
    center: cranpose_ui_graphics::Point,
    primitives: Rc<Vec<cranpose_ui_graphics::DrawPrimitive>>,
    recording: Rc<cranpose_ui_graphics::CommandRecording>,
    epoch: u64,
    generation: u64,
}

fn stale_transition_enabled() -> bool {
    matches!(
        crate::debug_toggles::debug_toggle("CRANPOSE_STALE_TRANSITION").as_deref(),
        Some(value) if !value.is_empty() && value != "0"
    )
}

fn sanitized_replay_spans(
    spans: &[cranpose_ui_graphics::FrameSpan],
) -> Vec<cranpose_ui_graphics::FrameSpan> {
    use cranpose_ui_graphics::FrameSpan;
    spans
        .iter()
        .map(|span| match span {
            FrameSpan::Retained {
                capture: true,
                range,
                ..
            } => FrameSpan::Dynamic { range: *range },
            FrameSpan::Retained {
                slot,
                capture: false,
                slot_offset,
                range,
                tape_range,
                transform,
                recolors: _,
                bounds,
            } => FrameSpan::Retained {
                slot: *slot,
                capture: false,
                slot_offset: *slot_offset,
                range: *range,
                tape_range: *tape_range,
                transform: *transform,
                recolors: Vec::new(),
                bounds: *bounds,
            },
            FrameSpan::Dynamic { range } => FrameSpan::Dynamic { range: *range },
        })
        .collect()
}

fn saved_emission_available(id: DrawCommandId) -> bool {
    let Some(epoch) = RETAINED_FEED_EPOCH.with(std::cell::Cell::get) else {
        return false;
    };
    let generation = RECORDING_GENERATION.with(std::cell::Cell::get);
    COMMAND_RECORDINGS.with(|map| {
        map.borrow()
            .get(&id)
            .and_then(|slot| slot.saved_emission.as_ref())
            .is_some_and(|saved| {
                saved.generation.wrapping_add(1) == generation && saved.epoch == epoch
            })
    })
}

fn take_saved_emission(id: DrawCommandId) -> Option<SavedReplayEmission> {
    COMMAND_RECORDINGS.with(|map| {
        map.borrow_mut()
            .get_mut(&id)
            .and_then(|slot| slot.saved_emission.take())
    })
}

fn store_saved_emission(id: DrawCommandId, saved: Option<SavedReplayEmission>) {
    COMMAND_RECORDINGS.with(|map| {
        if let Some(slot) = map.borrow_mut().get_mut(&id) {
            slot.saved_emission = saved;
        }
    });
}

thread_local! {
    static COMMAND_RECORDINGS: std::cell::RefCell<
        std::collections::HashMap<DrawCommandId, RecorderSlot, cranpose_ui_graphics::FxBuildHasher>,
    > = std::cell::RefCell::new(std::collections::HashMap::default());
    static RECORDING_GENERATION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static RETAINED_FEED_EPOCH: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Declares that the renderer consuming graphs built on this thread retains
/// draw-run spans by identity, so scene building should verify command
/// recordings and attach [`cranpose_ui_graphics::CommandReplayFrame`]s to
/// the runs it builds. The epoch names the renderer's retained-slot
/// universe: bumping it (device loss, scale change — anything that dropped
/// slots wholesale) resets every command's verification state, so no frame
/// ever references a slot from a dead universe. Renderers that draw every
/// primitive (pixels) never call this and never pay for verification.
pub fn set_retained_feed_epoch(epoch: Option<u64>) {
    RETAINED_FEED_EPOCH.with(|cell| cell.set(epoch));
}

thread_local! {
    static CONFIRMED_RETAINED_SLOTS: std::cell::RefCell<
        std::collections::HashMap<(DrawCommandId, u32), u64, cranpose_ui_graphics::FxBuildHasher>,
    > = std::cell::RefCell::new(std::collections::HashMap::default());
}

/// The renderer's word that it holds a live retained buffer for this
/// (command, slot) identity, stamped with the retained-feed generation the
/// buffer was captured under. Only confirmed spans may skip materialization:
/// an unconfirmed span's primitives are the renderer's only way to draw it
/// in the same frame. The stamp is what keeps the word LIVE: a confirmation
/// from a dead slot universe (renderer replaced, device lost, root-scale
/// change) stops matching the epoch declared for the build and bypass fails
/// closed instead of trusting a buffer that no longer exists.
pub fn confirm_retained_slot(command: DrawCommandId, slot: u32, generation: u64) {
    CONFIRMED_RETAINED_SLOTS.with(|map| {
        map.borrow_mut().insert((command, slot), generation);
    });
}

/// The renderer released this identity's buffer (aged out, replaced):
/// spans referencing it must materialize again.
pub fn revoke_retained_slot(command: DrawCommandId, slot: u32) {
    CONFIRMED_RETAINED_SLOTS.with(|map| {
        map.borrow_mut().remove(&(command, slot));
    });
}

/// Wholesale retire: every confirmation dies with the slots.
pub fn clear_retained_slot_confirmations() {
    CONFIRMED_RETAINED_SLOTS.with(|map| map.borrow_mut().clear());
}

/// Whether the renderer has confirmed a live retained buffer for this
/// identity UNDER THE EPOCH DECLARED FOR THIS BUILD (readable by tests
/// driving the recorder by hand). No declared epoch means no consumer for
/// bypassed spans, so nothing may skip materialization; a stored generation
/// from another epoch is a buffer of a dead slot universe.
pub fn retained_slot_confirmed(command: DrawCommandId, slot: u32) -> bool {
    let Some(epoch) = RETAINED_FEED_EPOCH.with(std::cell::Cell::get) else {
        return false;
    };
    CONFIRMED_RETAINED_SLOTS.with(|map| map.borrow().get(&(command, slot)) == Some(&epoch))
}

thread_local! {
    static VERIFY_EXECUTOR: std::cell::Cell<
        Option<&'static dyn cranpose_ui_graphics::VerifyExecutor>,
    > = const { std::cell::Cell::new(None) };
}

/// Lends the renderer's frame worker pool to command verification: while
/// set, segment commits at a record boundary run across the pool's lanes
/// instead of on the build thread alone. `None` (the default, and the wasm
/// state) keeps verification serial.
pub fn set_verify_executor(pool: Option<&'static dyn cranpose_ui_graphics::VerifyExecutor>) {
    VERIFY_EXECUTOR.with(|cell| cell.set(pool));
}

/// The executor lent by [`set_verify_executor`], if any.
pub fn verify_executor() -> Option<&'static dyn cranpose_ui_graphics::VerifyExecutor> {
    VERIFY_EXECUTOR.with(|cell| cell.get())
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

fn acquire_recording(
    id: DrawCommandId,
) -> (
    cranpose_ui_graphics::CommandRecording,
    Vec<cranpose_ui_graphics::DrawPrimitive>,
    Option<cranpose_ui_graphics::CommandReplayState>,
) {
    let feed_epoch = RETAINED_FEED_EPOCH.with(std::cell::Cell::get);
    COMMAND_RECORDINGS.with(|map| {
        let mut map = map.borrow_mut();
        let Some(slot) = map.get_mut(&id) else {
            return (
                cranpose_ui_graphics::CommandRecording::default(),
                Vec::new(),
                feed_epoch.map(|_| cranpose_ui_graphics::CommandReplayState::default()),
            );
        };
        let mut recording = cranpose_ui_graphics::CommandRecording::default();
        for shared in &mut slot.recordings {
            if shared
                .as_ref()
                .is_some_and(|shared| Rc::strong_count(shared) == 1)
            {
                let shared = shared.take().expect("checked some above");
                recording = Rc::try_unwrap(shared).expect("sole owner checked above");
                break;
            }
        }
        let replay = feed_epoch.map(|epoch| {
            if slot.replay_epoch == Some(epoch) {
                std::mem::take(&mut slot.replay)
            } else {
                cranpose_ui_graphics::CommandReplayState::default()
            }
        });
        for handle in &mut slot.handles {
            if handle
                .as_ref()
                .is_some_and(|shared| Rc::strong_count(shared) == 1)
            {
                let shared = handle.take().expect("checked some above");
                let storage = Rc::try_unwrap(shared).expect("sole owner checked above");
                return (recording, storage, replay);
            }
        }
        (recording, Vec::new(), replay)
    })
}

fn publish_recording(
    id: DrawCommandId,
    recording: cranpose_ui_graphics::CommandRecording,
    primitives: Vec<cranpose_ui_graphics::DrawPrimitive>,
    replay: Option<cranpose_ui_graphics::CommandReplayState>,
) -> (
    Rc<Vec<cranpose_ui_graphics::DrawPrimitive>>,
    Rc<cranpose_ui_graphics::CommandRecording>,
) {
    let shared = Rc::new(primitives);
    let recording = Rc::new(recording);
    COMMAND_RECORDINGS.with(|map| {
        let mut map = map.borrow_mut();
        let generation = RECORDING_GENERATION.with(std::cell::Cell::get);
        let slot = map.entry(id).or_insert_with(|| RecorderSlot {
            generation,
            handles: [None, None],
            recordings: [None, None],
            replay: cranpose_ui_graphics::CommandReplayState::default(),
            replay_epoch: None,
            saved_emission: None,
        });
        slot.generation = generation;
        if let Some(replay) = replay {
            slot.replay = replay;
            slot.replay_epoch = RETAINED_FEED_EPOCH.with(std::cell::Cell::get);
        }
        slot.recordings[1] = slot.recordings[0].take();
        slot.recordings[0] = Some(recording.clone());
        slot.handles[1] = slot.handles[0].take();
        slot.handles[0] = Some(shared.clone());
    });
    (shared, recording)
}

fn draw_nodes(
    node_id: NodeId,
    commands: &[DrawCommand],
    placement: DrawPlacement,
    size: Size,
    phase: PrimitivePhase,
) -> Vec<RenderNode> {
    let mut nodes = Vec::new();
    let stale_transition = stale_transition_enabled();
    for (command_index, command) in commands.iter().enumerate() {
        let id = DrawCommandId {
            node_id,
            command_index: command_index as u32,
            placement,
        };
        let (recording, storage, mut replay) = acquire_recording(id);
        let stale_available = stale_transition && replay.is_some() && saved_emission_available(id);
        let mut ctx = replay
            .as_mut()
            .map(|state| crate::style_shared::CommandReplayContext {
                state,
                stale_available,
                serve_stale: false,
            });
        let (primitives, recording, frame) = primitives_for_placement_verified(
            command,
            placement,
            size,
            recording,
            storage,
            &mut ctx,
            Some(id),
        );
        if ctx.is_some_and(|ctx| ctx.serve_stale) {
            publish_recording(id, recording, primitives, replay);
            if let Some(saved) = take_saved_emission(id) {
                let frame = cranpose_ui_graphics::CommandReplayFrame {
                    center: saved.center,
                    spans: saved.spans,
                    fallback: Some(saved.recording),
                };
                nodes.push(RenderNode::DrawRun(DrawRunNode::for_command_replayed(
                    phase,
                    Some(id),
                    saved.primitives,
                    Some(Box::new(frame)),
                )));
            } else {
                debug_assert!(false, "serve_stale without a saved emission");
            }
            continue;
        }
        let has_replay_spans = frame.as_ref().is_some_and(|frame| !frame.spans.is_empty());
        if primitives.is_empty() && primitives.capacity() == 0 && !has_replay_spans {
            retain_empty_draw_command(&mut nodes, phase, id, placement, command);
            continue;
        }
        let (shared, published_recording) = publish_recording(id, recording, primitives, replay);
        if stale_transition {
            let saved = frame.as_ref().and_then(|frame| {
                RETAINED_FEED_EPOCH
                    .with(std::cell::Cell::get)
                    .map(|epoch| SavedReplayEmission {
                        spans: sanitized_replay_spans(&frame.spans),
                        center: frame.center,
                        primitives: shared.clone(),
                        recording: published_recording.clone(),
                        epoch,
                        generation: RECORDING_GENERATION.with(std::cell::Cell::get),
                    })
            });
            store_saved_emission(id, saved);
        }
        if shared.is_empty() && !has_replay_spans {
            retain_empty_draw_command(&mut nodes, phase, id, placement, command);
            continue;
        }
        let frame = frame.map(|mut frame| {
            frame.fallback = Some(published_recording);
            frame
        });
        nodes.push(RenderNode::DrawRun(DrawRunNode::for_command_replayed(
            phase,
            Some(id),
            shared,
            frame.map(Box::new),
        )));
    }
    nodes
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
    draw_nodes(node_id, commands, placement, size, phase)
}

struct TextNodeParts<'a> {
    node_id: NodeId,
    local_bounds: Rect,
    measured_max_width: Option<f32>,
    resolved_modifiers: &'a ResolvedModifiers,
    annotated_text: Option<&'a AnnotatedString>,
    text_style: Option<&'a TextStyle>,
    text_layout_options: Option<TextLayoutOptions>,
    text_pan: Option<TextPanResolver>,
    modifier_slices: Option<&'a ModifierNodeSlices>,
}

fn text_node_from_parts(parts: TextNodeParts<'_>) -> Option<TextPrimitiveNode> {
    let TextNodeParts {
        node_id,
        local_bounds,
        measured_max_width,
        resolved_modifiers,
        annotated_text,
        text_style,
        text_layout_options,
        text_pan,
        modifier_slices,
    } = parts;
    let value = annotated_text?;
    let default_text_style = TextStyle::default();
    let text_style = text_style.cloned().unwrap_or(default_text_style);
    let options = text_layout_options.unwrap_or_default().normalized();
    let padding = resolved_modifiers.padding();
    let content_width = (local_bounds.width - padding.left - padding.right).max(0.0);
    if content_width <= 0.0 {
        return None;
    }

    let pan_offset = text_pan
        .as_ref()
        .map(|resolve| resolve(content_width))
        .unwrap_or(0.0);
    let pans_horizontally = text_pan.is_some();

    let max_width = if pans_horizontally {
        None
    } else {
        let measure_width =
            resolve_text_measure_width(content_width, padding, measured_max_width, options);
        Some(measure_width).filter(|width| width.is_finite() && *width > 0.0)
    };
    let prepared = modifier_slices
        .and_then(|slices| slices.prepare_text_layout(max_width))
        .unwrap_or_else(|| prepare_text_layout(value, &text_style, options, max_width));
    let visual_style = prepared.visual_style.clone();
    let measured_draw_width = prepared.metrics.width.max(0.0);
    let draw_width = if options.overflow == TextOverflow::Visible || pans_horizontally {
        measured_draw_width
    } else {
        measured_draw_width.min(content_width)
    };
    let alignment_offset = resolve_text_horizontal_offset(
        &text_style,
        prepared.text.text.as_str(),
        content_width,
        prepared.metrics.width,
    );
    let rect = Rect {
        x: padding.left + alignment_offset - pan_offset,
        y: padding.top,
        width: draw_width,
        height: prepared.metrics.height,
    };
    let text_bounds = Rect {
        x: padding.left,
        y: padding.top,
        width: content_width,
        height: (local_bounds.height - padding.top - padding.bottom).max(0.0),
    };
    let font_size = visual_style.resolve_font_size(14.0);
    let expanded_bounds =
        expand_text_bounds_for_baseline_shift(text_bounds, &visual_style, font_size);
    let clip = if options.overflow == TextOverflow::Visible && !pans_horizontally {
        None
    } else {
        Some(pad_clip_rect(expanded_bounds))
    };

    Some(TextPrimitiveNode {
        node_id,
        rect,
        text: std::rc::Rc::new(prepared.text),
        text_style: visual_style,
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
        motion_context_animated: node.node_data.modifier_slices.motion_context_animated(),
        translated_content_context: node.node_data.modifier_slices.translated_content_context(),
        has_own_origin_sinks: modifier_slices_have_origin_sinks(&node.node_data.modifier_slices),
        measured_max_width: None,
        resolved_modifiers: node.node_data.resolved_modifiers,
        draw_commands: node.node_data.modifier_slices.draw_commands().to_vec(),
        click_actions: node.node_data.modifier_slices.click_handlers().to_vec(),
        pointer_inputs: node.node_data.modifier_slices.pointer_inputs().to_vec(),
        clip_to_bounds: node.node_data.modifier_slices.clip_to_bounds(),
        annotated_text: node.node_data.modifier_slices.annotated_string(),
        text_style: node.node_data.modifier_slices.text_style().cloned(),
        text_layout_options: node.node_data.modifier_slices.text_layout_options(),
        text_pan: node.node_data.modifier_slices.text_pan_resolver(),
        graphics_layer: has_graphics_layer.then_some(graphics_layer),
        children,
    }
}

fn modifier_slices_have_origin_sinks(slices: &ModifierNodeSlices) -> bool {
    slices.text_field_window_origin().is_some() || slices.viewport_window_rect().is_some()
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

fn expand_text_bounds_for_baseline_shift(
    text_bounds: Rect,
    text_style: &TextStyle,
    font_size: f32,
) -> Rect {
    let baseline_shift_px = text_style
        .span_style
        .baseline_shift
        .filter(|shift| shift.is_specified())
        .map(|shift| -(shift.0 * font_size))
        .unwrap_or(0.0);
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

/// The width the paint pass must lay this paragraph out at.
///
/// **It is the width LAYOUT wrapped at, not the width the node ended up.** A
/// `Text` without `fill_max_width` is placed at its own `metrics.width` — the
/// widest line it produced — which is by construction NARROWER than the
/// constraint it wrapped under. Re-wrapping at that narrower width is not the
/// no-op it looks like: the widest line is the one that exactly fills the
/// limit, so measuring it against itself puts its last word over the edge and
/// the paragraph gains a line. Measured against the real font backend
/// (`SoftwareTextMeasurer`, the one the wgpu renderer installs), that fires on
/// 46% of multi-line paragraphs — the block then paints a line taller than the
/// box layout reserved for it, its last line is clipped away, and every
/// following sibling has been placed as if that line did not exist.
///
/// So an unlimited soft-wrapping clip paragraph keeps the measurement width
/// even when the node came out narrower — `may_expand_to_avoid_synthetic_wrap`.
/// The modes that deliberately re-fit (no soft wrap, a finite `max_lines`, or
/// an ellipsis budget) still take the node's own width, because for those the
/// node width IS the fitting constraint.
///
/// This is the shared implementation. It exists because the wgpu and pixels
/// pipelines each grew a private copy WITH this rule and its contract tests,
/// while the scene builder — the copy that the retained render graph actually
/// runs — kept a plain `available.min(content_width)`. The two private copies
/// were reachable only from their own tests. One function now, so the tests
/// guard the code that runs.
pub fn resolve_text_measure_width(
    content_width: f32,
    padding: cranpose_ui::EdgeInsets,
    measured_max_width: Option<f32>,
    options: TextLayoutOptions,
) -> f32 {
    let width = content_width.max(0.0);
    if let Some(max_width) = measured_max_width.filter(|w| w.is_finite() && *w > 0.0) {
        let measured_content_width = (max_width - padding.left - padding.right).max(0.0);
        if measured_content_width <= width {
            return measured_content_width;
        }

        let may_expand_to_avoid_synthetic_wrap = options.soft_wrap
            && options.max_lines == usize::MAX
            && options.overflow == TextOverflow::Clip;
        if may_expand_to_avoid_synthetic_wrap {
            return measured_content_width;
        }
    }
    width
}

/// How much of the slack a `TextAlign` puts *before* the text: 0 at the start
/// edge, 0.5 centred, 1 at the end edge.
///
/// Split out because the same fraction has to be applied twice and by two
/// different pieces of code. Compose aligns a paragraph **line by line** —
/// `TextAlign.Center` centres each line in the paragraph's width, it does not
/// centre the paragraph's box in its parent — so the block offset computed
/// here and the per-line offset the rasteriser applies inside the block are
/// two halves of one rule. They telescope: block at `(box - block) * f`, line
/// at `(block - line) * f`, which sums to `(box - line) * f`, exactly the
/// offset Compose gives that line. Getting one without the other leaves every
/// wrapped continuation line start-aligned under a centred first line.
pub fn text_align_fraction(text_style: &TextStyle, text: &str) -> f32 {
    let paragraph_style = &text_style.paragraph_style;
    let direction = resolve_text_direction(text, Some(paragraph_style.text_direction));
    let rtl = direction == cranpose_ui::text::ResolvedTextDirection::Rtl;
    match paragraph_style.text_align {
        TextAlign::Center => 0.5,
        TextAlign::End | TextAlign::Right => 1.0,
        TextAlign::Start | TextAlign::Left | TextAlign::Justify | TextAlign::Unspecified => {
            if rtl {
                1.0
            } else {
                0.0
            }
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
    remaining * text_align_fraction(text_style, text)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use cranpose_foundation::lazy::{LazyListScope, LazyListState, rememberLazyListState};
    use cranpose_ui::{
        Color, Column, ColumnSpec, DrawCommand, LayoutEngine, LazyColumn, LazyColumnSpec,
        LinearArrangement, Modifier, Point, Rect, ResolvedModifiers, RoundedCornerShape,
        ScrollState, Size, Spacer, Text, TextStyle,
        text::{AnnotatedString, BaselineShift, SpanStyle, TextAlign, TextDirection, TextMotion},
    };
    use cranpose_ui_graphics::{
        Brush, DrawPrimitive, DrawScope as _, DrawScopeDefault, GraphicsLayer, RenderEffect,
    };

    use super::*;

    fn find_text_motion(layer: &LayerNode, label: &str) -> Option<Option<TextMotion>> {
        for child in &layer.children {
            match child {
                RenderNode::Primitive(primitive) => {
                    let PrimitiveNode::Text(text) = &primitive.node else {
                        continue;
                    };
                    if text.text.text == label {
                        return Some(text.text_style.paragraph_style.text_motion);
                    }
                }
                RenderNode::Layer(child_layer) => {
                    if let Some(motion) = find_text_motion(child_layer, label) {
                        return Some(motion);
                    }
                }
                RenderNode::DrawRun(_) => {}
            }
        }

        None
    }

    fn collect_text_labels(layer: &LayerNode, labels: &mut Vec<String>) {
        for child in &layer.children {
            match child {
                RenderNode::Primitive(primitive) => {
                    let PrimitiveNode::Text(text) = &primitive.node else {
                        continue;
                    };
                    labels.push(text.text.text.clone());
                }
                RenderNode::Layer(child_layer) => collect_text_labels(child_layer, labels),
                RenderNode::DrawRun(_) => {}
            }
        }
    }

    fn find_text_top(layer: &LayerNode, label: &str) -> Option<f32> {
        fn search(layer: &LayerNode, label: &str, transform: ProjectiveTransform) -> Option<f32> {
            for child in &layer.children {
                match child {
                    RenderNode::Primitive(primitive) => {
                        let PrimitiveNode::Text(text) = &primitive.node else {
                            continue;
                        };
                        if text.text.text == label {
                            let quad = transform.map_rect(text.rect);
                            let top = quad
                                .iter()
                                .map(|point| point[1])
                                .fold(f32::INFINITY, f32::min);
                            return top.is_finite().then_some(top);
                        }
                    }
                    RenderNode::Layer(child_layer) => {
                        let child_transform = child_layer.transform_to_parent.then(transform);
                        if let Some(top) = search(child_layer, label, child_transform) {
                            return Some(top);
                        }
                    }
                    RenderNode::DrawRun(_) => {}
                }
            }
            None
        }

        search(layer, label, ProjectiveTransform::identity())
    }

    fn find_layer_by_node_id(layer: &LayerNode, node_id: NodeId) -> Option<&LayerNode> {
        if layer.node_id == Some(node_id) {
            return Some(layer);
        }
        layer.children.iter().find_map(|child| match child {
            RenderNode::Layer(child_layer) => find_layer_by_node_id(child_layer, node_id),
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
        })
    }

    fn find_layer_origin(layer: &LayerNode, node_id: NodeId) -> Option<Point> {
        fn search(
            layer: &LayerNode,
            node_id: NodeId,
            transform: ProjectiveTransform,
        ) -> Option<Point> {
            if layer.node_id == Some(node_id) {
                return Some(transform.map_point(Point::default()));
            }
            layer.children.iter().find_map(|child| match child {
                RenderNode::Layer(child_layer) => search(
                    child_layer,
                    node_id,
                    child_layer.transform_to_parent.then(transform),
                ),
                RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
            })
        }

        search(layer, node_id, ProjectiveTransform::identity())
    }

    fn find_translated_content_offset(layer: &LayerNode) -> Option<Point> {
        if layer.translated_content_context {
            return Some(layer.translated_content_offset);
        }
        for child in &layer.children {
            if let RenderNode::Layer(child_layer) = child
                && let Some(offset) = find_translated_content_offset(child_layer)
            {
                return Some(offset);
            }
        }
        None
    }

    fn graph_has_runtime_shader_effect(layer: &LayerNode) -> bool {
        layer
            .graphics_layer
            .render_effect
            .as_ref()
            .is_some_and(RenderEffect::contains_runtime_shader)
            || layer.children.iter().any(|child| match child {
                RenderNode::Layer(child_layer) => graph_has_runtime_shader_effect(child_layer),
                RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
            })
    }

    fn build_layer_node_for_test(
        snapshot: BuildNodeSnapshot,
        scale: f32,
        has_external_backdrop_input: bool,
    ) -> LayerNode {
        let app_context = cranpose_ui::AppContext::new();
        app_context.enter(|| build_layer_node(snapshot, scale, has_external_backdrop_input))
    }

    fn snapshot_with_translation(tx: f32) -> BuildNodeSnapshot {
        let child_command = DrawCommand::Behind(Rc::new(|scope: &mut DrawScopeDefault| {
            scope.push_recorded(vec![DrawPrimitive::Rect {
                rect: Rect {
                    x: 3.0,
                    y: 4.0,
                    width: 20.0,
                    height: 8.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            }]);
        }));

        let child = BuildNodeSnapshot {
            node_id: 2,
            placement: Point { x: 11.0, y: 7.0 },
            size: Size {
                width: 40.0,
                height: 20.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![child_command],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };

        BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: 80.0,
                height: 50.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: Some(GraphicsLayer {
                translation_x: tx,
                ..GraphicsLayer::default()
            }),
            children: vec![child],
        }
    }

    #[test]
    fn parent_translation_changes_layer_transform_but_not_child_local_geometry() {
        let static_graph = build_layer_node_for_test(snapshot_with_translation(0.0), 1.0, false);
        let moved_graph = build_layer_node_for_test(snapshot_with_translation(23.5), 1.0, false);

        let RenderNode::Layer(static_child) = &static_graph.children[0] else {
            panic!("expected child layer");
        };
        let RenderNode::Layer(moved_child) = &moved_graph.children[0] else {
            panic!("expected child layer");
        };
        let RenderNode::DrawRun(static_run) = &static_child.children[0] else {
            panic!("expected draw run");
        };
        let static_draw = &static_run.primitives[0];
        let RenderNode::DrawRun(moved_run) = &moved_child.children[0] else {
            panic!("expected draw run");
        };
        let moved_draw = &moved_run.primitives[0];

        assert_ne!(
            static_graph.transform_to_parent, moved_graph.transform_to_parent,
            "parent transform should encode translation"
        );
        assert_eq!(
            static_draw, moved_draw,
            "child local primitive geometry must stay stable under parent translation"
        );
    }

    #[test]
    fn stored_content_hash_ignores_parent_translation() {
        let static_graph = build_layer_node_for_test(snapshot_with_translation(0.0), 1.0, false);
        let moved_graph = build_layer_node_for_test(snapshot_with_translation(23.5), 1.0, false);

        assert_eq!(
            static_graph.target_content_hash(),
            moved_graph.target_content_hash(),
            "parent rigid motion must not invalidate the subtree content hash"
        );
    }

    #[test]
    fn parent_content_offset_is_encoded_in_child_transform() {
        let child = BuildNodeSnapshot {
            node_id: 2,
            placement: Point { x: 11.0, y: 7.0 },
            size: Size {
                width: 40.0,
                height: 20.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };

        let parent = BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: 80.0,
                height: 50.0,
            },
            content_offset: Point { x: 13.0, y: -9.0 },
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![child],
        };

        let graph = build_layer_node_for_test(parent, 1.0, false);
        let RenderNode::Layer(child) = &graph.children[0] else {
            panic!("expected child layer");
        };

        let top_left = child.transform_to_parent.map_point(Point::default());
        assert_eq!(top_left, Point { x: 24.0, y: -2.0 });
    }

    #[test]
    fn translated_content_offset_changes_visual_position_and_full_surface_hash() {
        fn parent_with_offset(offset: Point, motion_context_animated: bool) -> BuildNodeSnapshot {
            let child_command = DrawCommand::Behind(Rc::new(|scope: &mut DrawScopeDefault| {
                scope.push_recorded(vec![DrawPrimitive::Rect {
                    rect: Rect {
                        x: 3.0,
                        y: 4.0,
                        width: 20.0,
                        height: 8.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                }]);
            }));

            let child = BuildNodeSnapshot {
                node_id: 2,
                placement: Point { x: 11.0, y: 7.0 },
                size: Size {
                    width: 40.0,
                    height: 20.0,
                },
                content_offset: Point::default(),
                motion_context_animated: false,
                translated_content_context: false,
                has_own_origin_sinks: false,
                measured_max_width: None,
                resolved_modifiers: ResolvedModifiers::default(),
                draw_commands: vec![child_command],
                click_actions: vec![],
                pointer_inputs: vec![],
                clip_to_bounds: false,
                annotated_text: None,
                text_style: None,
                text_layout_options: None,
                text_pan: None,
                graphics_layer: None,
                children: vec![],
            };

            BuildNodeSnapshot {
                node_id: 1,
                placement: Point::default(),
                size: Size {
                    width: 80.0,
                    height: 50.0,
                },
                content_offset: offset,
                motion_context_animated,
                translated_content_context: true,
                has_own_origin_sinks: false,
                measured_max_width: None,
                resolved_modifiers: ResolvedModifiers::default(),
                draw_commands: vec![],
                click_actions: vec![],
                pointer_inputs: vec![],
                clip_to_bounds: false,
                annotated_text: None,
                text_style: None,
                text_layout_options: None,
                text_pan: None,
                graphics_layer: None,
                children: vec![child],
            }
        }

        let base = build_layer_node_for_test(
            parent_with_offset(Point { x: 0.0, y: -18.0 }, true),
            1.0,
            false,
        );
        let moved = build_layer_node_for_test(
            parent_with_offset(Point { x: 0.0, y: -32.0 }, true),
            1.0,
            false,
        );
        let rested = build_layer_node_for_test(
            parent_with_offset(Point { x: 0.0, y: -18.0 }, false),
            1.0,
            false,
        );

        let RenderNode::Layer(base_child) = &base.children[0] else {
            panic!("expected child layer");
        };
        let RenderNode::Layer(moved_child) = &moved.children[0] else {
            panic!("expected child layer");
        };

        assert_ne!(
            base_child.transform_to_parent.map_point(Point::default()),
            moved_child.transform_to_parent.map_point(Point::default()),
            "scroll offset still has to move child content visually"
        );
        assert_eq!(
            base_child.target_content_hash(),
            moved_child.target_content_hash(),
            "child source content identity stays stable when only the parent scroll offset changes"
        );
        assert_ne!(
            base.target_content_hash(),
            moved.target_content_hash(),
            "a full-surface cache of the scroll viewport must include the scroll offset"
        );
        assert_ne!(
            base.target_content_hash(),
            rested.target_content_hash(),
            "full-surface cache keys must include active scroll motion policy"
        );
    }

    #[test]
    fn rounded_clip_to_bounds_records_shape_clip_without_runtime_shader() {
        let layer = graphics_layer_with_shaped_clip(
            GraphicsLayer::default(),
            true,
            Some(RoundedCornerShape::new(4.0, 8.0, 12.0, 16.0)),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        );

        assert!(layer.clip);
        assert!(layer.render_effect.is_none());
        let LayerShape::Rounded(shape) = layer.shape else {
            panic!("rounded clip must be recorded as layer shape");
        };
        assert_eq!(shape, RoundedCornerShape::new(4.0, 8.0, 12.0, 16.0));
        assert!(isolation_reasons(&layer).shape_clip);
    }

    #[test]
    fn rounded_clip_to_bounds_keeps_existing_effect_inside_mask() {
        let existing = RenderEffect::blur(3.0);
        let layer = graphics_layer_with_shaped_clip(
            GraphicsLayer {
                render_effect: Some(existing.clone()),
                ..GraphicsLayer::default()
            },
            true,
            Some(RoundedCornerShape::uniform(10.0)),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        );

        let Some(RenderEffect::Chain { first, second }) = layer.render_effect else {
            panic!("existing effect should chain into rounded clip mask");
        };
        assert_eq!(*first, existing);
        assert!(
            matches!(*second, RenderEffect::Shader { .. }),
            "rounded mask must be the outer effect"
        );
    }

    #[test]
    fn rounded_corners_clip_to_bounds_builds_graph_shape_clip_from_modifier_chain() {
        let mut composition = cranpose_ui::run_test_composition(|| {
            cranpose_ui::Box(
                Modifier::empty()
                    .width(100.0)
                    .height(40.0)
                    .rounded_corner_shape(RoundedCornerShape::new(4.0, 8.0, 12.0, 16.0))
                    .clip_to_bounds(),
                cranpose_ui::BoxSpec::default(),
                || {
                    Text("rounded child", Modifier::empty(), TextStyle::default());
                },
            );
        });

        let root = composition.root().expect("rounded clip root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(
                root,
                Size {
                    width: 160.0,
                    height: 100.0,
                },
            )
            .expect("rounded clip layout");
        let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("rounded clip graph");
        applier.clear_runtime_handle();

        let rounded_layer = find_layer_by_node_id(&graph.root, root).expect("rounded layer");
        assert!(rounded_layer.graphics_layer.clip);
        assert!(matches!(
            rounded_layer.graphics_layer.shape,
            LayerShape::Rounded(_)
        ));
        assert!(rounded_layer.graphics_layer.render_effect.is_none());
        assert!(rounded_layer.isolation.shape_clip);
        assert!(
            !graph_has_runtime_shader_effect(&graph.root),
            "simple rounded_corners().clip_to_bounds() must not become a runtime shader effect"
        );
    }

    #[test]
    fn update_graph_from_applier_replaces_dirty_child_layer() {
        let state_holder: Rc<RefCell<Option<cranpose_core::MutableState<String>>>> =
            Rc::new(RefCell::new(None));
        let child_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let state_holder_for_comp = state_holder.clone();
        let child_id_holder_for_comp = child_id_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let label = cranpose_core::rememberMutableStateOf(|| "before".to_string());
            *state_holder_for_comp.borrow_mut() = Some(label);
            let child_id_holder_for_content = child_id_holder_for_comp.clone();
            cranpose_ui::Box(
                Modifier::empty().size_points(240.0, 80.0),
                cranpose_ui::BoxSpec::default(),
                move || {
                    let child_id = Text(label, Modifier::empty(), TextStyle::default());
                    *child_id_holder_for_content.borrow_mut() = Some(child_id);
                    Text("stable", Modifier::empty(), TextStyle::default());
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 240.0,
            height: 80.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
        let child_id = child_id_holder
            .borrow()
            .expect("text child id should be captured");
        let initial_transform = find_layer_by_node_id(&graph.root, child_id)
            .expect("text child layer")
            .transform_to_parent;
        applier.clear_runtime_handle();
        drop(applier);

        let label = state_holder
            .borrow()
            .as_ref()
            .copied()
            .expect("label state should be captured");
        label.set_value("after".to_string());
        composition
            .process_invalid_scopes()
            .expect("text recomposition");

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("updated layout");
        let child_id = child_id_holder
            .borrow()
            .expect("text child id should remain captured");

        assert!(
            update_graph_from_applier(&mut applier, &mut graph, &[child_id], 1.0),
            "dirty child should be replaceable from retained applier state"
        );
        applier.clear_runtime_handle();

        let mut labels = Vec::new();
        collect_text_labels(&graph.root, &mut labels);
        assert!(
            labels.iter().any(|label| label == "after"),
            "updated graph should contain refreshed child text, got {labels:?}"
        );
        assert!(
            !labels.iter().any(|label| label == "before"),
            "updated graph should not retain stale child text, got {labels:?}"
        );
        assert!(
            labels.iter().any(|label| label == "stable"),
            "sibling content should remain present, got {labels:?}"
        );
        assert_eq!(
            find_layer_by_node_id(&graph.root, child_id)
                .expect("updated text child layer")
                .transform_to_parent,
            initial_transform,
            "draw-only child replacement must preserve the retained parent placement transform"
        );
    }

    fn assert_same_cache_hash_state(dirty_road: &LayerNode, full_road: &LayerNode, path: &str) {
        assert_eq!(
            dirty_road.node_id, full_road.node_id,
            "tree shape must match at {path}"
        );
        assert_eq!(
            dirty_road.cache_hashes_valid, full_road.cache_hashes_valid,
            "hash validity at {path} (node {:?})",
            dirty_road.node_id
        );
        if full_road.cache_hashes_valid {
            assert_eq!(
                dirty_road.cache_hashes, full_road.cache_hashes,
                "stored hashes at {path} (node {:?})",
                dirty_road.node_id
            );
        }
        assert_eq!(
            dirty_road.target_content_hash(),
            full_road.target_content_hash(),
            "target content hash at {path} (node {:?})",
            dirty_road.node_id
        );
        assert_eq!(
            dirty_road.children.len(),
            full_road.children.len(),
            "child count at {path}"
        );
        for (index, (dirty_child, full_child)) in dirty_road
            .children
            .iter()
            .zip(full_road.children.iter())
            .enumerate()
        {
            if let (RenderNode::Layer(dirty_child), RenderNode::Layer(full_child)) =
                (dirty_child, full_child)
            {
                assert_same_cache_hash_state(dirty_child, full_child, &format!("{path}/{index}"));
            }
        }
    }

    fn assert_dirty_hash_road_matches_full_walk(graph: &RenderGraph) {
        let mut full_road = graph.root.clone();
        full_road.recompute_raster_cache_hashes();
        assert_same_cache_hash_state(&graph.root, &full_road, "root");
    }

    #[test]
    fn dirty_update_leaves_the_hashes_a_full_walk_leaves() {
        let label_holder: Rc<RefCell<Option<cranpose_core::MutableState<String>>>> =
            Rc::new(RefCell::new(None));
        let child_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let label_holder_for_comp = label_holder.clone();
        let child_id_holder_for_comp = child_id_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let label = cranpose_core::rememberMutableStateOf(|| "before".to_string());
            *label_holder_for_comp.borrow_mut() = Some(label);
            let child_id_holder_for_content = child_id_holder_for_comp.clone();
            Column(
                Modifier::empty().size_points(240.0, 200.0),
                ColumnSpec::default(),
                move || {
                    cranpose_ui::Box(
                        Modifier::empty()
                            .size_points(240.0, 80.0)
                            .graphics_layer(|| GraphicsLayer {
                                alpha: 0.5,
                                ..GraphicsLayer::default()
                            }),
                        cranpose_ui::BoxSpec::default(),
                        {
                            let child_id_holder_for_box = child_id_holder_for_content.clone();
                            move || {
                                let child_id = Text(label, Modifier::empty(), TextStyle::default());
                                *child_id_holder_for_box.borrow_mut() = Some(child_id);
                            }
                        },
                    );
                    cranpose_ui::Box(
                        Modifier::empty()
                            .size_points(240.0, 80.0)
                            .graphics_layer(|| GraphicsLayer {
                                alpha: 0.75,
                                ..GraphicsLayer::default()
                            }),
                        cranpose_ui::BoxSpec::default(),
                        || {
                            Text("stable", Modifier::empty(), TextStyle::default());
                        },
                    );
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 240.0,
            height: 200.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
        graph.root.recompute_raster_cache_hashes();
        applier.clear_runtime_handle();
        drop(applier);
        assert_dirty_hash_road_matches_full_walk(&graph);

        let label = label_holder
            .borrow()
            .as_ref()
            .copied()
            .expect("label state should be captured");
        label.set_value("after".to_string());
        composition
            .process_invalid_scopes()
            .expect("text recomposition");

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("updated layout");
        let child_id = child_id_holder
            .borrow()
            .expect("text child id should be captured");
        let report = update_graph_from_applier_report(&mut applier, &mut graph, &[child_id], 1.0);
        applier.clear_runtime_handle();

        assert!(
            report.applied(),
            "dirty child update should apply in place, got {:?}",
            report.update
        );
        assert_dirty_hash_road_matches_full_walk(&graph);
    }

    #[test]
    fn dirty_update_with_a_new_row_leaves_the_hashes_a_full_walk_leaves() {
        let rows_holder: Rc<RefCell<Option<cranpose_core::MutableState<usize>>>> =
            Rc::new(RefCell::new(None));
        let column_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let rows_holder_for_comp = rows_holder.clone();
        let column_id_holder_for_comp = column_id_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let rows = cranpose_core::rememberMutableStateOf(|| 2usize);
            *rows_holder_for_comp.borrow_mut() = Some(rows);
            let column_id_holder_for_content = column_id_holder_for_comp.clone();
            cranpose_ui::Box(
                Modifier::empty()
                    .size_points(240.0, 240.0)
                    .graphics_layer(|| GraphicsLayer {
                        alpha: 0.5,
                        ..GraphicsLayer::default()
                    }),
                cranpose_ui::BoxSpec::default(),
                move || {
                    let column_id = Column(
                        Modifier::empty().size_points(240.0, 240.0),
                        ColumnSpec::default(),
                        move || {
                            for index in 0..rows.get() {
                                Text(
                                    format!("row {index}"),
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                            }
                        },
                    );
                    *column_id_holder_for_content.borrow_mut() = Some(column_id);
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 240.0,
            height: 240.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
        graph.root.recompute_raster_cache_hashes();
        applier.clear_runtime_handle();
        drop(applier);

        let rows = rows_holder
            .borrow()
            .as_ref()
            .copied()
            .expect("row count state should be captured");
        rows.set_value(3);
        composition
            .process_invalid_scopes()
            .expect("row recomposition");

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("updated layout");
        let column_id = column_id_holder.borrow().expect("column id");
        let report = update_graph_from_applier_report(&mut applier, &mut graph, &[column_id], 1.0);
        applier.clear_runtime_handle();

        assert!(
            report.applied(),
            "structural update should apply in place, got {:?}",
            report.update
        );
        let mut labels = Vec::new();
        collect_text_labels(&graph.root, &mut labels);
        assert!(
            labels.iter().any(|label| label == "row 2"),
            "the new row must be in the patched graph, got {labels:?}"
        );
        assert_dirty_hash_road_matches_full_walk(&graph);
    }

    #[test]
    fn scene_build_publishes_live_window_rect_without_layout_tree() {
        use std::cell::Cell;

        use cranpose_ui::{Box, BoxSpec, MeasureLayoutOptions, measure_layout_with_options};

        let spacer_before = 120.0_f32;
        let sink: Rc<Cell<Rect>> = Rc::new(Cell::new(Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        }));
        let sink_for_comp = sink.clone();
        let mut composition = cranpose_ui::run_test_composition(move || {
            let sink = sink_for_comp.clone();
            Column(
                Modifier::empty().size_points(200.0, 400.0),
                ColumnSpec::default(),
                move || {
                    Spacer(Size {
                        width: 200.0,
                        height: spacer_before,
                    });
                    Box(
                        Modifier::empty()
                            .size_points(200.0, 50.0)
                            .report_window_rect(sink.clone()),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 200.0,
            height: 400.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        measure_layout_with_options(
            &mut applier,
            root,
            viewport,
            MeasureLayoutOptions {
                collect_semantics: false,
                build_layout_tree: false,
            },
        )
        .expect("layout");
        assert_eq!(
            sink.get().height,
            0.0,
            "sink must start empty (place disabled)"
        );

        let _graph = build_graph_from_applier(&mut applier, root, 1.0).expect("scene graph");
        applier.clear_runtime_handle();

        let rect = sink.get();
        assert!(
            (rect.y - spacer_before).abs() < 0.5,
            "scene build must publish the box's live window-y (below the {spacer_before}px \
             spacer), got {}",
            rect.y
        );
        assert!(
            rect.width > 0.0 && rect.height > 0.0,
            "scene build must publish a non-empty window rect, got {rect:?}"
        );
    }

    #[test]
    fn update_graph_from_applier_reports_failed_dirty_child_rebuild() {
        let mut graph = RenderGraph {
            root: build_layer_node_for_test(snapshot_with_translation(0.0), 1.0, false),
        };
        let mut applier = MemoryApplier::new();

        let report = update_graph_from_applier_report(&mut applier, &mut graph, &[2], 1.0);

        assert_eq!(
            report,
            GraphUpdateReport {
                update: GraphUpdate::NeedsRebuild(GraphRebuildReason::DirtyLayerUnavailable),
                hit_graph_dirty: true,
            },
            "dirty child graph updates must not report success when the replacement cannot be rebuilt"
        );
    }

    #[test]
    fn scrolled_list_under_a_composited_layer_keeps_the_hashes_a_full_walk_leaves() {
        let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
        let scroll_holder_for_comp = scroll_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let scroll_state =
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
            *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
            cranpose_ui::Box(
                Modifier::empty()
                    .size_points(240.0, 320.0)
                    .graphics_layer(|| GraphicsLayer {
                        alpha: 0.6,
                        ..GraphicsLayer::default()
                    }),
                cranpose_ui::BoxSpec::default(),
                move || {
                    Column(
                        Modifier::empty()
                            .size_points(240.0, 320.0)
                            .vertical_scroll(scroll_state, false),
                        ColumnSpec::default(),
                        || {
                            for index in 0..12usize {
                                cranpose_ui::Box(
                                    Modifier::empty().size_points(240.0, 60.0).graphics_layer(
                                        || GraphicsLayer {
                                            alpha: 0.8,
                                            ..GraphicsLayer::default()
                                        },
                                    ),
                                    cranpose_ui::BoxSpec::default(),
                                    move || {
                                        Text(
                                            format!("row {index}"),
                                            Modifier::empty(),
                                            TextStyle::default(),
                                        );
                                    },
                                );
                            }
                        },
                    );
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 240.0,
            height: 320.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial scroll layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
        graph.root.recompute_raster_cache_hashes();
        applier.clear_runtime_handle();
        drop(applier);

        let scroll_state = scroll_holder
            .borrow()
            .as_ref()
            .cloned()
            .expect("scroll state should be captured");
        assert!(
            scroll_state.dispatch_raw_delta(96.0) > 0.0,
            "test scroll must be consumed"
        );
        let dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
        assert!(
            !dirty_nodes.is_empty(),
            "a scroll must schedule a scoped scene update"
        );

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("scrolled layout");
        let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);
        applier.clear_runtime_handle();

        assert!(
            report.applied(),
            "scroll update should apply in place, got {:?}",
            report.update
        );
        assert_dirty_hash_road_matches_full_walk(&graph);
    }

    #[test]
    fn update_graph_from_applier_refreshes_scroll_content_offset() {
        let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
        let scroll_holder_for_comp = scroll_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let scroll_state =
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
            *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
            Column(
                Modifier::empty()
                    .size_points(240.0, 120.0)
                    .vertical_scroll(scroll_state, false),
                ColumnSpec::default(),
                || {
                    Text("scroll top", Modifier::empty(), TextStyle::default());
                    Spacer(Size {
                        width: 0.0,
                        height: 160.0,
                    });
                    Text("scroll target", Modifier::empty(), TextStyle::default());
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 240.0,
            height: 120.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial scroll layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
        graph.root.recompute_raster_cache_hashes();
        let initial_target_top =
            find_text_top(&graph.root, "scroll target").expect("initial target text");
        applier.clear_runtime_handle();
        drop(applier);

        let scroll_state = scroll_holder
            .borrow()
            .as_ref()
            .cloned()
            .expect("scroll state should be captured");
        let consumed_scroll = scroll_state.dispatch_raw_delta(96.0);
        assert!(consumed_scroll > 0.0, "test scroll must be consumed");
        let dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
        assert!(
            !dirty_nodes.is_empty(),
            "scroll state invalidation must schedule scoped layout graph update"
        );

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("scrolled layout");
        let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);
        applier.clear_runtime_handle();

        assert!(
            report.applied(),
            "scroll graph update should apply in place, got {:?}",
            report.update
        );
        let updated_target_top =
            find_text_top(&graph.root, "scroll target").expect("updated target text");
        assert!(
            updated_target_top < initial_target_top - consumed_scroll * 0.75,
            "partial graph update must refresh scroll content offset: initial_y={initial_target_top} updated_y={updated_target_top} dirty_nodes={dirty_nodes:?}"
        );
        assert_dirty_hash_road_matches_full_walk(&graph);
    }

    #[test]
    fn an_overmarked_ancestor_chain_still_translates_instead_of_relowering() {
        let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
        let scroll_holder_for_comp = scroll_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let scroll_state =
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
            *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
            cranpose_ui::Box(
                Modifier::empty().size_points(240.0, 320.0),
                cranpose_ui::BoxSpec::default(),
                move || {
                    Column(
                        Modifier::empty()
                            .size_points(240.0, 320.0)
                            .vertical_scroll(scroll_state, false),
                        ColumnSpec::default(),
                        || {
                            for index in 0..12usize {
                                cranpose_ui::Box(
                                    Modifier::empty()
                                        .size_points(240.0, 60.0)
                                        .background(Color(0.9, 0.9, 0.92, 1.0)),
                                    cranpose_ui::BoxSpec::default(),
                                    move || {
                                        Text(
                                            format!("row {index}"),
                                            Modifier::empty(),
                                            TextStyle::default(),
                                        );
                                    },
                                );
                            }
                        },
                    );
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 240.0,
            height: 320.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial scroll layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
        graph.root.recompute_raster_cache_hashes();
        let initial_row_top = find_text_top(&graph.root, "row 3").expect("initial row text");
        applier.clear_runtime_handle();
        drop(applier);

        let scroll_state = scroll_holder
            .borrow()
            .as_ref()
            .cloned()
            .expect("scroll state should be captured");
        let consumed_scroll = scroll_state.dispatch_raw_delta(96.0);
        assert!(consumed_scroll > 0.0, "test scroll must be consumed");
        let mut dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
        dirty_nodes.push(graph.root.node_id.expect("root id"));
        let mut cursor = &graph.root;
        while let Some(RenderNode::Layer(child)) = cursor
            .children
            .iter()
            .find(|child| matches!(child, RenderNode::Layer(_)))
        {
            let chain = child.node_id.expect("chain node id");
            if dirty_nodes.contains(&chain) {
                break;
            }
            dirty_nodes.push(chain);
            cursor = child;
        }

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("scrolled layout");
        reset_lowered_layer_count();
        let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);
        applier.clear_runtime_handle();

        assert!(
            report.applied(),
            "chain update should apply in place, got {:?}",
            report.update
        );
        let lowered = lowered_layer_count();
        assert_eq!(
            lowered, 0,
            "an over-marked ancestor chain over a pure scroll must still \
             translate; {lowered} layers were rebuilt (dirty={dirty_nodes:?})"
        );
        let updated_row_top = find_text_top(&graph.root, "row 3").expect("updated row text");
        assert!(
            updated_row_top < initial_row_top - consumed_scroll * 0.75,
            "the translation must land through the chain: initial_y={initial_row_top} updated_y={updated_row_top}"
        );
        assert_dirty_hash_road_matches_full_walk(&graph);
    }

    #[test]
    fn a_scrolled_container_translates_clean_children_instead_of_relowering() {
        let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
        let scroll_holder_for_comp = scroll_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let scroll_state =
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
            *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
            Column(
                Modifier::empty()
                    .size_points(240.0, 320.0)
                    .vertical_scroll(scroll_state, false),
                ColumnSpec::default(),
                || {
                    for index in 0..12usize {
                        cranpose_ui::Box(
                            Modifier::empty()
                                .size_points(240.0, 60.0)
                                .background(Color(0.9, 0.9, 0.92, 1.0)),
                            cranpose_ui::BoxSpec::default(),
                            move || {
                                Text(
                                    format!("row {index}"),
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                            },
                        );
                    }
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 240.0,
            height: 320.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial scroll layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
        graph.root.recompute_raster_cache_hashes();
        let initial_row_top = find_text_top(&graph.root, "row 3").expect("initial row text");
        applier.clear_runtime_handle();
        drop(applier);

        let scroll_state = scroll_holder
            .borrow()
            .as_ref()
            .cloned()
            .expect("scroll state should be captured");
        let consumed_scroll = scroll_state.dispatch_raw_delta(96.0);
        assert!(consumed_scroll > 0.0, "test scroll must be consumed");
        let dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
        assert!(
            !dirty_nodes.is_empty(),
            "a scroll must schedule a scoped scene update"
        );

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("scrolled layout");
        reset_lowered_layer_count();
        let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);
        applier.clear_runtime_handle();

        assert!(
            report.applied(),
            "scroll update should apply in place, got {:?}",
            report.update
        );
        let lowered = lowered_layer_count();
        assert_eq!(
            lowered, 0,
            "a pure scroll of clean children must translate the retained \
             subtrees, not re-lower them; {lowered} layers were rebuilt"
        );
        let updated_row_top = find_text_top(&graph.root, "row 3").expect("updated row text");
        assert!(
            updated_row_top < initial_row_top - consumed_scroll * 0.75,
            "the translation must actually land: initial_y={initial_row_top} updated_y={updated_row_top}"
        );
        assert_dirty_hash_road_matches_full_walk(&graph);
    }

    #[test]
    fn a_sliding_lazy_window_lowers_only_the_entering_rows() {
        let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let state_holder_for_comp = state_holder.clone();
        let mut composition = cranpose_ui::run_test_composition(move || {
            let list_state = rememberLazyListState();
            *state_holder_for_comp.borrow_mut() = Some(list_state);
            LazyColumn(
                Modifier::empty().size_points(240.0, 320.0),
                list_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(60, |index| {
                        cranpose_ui::Box(
                            Modifier::empty()
                                .size_points(240.0, 60.0)
                                .background(Color(0.9, 0.9, 0.92, 1.0)),
                            cranpose_ui::BoxSpec::default(),
                            move || {
                                Text(
                                    format!("row {index}"),
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                            },
                        );
                    });
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 240.0,
            height: 320.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial lazy layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
        graph.root.recompute_raster_cache_hashes();
        let _ = applier.take_structural_change_parents_attached_to(root);
        let initial_row_top = find_text_top(&graph.root, "row 4").expect("initial row text");
        applier.clear_runtime_handle();
        drop(applier);

        let list_state = (*state_holder.borrow()).expect("list state should be captured");
        let consumed = list_state.dispatch_scroll_delta(-96.0);
        assert!(consumed != 0.0, "the lazy scroll must consume the delta");
        let mut dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
        dirty_nodes.extend(cranpose_ui::pending_measure_repass_nodes_snapshot());

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("scrolled lazy layout");
        dirty_nodes.extend(applier.take_structural_change_parents_attached_to(root));
        dirty_nodes.sort_unstable();
        dirty_nodes.dedup();
        assert!(
            !dirty_nodes.is_empty(),
            "a lazy scroll must mark the list dirty"
        );

        reset_lowered_layer_count();
        let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);
        applier.clear_runtime_handle();

        assert!(
            report.applied(),
            "the boundary frame must apply in place, got {:?}",
            report.update
        );
        let lowered = lowered_layer_count();
        assert!(
            lowered > 0,
            "rows crossed the window boundary; the entering subtrees must lower"
        );
        assert!(
            lowered <= 8,
            "only the entering rows may lower on a boundary frame; \
             {lowered} layers were rebuilt (dirty={dirty_nodes:?})"
        );
        let updated_row_top = find_text_top(&graph.root, "row 4").expect("updated row text");
        assert!(
            updated_row_top < initial_row_top - 60.0,
            "the retained rows must move with the scroll: \
             initial_y={initial_row_top} updated_y={updated_row_top}"
        );
        assert_dirty_hash_road_matches_full_walk(&graph);
    }

    fn collect_text_tops(layer: &LayerNode) -> std::collections::BTreeMap<String, i64> {
        fn walk(
            layer: &LayerNode,
            transform: ProjectiveTransform,
            out: &mut std::collections::BTreeMap<String, i64>,
        ) {
            for child in &layer.children {
                match child {
                    RenderNode::Primitive(primitive) => {
                        if let PrimitiveNode::Text(text) = &primitive.node {
                            let quad = transform.map_rect(text.rect);
                            let top = quad
                                .iter()
                                .map(|point| point[1])
                                .fold(f32::INFINITY, f32::min);
                            if top.is_finite() {
                                out.insert(text.text.text.clone(), (top * 10.0).round() as i64);
                            }
                        }
                    }
                    RenderNode::Layer(child_layer) => {
                        let child_transform = child_layer.transform_to_parent.then(transform);
                        walk(child_layer, child_transform, out);
                    }
                    RenderNode::DrawRun(_) => {}
                }
            }
        }
        let mut out = std::collections::BTreeMap::new();
        walk(layer, ProjectiveTransform::identity(), &mut out);
        out
    }

    #[test]
    fn a_lazy_jump_of_any_distance_patches_to_what_a_fresh_build_shows() {
        for delta in [-30.0f32, -60.0, -96.0, -180.0, -300.0] {
            let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
            let state_holder_for_comp = state_holder.clone();
            let mut composition = cranpose_ui::run_test_composition(move || {
                let list_state = rememberLazyListState();
                *state_holder_for_comp.borrow_mut() = Some(list_state);
                LazyColumn(
                    Modifier::empty().size_points(240.0, 320.0),
                    list_state,
                    LazyColumnSpec::default(),
                    |scope| {
                        scope.items(60, |index| {
                            cranpose_ui::Box(
                                Modifier::empty()
                                    .size_points(240.0, 60.0)
                                    .background(Color(0.9, 0.9, 0.92, 1.0)),
                                cranpose_ui::BoxSpec::default(),
                                move || {
                                    Text(
                                        format!("row {index}"),
                                        Modifier::empty(),
                                        TextStyle::default(),
                                    );
                                },
                            );
                        });
                    },
                );
            });

            let root = composition.root().expect("composition root");
            let viewport = Size {
                width: 240.0,
                height: 320.0,
            };
            let handle = composition.runtime_handle();
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(handle);
            applier
                .compute_layout(root, viewport)
                .expect("initial lazy layout");
            let mut graph =
                build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
            graph.root.recompute_raster_cache_hashes();
            let _ = applier.take_structural_change_parents_attached_to(root);
            applier.clear_runtime_handle();
            drop(applier);

            let list_state = (*state_holder.borrow()).expect("list state should be captured");
            let consumed = list_state.dispatch_scroll_delta(delta);
            assert!(consumed != 0.0, "delta {delta}: the scroll must consume");
            let mut dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
            dirty_nodes.extend(cranpose_ui::pending_measure_repass_nodes_snapshot());

            let handle = composition.runtime_handle();
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(handle);
            applier
                .compute_layout(root, viewport)
                .expect("scrolled lazy layout");
            dirty_nodes.extend(applier.take_structural_change_parents_attached_to(root));
            dirty_nodes.sort_unstable();
            dirty_nodes.dedup();
            reset_lowered_layer_count();
            let report =
                update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);
            assert!(report.applied(), "delta {delta}: boundary frame must apply");
            if delta == -30.0 {
                assert_eq!(
                    lowered_layer_count(),
                    0,
                    "a buffer-absorbed scroll must not lower any layer"
                );
            }

            let fresh =
                build_graph_from_applier(&mut applier, root, 1.0).expect("fresh comparison graph");
            applier.clear_runtime_handle();

            let patched_texts = collect_text_tops(&graph.root);
            let fresh_texts = collect_text_tops(&fresh.root);
            assert_eq!(
                patched_texts, fresh_texts,
                "delta {delta}: the patched scene must show exactly what a \
                 fresh build shows (dirty={dirty_nodes:?})"
            );
        }
    }

    #[test]
    fn update_graph_from_applier_keeps_parent_content_offset_for_dirty_scroll_child() {
        let label_holder: Rc<RefCell<Option<cranpose_core::MutableState<String>>>> =
            Rc::new(RefCell::new(None));
        let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
        let child_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let label_holder_for_comp = label_holder.clone();
        let scroll_holder_for_comp = scroll_holder.clone();
        let child_id_holder_for_comp = child_id_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let label =
                cranpose_core::rememberMutableStateOf(|| "scrolled child before".to_string());
            let scroll_state =
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
            *label_holder_for_comp.borrow_mut() = Some(label);
            *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
            let child_id_holder_for_content = child_id_holder_for_comp.clone();
            Column(
                Modifier::empty()
                    .size_points(260.0, 90.0)
                    .vertical_scroll(scroll_state, false),
                ColumnSpec::default(),
                move || {
                    Spacer(Size {
                        width: 0.0,
                        height: 24.0,
                    });
                    let child_id = Text(label, Modifier::empty(), TextStyle::default());
                    *child_id_holder_for_content.borrow_mut() = Some(child_id);
                    Spacer(Size {
                        width: 0.0,
                        height: 220.0,
                    });
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 260.0,
            height: 90.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial layout");
        applier.clear_runtime_handle();
        drop(applier);

        let scroll_state = scroll_holder
            .borrow()
            .as_ref()
            .cloned()
            .expect("scroll state should be captured");
        assert!(scroll_state.dispatch_raw_delta(36.0) > 0.0);

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("scrolled layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("scrolled graph");
        let child_id = child_id_holder
            .borrow()
            .expect("text child id should be captured");
        let scrolled_transform = find_layer_by_node_id(&graph.root, child_id)
            .expect("scrolled child layer")
            .transform_to_parent;
        applier.clear_runtime_handle();
        drop(applier);

        let label = label_holder
            .borrow()
            .as_ref()
            .copied()
            .expect("label state should be captured");
        label.set_value("scrolled child after".to_string());
        composition
            .process_invalid_scopes()
            .expect("text recomposition");

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("updated scrolled layout");
        let child_id = child_id_holder
            .borrow()
            .expect("text child id should remain captured");
        let report = update_graph_from_applier_report(&mut applier, &mut graph, &[child_id], 1.0);
        applier.clear_runtime_handle();

        assert!(
            report.applied(),
            "dirty child graph update should apply, got {:?}",
            report.update
        );
        let updated = find_layer_by_node_id(&graph.root, child_id).expect("updated child layer");
        assert_eq!(
            updated.transform_to_parent, scrolled_transform,
            "dirty child replacement inside a scrolled parent must keep the parent's content-offset transform"
        );
        let mut labels = Vec::new();
        collect_text_labels(&graph.root, &mut labels);
        assert!(
            labels.iter().any(|label| label == "scrolled child after"),
            "updated graph should contain refreshed text, got {labels:?}"
        );
    }

    #[test]
    fn dirty_scrolled_overlay_graphics_layer_stays_aligned_with_underlay() {
        let alpha_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
            Rc::new(RefCell::new(None));
        let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
        let underlay_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let overlay_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let alpha_holder_for_comp = alpha_holder.clone();
        let scroll_holder_for_comp = scroll_holder.clone();
        let underlay_id_holder_for_comp = underlay_id_holder.clone();
        let overlay_id_holder_for_comp = overlay_id_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let alpha = cranpose_core::rememberMutableStateOf(|| 1.0f32);
            let scroll_state =
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
            *alpha_holder_for_comp.borrow_mut() = Some(alpha);
            *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
            let underlay_id_holder_for_content = underlay_id_holder_for_comp.clone();
            let overlay_id_holder_for_content = overlay_id_holder_for_comp.clone();
            Column(
                Modifier::empty()
                    .size_points(260.0, 120.0)
                    .vertical_scroll(scroll_state, false),
                ColumnSpec::default(),
                move || {
                    Spacer(Size {
                        width: 0.0,
                        height: 180.0,
                    });
                    cranpose_ui::Box(
                        Modifier::empty().size_points(188.0, 88.0),
                        cranpose_ui::BoxSpec::default(),
                        {
                            let underlay_id_holder_for_box = underlay_id_holder_for_content.clone();
                            let overlay_id_holder_for_box = overlay_id_holder_for_content.clone();
                            move || {
                                let underlay_id = cranpose_ui::Box(
                                    Modifier::empty().size_points(188.0, 88.0),
                                    cranpose_ui::BoxSpec::default(),
                                    || {
                                        Text(
                                            "UNDERLAY CONTENT",
                                            Modifier::empty().absolute_offset(12.0, 8.0),
                                            TextStyle::default(),
                                        );
                                    },
                                );
                                *underlay_id_holder_for_box.borrow_mut() = Some(underlay_id);
                                let overlay_id = cranpose_ui::Box(
                                    Modifier::empty().size_points(188.0, 88.0).graphics_layer(
                                        move || GraphicsLayer {
                                            alpha: alpha.get(),
                                            ..GraphicsLayer::default()
                                        },
                                    ),
                                    cranpose_ui::BoxSpec::default(),
                                    || {
                                        Text(
                                            "TOP LAYER",
                                            Modifier::empty().absolute_offset(74.0, 39.6),
                                            TextStyle::default(),
                                        );
                                    },
                                );
                                *overlay_id_holder_for_box.borrow_mut() = Some(overlay_id);
                            }
                        },
                    );
                    Spacer(Size {
                        width: 0.0,
                        height: 280.0,
                    });
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 260.0,
            height: 120.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial layout");
        applier.clear_runtime_handle();
        drop(applier);

        let scroll_state = scroll_holder
            .borrow()
            .as_ref()
            .cloned()
            .expect("scroll state should be captured");
        assert!(scroll_state.dispatch_raw_delta(96.0) > 0.0);

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("scrolled layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("scrolled graph");
        applier.clear_runtime_handle();
        drop(applier);

        let underlay_id = underlay_id_holder
            .borrow()
            .expect("underlay id should be captured");
        let overlay_id = overlay_id_holder
            .borrow()
            .expect("overlay id should be captured");
        let scrolled_underlay_origin =
            find_layer_origin(&graph.root, underlay_id).expect("underlay origin");
        let scrolled_overlay_origin =
            find_layer_origin(&graph.root, overlay_id).expect("overlay origin");
        assert_eq!(scrolled_underlay_origin, scrolled_overlay_origin);

        let alpha = alpha_holder
            .borrow()
            .as_ref()
            .copied()
            .expect("alpha state should be captured");
        alpha.set_value(0.35);

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let report = update_graph_from_applier_report(&mut applier, &mut graph, &[overlay_id], 1.0);
        applier.clear_runtime_handle();

        assert!(
            report.applied(),
            "dirty overlay graph update should apply, got {:?}",
            report.update
        );
        let updated_underlay_origin =
            find_layer_origin(&graph.root, underlay_id).expect("updated underlay origin");
        let updated_overlay_origin =
            find_layer_origin(&graph.root, overlay_id).expect("updated overlay origin");
        assert_eq!(
            updated_underlay_origin, scrolled_underlay_origin,
            "stable underlay must keep its scrolled origin"
        );
        assert_eq!(
            updated_overlay_origin, updated_underlay_origin,
            "dirty overlay graphics layer must stay aligned with its stable underlay"
        );
    }

    #[test]
    fn update_graph_from_applier_refreshes_dirty_graphics_layer_transform() {
        let offset_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
            Rc::new(RefCell::new(None));
        let node_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let offset_holder_for_comp = offset_holder.clone();
        let node_id_holder_for_comp = node_id_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let offset = cranpose_core::rememberMutableStateOf(|| 0.0f32);
            *offset_holder_for_comp.borrow_mut() = Some(offset);
            let node_id = cranpose_ui::Box(
                Modifier::empty()
                    .size_points(40.0, 20.0)
                    .graphics_layer(move || GraphicsLayer {
                        translation_x: offset.get(),
                        ..GraphicsLayer::default()
                    }),
                cranpose_ui::BoxSpec::default(),
                || {},
            );
            *node_id_holder_for_comp.borrow_mut() = Some(node_id);
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 120.0,
            height: 80.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
        let node_id = node_id_holder
            .borrow()
            .expect("graphics layer node id should be captured");
        let initial_origin = find_layer_by_node_id(&graph.root, node_id)
            .expect("initial graphics layer")
            .transform_to_parent
            .map_point(Point::default());
        applier.clear_runtime_handle();
        drop(applier);

        let offset = offset_holder
            .borrow()
            .as_ref()
            .copied()
            .expect("offset state should be captured");
        offset.set_value(32.0);

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let report = update_graph_from_applier_report(&mut applier, &mut graph, &[node_id], 1.0);
        assert!(
            report.applied(),
            "dirty graphics layer should be replaceable from retained applier state, got {:?}",
            report.update
        );
        assert!(
            !report.hit_graph_dirty,
            "a moved visual-only layer should not force hit graph refresh"
        );
        applier.clear_runtime_handle();

        let updated_origin = find_layer_by_node_id(&graph.root, node_id)
            .expect("updated graphics layer")
            .transform_to_parent
            .map_point(Point::default());
        assert!(
            (updated_origin.x - (initial_origin.x + 32.0)).abs() < 0.1,
            "scoped graph update must refresh graphics-layer translation: initial={initial_origin:?} updated={updated_origin:?}"
        );
    }

    #[test]
    fn update_graph_from_applier_reports_hit_dirty_for_moved_clickable_layer() {
        let offset_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
            Rc::new(RefCell::new(None));
        let node_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let offset_holder_for_comp = offset_holder.clone();
        let node_id_holder_for_comp = node_id_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let offset = cranpose_core::rememberMutableStateOf(|| 0.0f32);
            *offset_holder_for_comp.borrow_mut() = Some(offset);
            let node_id = cranpose_ui::Box(
                Modifier::empty()
                    .size_points(40.0, 20.0)
                    .graphics_layer(move || GraphicsLayer {
                        translation_x: offset.get(),
                        ..GraphicsLayer::default()
                    })
                    .clickable(|_| {}),
                cranpose_ui::BoxSpec::default(),
                || {},
            );
            *node_id_holder_for_comp.borrow_mut() = Some(node_id);
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 120.0,
            height: 80.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
        let node_id = node_id_holder
            .borrow()
            .expect("graphics layer node id should be captured");
        applier.clear_runtime_handle();
        drop(applier);

        let offset = offset_holder
            .borrow()
            .as_ref()
            .copied()
            .expect("offset state should be captured");
        offset.set_value(32.0);

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let report = update_graph_from_applier_report(&mut applier, &mut graph, &[node_id], 1.0);
        applier.clear_runtime_handle();

        assert!(
            report.applied(),
            "dirty clickable graphics layer should be replaceable from retained applier state, got {:?}",
            report.update
        );
        assert!(
            report.hit_graph_dirty,
            "moved clickable layers must refresh hit geometry"
        );
    }

    #[test]
    fn overlay_draw_commands_are_tagged_after_children() {
        let child = BuildNodeSnapshot {
            node_id: 2,
            placement: Point { x: 4.0, y: 5.0 },
            size: Size {
                width: 20.0,
                height: 10.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };
        let behind = DrawCommand::Behind(Rc::new(|scope: &mut DrawScopeDefault| {
            scope.push_recorded(vec![cranpose_ui_graphics::DrawPrimitive::Rect {
                rect: Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 8.0,
                    height: 6.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            }]);
        }));
        let overlay = DrawCommand::Overlay(Rc::new(|scope: &mut DrawScopeDefault| {
            scope.push_recorded(vec![cranpose_ui_graphics::DrawPrimitive::Rect {
                rect: Rect {
                    x: 3.0,
                    y: 1.0,
                    width: 5.0,
                    height: 4.0,
                },
                brush: Brush::solid(Color::BLACK),
                stroke: None,
            }]);
        }));

        let parent = BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: 80.0,
                height: 50.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![behind, overlay],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![child],
        };

        let graph = build_layer_node_for_test(parent, 1.0, false);
        let RenderNode::DrawRun(behind) = &graph.children[0] else {
            panic!("expected before-children draw run");
        };
        let RenderNode::Layer(_) = &graph.children[1] else {
            panic!("expected child layer");
        };
        let RenderNode::DrawRun(overlay) = &graph.children[2] else {
            panic!("expected after-children draw run");
        };

        assert_eq!(behind.phase, PrimitivePhase::BeforeChildren);
        assert_eq!(overlay.phase, PrimitivePhase::AfterChildren);
    }

    #[test]
    fn command_recordings_reuse_buffers_across_rebuilds() {
        let snapshot = || BuildNodeSnapshot {
            node_id: 7001,
            placement: Point::default(),
            size: Size {
                width: 40.0,
                height: 20.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![DrawCommand::Behind(Rc::new(
                |scope: &mut DrawScopeDefault| {
                    scope.draw_rect_at(
                        Rect {
                            x: 1.0,
                            y: 2.0,
                            width: 8.0,
                            height: 6.0,
                        },
                        Brush::solid(Color::WHITE),
                    );
                },
            ))],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };
        fn run_of(layer: &LayerNode) -> &DrawRunNode {
            let RenderNode::DrawRun(run) = &layer.children[0] else {
                panic!("expected draw run");
            };
            run
        }

        let graph_a = build_layer_node_for_test(snapshot(), 1.0, false);
        let ptr_a = run_of(&graph_a).primitives.as_ptr();

        let graph_b = build_layer_node_for_test(snapshot(), 1.0, false);
        let ptr_b = run_of(&graph_b).primitives.as_ptr();
        assert_ne!(
            ptr_a, ptr_b,
            "a buffer a live graph shares must never be recorded into"
        );
        assert_eq!(
            run_of(&graph_a).primitives,
            run_of(&graph_b).primitives,
            "re-recording must reproduce the recording"
        );

        drop(graph_a);
        let graph_c = build_layer_node_for_test(snapshot(), 1.0, false);
        assert_eq!(
            run_of(&graph_c).primitives.as_ptr(),
            ptr_a,
            "the released buffer must be reused for the next recording"
        );

        let held = std::rc::Rc::clone(&run_of(&graph_c).primitives);
        drop(graph_c);
        let graph_d = build_layer_node_for_test(snapshot(), 1.0, false);
        let ptr_d = run_of(&graph_d).primitives.as_ptr();
        assert_ne!(ptr_d, held.as_ptr());
        assert_ne!(ptr_d, run_of(&graph_b).primitives.as_ptr());
    }

    #[test]
    fn stored_content_hash_changes_when_child_transform_changes() {
        let child = BuildNodeSnapshot {
            node_id: 2,
            placement: Point { x: 4.0, y: 5.0 },
            size: Size {
                width: 20.0,
                height: 10.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };
        let mut moved_child = child.clone();
        moved_child.placement.x += 7.0;

        let parent = BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: 80.0,
                height: 50.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![child],
        };
        let moved_parent = BuildNodeSnapshot {
            children: vec![moved_child],
            ..parent.clone()
        };

        let static_graph = build_layer_node_for_test(parent, 1.0, false);
        let moved_graph = build_layer_node_for_test(moved_parent, 1.0, false);

        assert_ne!(
            static_graph.target_content_hash(),
            moved_graph.target_content_hash(),
            "moving a child within the parent must invalidate the parent subtree hash"
        );
    }

    #[test]
    fn stored_effect_hash_tracks_local_effect_only() {
        let base = BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: 80.0,
                height: 50.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };
        let mut effected = base.clone();
        effected.graphics_layer = Some(GraphicsLayer {
            render_effect: Some(cranpose_ui_graphics::RenderEffect::blur(6.0)),
            ..GraphicsLayer::default()
        });

        let base_graph = build_layer_node_for_test(base, 1.0, false);
        let effected_graph = build_layer_node_for_test(effected, 1.0, false);

        assert_eq!(
            base_graph.target_content_hash(),
            effected_graph.target_content_hash(),
            "post-processing effect parameters belong to the effect hash, not the content hash"
        );
        assert_ne!(base_graph.effect_hash(), effected_graph.effect_hash());
    }

    #[test]
    fn text_node_preserves_rtl_alignment_clip_and_baseline_shift() {
        let mut text_style = TextStyle::default();
        text_style.paragraph_style.text_align = TextAlign::Start;
        text_style.paragraph_style.text_direction = TextDirection::Rtl;
        text_style.span_style.baseline_shift = Some(BaselineShift::SUPERSCRIPT);

        let snapshot = BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: 180.0,
                height: 48.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: Some(180.0),
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: Some(AnnotatedString::from("rtl")),
            text_style: Some(text_style),
            text_layout_options: Some(cranpose_ui::TextLayoutOptions {
                overflow: cranpose_ui::TextOverflow::Clip,
                ..Default::default()
            }),
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };

        let graph = build_layer_node_for_test(snapshot, 1.0, false);
        let RenderNode::Primitive(text_primitive) = &graph.children[0] else {
            panic!("expected text primitive");
        };
        let PrimitiveNode::Text(text) = &text_primitive.node else {
            panic!("expected text primitive");
        };
        let clip = text
            .clip
            .expect("clipped overflow should produce a clip rect");

        assert!(
            text.rect.x > 0.0,
            "RTL start alignment should shift the text rect within the available width"
        );
        assert!(
            clip.y < text.rect.y,
            "baseline shift must expand the clip upward so superscript glyphs are preserved"
        );
        assert!(
            clip.intersect(text.rect).is_some(),
            "the clip rect must intersect the shifted text draw rect"
        );
    }

    #[test]
    fn clipped_text_node_raster_bounds_use_measured_text_width_not_full_box() {
        let snapshot = BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: 320.0,
                height: 48.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: Some(320.0),
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: Some(AnnotatedString::from("short")),
            text_style: Some(TextStyle::default()),
            text_layout_options: Some(cranpose_ui::TextLayoutOptions {
                overflow: cranpose_ui::TextOverflow::Clip,
                ..Default::default()
            }),
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };

        let graph = build_layer_node_for_test(snapshot, 1.0, false);
        let RenderNode::Primitive(text_primitive) = &graph.children[0] else {
            panic!("expected text primitive");
        };
        let PrimitiveNode::Text(text) = &text_primitive.node else {
            panic!("expected text primitive");
        };
        let clip = text.clip.expect("clipped text should keep a clip rect");

        assert!(
            text.rect.width < 320.0,
            "text raster bounds should track measured glyph width instead of full content width"
        );
        assert_eq!(
            clip.width, 322.0,
            "text clip should still preserve the full content box plus clip padding"
        );
    }

    #[test]
    fn text_field_pan_shifts_glyphs_and_clips_to_field_bounds() {
        let pan_offset = 25.0_f32;
        let field_width = 80.0_f32;
        let resolved_viewports = Rc::new(std::cell::RefCell::new(Vec::new()));
        let viewports = resolved_viewports.clone();
        let make_snapshot = |text_pan: Option<cranpose_ui::TextPanResolver>| BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: field_width,
                height: 24.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: Some(field_width),
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: Some(AnnotatedString::from(
                "a very long single line of text that cannot fit",
            )),
            text_style: Some(TextStyle::default()),
            text_layout_options: Some(cranpose_ui::TextLayoutOptions::default()),
            text_pan,
            graphics_layer: None,
            children: vec![],
        };

        let text_node = |snapshot: BuildNodeSnapshot| {
            let graph = build_layer_node_for_test(snapshot, 1.0, false);
            let RenderNode::Primitive(text_primitive) = &graph.children[0] else {
                panic!("expected text primitive");
            };
            let PrimitiveNode::Text(text) = &text_primitive.node else {
                panic!("expected text primitive");
            };
            (**text).clone()
        };

        let unpanned = text_node(make_snapshot(None));
        let panned = text_node(make_snapshot(Some(Rc::new(move |viewport| {
            viewports.borrow_mut().push(viewport);
            pan_offset
        }))));

        assert_eq!(
            resolved_viewports.borrow().as_slice(),
            &[field_width],
            "the pan resolver must receive the content viewport width"
        );
        assert_eq!(
            panned.rect.x, -pan_offset,
            "text glyphs must shift left by the pan offset"
        );
        assert!(
            panned.rect.width > field_width,
            "panned single-line text must be laid out unconstrained, got {}",
            panned.rect.width
        );
        assert!(
            panned.rect.width >= unpanned.rect.width,
            "unconstrained layout must not be narrower than wrapped layout"
        );
        assert!(
            panned.rect.height <= unpanned.rect.height,
            "single-line layout must not wrap onto extra lines"
        );
        let clip = panned
            .clip
            .expect("panned text field must clip to field bounds");
        assert!(
            clip.x + clip.width <= field_width + TEXT_CLIP_PAD + f32::EPSILON,
            "clip must not extend past the field bounds, got {clip:?}"
        );
    }

    #[test]
    fn translated_content_context_preserves_descendant_text_motion_when_unspecified() {
        let child = BuildNodeSnapshot {
            node_id: 2,
            placement: Point { x: 11.0, y: 7.0 },
            size: Size {
                width: 120.0,
                height: 32.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: Some(120.0),
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: Some(AnnotatedString::from("scrolling")),
            text_style: Some(TextStyle::default()),
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };
        let parent = BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: 160.0,
                height: 64.0,
            },
            content_offset: Point { x: 0.0, y: -18.5 },
            motion_context_animated: false,
            translated_content_context: true,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![child],
        };

        let graph = build_layer_node_for_test(parent, 1.0, false);
        let RenderNode::Layer(child_layer) = &graph.children[0] else {
            panic!("expected child layer");
        };
        let RenderNode::Primitive(text_primitive) = &child_layer.children[0] else {
            panic!("expected text primitive");
        };
        let PrimitiveNode::Text(text) = &text_primitive.node else {
            panic!("expected text primitive");
        };

        assert_eq!(text.text_style.paragraph_style.text_motion, None);
        assert!(!child_layer.motion_context_animated);
    }

    #[test]
    fn content_offset_without_translated_context_keeps_descendant_text_unspecified() {
        let child = BuildNodeSnapshot {
            node_id: 2,
            placement: Point { x: 11.0, y: 7.0 },
            size: Size {
                width: 120.0,
                height: 32.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: Some(120.0),
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: Some(AnnotatedString::from("scrolling")),
            text_style: Some(TextStyle::default()),
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };
        let parent = BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: 160.0,
                height: 64.0,
            },
            content_offset: Point { x: 0.0, y: -18.0 },
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![child],
        };

        let graph = build_layer_node_for_test(parent, 1.0, false);
        let RenderNode::Layer(child_layer) = &graph.children[0] else {
            panic!("expected child layer");
        };
        let RenderNode::Primitive(text_primitive) = &child_layer.children[0] else {
            panic!("expected text primitive");
        };
        let PrimitiveNode::Text(text) = &text_primitive.node else {
            panic!("expected text primitive");
        };

        assert_eq!(
            text.text_style.paragraph_style.text_motion, None,
            "content_offset alone must not force text onto the translated-content motion path"
        );
        assert!(!child_layer.motion_context_animated);
    }

    #[test]
    fn translated_content_context_preserves_effectful_text_motion_when_unspecified() {
        let child = BuildNodeSnapshot {
            node_id: 2,
            placement: Point { x: 11.0, y: 7.0 },
            size: Size {
                width: 120.0,
                height: 32.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: Some(120.0),
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: Some(AnnotatedString::from("shadow")),
            text_style: Some(TextStyle::from_span_style(SpanStyle {
                shadow: Some(cranpose_ui::text::Shadow {
                    color: Color::BLACK,
                    offset: Point::new(1.0, 2.0),
                    blur_radius: 3.0,
                }),
                ..SpanStyle::default()
            })),
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };
        let parent = BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: 160.0,
                height: 64.0,
            },
            content_offset: Point { x: 0.0, y: -18.5 },
            motion_context_animated: false,
            translated_content_context: true,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![child],
        };

        let graph = build_layer_node_for_test(parent, 1.0, false);
        let RenderNode::Layer(child_layer) = &graph.children[0] else {
            panic!("expected child layer");
        };
        let RenderNode::Primitive(text_primitive) = &child_layer.children[0] else {
            panic!("expected text primitive");
        };
        let PrimitiveNode::Text(text) = &text_primitive.node else {
            panic!("expected text primitive");
        };

        assert_eq!(text.text_style.paragraph_style.text_motion, None);
    }

    #[test]
    fn animated_motion_marker_preserves_descendant_text_motion_when_unspecified() {
        let child = BuildNodeSnapshot {
            node_id: 2,
            placement: Point { x: 11.0, y: 7.0 },
            size: Size {
                width: 120.0,
                height: 32.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: Some(120.0),
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: Some(AnnotatedString::from("lazy")),
            text_style: Some(TextStyle::default()),
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };
        let parent = BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: 160.0,
                height: 64.0,
            },
            content_offset: Point::default(),
            motion_context_animated: true,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![child],
        };

        let graph = build_layer_node_for_test(parent, 1.0, false);
        let RenderNode::Layer(child_layer) = &graph.children[0] else {
            panic!("expected child layer");
        };
        let RenderNode::Primitive(text_primitive) = &child_layer.children[0] else {
            panic!("expected text primitive");
        };
        let PrimitiveNode::Text(text) = &text_primitive.node else {
            panic!("expected text primitive");
        };

        assert_eq!(text.text_style.paragraph_style.text_motion, None);
        assert!(graph.motion_context_animated);
        assert!(child_layer.motion_context_animated);
    }

    #[test]
    fn lazy_column_item_text_keeps_unspecified_motion_at_origin() {
        let mut composition = cranpose_ui::run_test_composition(|| {
            let list_state = rememberLazyListState();
            LazyColumn(
                Modifier::empty(),
                list_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.item_keyed(Some(0), None, || {
                        Text("LazyMotion", Modifier::empty(), TextStyle::default());
                    });
                },
            );
        });

        let root = composition.root().expect("lazy column root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let _ = applier
            .compute_layout(
                root,
                Size {
                    width: 240.0,
                    height: 240.0,
                },
            )
            .expect("lazy column layout");
        let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("lazy column graph");
        applier.clear_runtime_handle();

        assert_eq!(find_text_motion(&graph.root, "LazyMotion"), Some(None));
    }

    #[test]
    fn scrolled_lazy_column_item_text_keeps_unspecified_motion_at_rest() {
        use std::{cell::RefCell, rc::Rc};

        let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let state_holder_for_comp = state_holder.clone();
        let mut composition = cranpose_ui::run_test_composition(move || {
            let list_state = rememberLazyListState();
            *state_holder_for_comp.borrow_mut() = Some(list_state);
            LazyColumn(
                Modifier::empty().height(120.0),
                list_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(8, |index| {
                        Text(
                            format!("LazyMotion {index}"),
                            Modifier::empty().padding(4.0),
                            TextStyle::default(),
                        );
                    });
                },
            );
        });

        let list_state = (*state_holder.borrow()).expect("lazy list state should be captured");
        list_state.scroll_to_item(3, 0.0);

        let root = composition.root().expect("lazy column root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let _ = applier
            .compute_layout(
                root,
                Size {
                    width: 240.0,
                    height: 240.0,
                },
            )
            .expect("lazy column layout");
        let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("lazy column graph");
        let active_children = applier
            .with_node::<SubcomposeLayoutNode, _>(root, |node| node.active_children())
            .expect("lazy column should be subcompose");
        let child_debug: Vec<String> = active_children
            .iter()
            .map(|&child_id| {
                if let Ok(summary) = applier.with_node::<LayoutNode, _>(child_id, |node| {
                    format!(
                        "layout#{child_id} placed={} text={:?} children={:?}",
                        node.layout_state().is_placed(),
                        node.modifier_slices_snapshot()
                            .text_content()
                            .map(str::to_string),
                        node.children.clone()
                    )
                }) {
                    summary
                } else if let Ok(summary) =
                    applier.with_node::<SubcomposeLayoutNode, _>(child_id, |node| {
                        format!(
                            "subcompose#{child_id} placed={} active_children={:?}",
                            node.layout_state().is_placed(),
                            node.active_children()
                        )
                    })
                {
                    summary
                } else {
                    format!("missing#{child_id}")
                }
            })
            .collect();
        applier.clear_runtime_handle();

        let first_index = list_state.first_visible_item_index();
        assert!(
            first_index > 0,
            "lazy list should move away from origin before graph building, observed first_index={first_index}"
        );
        let mut labels = Vec::new();
        collect_text_labels(&graph.root, &mut labels);
        assert_eq!(
            find_text_motion(&graph.root, &format!("LazyMotion {first_index}")),
            Some(None),
            "graph labels after scroll: {:?}, active_children={:?}, child_debug={:?}",
            labels,
            active_children,
            child_debug
        );
    }

    #[test]
    fn scrolled_lazy_column_render_graph_keeps_beyond_bound_text_rows() {
        use std::{cell::RefCell, rc::Rc};

        let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let state_holder_for_comp = state_holder.clone();
        let mut composition = cranpose_ui::run_test_composition(move || {
            let list_state = rememberLazyListState();
            *state_holder_for_comp.borrow_mut() = Some(list_state);
            let mut spec =
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0));
            spec.beyond_bounds_item_count = 0;
            LazyColumn(Modifier::empty().height(96.0), list_state, spec, |scope| {
                scope.items(12, |index| {
                    Text(
                        format!("WarmRow {index}"),
                        Modifier::empty().height(32.0),
                        TextStyle::default(),
                    );
                });
            });
        });

        let list_state = (*state_holder.borrow()).expect("lazy list state should be captured");
        list_state.scroll_to_item(4, 0.0);

        let root = composition.root().expect("lazy column root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let _ = applier
            .compute_layout(
                root,
                Size {
                    width: 240.0,
                    height: 240.0,
                },
            )
            .expect("lazy column layout");
        let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("lazy column graph");
        let active_children = applier
            .with_node::<SubcomposeLayoutNode, _>(root, |node| node.active_children())
            .expect("lazy column should be subcompose");
        applier.clear_runtime_handle();

        let visible_indices: Vec<_> = list_state
            .layout_info()
            .visible_items_info
            .iter()
            .map(|item| item.index)
            .collect();
        let mut labels = Vec::new();
        collect_text_labels(&graph.root, &mut labels);

        assert_eq!(
            visible_indices,
            vec![4, 5, 6],
            "test setup expects exactly three viewport-visible rows"
        );
        assert!(
            labels.iter().any(|label| label == "WarmRow 7"),
            "render graph must retain at least one after-bound text row for glyph prewarm; labels={labels:?}, active_children={active_children:?}"
        );
    }

    #[test]
    fn scrolled_lazy_column_uses_visible_item_offset_as_snap_anchor_offset() {
        use std::{cell::RefCell, rc::Rc};

        let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let state_holder_for_comp = state_holder.clone();
        let mut composition = cranpose_ui::run_test_composition(move || {
            let list_state = rememberLazyListState();
            *state_holder_for_comp.borrow_mut() = Some(list_state);
            LazyColumn(
                Modifier::empty().height(120.0),
                list_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(8, |index| {
                        Text(
                            format!("LazySnap {index}"),
                            Modifier::empty().padding(4.0),
                            TextStyle::default(),
                        );
                    });
                },
            );
        });

        let list_state = (*state_holder.borrow()).expect("lazy list state should be captured");
        list_state.scroll_to_item(2, 7.5);

        let root = composition.root().expect("lazy column root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let _ = applier
            .compute_layout(
                root,
                Size {
                    width: 240.0,
                    height: 240.0,
                },
            )
            .expect("lazy column layout");
        let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("lazy column graph");
        applier.clear_runtime_handle();

        let layout_info = list_state.layout_info();
        let first_visible_offset = layout_info
            .visible_items_info
            .first()
            .expect("lazy layout should expose visible item info")
            .offset;
        let snap_offset = find_translated_content_offset(&graph.root)
            .expect("lazy list graph should include translated content context");

        assert!(
            (snap_offset.y - first_visible_offset).abs() <= 0.001,
            "lazy snap offset must follow the visible content origin; snap_offset={snap_offset:?} first_visible_offset={first_visible_offset}"
        );
    }

    #[test]
    fn explicit_static_text_motion_is_preserved_under_scrolling_context() {
        let child = BuildNodeSnapshot {
            node_id: 2,
            placement: Point { x: 11.0, y: 7.0 },
            size: Size {
                width: 120.0,
                height: 32.0,
            },
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            has_own_origin_sinks: false,
            measured_max_width: Some(120.0),
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: Some(AnnotatedString::from("static")),
            text_style: Some(TextStyle::from_paragraph_style(
                cranpose_ui::text::ParagraphStyle {
                    text_motion: Some(TextMotion::Static),
                    ..Default::default()
                },
            )),
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![],
        };
        let parent = BuildNodeSnapshot {
            node_id: 1,
            placement: Point::default(),
            size: Size {
                width: 160.0,
                height: 64.0,
            },
            content_offset: Point { x: 0.0, y: -18.5 },
            motion_context_animated: false,
            translated_content_context: true,
            has_own_origin_sinks: false,
            measured_max_width: None,
            resolved_modifiers: ResolvedModifiers::default(),
            draw_commands: vec![],
            click_actions: vec![],
            pointer_inputs: vec![],
            clip_to_bounds: false,
            annotated_text: None,
            text_style: None,
            text_layout_options: None,
            text_pan: None,
            graphics_layer: None,
            children: vec![child],
        };

        let graph = build_layer_node_for_test(parent, 1.0, false);
        let RenderNode::Layer(child_layer) = &graph.children[0] else {
            panic!("expected child layer");
        };
        let RenderNode::Primitive(text_primitive) = &child_layer.children[0] else {
            panic!("expected text primitive");
        };
        let PrimitiveNode::Text(text) = &text_primitive.node else {
            panic!("expected text primitive");
        };

        assert_eq!(
            text.text_style.paragraph_style.text_motion,
            Some(TextMotion::Static),
            "explicit text motion must win over inherited scrolling motion context"
        );
    }

    #[test]
    fn wrapped_paragraph_paints_the_height_it_measured() {
        const BODY: &str = "fed back картица scored fp32 износ once paper fed Vision dropped \
             fed widest the strip mask prompt mask threshold Vision on датум instance mask \
             износ Apple";
        const FOLLOWING: &str = "FOLLOWING SIBLING";

        let app_context = cranpose_ui::AppContext::new();
        app_context.enter(|| {
            cranpose_ui::text::set_text_measurer(
                crate::software_text_raster::SoftwareTextMeasurer::from_fonts_or_default(&[], 8192),
            );
            let mut composition = cranpose_ui::run_test_composition(move || {
                Column(
                    Modifier::empty().fill_max_width(),
                    ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                    move || {
                        Text(BODY.to_string(), Modifier::empty(), TextStyle::default());
                        Text(
                            FOLLOWING.to_string(),
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                    },
                );
            });

            let root = composition.root().expect("composition root");
            let handle = composition.runtime_handle();
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(handle);
            let layout = applier
                .compute_layout(
                    root,
                    Size {
                        width: 245.0,
                        height: 900.0,
                    },
                )
                .expect("layout");

            fn find_box<'a>(node: &'a LayoutBox, value: &str) -> Option<&'a LayoutBox> {
                if node
                    .node_data
                    .modifier_slices()
                    .text_content()
                    .is_some_and(|text| text == value)
                {
                    return Some(node);
                }
                node.children
                    .iter()
                    .find_map(|child| find_box(child, value))
            }
            let body_box = find_box(layout.root(), BODY).expect("measured paragraph box");
            let following_box = find_box(layout.root(), FOLLOWING).expect("measured sibling box");
            let measured_height = body_box.rect.height;
            let following_top = following_box.rect.y;
            assert!(
                measured_height > 60.0,
                "test setup expects a genuinely multi-line paragraph, got {measured_height}"
            );
            assert!(
                body_box.rect.width < 245.0,
                "test setup expects the node to be placed at its own measured width, \
                 not the full constraint, got {}",
                body_box.rect.width
            );

            let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("render graph");
            applier.clear_runtime_handle();

            fn squashed(value: &str) -> String {
                value.chars().filter(|c| !c.is_whitespace()).collect()
            }
            fn find_text<'a>(layer: &'a LayerNode, value: &str) -> Option<&'a TextPrimitiveNode> {
                for child in &layer.children {
                    match child {
                        RenderNode::Primitive(primitive) => {
                            if let PrimitiveNode::Text(text) = &primitive.node
                                && squashed(&text.text.text) == squashed(value)
                            {
                                return Some(text);
                            }
                        }
                        RenderNode::Layer(child_layer) => {
                            if let Some(found) = find_text(child_layer, value) {
                                return Some(found);
                            }
                        }
                        RenderNode::DrawRun(_) => {}
                    }
                }
                None
            }
            let painted = find_text(&graph.root, BODY).expect("painted paragraph");

            assert!(
                (painted.rect.height - measured_height).abs() < 0.5,
                "paragraph painted {:.2} tall into a box layout measured at {:.2} \
                 (painted rect {:?})",
                painted.rect.height,
                measured_height,
                painted.rect
            );
            assert!(
                painted.rect.y + painted.rect.height <= following_top + 0.5,
                "painted paragraph bottom {:.2} runs past the following sibling placed at \
                 {:.2}",
                painted.rect.y + painted.rect.height,
                following_top
            );
        });
    }

    #[test]
    fn retained_slot_confirmations_are_live_only_under_their_generation() {
        let command = DrawCommandId {
            node_id: 990_101,
            command_index: 0,
            placement: DrawPlacement::Behind,
        };
        set_retained_feed_epoch(Some(7));
        confirm_retained_slot(command, 3, 7);
        assert!(retained_slot_confirmed(command, 3));
        set_retained_feed_epoch(Some(8));
        assert!(!retained_slot_confirmed(command, 3));
        set_retained_feed_epoch(None);
        assert!(!retained_slot_confirmed(command, 3));
        set_retained_feed_epoch(Some(7));
        assert!(retained_slot_confirmed(command, 3));
        revoke_retained_slot(command, 3);
        assert!(!retained_slot_confirmed(command, 3));
        set_retained_feed_epoch(None);
        clear_retained_slot_confirmations();
    }

    fn record_sweep_test_rings(scope: &mut DrawScopeDefault) {
        let count = 600usize;
        let sweep = std::f32::consts::TAU / count as f32 * 0.8;
        for i in 0..count {
            let start = i as f32 * (std::f32::consts::TAU / count as f32);
            scope.draw_annular_sector(
                Brush::solid(cranpose_ui_graphics::Color(0.2, 0.4, 0.6, 1.0)),
                cranpose_ui_graphics::Point::new(204.0, 204.0),
                140.0,
                150.0,
                start,
                sweep,
            );
        }
    }

    #[test]
    fn recording_sweep_cannot_sever_a_frames_fallback() {
        let command = DrawCommandId {
            node_id: 990_102,
            command_index: 0,
            placement: DrawPlacement::Behind,
        };
        set_retained_feed_epoch(Some(41));
        for slot in 0..64 {
            confirm_retained_slot(command, slot, 41);
        }

        let mut state = cranpose_ui_graphics::CommandReplayState::default();
        let mut published = None;
        for _frame in 0..4 {
            let (recording, storage, _) = acquire_recording(command);
            let mut scope = DrawScopeDefault::with_recording(
                cranpose_ui_graphics::Size::new(408.0, 408.0),
                None,
                recording,
                storage,
            );
            record_sweep_test_rings(&mut scope);
            let outcome = state.advance(scope.recorded());
            let center = state.center();
            let (finished, frame) = scope.finish_replay(center, outcome, &mut |slot| {
                retained_slot_confirmed(command, slot)
            });
            let (primitives, fallback) =
                publish_recording(command, finished.recording, finished.primitives, None);
            let frame = frame.map(|mut frame| {
                frame.fallback = Some(fallback.clone());
                frame
            });
            published = Some((primitives, fallback, frame));
        }
        let (_primitives, fallback, frame) = published.expect("four frames published");
        let frame = frame.expect("the replay must produce a frame with retained spans");
        let bypassed: Vec<(u32, u32)> = frame
            .spans
            .iter()
            .filter_map(|span| match span {
                cranpose_ui_graphics::FrameSpan::Retained {
                    capture: false,
                    range,
                    tape_range,
                    ..
                } if range.1 <= range.0 => Some(*tape_range),
                _ => None,
            })
            .collect();
        assert!(
            !bypassed.is_empty(),
            "confirmed slots must actually have bypassed materialization"
        );
        let expected: Vec<Vec<DrawPrimitive>> = bypassed
            .iter()
            .map(|tape_range| {
                fallback
                    .materialize_range(tape_range.0 as usize, tape_range.1 as usize)
                    .expect("a frame-consistent tape range must materialize")
            })
            .collect();

        for _ in 0..1024 {
            bump_recording_generation();
        }
        assert!(
            COMMAND_RECORDINGS.with(|map| !map.borrow().contains_key(&command)),
            "the sweep must stay pure capacity management: a live confirmation \
             no longer pins the registry slot"
        );

        for (tape_range, expected) in bypassed.iter().zip(&expected) {
            let after = fallback
                .materialize_range(tape_range.0 as usize, tape_range.1 as usize)
                .expect("the frame-owned recording must outlive the sweep");
            assert_eq!(
                &after, expected,
                "post-sweep rematerialization must be byte-identical"
            );
        }
        set_retained_feed_epoch(None);
        clear_retained_slot_confirmations();
    }

    #[test]
    fn command_recordings_reuse_recording_buffers_across_rebuilds() {
        let command = DrawCommandId {
            node_id: 990_103,
            command_index: 0,
            placement: DrawPlacement::Behind,
        };
        let mut held = None;
        let mut ptrs = Vec::new();
        for _build in 0..8 {
            let (recording, storage, _) = acquire_recording(command);
            let mut scope = DrawScopeDefault::with_recording(
                cranpose_ui_graphics::Size::new(64.0, 64.0),
                None,
                recording,
                storage,
            );
            scope.draw_rect_at(
                Rect {
                    x: 4.0,
                    y: 4.0,
                    width: 16.0,
                    height: 8.0,
                },
                Brush::solid(Color::WHITE),
            );
            let finished = scope.finish();
            let (primitives, recording) =
                publish_recording(command, finished.recording, finished.primitives, None);
            ptrs.push(recording.tape_ptr());
            held = Some((primitives, recording));
        }
        drop(held);
        for build in 2..8 {
            assert_eq!(
                ptrs[build],
                ptrs[build - 2],
                "steady-state publishes must ping-pong between the pair's \
                 buffers (build {build} allocated)"
            );
        }
        assert_ne!(
            ptrs[6], ptrs[7],
            "a recording a live frame still shares must never be recorded into"
        );
    }

    #[test]
    fn sanitized_spans_drop_recolors_and_downgrade_captures() {
        use cranpose_ui_graphics::{FrameSpan, RecordTransform};
        let bounds = Rect {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        };
        let spans = vec![
            FrameSpan::Dynamic { range: (0, 5) },
            FrameSpan::Retained {
                slot: 7,
                capture: true,
                slot_offset: 0,
                range: (5, 105),
                tape_range: (5, 105),
                transform: RecordTransform::IDENTITY,
                recolors: Vec::new(),
                bounds,
            },
            FrameSpan::Retained {
                slot: 8,
                capture: false,
                slot_offset: 3,
                range: (105, 205),
                tape_range: (110, 210),
                transform: RecordTransform {
                    scale: 0.999,
                    angle: 0.05,
                },
                recolors: vec![(4, cranpose_ui_graphics::Color(1.0, 0.5, 0.2, 1.0))],
                bounds,
            },
        ];
        let sanitized = sanitized_replay_spans(&spans);
        assert_eq!(sanitized[0], FrameSpan::Dynamic { range: (0, 5) });
        assert_eq!(sanitized[1], FrameSpan::Dynamic { range: (5, 105) });
        match &sanitized[2] {
            FrameSpan::Retained {
                slot,
                capture,
                slot_offset,
                range,
                tape_range,
                transform,
                recolors,
                bounds: sanitized_bounds,
            } => {
                assert_eq!((*slot, *capture, *slot_offset), (8, false, 3));
                assert_eq!((*range, *tape_range), ((105, 205), (110, 210)));
                assert_eq!(transform.angle, 0.05);
                assert!(recolors.is_empty(), "recolors must be emptied");
                assert_eq!(*sanitized_bounds, bounds);
            }
            other => panic!("expected a retained span, got {other:?}"),
        }
    }

    #[test]
    fn a_saved_emission_serves_the_next_build_once() {
        let command = DrawCommandId {
            node_id: 990_303,
            command_index: 0,
            placement: DrawPlacement::Behind,
        };
        set_retained_feed_epoch(Some(77));
        publish_recording(
            command,
            cranpose_ui_graphics::CommandRecording::default(),
            Vec::new(),
            None,
        );
        let saved = || SavedReplayEmission {
            spans: vec![cranpose_ui_graphics::FrameSpan::Dynamic { range: (0, 3) }],
            center: cranpose_ui_graphics::Point::new(204.0, 204.0),
            primitives: Rc::new(Vec::new()),
            recording: Rc::new(cranpose_ui_graphics::CommandRecording::default()),
            epoch: 77,
            generation: RECORDING_GENERATION.with(std::cell::Cell::get),
        };

        store_saved_emission(command, Some(saved()));
        assert!(!saved_emission_available(command));
        bump_recording_generation();
        assert!(saved_emission_available(command));
        bump_recording_generation();
        assert!(!saved_emission_available(command));

        store_saved_emission(command, Some(saved()));
        bump_recording_generation();
        assert!(saved_emission_available(command));
        assert!(take_saved_emission(command).is_some());
        assert!(
            !saved_emission_available(command),
            "a second serve of one emission must be unconstructible"
        );
        assert!(take_saved_emission(command).is_none());

        store_saved_emission(command, Some(saved()));
        bump_recording_generation();
        set_retained_feed_epoch(Some(78));
        assert!(!saved_emission_available(command));
        set_retained_feed_epoch(None);
        assert!(!saved_emission_available(command));
        set_retained_feed_epoch(Some(77));
        assert!(saved_emission_available(command));
        set_retained_feed_epoch(None);
    }
}
