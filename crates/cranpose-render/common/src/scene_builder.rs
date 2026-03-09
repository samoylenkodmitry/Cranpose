use std::rc::Rc;

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui::text::AnnotatedString;
use cranpose_ui::text::{resolve_text_direction, TextAlign, TextStyle};
use cranpose_ui::{
    prepare_text_layout, DrawCommand, LayoutBox, LayoutNode, Point, Rect, ResolvedModifiers, Size,
    SubcomposeLayoutNode, TextLayoutOptions, TextOverflow,
};
use cranpose_ui_graphics::{CompositingStrategy, GraphicsLayer};

use crate::graph::{
    CachePolicy, DrawPrimitiveNode, HitTestNode, IsolationReasons, LayerNode, PrimitiveEntry,
    PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode, TextPrimitiveNode,
};
use crate::layer_transform::layer_transform_to_parent;
use crate::raster_cache::LayerRasterCacheHashes;
use crate::style_shared::{combine_layers, primitives_for_placement, DrawPlacement};

const TEXT_CLIP_PAD: f32 = 1.0;

#[derive(Clone)]
struct BuildNodeSnapshot {
    node_id: NodeId,
    placement: Point,
    size: Size,
    content_offset: Point,
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
    draw_commands: Vec<DrawCommand>,
    click_actions: Vec<Rc<dyn Fn(Point)>>,
    pointer_inputs: Vec<Rc<dyn Fn(cranpose_foundation::PointerEvent)>>,
    clip_to_bounds: bool,
    annotated_text: Option<AnnotatedString>,
    text_style: Option<TextStyle>,
    text_layout_options: Option<TextLayoutOptions>,
    graphics_layer: Option<GraphicsLayer>,
    resolved_modifiers: ResolvedModifiers,
    children: Vec<NodeId>,
}

pub fn build_graph_from_layout_tree(root: &LayoutBox, scale: f32) -> RenderGraph {
    let root_snapshot = layout_box_to_snapshot(root, None);
    RenderGraph {
        root: build_layer_node(root_snapshot, scale),
    }
}

pub fn build_graph_from_applier(
    applier: &mut MemoryApplier,
    root: NodeId,
    scale: f32,
) -> Option<RenderGraph> {
    let snapshot = snapshot_from_applier(applier, root)?;
    Some(RenderGraph {
        root: build_layer_node(snapshot, scale),
    })
}

fn build_layer_node(snapshot: BuildNodeSnapshot, root_scale: f32) -> LayerNode {
    let local_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: snapshot.size.width,
        height: snapshot.size.height,
    };
    let mut graphics_layer = snapshot.graphics_layer.clone().unwrap_or_default();
    if root_scale != 1.0 {
        graphics_layer = combine_layers(
            GraphicsLayer {
                scale: root_scale,
                ..GraphicsLayer::default()
            },
            Some(graphics_layer),
        );
    }
    let transform_to_parent =
        layer_transform_to_parent(local_bounds, snapshot.placement, &graphics_layer);
    let isolation = isolation_reasons(&graphics_layer);
    let cache_policy = if isolation.has_any() {
        CachePolicy::Auto
    } else {
        CachePolicy::None
    };
    let clip_to_bounds = snapshot.clip_to_bounds;
    let shadow_clip = clip_to_bounds.then_some(local_bounds);
    let hit_test = (!snapshot.click_actions.is_empty() || !snapshot.pointer_inputs.is_empty())
        .then(|| HitTestNode {
            shape: None,
            click_actions: snapshot.click_actions.clone(),
            pointer_inputs: snapshot.pointer_inputs.clone(),
            clip: (clip_to_bounds || graphics_layer.clip).then_some(local_bounds),
        });

    let mut children = draw_nodes(
        &snapshot.draw_commands,
        DrawPlacement::Behind,
        snapshot.size,
        PrimitivePhase::BeforeChildren,
    );
    if let Some(text) = text_node(&snapshot, local_bounds) {
        children.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(text)),
        }));
    }
    for child in snapshot.children {
        let mut child_layer = build_layer_node(child, 1.0);
        if snapshot.content_offset != Point::default() {
            child_layer.transform_to_parent =
                child_layer
                    .transform_to_parent
                    .then(ProjectiveTransform::translation(
                        snapshot.content_offset.x,
                        snapshot.content_offset.y,
                    ));
        }
        children.push(RenderNode::Layer(Box::new(child_layer)));
    }
    children.extend(draw_nodes(
        &snapshot.draw_commands,
        DrawPlacement::Overlay,
        snapshot.size,
        PrimitivePhase::AfterChildren,
    ));

    let mut layer = LayerNode {
        node_id: Some(snapshot.node_id),
        local_bounds,
        placement: snapshot.placement,
        content_offset: snapshot.content_offset,
        transform_to_parent,
        graphics_layer,
        clip_to_bounds,
        shadow_clip,
        hit_test,
        isolation,
        cache_policy,
        cache_hashes: LayerRasterCacheHashes::default(),
        children,
    };
    layer.recompute_raster_cache_hashes();
    layer
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

fn text_node(snapshot: &BuildNodeSnapshot, local_bounds: Rect) -> Option<TextPrimitiveNode> {
    let value = snapshot.annotated_text.as_ref()?;
    let default_text_style = TextStyle::default();
    let text_style = snapshot.text_style.clone().unwrap_or(default_text_style);
    let options = snapshot
        .text_layout_options
        .unwrap_or_default()
        .normalized();
    let padding = snapshot.resolved_modifiers.padding();
    let content_width = (local_bounds.width - padding.left - padding.right).max(0.0);
    if content_width <= 0.0 {
        return None;
    }

    let measure_width =
        resolve_text_measure_width(content_width, padding, snapshot.measured_max_width, options);
    let prepared = prepare_text_layout(
        value,
        &text_style,
        options,
        Some(measure_width).filter(|width| width.is_finite() && *width > 0.0),
    );
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
    let font_size = text_style.resolve_font_size(14.0);
    let expanded_bounds =
        expand_text_bounds_for_baseline_shift(text_bounds, &text_style, font_size);
    let clip = if options.overflow == TextOverflow::Visible {
        None
    } else {
        Some(pad_clip_rect(expanded_bounds))
    };

    Some(TextPrimitiveNode {
        node_id: snapshot.node_id,
        rect,
        text: prepared.text,
        text_style,
        font_size,
        layout_options: options,
        clip,
    })
}

fn snapshot_from_applier(
    applier: &mut MemoryApplier,
    node_id: NodeId,
) -> Option<BuildNodeSnapshot> {
    if let Ok(data) = applier.with_node::<LayoutNode, _>(node_id, |node| {
        let state = node.layout_state();
        let children = node.children.clone();
        let modifier_slices = node.modifier_slices_snapshot();
        SnapshotNodeData {
            layout_state: state,
            draw_commands: modifier_slices.draw_commands().to_vec(),
            click_actions: modifier_slices.click_handlers().to_vec(),
            pointer_inputs: modifier_slices.pointer_inputs().to_vec(),
            clip_to_bounds: modifier_slices.clip_to_bounds(),
            annotated_text: modifier_slices.annotated_string(),
            text_style: modifier_slices.text_style().cloned(),
            text_layout_options: modifier_slices.text_layout_options(),
            graphics_layer: modifier_slices.graphics_layer(),
            resolved_modifiers: node.resolved_modifiers(),
            children,
        }
    }) {
        return snapshot_from_node_data(applier, node_id, data);
    }

    if let Ok(data) = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
        let state = node.layout_state();
        let children = node.active_children();
        let modifier_slices = node.modifier_slices_snapshot();
        SnapshotNodeData {
            layout_state: state,
            draw_commands: modifier_slices.draw_commands().to_vec(),
            click_actions: modifier_slices.click_handlers().to_vec(),
            pointer_inputs: modifier_slices.pointer_inputs().to_vec(),
            clip_to_bounds: modifier_slices.clip_to_bounds(),
            annotated_text: modifier_slices.annotated_string(),
            text_style: modifier_slices.text_style().cloned(),
            text_layout_options: modifier_slices.text_layout_options(),
            graphics_layer: modifier_slices.graphics_layer(),
            resolved_modifiers: node.resolved_modifiers(),
            children,
        }
    }) {
        return snapshot_from_node_data(applier, node_id, data);
    }

    None
}

fn snapshot_from_node_data(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    data: SnapshotNodeData,
) -> Option<BuildNodeSnapshot> {
    let SnapshotNodeData {
        layout_state,
        draw_commands,
        click_actions,
        pointer_inputs,
        clip_to_bounds,
        annotated_text,
        text_style,
        text_layout_options,
        graphics_layer,
        resolved_modifiers,
        children,
    } = data;
    if !layout_state.is_placed {
        return None;
    }

    let mut child_snapshots = Vec::new();
    for child_id in children {
        let Some(child_snapshot) = snapshot_from_applier(applier, child_id) else {
            continue;
        };
        child_snapshots.push(child_snapshot);
    }

    Some(BuildNodeSnapshot {
        node_id,
        placement: layout_state.position,
        size: layout_state.size,
        content_offset: layout_state.content_offset,
        measured_max_width: layout_state
            .measurement_constraints
            .max_width
            .is_finite()
            .then_some(layout_state.measurement_constraints.max_width),
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

    BuildNodeSnapshot {
        node_id: node.node_id,
        placement,
        size: Size {
            width: node.rect.width,
            height: node.rect.height,
        },
        content_offset: node.content_offset,
        measured_max_width: None,
        resolved_modifiers: node.node_data.resolved_modifiers,
        draw_commands: node.node_data.modifier_slices.draw_commands().to_vec(),
        click_actions: node.node_data.modifier_slices.click_handlers().to_vec(),
        pointer_inputs: node.node_data.modifier_slices.pointer_inputs().to_vec(),
        clip_to_bounds: node.node_data.modifier_slices.clip_to_bounds(),
        annotated_text: node.node_data.modifier_slices.annotated_string(),
        text_style: node.node_data.modifier_slices.text_style().cloned(),
        text_layout_options: node.node_data.modifier_slices.text_layout_options(),
        graphics_layer: node.node_data.modifier_slices.graphics_layer(),
        children,
    }
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
    use std::rc::Rc;

    use cranpose_ui::text::{AnnotatedString, BaselineShift, TextAlign, TextDirection};
    use cranpose_ui::{Color, DrawCommand, Point, Rect, ResolvedModifiers, Size};
    use cranpose_ui_graphics::{Brush, DrawPrimitive, GraphicsLayer};

    use super::*;

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
        let static_graph = build_layer_node(snapshot_with_translation(0.0), 1.0);
        let moved_graph = build_layer_node(snapshot_with_translation(23.5), 1.0);

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
        let static_graph = build_layer_node(snapshot_with_translation(0.0), 1.0);
        let moved_graph = build_layer_node(snapshot_with_translation(23.5), 1.0);

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

        let graph = build_layer_node(parent, 1.0);
        let RenderNode::Layer(child) = &graph.children[0] else {
            panic!("expected child layer");
        };

        let top_left = child.transform_to_parent.map_point(Point::default());
        assert_eq!(top_left, Point { x: 24.0, y: -2.0 });
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

        let graph = build_layer_node(parent, 1.0);
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

        let static_graph = build_layer_node(parent, 1.0);
        let moved_graph = build_layer_node(moved_parent, 1.0);

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

        let base_graph = build_layer_node(base, 1.0);
        let effected_graph = build_layer_node(effected, 1.0);

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

        let graph = build_layer_node(snapshot, 1.0);
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
}
