//! Scene building pipeline - copies layout tree to render scene.
//! This module is copied from the pixels renderer to maintain compatibility.

use std::rc::Rc;

use compose_foundation::PointerEvent;
use compose_render_common::Brush;
use compose_ui::{measure_text, LayoutBox, LayoutNodeKind};
use compose_ui_graphics::{Color, GraphicsLayer, Rect, RoundedCornerShape, Size};

use crate::scene::{ClickAction, Scene};

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
    };
    render_layout_node(root, root_layer, scene, None, None, Vec::new());
}

fn render_layout_node(
    layout: &LayoutBox,
    parent_layer: GraphicsLayer,
    scene: &mut Scene,
    parent_visual_clip: Option<Rect>,
    parent_hit_clip: Option<Rect>,
    inherited_pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
) {
    match &layout.node_data.kind {
        LayoutNodeKind::Spacer => {
            render_spacer(
                layout,
                parent_layer,
                parent_visual_clip,
                parent_hit_clip,
                scene,
                inherited_pointer_inputs,
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
                inherited_pointer_inputs,
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
                inherited_pointer_inputs,
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
    mut inherited_pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
) {
    let style = NodeStyle::from_layout_node(&layout.node_data);
    let node_layer = combine_layers(parent_layer, style.graphics_layer);
    let rect = layout.rect;
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    let origin = (rect.x, rect.y);
    let transformed_rect = apply_layer_to_rect(rect, origin, node_layer);

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

    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Behind,
        rect,
        origin,
        size,
        node_layer,
        visual_clip,
        scene,
    );

    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(resolved, node_layer.scale))
    });

    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), node_layer);
        scene.push_shape(transformed_rect, brush, scaled_shape, visual_clip);
    }

    // Combine inherited pointer input handlers with the node's own handlers so that
    // ancestors (e.g., scroll containers) still receive events when interacting with
    // deeply nested children.
    inherited_pointer_inputs.extend(style.pointer_inputs.iter().cloned());

    // Render text content if present in modifier slices.
    // Text is now handled via TextModifierNode in the modifier chain.
    if let Some(value) = layout.node_data.modifier_slices().text_content() {
        let metrics = measure_text(value);
        let padding = style.padding;
        let text_rect = Rect {
            x: rect.x + padding.left,
            y: rect.y + padding.top,
            width: metrics.width,
            height: metrics.height,
        };
        let transformed_text_rect = apply_layer_to_rect(text_rect, origin, node_layer);
        scene.push_text(
            transformed_text_rect,
            value.to_string(),
            apply_layer_to_color(Color(1.0, 1.0, 1.0, 1.0), node_layer),
            node_layer.scale,
            visual_clip,
        );
    }

    for handler in &style.click_actions {
        extra_clicks.push(ClickAction::WithPoint(handler.clone()));
    }

    scene.push_hit(
        transformed_rect,
        scaled_shape,
        extra_clicks,
        inherited_pointer_inputs.clone(),
        hit_clip,
    );

    for child_layout in &layout.children {
        render_layout_node(
            child_layout,
            node_layer,
            scene,
            visual_clip,
            hit_clip,
            inherited_pointer_inputs.clone(),
        );
    }

    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Overlay,
        rect,
        origin,
        size,
        node_layer,
        visual_clip,
        scene,
    );
}

fn render_spacer(
    layout: &LayoutBox,
    parent_layer: GraphicsLayer,
    parent_visual_clip: Option<Rect>,
    parent_hit_clip: Option<Rect>,
    scene: &mut Scene,
    inherited_pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
) {
    render_container(
        layout,
        parent_layer,
        parent_visual_clip,
        parent_hit_clip,
        scene,
        Vec::new(),
        inherited_pointer_inputs,
    );
}

fn render_button(
    layout: &LayoutBox,
    on_click: Rc<std::cell::RefCell<dyn FnMut()>>,
    parent_layer: GraphicsLayer,
    parent_visual_clip: Option<Rect>,
    parent_hit_clip: Option<Rect>,
    scene: &mut Scene,
    inherited_pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
) {
    let clicks = vec![ClickAction::Simple(on_click)];
    render_container(
        layout,
        parent_layer,
        parent_visual_clip,
        parent_hit_clip,
        scene,
        clicks,
        inherited_pointer_inputs,
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
