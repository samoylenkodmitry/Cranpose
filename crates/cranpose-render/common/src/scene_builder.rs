use std::rc::Rc;

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui::text::AnnotatedString;
use cranpose_ui::text::{resolve_text_direction, TextAlign, TextStyle};
use cranpose_ui::{
    prepare_text_layout, DrawCommand, LayoutBox, LayoutNode, ModifierNodeSlices, Point, Rect,
    ResolvedModifiers, Size, SubcomposeLayoutNode, TextLayoutOptions, TextOverflow,
};
use cranpose_ui_graphics::{
    rounded_corner_alpha_mask_effect, CompositingStrategy, GraphicsLayer, RoundedCornerShape,
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
    graphics_layer: Option<GraphicsLayer>,
    children: Vec<Self>,
}

struct SnapshotNodeData {
    layout_state: cranpose_ui::widgets::LayoutState,
    modifier_slices: Rc<ModifierNodeSlices>,
    resolved_modifiers: ResolvedModifiers,
    children: Vec<NodeId>,
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
    if dirty_nodes.is_empty() {
        return true;
    }

    let mut updated = false;
    for &node_id in dirty_nodes {
        if graph.root.node_id == Some(node_id) {
            let Some(root) = build_layer_node_from_applier(applier, node_id, scale, false) else {
                return false;
            };
            graph.root = root;
            updated = true;
            continue;
        }

        let inherited_translated_content_context = graph.root.translated_content_context;
        match replace_layer_from_applier(
            applier,
            &mut graph.root,
            node_id,
            inherited_translated_content_context,
        ) {
            Some(true) => updated = true,
            Some(false) => return false,
            None => return false,
        }
    }

    if updated {
        graph.root.recompute_raster_cache_hashes();
    }
    true
}

fn replace_layer_from_applier(
    applier: &mut MemoryApplier,
    parent: &mut LayerNode,
    node_id: NodeId,
    inherited_translated_content_context: bool,
) -> Option<bool> {
    let child_inherited_translated_content_context =
        inherited_translated_content_context || parent.translated_content_context;

    for child in &mut parent.children {
        let RenderNode::Layer(child_layer) = child else {
            continue;
        };

        if child_layer.node_id == Some(node_id) {
            let old_transform = child_layer.transform_to_parent;
            let Some(mut replacement) = build_layer_node_from_applier_internal(
                applier,
                node_id,
                parent.motion_context_animated,
                child_inherited_translated_content_context,
            ) else {
                return Some(false);
            };
            replacement.transform_to_parent = old_transform;
            **child_layer = replacement;
            parent.has_hit_targets = parent.hit_test.is_some()
                || parent.children.iter().any(|child| match child {
                    RenderNode::Layer(child_layer) => child_layer.has_hit_targets,
                    RenderNode::Primitive(_) => false,
                });
            return Some(true);
        }

        if let Some(updated) = replace_layer_from_applier(
            applier,
            child_layer,
            node_id,
            child_inherited_translated_content_context,
        ) {
            if updated {
                parent.has_hit_targets = parent.hit_test.is_some()
                    || parent.children.iter().any(|child| match child {
                        RenderNode::Layer(child_layer) => child_layer.has_hit_targets,
                        RenderNode::Primitive(_) => false,
                    });
            }
            return Some(updated);
        }
    }

    None
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
        motion_context_animated: node_motion_context_animated,
        translated_content_context,
        translated_content_offset: if translated_content_context {
            content_offset
        } else {
            Point::default()
        },
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
    )
}

fn build_layer_node_from_applier_internal(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    inherited_motion_context_animated: bool,
    inherited_translated_content_context: bool,
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
        motion_context_animated: node_motion_context_animated,
        translated_content_context: local_translated_content_context,
        translated_content_offset: if local_translated_content_context {
            local_translated_content_offset
        } else {
            Point::default()
        },
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

    let measure_width =
        resolve_text_measure_width(content_width, padding, measured_max_width, options);
    let max_width = Some(measure_width).filter(|width| width.is_finite() && *width > 0.0);
    let prepared = modifier_slices
        .and_then(|slices| slices.prepare_text_layout(max_width))
        .unwrap_or_else(|| prepare_text_layout(value, &text_style, options, max_width));
    let visual_style = prepared.visual_style.clone();
    let draw_width = if options.overflow == TextOverflow::Visible {
        prepared.metrics.width
    } else {
        content_width
    };
    let alignment_offset = resolve_text_horizontal_offset(
        &text_style,
        prepared.text.text.as_str(),
        content_width,
        prepared.metrics.width,
    );
    let rect = Rect {
        x: padding.left + alignment_offset,
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
    let clip = if options.overflow == TextOverflow::Visible {
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

    let rounded_clip = rounded_corner_alpha_mask_effect(
        local_bounds.width,
        local_bounds.height,
        radii,
        ROUNDED_CLIP_EDGE_FEATHER,
    );
    graphics_layer.render_effect = Some(match graphics_layer.render_effect.take() {
        Some(existing) => existing.then(rounded_clip),
        None => rounded_clip,
    });
    graphics_layer
}

fn isolation_reasons(layer: &GraphicsLayer) -> IsolationReasons {
    IsolationReasons {
        explicit_offscreen: layer.compositing_strategy == CompositingStrategy::Offscreen,
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
        Color, DrawCommand, LayoutEngine, LazyColumn, LazyColumnSpec, Modifier, Point, Rect,
        ResolvedModifiers, RoundedCornerShape, Size, Text, TextStyle,
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

    fn find_layer_by_node_id(layer: &LayerNode, node_id: NodeId) -> Option<&LayerNode> {
        if layer.node_id == Some(node_id) {
            return Some(layer);
        }
        layer.children.iter().find_map(|child| match child {
            RenderNode::Layer(child_layer) => find_layer_by_node_id(child_layer, node_id),
            RenderNode::Primitive(_) => None,
        })
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
    fn rounded_clip_to_bounds_injects_per_corner_mask_effect() {
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

        let Some(RenderEffect::Shader { shader }) = layer.render_effect else {
            panic!("rounded clip must emit a shader mask");
        };
        let uniforms = shader.uniforms();
        assert_eq!(uniforms[0], 100.0);
        assert_eq!(uniforms[1], 40.0);
        assert_eq!(uniforms[2], ROUNDED_CLIP_EDGE_FEATHER);
        assert_eq!(uniforms[3], 4.0);
        assert_eq!(uniforms[4], 8.0);
        assert_eq!(uniforms[5], 12.0);
        assert_eq!(uniforms[6], 16.0);
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
    fn rounded_corners_clip_to_bounds_builds_graph_mask_from_modifier_chain() {
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

        assert!(
            graph_has_runtime_shader_effect(&graph.root),
            "rounded_corners().clip_to_bounds() must build a shaped mask effect for descendants"
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
