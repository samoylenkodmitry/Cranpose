use std::{cell::Cell, rc::Rc};

use cranpose_core::{MemoryApplier, Node, NodeId, collections::map::HashSet};
use cranpose_ui::{
    DrawCommand, LayoutBox, LayoutNode, ModifierNodeSlices, Point, PreparedTextLayout, Rect,
    ResolvedModifiers, Size, SubcomposeLayoutNode, TextLayoutOptions, TextOverflow,
    TextPanResolver, prepare_text_layout,
    text::{AnnotatedString, TextAlign, TextStyle, resolve_text_direction},
};
use cranpose_ui_graphics::{
    CommandRecording, CompositingStrategy, GraphicsLayer, LayerShape, PointerIcon,
    RoundedCornerShape, rounded_corner_alpha_mask_effect,
};
use smallvec::SmallVec;

use crate::{
    graph::{
        CachePolicy, DrawCommandId, DrawRunNode, HitTestNode, IsolationReasons, LayerNode,
        PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph,
        RenderNode, TextPrimitiveNode,
    },
    layer_transform::layer_transform_to_parent,
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
    motion_context_animated: bool,
    translated_content_context: bool,
    has_own_origin_sinks: bool,
    measured_max_width: Option<f32>,
    measured_text_layout: Option<PreparedTextLayout>,
    resolved_modifiers: ResolvedModifiers,
    draw_commands: Vec<DrawCommand>,
    outer_draw_command_count: usize,
    click_actions: Vec<Rc<dyn Fn(Point)>>,
    pointer_inputs: Vec<Rc<dyn Fn(cranpose_foundation::PointerEvent)>>,
    pointer_icon: Option<PointerIcon>,
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
    let Some(report) = replace_dirty_layers_from_applier(
        applier,
        &mut graph.root,
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
                layer_identity(child_layer).expect("dirty layer must have a node id"),
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
    clip_to_bounds: bool,
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
        || layout_state.size().width != container.local_bounds.width
        || layout_state.size().height != container.local_bounds.height
    {
        return Err("container unplaced or resized");
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
        clip_to_bounds,
        graphics_layer,
    })
}

struct TranslatedChildren {
    placed_fresh: SmallVec<[(NodeId, cranpose_ui::widgets::LayoutState); 8]>,
    children_unchanged: bool,
    old_index_by_id: std::collections::HashMap<NodeId, usize>,
}

fn translated_children(
    applier: &mut MemoryApplier,
    container: &LayerNode,
    dirty_nodes: &HashSet<NodeId>,
    fresh_children: &[NodeId],
) -> Result<TranslatedChildren, &'static str> {
    let mut placed_fresh = SmallVec::<[_; 8]>::with_capacity(fresh_children.len());
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
            .zip(&placed_fresh)
            .all(|(child, (id, _))| {
                matches!(child, RenderNode::Layer(layer) if layer_identity(layer) == Some(*id))
            });
    let old_index_by_id = if children_unchanged {
        std::collections::HashMap::new()
    } else {
        let Some(index): Option<std::collections::HashMap<NodeId, usize>> = container
            .children
            .iter()
            .enumerate()
            .map(|(index, child)| match child {
                RenderNode::Layer(layer) => layer_identity(layer).map(|id| (id, index)),
                _ => None,
            })
            .collect()
        else {
            return Err("child without node id");
        };
        index
    };
    check_retained_children(
        container,
        dirty_nodes,
        &placed_fresh,
        children_unchanged,
        &old_index_by_id,
    )?;
    Ok(TranslatedChildren {
        placed_fresh,
        children_unchanged,
        old_index_by_id,
    })
}

fn check_retained_children(
    container: &LayerNode,
    dirty_nodes: &HashSet<NodeId>,
    placed_fresh: &[(NodeId, cranpose_ui::widgets::LayoutState)],
    children_unchanged: bool,
    old_index_by_id: &std::collections::HashMap<NodeId, usize>,
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
        if state.size().width != layer.local_bounds.width
            || state.size().height != layer.local_bounds.height
        {
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
    translation_delta: Point,
}

impl TranslateGeometry {
    fn new(
        container: &LayerNode,
        layout_state: &cranpose_ui::widgets::LayoutState,
        graphics_layer: &GraphicsLayer,
        parent_abs: AbsOrigin,
    ) -> Self {
        let content_offset = layout_state.content_offset;
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
            translation_delta: Point {
                x: layer_translation.x - container.scene_children_layer_translation.x,
                y: layer_translation.y - container.scene_children_layer_translation.y,
            },
        }
    }
}

fn build_entering_children(
    applier: &mut MemoryApplier,
    container: &LayerNode,
    placed_fresh: &[(NodeId, cranpose_ui::widgets::LayoutState)],
    retained: (bool, &std::collections::HashMap<NodeId, usize>),
    geometry: TranslateGeometry,
    inherited: (bool, bool),
) -> std::collections::HashMap<NodeId, LayerNode> {
    let (children_unchanged, old_index_by_id) = retained;
    let (child_inherited_translated_content_context, children_ancestor_hashed) = inherited;
    let mut entering: std::collections::HashMap<NodeId, LayerNode> =
        std::collections::HashMap::new();
    for (child_id, _) in placed_fresh {
        if children_unchanged || old_index_by_id.contains_key(child_id) {
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
        entering.insert(*child_id, lowered);
    }
    entering
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
    if let Some(sink) = modifier_slices.text_field_window_origin() {
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
    container.scene_children_origin = geometry.child_origin;
    container.scene_children_layer_translation = geometry.layer_translation;
}

fn reconcile_translated_children(
    container: &mut LayerNode,
    dirty_nodes: &mut HashSet<NodeId>,
    changed_nodes: &mut Vec<NodeId>,
    placed_fresh: &[(NodeId, cranpose_ui::widgets::LayoutState)],
    children_unchanged: bool,
    entering: &mut std::collections::HashMap<NodeId, LayerNode>,
    geometry: TranslateGeometry,
) {
    if children_unchanged {
        for (child, (child_id, state)) in container.children.iter_mut().zip(placed_fresh) {
            let RenderNode::Layer(layer) = child else {
                unreachable!("retained child identities were checked");
            };
            if !dirty_nodes.contains(child_id) {
                translate_retained_child(
                    layer,
                    state,
                    geometry.content_offset,
                    geometry.child_origin,
                    geometry.translation_delta,
                );
                changed_nodes.push(*child_id);
            }
        }
        return;
    }
    let fresh_id_set: HashSet<NodeId> = placed_fresh.iter().map(|(id, _)| *id).collect();
    let mut old_by_id: std::collections::HashMap<NodeId, Box<LayerNode>> =
        std::collections::HashMap::new();
    for child in container.children.drain(..) {
        let RenderNode::Layer(layer) = child else {
            continue;
        };
        let child_id = layer_identity(&layer).expect("checked above");
        if fresh_id_set.contains(&child_id) {
            old_by_id.insert(child_id, layer);
        } else {
            collect_layer_node_ids(&layer, changed_nodes);
        }
    }
    let mut new_children = Vec::with_capacity(placed_fresh.len());
    for (child_id, state) in placed_fresh {
        if let Some(mut layer) = old_by_id.remove(child_id) {
            if !dirty_nodes.contains(child_id) {
                translate_retained_child(
                    &mut layer,
                    state,
                    geometry.content_offset,
                    geometry.child_origin,
                    geometry.translation_delta,
                );
                changed_nodes.push(*child_id);
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
        resolved_modifiers: _,
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
        clip_to_bounds,
        graphics_layer,
    } = container_plan;
    let child_plan = match translated_children(applier, container, dirty_nodes, &fresh_children) {
        Ok(plan) => plan,
        Err(reason) => return translate_bail(reason),
    };
    let TranslatedChildren {
        placed_fresh,
        children_unchanged,
        old_index_by_id,
    } = child_plan;

    let geometry = TranslateGeometry::new(container, &layout_state, &graphics_layer, parent_abs);
    let child_inherited_translated_content_context =
        inherited_translated_content_context || container.translated_content_context;
    let children_ancestor_hashed =
        crate::graph_hash::layer_children_ancestor_hashed(container, container_ancestor_hashed);
    let mut entering = build_entering_children(
        applier,
        container,
        &placed_fresh,
        (children_unchanged, &old_index_by_id),
        geometry,
        (
            child_inherited_translated_content_context,
            children_ancestor_hashed,
        ),
    );

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
        &placed_fresh,
        children_unchanged,
        &mut entering,
        geometry,
    );
    modifier_slices.publish_pointer_input_size(layout_state.size());
    container.hit_test = hit_test_from_slices(
        &modifier_slices,
        container.local_bounds,
        clip_to_bounds || graphics_layer.clip,
    );

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

fn translate_retained_child(
    layer: &mut LayerNode,
    state: &cranpose_ui::widgets::LayoutState,
    content_offset: Point,
    child_origin: Point,
    translation_delta: Point,
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
    let new_children_origin = Point {
        x: child_origin.x + state.position().x + layer.content_offset.x,
        y: child_origin.y + state.position().y + layer.content_offset.y,
    };
    let origin_delta = Point {
        x: new_children_origin.x - layer.scene_children_origin.x,
        y: new_children_origin.y - layer.scene_children_origin.y,
    };
    offset_scene_origins(layer, origin_delta, translation_delta);
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
        measured_text_layout,
        resolved_modifiers,
        draw_commands,
        outer_draw_command_count,
        click_actions,
        pointer_inputs,
        pointer_icon,
        clip_to_bounds,
        annotated_text,
        text_style,
        text_layout_options,
        text_pan,
        graphics_layer,
        children: child_snapshots,
    } = snapshot;
    let outer = outer_draws(node_id, &draw_commands, outer_draw_command_count, size);
    let layer_draw_commands = &draw_commands[outer_draw_command_count..];
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
    let hit_test = (!click_actions.is_empty()
        || !pointer_inputs.is_empty()
        || pointer_icon.is_some())
    .then(|| HitTestNode {
        shape: None,
        click_actions,
        pointer_inputs,
        pointer_icon,
        clip: (clip_to_bounds || graphics_layer.clip).then_some(local_bounds),
    });

    let node_motion_context_animated = inherited_motion_context_animated || motion_context_animated;
    let child_translated_content_context =
        inherited_translated_content_context || translated_content_context;

    let mut children = Vec::with_capacity(layer_node_capacity(
        layer_draw_commands,
        child_snapshots.len(),
        annotated_text.is_some(),
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
        local_bounds,
        measured_max_width,
        resolved_modifiers: &resolved_modifiers,
        annotated_text: annotated_text.as_ref(),
        text_style: text_style.as_ref(),
        text_layout_options,
        text_pan,
        measured_layout: measured_text_layout,
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
            resolved_modifiers: node.resolved_modifiers(),
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
                resolved_modifiers: node.resolved_modifiers(),
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

fn hit_test_from_slices(
    slices: &ModifierNodeSlices,
    bounds: Rect,
    clip: bool,
) -> Option<HitTestNode> {
    let click_actions = slices.click_handlers();
    let pointer_inputs = slices.pointer_inputs();
    let pointer_icon = slices.pointer_icon();
    (!click_actions.is_empty() || !pointer_inputs.is_empty() || pointer_icon.is_some()).then(|| {
        HitTestNode {
            shape: None,
            click_actions: click_actions.to_vec(),
            pointer_inputs: pointer_inputs.to_vec(),
            pointer_icon: pointer_icon.cloned(),
            clip: clip.then_some(bounds),
        }
    })
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
        window_root: _,
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
    let shadow_clip = clip_to_bounds.then_some(local_bounds);
    let hit_test = hit_test_from_slices(
        &modifier_slices,
        local_bounds,
        clip_to_bounds || graphics_layer.clip,
    );

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

    let outer_draw_command_count = modifier_slices.outer_draw_command_count();
    let outer = outer_draws(
        node_id,
        modifier_slices.draw_commands(),
        outer_draw_command_count,
        layout_state.size(),
    );
    let layer_draw_commands = &modifier_slices.draw_commands()[outer_draw_command_count..];
    let mut render_children = Vec::with_capacity(layer_node_capacity(
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
        measured_layout: modifier_slices.measured_text_layout(),
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
    Some(finish_layer(layer, layout_state.position(), outer))
}

struct RecorderSlot {
    generation: u64,
    handles: [Option<Rc<CommandRecording>>; 2],
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
            if handle
                .as_ref()
                .is_some_and(|shared| Rc::strong_count(shared) == 1)
            {
                let shared = handle.take().expect("checked some above");
                return Rc::try_unwrap(shared).expect("sole owner checked above");
            }
        }
        CommandRecording::default()
    })
}

fn publish_recording(id: DrawCommandId, recording: CommandRecording) -> Rc<CommandRecording> {
    let shared = Rc::new(recording);
    COMMAND_RECORDINGS.with(|map| {
        let mut map = map.borrow_mut();
        let generation = RECORDING_GENERATION.with(Cell::get);
        let slot = map.entry(id).or_insert_with(|| RecorderSlot {
            generation,
            handles: [None, None],
        });
        slot.generation = generation;
        slot.handles[1] = slot.handles[0].take();
        slot.handles[0] = Some(shared.clone());
    });
    shared
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
    let mut nodes = Vec::new();
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

fn finish_layer(layer: LayerNode, placement: Point, outer: Option<OuterDraws>) -> LayerNode {
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
    let local_bounds = layer.local_bounds;
    layer.transform_to_parent =
        layer_transform_to_parent(local_bounds, Point::default(), &layer.graphics_layer);
    let wrapper = LayerNode {
        wraps: layer.node_id,
        local_bounds,
        transform_to_parent: layer_transform_to_parent(
            local_bounds,
            placement,
            &GraphicsLayer::default(),
        ),
        scene_children_origin: Point {
            x: layer.scene_children_origin.x - layer.content_offset.x,
            y: layer.scene_children_origin.y - layer.content_offset.y,
        },
        scene_children_layer_translation: Point {
            x: layer.scene_children_layer_translation.x - layer.graphics_layer.translation_x,
            y: layer.scene_children_layer_translation.y - layer.graphics_layer.translation_y,
        },
        motion_context_animated: layer.motion_context_animated,
        has_hit_targets: layer.has_hit_targets,
        has_origin_sinks: layer.has_origin_sinks,
        ..Default::default()
    };
    let mut children = outer.behind;
    children.push(RenderNode::Layer(Box::new(layer)));
    children.extend(outer.overlay);
    LayerNode {
        children,
        ..wrapper
    }
}

fn layer_identity(layer: &LayerNode) -> Option<NodeId> {
    layer.node_id.or(layer.wraps)
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
    measured_layout: Option<PreparedTextLayout>,
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
        measured_layout,
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
        .map_or(0.0, |resolve| resolve(content_width));
    let pans_horizontally = text_pan.is_some();

    let prepared = measured_layout.unwrap_or_else(|| {
        let max_width = if pans_horizontally {
            None
        } else {
            Some(resolve_text_measure_width(
                content_width,
                padding,
                measured_max_width,
            ))
            .filter(|width| width.is_finite() && *width > 0.0)
        };
        prepare_text_layout(value, &text_style, options, max_width)
    });
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
        text: prepared.text,
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
        measured_text_layout: node.node_data.modifier_slices.measured_text_layout(),
        resolved_modifiers: node.node_data.resolved_modifiers,
        draw_commands: node.node_data.modifier_slices.draw_commands().to_vec(),
        outer_draw_command_count: node.node_data.modifier_slices.outer_draw_command_count(),
        click_actions: node.node_data.modifier_slices.click_handlers().to_vec(),
        pointer_inputs: node.node_data.modifier_slices.pointer_inputs().to_vec(),
        pointer_icon: node.node_data.modifier_slices.pointer_icon().cloned(),
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

/// The width the paint pass lays text out at when no `Text` node measured it,
/// as for text fields.
///
/// It is the width layout measured under, less padding, not the width the node
/// ended up. A node is placed at its widest line, and re-wrapping a paragraph
/// at the width of its own widest line can push that line's last word over the
/// edge and add a line. The node's content width is used only when layout
/// recorded no constraint.
pub fn resolve_text_measure_width(
    content_width: f32,
    padding: cranpose_ui::EdgeInsets,
    measured_max_width: Option<f32>,
) -> f32 {
    measured_max_width
        .filter(|width| width.is_finite() && *width > 0.0)
        .map_or_else(
            || content_width.max(0.0),
            |max_width| (max_width - padding.left - padding.right).max(0.0),
        )
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
#[path = "tests/scene_builder_tests.rs"]
mod tests;
