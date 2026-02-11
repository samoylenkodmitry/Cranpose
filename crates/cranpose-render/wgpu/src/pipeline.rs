//! Scene building pipeline - copies layout tree to render scene.
//! This module is copied from the pixels renderer to maintain compatibility.

use std::rc::Rc;

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::Brush;
use cranpose_ui::{measure_text, LayoutBox, LayoutNode, LayoutNodeKind, SubcomposeLayoutNode};
use cranpose_ui_graphics::{Color, GraphicsLayer, Point, Rect, RoundedCornerShape, Size};

use crate::scene::{BackdropLayer, ClickAction, EffectLayer, Scene};

// Re-use style functions from a local copy
mod style;
use style::{
    apply_draw_commands, apply_layer_to_brush, apply_layer_to_color, apply_layer_to_rect,
    combine_layers, scale_corner_radii, DrawPlacement, NodeStyle,
};

#[allow(dead_code)]
pub(crate) fn render_layout_tree(root: &LayoutBox, scene: &mut Scene) {
    render_layout_tree_with_scale(root, scene, 1.0);
}

pub(crate) fn render_layout_tree_with_scale(root: &LayoutBox, scene: &mut Scene, scale: f32) {
    let root_layer = GraphicsLayer {
        alpha: 1.0,
        scale,
        translation_x: 0.0,
        translation_y: 0.0,
        color_filter: None,
        render_effect: None,
        backdrop_effect: None,
    };
    render_layout_node(root, root_layer, scene, None, None);
}

fn render_layout_node(
    layout: &LayoutBox,
    parent_layer: GraphicsLayer,
    scene: &mut Scene,
    parent_visual_clip: Option<Rect>,
    parent_hit_clip: Option<Rect>,
) {
    match &layout.node_data.kind {
        LayoutNodeKind::Spacer => {
            render_spacer(
                layout,
                parent_layer,
                parent_visual_clip,
                parent_hit_clip,
                scene,
            );
        }
        LayoutNodeKind::Button { on_click } => {
            render_button(
                layout,
                Rc::clone(on_click),
                parent_layer,
                parent_visual_clip,
                parent_hit_clip,
                scene,
            );
        }
        LayoutNodeKind::Layout | LayoutNodeKind::Subcompose | LayoutNodeKind::Unknown => {
            render_container(
                layout,
                parent_layer,
                parent_visual_clip,
                parent_hit_clip,
                scene,
                Vec::new(),
            );
        }
    }
}

fn render_container(
    layout: &LayoutBox,
    parent_layer: GraphicsLayer,
    parent_visual_clip: Option<Rect>,
    parent_hit_clip: Option<Rect>,
    scene: &mut Scene,
    mut extra_clicks: Vec<ClickAction>,
) {
    let style = NodeStyle::from_layout_node(&layout.node_data);
    let node_layer = combine_layers(parent_layer, style.graphics_layer);
    let rect = layout.rect;
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    let origin = (rect.x, rect.y);
    let transformed_rect = apply_layer_to_rect(rect, origin, &node_layer);

    if transformed_rect.width <= 0.0 || transformed_rect.height <= 0.0 {
        return;
    }

    let requested_visual_clip = style.clip_to_bounds.then_some(transformed_rect);
    let visual_clip = match (parent_visual_clip, requested_visual_clip) {
        (Some(parent), Some(current)) => intersect_rect(parent, current),
        (Some(parent), None) => Some(parent),
        (None, Some(current)) => Some(current),
        (None, None) => None,
    };

    if style.clip_to_bounds && visual_clip.is_none() {
        return;
    }

    let requested_hit_clip = style.clip_to_bounds.then_some(transformed_rect);
    let hit_clip = match (parent_hit_clip, requested_hit_clip) {
        (Some(parent), Some(current)) => intersect_rect(parent, current),
        (Some(parent), None) => Some(parent),
        (None, Some(current)) => Some(current),
        (None, None) => None,
    };

    // Track z_start for layer events emitted by this node.
    let has_effect = node_layer.render_effect.is_some();
    let has_backdrop = node_layer.backdrop_effect.is_some();
    let z_start = scene.next_z;

    if has_backdrop {
        if let Some(effect) = &node_layer.backdrop_effect {
            scene.backdrop_layers.push(BackdropLayer {
                rect: transformed_rect,
                clip: visual_clip,
                effect: effect.clone(),
                z_index: z_start,
            });
        }
    }

    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Behind,
        rect,
        origin,
        size,
        &node_layer,
        visual_clip,
        scene,
    );

    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(resolved, node_layer.scale))
    });

    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), &node_layer);
        scene.push_shape(transformed_rect, brush, scaled_shape, visual_clip);
    }

    // Render text content if present in modifier slices.
    // Text is now handled via TextModifierNode in the modifier chain.
    if let Some(value) = layout.node_data.modifier_slices().text_content_rc() {
        let default_text_style = cranpose_ui::text::TextStyle::default();
        let text_style_ref = layout
            .node_data
            .modifier_slices()
            .text_style()
            .unwrap_or(&default_text_style);

        let metrics = measure_text(value.as_ref(), text_style_ref);
        let padding = style.padding;
        let text_rect = Rect {
            x: rect.x + padding.left,
            y: rect.y + padding.top,
            width: metrics.width,
            height: metrics.height,
        };
        let transformed_text_rect = apply_layer_to_rect(text_rect, origin, &node_layer);

        // Extract color and font size from text style or default
        let text_color = text_style_ref.color.unwrap_or(Color(1.0, 1.0, 1.0, 1.0));
        let font_size = match text_style_ref.font_size {
            cranpose_ui::text::TextUnit::Sp(v) => v,
            cranpose_ui::text::TextUnit::Em(v) => v * 14.0, // basic Em support
            cranpose_ui::text::TextUnit::Unspecified => 14.0,
        };

        scene.push_text(
            layout.node_id,
            transformed_text_rect,
            value,
            apply_layer_to_color(text_color, &node_layer),
            font_size,
            node_layer.scale,
            visual_clip,
        );
    }

    for handler in &style.click_actions {
        extra_clicks.push(ClickAction::WithPoint(handler.clone()));
    }

    scene.push_hit(
        layout.node_id,
        transformed_rect,
        scaled_shape,
        extra_clicks,
        style.pointer_inputs.clone(),
        hit_clip,
    );

    for child_layout in &layout.children {
        render_layout_node(
            child_layout,
            node_layer.clone(),
            scene,
            visual_clip,
            hit_clip,
        );
    }

    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Overlay,
        rect,
        origin,
        size,
        &node_layer,
        visual_clip,
        scene,
    );

    // Record effect layer if this node has a render effect
    if has_effect {
        if let Some(effect) = &node_layer.render_effect {
            scene.effect_layers.push(EffectLayer {
                rect: transformed_rect,
                clip: visual_clip,
                effect: effect.clone(),
                z_start,
                z_end: scene.next_z,
            });
        }
    }
}

fn render_spacer(
    layout: &LayoutBox,
    parent_layer: GraphicsLayer,
    parent_visual_clip: Option<Rect>,
    parent_hit_clip: Option<Rect>,
    scene: &mut Scene,
) {
    render_container(
        layout,
        parent_layer,
        parent_visual_clip,
        parent_hit_clip,
        scene,
        Vec::new(),
    );
}

fn render_button(
    layout: &LayoutBox,
    on_click: Rc<std::cell::RefCell<dyn FnMut()>>,
    parent_layer: GraphicsLayer,
    parent_visual_clip: Option<Rect>,
    parent_hit_clip: Option<Rect>,
    scene: &mut Scene,
) {
    let clicks = vec![ClickAction::Simple(on_click)];
    render_container(
        layout,
        parent_layer,
        parent_visual_clip,
        parent_hit_clip,
        scene,
        clicks,
    );
}

fn intersect_rect(a: Rect, b: Rect) -> Option<Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    let width = right - left;
    let height = bottom - top;
    if width <= 0.0 || height <= 0.0 {
        None
    } else {
        Some(Rect {
            x: left,
            y: top,
            width,
            height,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// New Architecture: Direct LayoutNode Tree Rendering
// ═══════════════════════════════════════════════════════════════════════════

/// Renders the scene by traversing the LayoutNode tree directly via Applier.
/// This eliminates the need for per-frame LayoutTree reconstruction.
pub(crate) fn render_from_applier(
    applier: &mut MemoryApplier,
    root: NodeId,
    scene: &mut Scene,
    scale: f32,
) {
    let root_layer = GraphicsLayer {
        alpha: 1.0,
        scale,
        translation_x: 0.0,
        translation_y: 0.0,
        color_filter: None,
        render_effect: None,
        backdrop_effect: None,
    };
    render_node_from_applier(
        applier,
        root,
        root_layer,
        scene,
        None,
        None,
        Point::default(),
    );
}

fn render_node_from_applier(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    parent_layer: GraphicsLayer,
    scene: &mut Scene,
    parent_visual_clip: Option<Rect>,
    parent_hit_clip: Option<Rect>,
    parent_offset: Point,
) {
    // Try LayoutNode first, then SubcomposeLayoutNode
    let node_data = if let Ok(data) = applier.with_node::<LayoutNode, _>(node_id, |node| {
        let state = node.layout_state();
        let modifier_slices = node.modifier_slices_snapshot();
        let resolved_modifiers = node.resolved_modifiers();
        let children: Vec<NodeId> = node.children.iter().copied().collect();
        (state, modifier_slices, resolved_modifiers, children)
    }) {
        data
    } else if let Ok(data) = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
        let state = node.layout_state();
        let modifier_slices = node.modifier_slices_snapshot();
        let resolved_modifiers = node.resolved_modifiers();
        // For SubcomposeLayoutNode, use active_children() which returns the placed children
        let children: Vec<NodeId> = node.active_children();
        (state, modifier_slices, resolved_modifiers, children)
    }) {
        data
    } else {
        // Node not found or type mismatch with both types
        return;
    };

    let (layout_state, modifier_slices, resolved_modifiers, children) = node_data;

    // Skip nodes that weren't placed
    if !layout_state.is_placed {
        return;
    }

    // Calculate absolute position (parent offset + node position)
    let abs_x = parent_offset.x + layout_state.position.x;
    let abs_y = parent_offset.y + layout_state.position.y;

    let rect = Rect {
        x: abs_x,
        y: abs_y,
        width: layout_state.size.width,
        height: layout_state.size.height,
    };

    // Build NodeStyle from modifier data (same approach as NodeStyle::from_layout_node)
    let style = NodeStyle {
        graphics_layer: modifier_slices.graphics_layer(),
        background: None, // Now rendered via draw commands
        shape: None,      // Now encoded in draw command round rects
        padding: resolved_modifiers.padding(),
        clip_to_bounds: modifier_slices.clip_to_bounds(),
        draw_commands: modifier_slices.draw_commands().to_vec(),
        click_actions: modifier_slices.click_handlers().to_vec(),
        pointer_inputs: modifier_slices.pointer_inputs().to_vec(),
    };

    let node_layer = combine_layers(parent_layer, style.graphics_layer);
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    let origin = (rect.x, rect.y);
    let transformed_rect = apply_layer_to_rect(rect, origin, &node_layer);

    if transformed_rect.width <= 0.0 || transformed_rect.height <= 0.0 {
        return;
    }

    let requested_visual_clip = style.clip_to_bounds.then_some(transformed_rect);
    let visual_clip = match (parent_visual_clip, requested_visual_clip) {
        (Some(parent), Some(current)) => intersect_rect(parent, current),
        (Some(parent), None) => Some(parent),
        (None, Some(current)) => Some(current),
        (None, None) => None,
    };

    if style.clip_to_bounds && visual_clip.is_none() {
        return;
    }

    let requested_hit_clip = style.clip_to_bounds.then_some(transformed_rect);
    let hit_clip = match (parent_hit_clip, requested_hit_clip) {
        (Some(parent), Some(current)) => intersect_rect(parent, current),
        (Some(parent), None) => Some(parent),
        (None, Some(current)) => Some(current),
        (None, None) => None,
    };

    // Track z_start for layer events emitted by this node.
    let has_effect = node_layer.render_effect.is_some();
    let has_backdrop = node_layer.backdrop_effect.is_some();
    let z_start = scene.next_z;

    if has_backdrop {
        if let Some(effect) = &node_layer.backdrop_effect {
            scene.backdrop_layers.push(BackdropLayer {
                rect: transformed_rect,
                clip: visual_clip,
                effect: effect.clone(),
                z_index: z_start,
            });
        }
    }

    // Draw behind layer
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Behind,
        rect,
        origin,
        size,
        &node_layer,
        visual_clip,
        scene,
    );

    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(resolved, node_layer.scale))
    });

    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), &node_layer);
        scene.push_shape(transformed_rect, brush, scaled_shape, visual_clip);
    }

    // Render text content if present
    if let Some(value) = modifier_slices.text_content_rc() {
        let default_text_style = cranpose_ui::text::TextStyle::default();
        let text_style_ref = modifier_slices.text_style().unwrap_or(&default_text_style);

        let metrics = measure_text(value.as_ref(), text_style_ref);
        let padding = style.padding;
        let text_rect = Rect {
            x: rect.x + padding.left,
            y: rect.y + padding.top,
            width: metrics.width,
            height: metrics.height,
        };
        let transformed_text_rect = apply_layer_to_rect(text_rect, origin, &node_layer);

        // Extract color and font size
        let text_color = text_style_ref.color.unwrap_or(Color(1.0, 1.0, 1.0, 1.0));
        let font_size = match text_style_ref.font_size {
            cranpose_ui::text::TextUnit::Sp(v) => v,
            cranpose_ui::text::TextUnit::Em(v) => v * 14.0,
            cranpose_ui::text::TextUnit::Unspecified => 14.0,
        };

        scene.push_text(
            node_id,
            transformed_text_rect,
            value,
            apply_layer_to_color(text_color, &node_layer),
            font_size,
            node_layer.scale,
            visual_clip,
        );
    }

    // Collect click actions
    let extra_clicks: Vec<ClickAction> = style
        .click_actions
        .iter()
        .map(|h| ClickAction::WithPoint(h.clone()))
        .collect();

    scene.push_hit(
        node_id,
        transformed_rect,
        scaled_shape,
        extra_clicks,
        style.pointer_inputs.clone(),
        hit_clip,
    );

    // Recurse to children with updated offset (including parent's content offset like padding)
    let child_offset = Point {
        x: abs_x + layout_state.content_offset.x,
        y: abs_y + layout_state.content_offset.y,
    };
    for child_id in children {
        render_node_from_applier(
            applier,
            child_id,
            node_layer.clone(),
            scene,
            visual_clip,
            hit_clip,
            child_offset,
        );
    }

    // Draw overlay layer
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Overlay,
        rect,
        origin,
        size,
        &node_layer,
        visual_clip,
        scene,
    );

    // Record effect layer if this node has a render effect
    if has_effect {
        if let Some(effect) = &node_layer.render_effect {
            scene.effect_layers.push(EffectLayer {
                rect: transformed_rect,
                clip: visual_clip,
                effect: effect.clone(),
                z_start,
                z_end: scene.next_z,
            });
        }
    }
}
