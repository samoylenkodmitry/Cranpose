use std::collections::HashSet;
use std::rc::Rc;

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui::text::AnnotatedString;
use cranpose_ui::text::{resolve_text_direction, TextAlign, TextStyle};
use cranpose_ui::{
    prepare_text_layout, DrawCommand, LayoutBox, LayoutNode, ModifierNodeSlices, Point, Rect,
    ResolvedModifiers, Size, SubcomposeLayoutNode, TextLayoutOptions, TextOverflow,
    TextPanResolver,
};
use cranpose_ui_graphics::{
    rounded_corner_alpha_mask_effect, CompositingStrategy, GraphicsLayer, LayerShape,
    RoundedCornerShape,
};

use crate::graph::{
    CachePolicy, DrawPrimitiveNode, HitTestNode, IsolationReasons, LayerNode, PrimitiveEntry,
    PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode, TextPrimitiveNode,
};
use crate::layer_transform::layer_transform_to_parent;
use crate::raster_cache::LayerRasterCacheHashes;
use crate::style_shared::{primitives_for_placement, DrawPlacement};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphUpdateReport {
    pub applied: bool,
    pub hit_graph_dirty: bool,
}

pub fn build_graph_from_layout_tree(root: &LayoutBox, scale: f32) -> RenderGraph {
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
    update_graph_from_applier_report(applier, graph, dirty_nodes, scale).applied
}

pub fn update_graph_from_applier_report(
    applier: &mut MemoryApplier,
    graph: &mut RenderGraph,
    dirty_nodes: &[NodeId],
    scale: f32,
) -> GraphUpdateReport {
    if dirty_nodes.is_empty() {
        return GraphUpdateReport {
            applied: true,
            hit_graph_dirty: false,
        };
    }

    if std::env::var_os("CRANPOSE_SCENE_UPDATE_DIAG").is_some() {
        eprintln!("[scene-update-diag] dirty={dirty_nodes:?}");
    }

    let mut remaining_dirty_nodes = dirty_nodes.iter().copied().collect::<HashSet<_>>();
    if let Some(root_id) = graph.root.node_id {
        if remaining_dirty_nodes.contains(&root_id) {
            let Some(root) = build_layer_node_from_applier(applier, root_id, scale, false) else {
                return GraphUpdateReport {
                    applied: false,
                    hit_graph_dirty: true,
                };
            };
            let hit_graph_dirty = layer_hit_graph_state_dirty(&graph.root, &root);
            graph.root = root;
            graph.root.recompute_raster_cache_hashes();
            return GraphUpdateReport {
                applied: true,
                hit_graph_dirty,
            };
        }
    }

    let inherited_translated_content_context = graph.root.translated_content_context;
    let report = match replace_dirty_layers_from_applier(
        applier,
        &mut graph.root,
        &mut remaining_dirty_nodes,
        inherited_translated_content_context,
    ) {
        Some(report) => report,
        None => {
            return GraphUpdateReport {
                applied: false,
                hit_graph_dirty: true,
            };
        }
    };

    if !remaining_dirty_nodes.is_empty() {
        return GraphUpdateReport {
            applied: false,
            hit_graph_dirty: true,
        };
    }

    if report.updated {
        graph.root.recompute_raster_cache_hashes();
    }
    GraphUpdateReport {
        applied: true,
        hit_graph_dirty: report.hit_graph_dirty,
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
) -> Option<ReplaceDirtyLayersReport> {
    if dirty_nodes.is_empty() {
        return Some(ReplaceDirtyLayersReport::default());
    }

    let child_inherited_translated_content_context =
        inherited_translated_content_context || parent.translated_content_context;
    let mut report = ReplaceDirtyLayersReport::default();

    for child in &mut parent.children {
        let RenderNode::Layer(child_layer) = child else {
            continue;
        };

        if child_layer
            .node_id
            .is_some_and(|node_id| dirty_nodes.remove(&node_id))
        {
            let mut replacement = build_layer_node_from_applier_internal(
                applier,
                child_layer
                    .node_id
                    .expect("dirty layer must have a node id"),
                parent.motion_context_animated,
                child_inherited_translated_content_context,
                // Resolve live window origins in the dirty subtree from the (clean)
                // parent's remembered child origin, so a `BasicTextField`'s
                // `node_origin` — and thus its overlay selection-handle / menu
                // `Popup`s — stay glued to the glyphs during a fling (which
                // rebuilds only the scrolling subtree, not the whole tree).
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
            **child_layer = replacement;
            report.updated = true;
            continue;
        }

        let child_report = replace_dirty_layers_from_applier(
            applier,
            child_layer,
            dirty_nodes,
            child_inherited_translated_content_context,
        )?;
        report.updated |= child_report.updated;
        report.hit_graph_dirty |= child_report.hit_graph_dirty;
    }

    if report.updated {
        parent.has_hit_targets = parent.hit_test.is_some()
            || parent.children.iter().any(|child| match child {
                RenderNode::Layer(child_layer) => child_layer.has_hit_targets,
                RenderNode::Primitive(_) => false,
            });
    }

    Some(report)
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
        &draw_commands,
        DrawPlacement::Overlay,
        size,
        PrimitivePhase::AfterChildren,
    ));
    let has_hit_targets = hit_test.is_some()
        || children.iter().any(|child| match child {
            RenderNode::Layer(child_layer) => child_layer.has_hit_targets,
            RenderNode::Primitive(_) => false,
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
        // The `LayoutBox` snapshot path does not carry live window origins (the
        // app runtime uses the applier path instead), so leave these at the
        // identity — origin sinks in this path are written by the layout `place`
        // pass.
        scene_children_origin: Point::default(),
        scene_children_layer_translation: Point::default(),
        graphics_layer,
        clip_to_bounds,
        shadow_clip,
        hit_test,
        has_hit_targets,
        isolation,
        cache_policy,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children,
    }
}

/// The composited window-space origin accumulated for a node while walking the
/// render graph. `content_origin` is where this node's own top-left sits before
/// its graphics-layer translation; `layer_translation` is the accumulated
/// ancestor graphics-layer translation. Mirrors the layout `place` pass, but is
/// computed during the per-frame scene build (which is the only pass that runs
/// in the app runtime — `build_layout_tree` is disabled there) so a scrolling
/// field's window-origin / a scroll container's viewport-rect sinks stay live
/// even during a fling (no re-layout, no pointer events). `None` in the partial
/// (dirty-subtree) rebuild path, where the ancestor origin is not known — the
/// sinks then keep their last full-build value rather than being corrupted.
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

fn build_layer_node_from_applier_internal(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    inherited_motion_context_animated: bool,
    inherited_translated_content_context: bool,
    parent_abs: Option<AbsOrigin>,
) -> Option<LayerNode> {
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
        return build_layer_node_from_data(
            applier,
            node_id,
            data,
            inherited_motion_context_animated,
            inherited_translated_content_context,
            parent_abs,
        );
    }

    if let Ok(data) = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
        let state = node.layout_state();
        let children = node.active_children();
        let modifier_slices = node.modifier_slices_snapshot();
        SnapshotNodeData {
            layout_state: state,
            modifier_slices,
            resolved_modifiers: node.resolved_modifiers(),
            children,
        }
    }) {
        return build_layer_node_from_data(
            applier,
            node_id,
            data,
            inherited_motion_context_animated,
            inherited_translated_content_context,
            parent_abs,
        );
    }

    None
}

fn build_layer_node_from_data(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    data: SnapshotNodeData,
    inherited_motion_context_animated: bool,
    inherited_translated_content_context: bool,
    parent_abs: Option<AbsOrigin>,
) -> Option<LayerNode> {
    let SnapshotNodeData {
        layout_state,
        modifier_slices,
        resolved_modifiers,
        children,
    } = data;
    if !layout_state.is_placed {
        return None;
    }

    let local_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: layout_state.size.width,
        height: layout_state.size.height,
    };
    if std::env::var_os("CRANPOSE_SCENE_UPDATE_DIAG").is_some() {
        eprintln!(
            "[scene-update-diag] build layer node={node_id:?} size=({:.2},{:.2}) pos=({:.2},{:.2})",
            layout_state.size.width,
            layout_state.size.height,
            layout_state.position.x,
            layout_state.position.y,
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
        layer_transform_to_parent(local_bounds, layout_state.position, &graphics_layer);
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

    let node_motion_context_animated =
        inherited_motion_context_animated || modifier_slices.motion_context_animated();
    let local_translated_content_context = modifier_slices.translated_content_context();
    let local_translated_content_offset = modifier_slices
        .translated_content_offset()
        .unwrap_or(layout_state.content_offset);
    let child_translated_content_context =
        inherited_translated_content_context || local_translated_content_context;

    // Publish this node's LIVE composited window origin during the per-frame
    // scene build. This is the only pass that runs in the app runtime
    // (`build_layout_tree` is disabled there), and it uses `local_translated_
    // content_offset` — the SAME live scroll/graphics-layer translation the
    // renderer applies to the content — so:
    //  * a `BasicTextField`'s `node_origin` (read by its draw closure to anchor
    //    the overlay selection-handle / context-menu `Popup`s) tracks the field
    //    through a fling, even though the in-content caret/highlight already
    //    follow via the layer transform; and
    //  * a scroll container's viewport rect is known for its
    //    `BringIntoViewResponder`.
    // `parent_abs` is `None` in the partial (dirty-subtree) rebuild path where
    // the ancestor origin is unknown; the sinks then keep their last full-build
    // value instead of being written wrong.
    let this_abs = parent_abs.map(|parent| {
        let (tx, ty) = modifier_slices
            .graphics_layer()
            .map(|layer| (layer.translation_x, layer.translation_y))
            .unwrap_or((0.0, 0.0));
        let top_left = Point {
            x: parent.content_origin.x + layout_state.position.x,
            y: parent.content_origin.y + layout_state.position.y,
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
                width: layout_state.size.width,
                height: layout_state.size.height,
            });
        }
    }
    // Children inherit this node's content origin plus its `content_offset` —
    // the SAME translation this build applies to child layer transforms below
    // (a `LazyColumn`/`vertical_scroll` bakes the live scroll into its children's
    // placement, so this tracks the scroll frame-to-frame). Using the layout
    // content offset (not the snap-anchor `translated_content_offset`, which is
    // a raster pixel-snap detail) keeps `node_origin` exactly on the rendered
    // glyphs, so the overlay handle/menu `Popup`s stay glued to the text.
    let child_abs = this_abs.map(|(top_left, layer_translation)| AbsOrigin {
        content_origin: Point {
            x: top_left.x + layout_state.content_offset.x,
            y: top_left.y + layout_state.content_offset.y,
        },
        layer_translation,
    });

    let mut render_children = draw_nodes(
        modifier_slices.draw_commands(),
        DrawPlacement::Behind,
        layout_state.size,
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
        modifier_slices.draw_commands(),
        DrawPlacement::Overlay,
        layout_state.size,
        PrimitivePhase::AfterChildren,
    ));
    let has_hit_targets = hit_test.is_some()
        || render_children.iter().any(|child| match child {
            RenderNode::Layer(child_layer) => child_layer.has_hit_targets,
            RenderNode::Primitive(_) => false,
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
        // Remember where this layer places its children so a later partial
        // rebuild of a dirty descendant subtree can resolve live window origins
        // without re-walking from the root (see `AbsOrigin`).
        scene_children_origin: child_abs.map(|c| c.content_origin).unwrap_or_default(),
        scene_children_layer_translation: child_abs
            .map(|c| c.layer_translation)
            .unwrap_or_default(),
        graphics_layer,
        clip_to_bounds,
        shadow_clip,
        hit_test,
        has_hit_targets,
        isolation,
        cache_policy,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children: render_children,
    };
    Some(layer)
}

fn draw_nodes(
    commands: &[DrawCommand],
    placement: DrawPlacement,
    size: Size,
    phase: PrimitivePhase,
) -> Vec<RenderNode> {
    let mut nodes = Vec::new();
    for command in commands {
        for primitive in primitives_for_placement(command, placement, size) {
            nodes.push(RenderNode::Primitive(PrimitiveEntry {
                phase,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive,
                    clip: None,
                }),
            }));
        }
    }
    nodes
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

    // Single-line text fields pan horizontally to keep the cursor visible:
    // the text is laid out unconstrained (no wrapping), shifted left by the
    // pan offset, and clipped to the field bounds.
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

fn resolve_text_measure_width(
    content_width: f32,
    padding: cranpose_ui::EdgeInsets,
    measured_max_width: Option<f32>,
    options: TextLayoutOptions,
) -> f32 {
    let available = measured_max_width
        .map(|max_width| (max_width - padding.left - padding.right).max(0.0))
        .unwrap_or(content_width);
    if options.soft_wrap || options.max_lines != 1 || options.overflow == TextOverflow::Clip {
        available.min(content_width)
    } else {
        content_width
    }
}

fn resolve_text_horizontal_offset(
    text_style: &TextStyle,
    text: &str,
    content_width: f32,
    measured_width: f32,
) -> f32 {
    let remaining = (content_width - measured_width).max(0.0);
    let paragraph_style = &text_style.paragraph_style;
    let direction = resolve_text_direction(text, Some(paragraph_style.text_direction));
    match paragraph_style.text_align {
        TextAlign::Center => remaining * 0.5,
        TextAlign::End | TextAlign::Right => remaining,
        TextAlign::Start | TextAlign::Left | TextAlign::Justify => {
            if direction == cranpose_ui::text::ResolvedTextDirection::Rtl {
                remaining
            } else {
                0.0
            }
        }
        TextAlign::Unspecified => {
            if direction == cranpose_ui::text::ResolvedTextDirection::Rtl {
                remaining
            } else {
                0.0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope, LazyListState};
    use cranpose_ui::text::{
        AnnotatedString, BaselineShift, SpanStyle, TextAlign, TextDirection, TextMotion,
    };
    use cranpose_ui::{
        Color, Column, ColumnSpec, DrawCommand, LayoutEngine, LazyColumn, LazyColumnSpec,
        LinearArrangement, Modifier, Point, Rect, ResolvedModifiers, RoundedCornerShape,
        ScrollState, Size, Spacer, Text, TextStyle,
    };
    use cranpose_ui_graphics::{Brush, DrawPrimitive, GraphicsLayer, RenderEffect};

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
            RenderNode::Primitive(_) => None,
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
                RenderNode::Primitive(_) => None,
            })
        }

        search(layer, node_id, ProjectiveTransform::identity())
    }

    fn find_translated_content_offset(layer: &LayerNode) -> Option<Point> {
        if layer.translated_content_context {
            return Some(layer.translated_content_offset);
        }
        for child in &layer.children {
            if let RenderNode::Layer(child_layer) = child {
                if let Some(offset) = find_translated_content_offset(child_layer) {
                    return Some(offset);
                }
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
                RenderNode::Primitive(_) => false,
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
        let child_command = DrawCommand::Behind(Rc::new(|_size: Size| {
            vec![DrawPrimitive::Rect {
                rect: Rect {
                    x: 3.0,
                    y: 4.0,
                    width: 20.0,
                    height: 8.0,
                },
                brush: Brush::solid(Color::WHITE),
            }]
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
        let RenderNode::Primitive(static_draw) = &static_child.children[0] else {
            panic!("expected draw primitive");
        };
        let PrimitiveNode::Draw(static_draw) = &static_draw.node else {
            panic!("expected draw primitive");
        };
        let RenderNode::Primitive(moved_draw) = &moved_child.children[0] else {
            panic!("expected draw primitive");
        };
        let PrimitiveNode::Draw(moved_draw) = &moved_draw.node else {
            panic!("expected draw primitive");
        };

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
            let child_command = DrawCommand::Behind(Rc::new(|_size: Size| {
                vec![DrawPrimitive::Rect {
                    rect: Rect {
                        x: 3.0,
                        y: 4.0,
                        width: 20.0,
                        height: 8.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                }]
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
            let label = cranpose_core::useState(|| "before".to_string());
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

    /// The per-frame scene build must publish a node's LIVE composited window
    /// rect into its `report_window_rect` sink — even when the layout tree is
    /// NOT built (`build_layout_tree: false`, exactly how the app runtime
    /// measures). This is the mechanism both bug 2 (a scroll container's
    /// `BringIntoViewResponder` viewport rect) and bug 3 (a text field's live
    /// `node_origin`, which anchors the overlay selection-handle / menu popups)
    /// rely on, since the layout `place` pass never runs in the runtime.
    #[test]
    fn scene_build_publishes_live_window_rect_without_layout_tree() {
        use cranpose_ui::{measure_layout_with_options, Box, BoxSpec, MeasureLayoutOptions};
        use std::cell::Cell;

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
        // Measure like the runtime: DO NOT build the layout tree, so the layout
        // `place` pass never writes the sink. Only the scene build can.
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
        // Sanity: nothing has written the sink yet.
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
                applied: false,
                hit_graph_dirty: true,
            },
            "dirty child graph updates must not report success when the replacement cannot be rebuilt"
        );
    }

    #[test]
    fn update_graph_from_applier_refreshes_scroll_content_offset() {
        let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
        let scroll_holder_for_comp = scroll_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let scroll_state =
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| state.clone());
            *scroll_holder_for_comp.borrow_mut() = Some(scroll_state.clone());
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

        assert!(report.applied, "scroll graph update should apply in place");
        let updated_target_top =
            find_text_top(&graph.root, "scroll target").expect("updated target text");
        assert!(
            updated_target_top < initial_target_top - consumed_scroll * 0.75,
            "partial graph update must refresh scroll content offset: initial_y={initial_target_top} updated_y={updated_target_top} dirty_nodes={dirty_nodes:?}"
        );
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
            let label = cranpose_core::useState(|| "scrolled child before".to_string());
            let scroll_state =
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| state.clone());
            *label_holder_for_comp.borrow_mut() = Some(label);
            *scroll_holder_for_comp.borrow_mut() = Some(scroll_state.clone());
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

        assert!(report.applied, "dirty child graph update should apply");
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
            let alpha = cranpose_core::useState(|| 1.0f32);
            let scroll_state =
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| state.clone());
            *alpha_holder_for_comp.borrow_mut() = Some(alpha);
            *scroll_holder_for_comp.borrow_mut() = Some(scroll_state.clone());
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

        assert!(report.applied, "dirty overlay graph update should apply");
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
            let offset = cranpose_core::useState(|| 0.0f32);
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
            report.applied,
            "dirty graphics layer should be replaceable from retained applier state"
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
            let offset = cranpose_core::useState(|| 0.0f32);
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
            report.applied,
            "dirty clickable graphics layer should be replaceable from retained applier state"
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
        let behind = DrawCommand::Behind(Rc::new(|_size: Size| {
            vec![cranpose_ui_graphics::DrawPrimitive::Rect {
                rect: Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 8.0,
                    height: 6.0,
                },
                brush: Brush::solid(Color::WHITE),
            }]
        }));
        let overlay = DrawCommand::Overlay(Rc::new(|_size: Size| {
            vec![cranpose_ui_graphics::DrawPrimitive::Rect {
                rect: Rect {
                    x: 3.0,
                    y: 1.0,
                    width: 5.0,
                    height: 4.0,
                },
                brush: Brush::solid(Color::BLACK),
            }]
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
        let RenderNode::Primitive(behind) = &graph.children[0] else {
            panic!("expected before-children primitive");
        };
        let RenderNode::Layer(_) = &graph.children[1] else {
            panic!("expected child layer");
        };
        let RenderNode::Primitive(overlay) = &graph.children[2] else {
            panic!("expected after-children primitive");
        };

        assert_eq!(behind.phase, PrimitivePhase::BeforeChildren);
        assert_eq!(overlay.phase, PrimitivePhase::AfterChildren);
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

    /// Single-line text fields provide a pan resolver: the glyphs must be
    /// laid out unconstrained (no wrapping), shifted left by the pan offset,
    /// and clipped to the field bounds.
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
            let list_state = remember_lazy_list_state();
            LazyColumn(
                Modifier::empty(),
                list_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.item(Some(0), None, || {
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
        use std::cell::RefCell;
        use std::rc::Rc;

        let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let state_holder_for_comp = state_holder.clone();
        let mut composition = cranpose_ui::run_test_composition(move || {
            let list_state = remember_lazy_list_state();
            *state_holder_for_comp.borrow_mut() = Some(list_state);
            LazyColumn(
                Modifier::empty().height(120.0),
                list_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(
                        8,
                        None::<fn(usize) -> u64>,
                        None::<fn(usize) -> u64>,
                        |index| {
                            Text(
                                format!("LazyMotion {index}"),
                                Modifier::empty().padding(4.0),
                                TextStyle::default(),
                            );
                        },
                    );
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
                        node.layout_state().is_placed,
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
                            node.layout_state().is_placed,
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
        use std::cell::RefCell;
        use std::rc::Rc;

        let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let state_holder_for_comp = state_holder.clone();
        let mut composition = cranpose_ui::run_test_composition(move || {
            let list_state = remember_lazy_list_state();
            *state_holder_for_comp.borrow_mut() = Some(list_state);
            let mut spec =
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0));
            spec.beyond_bounds_item_count = 0;
            LazyColumn(Modifier::empty().height(96.0), list_state, spec, |scope| {
                scope.items(
                    12,
                    None::<fn(usize) -> u64>,
                    None::<fn(usize) -> u64>,
                    |index| {
                        Text(
                            format!("WarmRow {index}"),
                            Modifier::empty().height(32.0),
                            TextStyle::default(),
                        );
                    },
                );
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
        use std::cell::RefCell;
        use std::rc::Rc;

        let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let state_holder_for_comp = state_holder.clone();
        let mut composition = cranpose_ui::run_test_composition(move || {
            let list_state = remember_lazy_list_state();
            *state_holder_for_comp.borrow_mut() = Some(list_state);
            LazyColumn(
                Modifier::empty().height(120.0),
                list_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(
                        8,
                        None::<fn(usize) -> u64>,
                        None::<fn(usize) -> u64>,
                        |index| {
                            Text(
                                format!("LazySnap {index}"),
                                Modifier::empty().padding(4.0),
                                TextStyle::default(),
                            );
                        },
                    );
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
}
