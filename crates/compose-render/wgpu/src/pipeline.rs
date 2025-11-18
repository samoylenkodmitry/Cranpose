//! Scene building pipeline - copies layout tree to render scene.
//! This module is copied from the pixels renderer to maintain compatibility.

use std::rc::Rc;

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

pub(crate) fn render_layout_tree(root: &LayoutBox, scene: &mut Scene) {
    render_layout_node(root, GraphicsLayer::default(), scene, None, None);
}

fn render_layout_node(
    layout: &LayoutBox,
    parent_layer: GraphicsLayer,
    scene: &mut Scene,
    parent_visual_clip: Option<Rect>,
    parent_hit_clip: Option<Rect>,
) {
    match &layout.node_data.kind {
        LayoutNodeKind::Text { value } => {
            render_text(
                layout,
                value,
                parent_layer,
                parent_visual_clip,
                parent_hit_clip,
                scene,
            );
        }
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
        scene.push_shape(transformed_rect, brush, scaled_shape.clone(), visual_clip);
    }

    for handler in &style.click_actions {
        extra_clicks.push(ClickAction::WithPoint(handler.clone()));
    }

    scene.push_hit(
        transformed_rect,
        scaled_shape.clone(),
        extra_clicks,
        style.pointer_inputs.clone(),
        hit_clip,
    );

    for child_layout in &layout.children {
        render_layout_node(child_layout, node_layer, scene, visual_clip, hit_clip);
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

fn render_text(
    layout: &LayoutBox,
    value: &str,
    parent_layer: GraphicsLayer,
    parent_visual_clip: Option<Rect>,
    parent_hit_clip: Option<Rect>,
    scene: &mut Scene,
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
        scene.push_shape(transformed_rect, brush, scaled_shape.clone(), visual_clip);
    }
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
    let mut click_actions = Vec::new();
    for handler in &style.click_actions {
        click_actions.push(ClickAction::WithPoint(handler.clone()));
    }
    scene.push_hit(
        transformed_rect,
        scaled_shape.clone(),
        click_actions,
        style.pointer_inputs.clone(),
        hit_clip,
    );
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
