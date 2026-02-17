use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::Brush;
use cranpose_ui::{
    prepare_text_layout, LayoutBox, LayoutNode, LayoutNodeKind, SubcomposeLayoutNode, TextOverflow,
};
use cranpose_ui_graphics::{
    BlendMode, Color, CompositingStrategy, GraphicsLayer, LayerShape, Point, Rect, RenderEffect,
    RoundedCornerShape, Size,
};

use crate::scene::{ClickAction, Scene};
use crate::style::{
    apply_draw_commands, apply_layer_affine_to_rect, apply_layer_to_brush, apply_layer_to_color,
    apply_layer_to_quad, apply_layer_to_rect, combine_layers, layer_uniform_scale, quad_bounds,
    scale_corner_radii, DrawPlacement, NodeStyle,
};

const TEXT_CLIP_PAD: f32 = 1.0;

fn pad_clip_rect(rect: Rect) -> Rect {
    Rect {
        x: rect.x - TEXT_CLIP_PAD,
        y: rect.y - TEXT_CLIP_PAD,
        width: (rect.width + TEXT_CLIP_PAD * 2.0).max(0.0),
        height: (rect.height + TEXT_CLIP_PAD * 2.0).max(0.0),
    }
}

static REPORTED_UNSUPPORTED_PIXELS_EFFECTS: AtomicBool = AtomicBool::new(false);

fn is_render_effect_supported(_effect: &RenderEffect) -> bool {
    false
}

fn layer_requires_effect_fallback(layer: &GraphicsLayer) -> bool {
    layer
        .render_effect
        .as_ref()
        .is_some_and(|effect| !is_render_effect_supported(effect))
        || layer
            .backdrop_effect
            .as_ref()
            .is_some_and(|effect| !is_render_effect_supported(effect))
        || layer.compositing_strategy == CompositingStrategy::Offscreen
        || layer.blend_mode != BlendMode::SrcOver
}

fn report_unsupported_effects(layer: &GraphicsLayer) {
    if layer_requires_effect_fallback(layer)
        && !REPORTED_UNSUPPORTED_PIXELS_EFFECTS.swap(true, Ordering::Relaxed)
    {
        log::warn!(
            "Pixels renderer does not support render/backdrop effects, offscreen compositing, or non-SrcOver layer blend modes; falling back to base layer rendering"
        );
    }
}

#[derive(Clone, Copy)]
struct ShadowSample {
    expansion: f32,
    weight: f32,
    spot_offset_scale: f32,
}

fn shadow_samples(elevation: f32) -> Vec<ShadowSample> {
    if elevation <= f32::EPSILON {
        return Vec::new();
    }

    let blur_radius = (elevation * 0.95).max(1.0);
    let sample_count = ((blur_radius * 2.4).ceil() as usize).clamp(8, 36);
    let sigma = (blur_radius * 0.5).max(1.0);
    let mut samples = Vec::with_capacity(sample_count);
    let mut weight_sum = 0.0f32;

    for index in 0..sample_count {
        let t0 = index as f32 / sample_count as f32;
        let t1 = (index + 1) as f32 / sample_count as f32;
        let center = blur_radius * (t0 + t1) * 0.5;
        let expansion = blur_radius * t1;
        let weight = (-0.5 * (center / sigma).powi(2)).exp().max(0.0001);
        samples.push(ShadowSample {
            expansion,
            weight,
            spot_offset_scale: 0.35 + t1 * 0.65,
        });
        weight_sum += weight;
    }

    if weight_sum <= f32::EPSILON {
        return vec![ShadowSample {
            expansion: blur_radius,
            weight: 1.0,
            spot_offset_scale: 1.0,
        }];
    }

    for sample in &mut samples {
        sample.weight /= weight_sum;
    }

    samples
}

fn push_layer_shadow(
    scene: &mut Scene,
    layer: &GraphicsLayer,
    layer_bounds: Rect,
    transformed_bounds: Rect,
    clip: Option<Rect>,
) {
    if layer.shadow_elevation <= 0.0 {
        return;
    }

    let scale = layer_uniform_scale(layer).max(0.1);
    let elevation = layer.shadow_elevation * scale;
    let spread = (elevation * 0.22).max(0.8);
    let spot_offset_x = elevation * 0.18;
    let spot_offset_y = elevation * 0.62;
    let samples = shadow_samples(elevation);
    if samples.is_empty() {
        return;
    }

    let resolved_shape = match layer.shape {
        LayerShape::Rectangle => None,
        LayerShape::Rounded(shape) => {
            let resolved = shape.resolve(layer_bounds.width, layer_bounds.height);
            Some(RoundedCornerShape::with_radii(scale_corner_radii(
                resolved, scale,
            )))
        }
    };

    let ambient_base_alpha = (layer.ambient_shadow_color.a() * 0.48).clamp(0.0, 1.0);
    let spot_base_alpha = (layer.spot_shadow_color.a() * 0.62).clamp(0.0, 1.0);

    for sample in samples.iter().rev() {
        let ambient_alpha = (ambient_base_alpha * sample.weight * 1.18).clamp(0.0, 1.0);
        if ambient_alpha > f32::EPSILON {
            let ambient = Color(
                layer.ambient_shadow_color.r(),
                layer.ambient_shadow_color.g(),
                layer.ambient_shadow_color.b(),
                ambient_alpha,
            );
            let ambient_expansion = spread + sample.expansion;
            let ambient_rect = Rect {
                x: transformed_bounds.x - ambient_expansion,
                y: transformed_bounds.y - ambient_expansion,
                width: transformed_bounds.width + ambient_expansion * 2.0,
                height: transformed_bounds.height + ambient_expansion * 2.0,
            };
            scene.push_shape(
                ambient_rect,
                Brush::solid(ambient),
                resolved_shape,
                clip,
                BlendMode::SrcOver,
            );
        }

        let spot_alpha = (spot_base_alpha * sample.weight * 1.24).clamp(0.0, 1.0);
        if spot_alpha > f32::EPSILON {
            let spot = Color(
                layer.spot_shadow_color.r(),
                layer.spot_shadow_color.g(),
                layer.spot_shadow_color.b(),
                spot_alpha,
            );
            let spot_expansion = spread * 0.7 + sample.expansion * 0.78;
            let spot_dx = spot_offset_x * sample.spot_offset_scale;
            let spot_dy = spot_offset_y * sample.spot_offset_scale;
            let spot_rect = Rect {
                x: transformed_bounds.x + spot_dx - spot_expansion,
                y: transformed_bounds.y + spot_dy - spot_expansion,
                width: transformed_bounds.width + spot_expansion * 2.0,
                height: transformed_bounds.height + spot_expansion * 2.0,
            };
            scene.push_shape(
                spot_rect,
                Brush::solid(spot),
                resolved_shape,
                clip,
                BlendMode::SrcOver,
            );
        }
    }
}

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
    report_unsupported_effects(&node_layer);
    let rect = layout.rect;
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    let transformed_rect = apply_layer_to_rect(rect, rect, &node_layer);

    if transformed_rect.width <= 0.0 || transformed_rect.height <= 0.0 {
        return;
    }

    let content_clip_to_bounds = style.clip_to_bounds || node_layer.clip;
    let visual_clip = resolve_clip(
        parent_visual_clip,
        content_clip_to_bounds.then_some(transformed_rect),
    );

    if content_clip_to_bounds && visual_clip.is_none() {
        return;
    }

    let hit_clip = resolve_clip(
        parent_hit_clip,
        content_clip_to_bounds.then_some(transformed_rect),
    );

    // GraphicsLayer clipping clips content, but should not clip its own shadow.
    // Explicit clip-to-bounds modifiers still clip both.
    let shadow_clip = resolve_clip(
        parent_visual_clip,
        style.clip_to_bounds.then_some(transformed_rect),
    );
    push_layer_shadow(scene, &node_layer, rect, transformed_rect, shadow_clip);

    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Behind,
        rect,
        size,
        &node_layer,
        visual_clip,
        scene,
    );

    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(
            resolved,
            layer_uniform_scale(&node_layer),
        ))
    });

    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), &node_layer);
        let local_rect = apply_layer_affine_to_rect(rect, rect, &node_layer);
        let quad = apply_layer_to_quad(rect, rect, &node_layer);
        scene.push_shape_with_geometry(
            quad_bounds(quad),
            local_rect,
            quad,
            brush,
            scaled_shape,
            visual_clip,
            BlendMode::SrcOver,
        );
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

        let options = layout
            .node_data
            .modifier_slices()
            .text_layout_options()
            .unwrap_or_default()
            .normalized();
        let padding = style.padding;
        let max_width = (rect.width - padding.left - padding.right).max(0.0);
        let prepared = prepare_text_layout(
            value.as_ref(),
            text_style_ref,
            options,
            Some(max_width).filter(|w| w.is_finite() && *w > 0.0),
        );
        let draw_width = if options.overflow == TextOverflow::Visible {
            prepared.metrics.width
        } else {
            max_width
        };
        let text_rect = Rect {
            x: rect.x + padding.left,
            y: rect.y + padding.top,
            width: draw_width,
            height: prepared.metrics.height,
        };
        let transformed_text_rect = apply_layer_to_rect(text_rect, rect, &node_layer);
        let text_bounds_rect = Rect {
            x: rect.x + padding.left,
            y: rect.y + padding.top,
            width: max_width,
            height: (rect.height - padding.top - padding.bottom).max(0.0),
        };
        let transformed_text_bounds = apply_layer_to_rect(text_bounds_rect, rect, &node_layer);
        let text_clip = resolve_text_clip(options.overflow, visual_clip, transformed_text_bounds);

        // Extract color and font size from text style or default
        let text_color = text_style_ref.color.unwrap_or(Color(1.0, 1.0, 1.0, 1.0));
        let font_size = match text_style_ref.font_size {
            cranpose_ui::text::TextUnit::Sp(v) => v,
            cranpose_ui::text::TextUnit::Em(v) => v * 14.0, // basic Em support
            cranpose_ui::text::TextUnit::Unspecified => 14.0,
        };

        if let Some(text_clip) = text_clip {
            scene.push_text(
                layout.node_id,
                transformed_text_rect,
                Rc::from(prepared.text),
                apply_layer_to_color(text_color, &node_layer),
                text_style_ref.clone(),
                font_size,
                layer_uniform_scale(&node_layer),
                options,
                text_clip,
            );
        }
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
        size,
        &node_layer,
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

fn resolve_clip(parent_clip: Option<Rect>, requested_clip: Option<Rect>) -> Option<Rect> {
    match (parent_clip, requested_clip) {
        (Some(parent), Some(current)) => parent.intersect(current),
        (Some(parent), None) => Some(parent),
        (None, Some(current)) => Some(current),
        (None, None) => None,
    }
}

fn resolve_text_clip(
    overflow: TextOverflow,
    visual_clip: Option<Rect>,
    transformed_text_bounds: Rect,
) -> Option<Option<Rect>> {
    if overflow == TextOverflow::Visible {
        return Some(visual_clip);
    }
    resolve_clip(visual_clip, Some(pad_clip_rect(transformed_text_bounds))).map(Some)
}

// ═══════════════════════════════════════════════════════════════════════════
// Direct LayoutNode Tree Rendering (from Applier)
// ═══════════════════════════════════════════════════════════════════════════

/// Renders the scene by traversing the LayoutNode tree directly via Applier.
/// This eliminates the need for per-frame LayoutTree reconstruction.
pub(crate) fn render_from_applier(applier: &mut MemoryApplier, root: NodeId, scene: &mut Scene) {
    let root_layer = GraphicsLayer::default();
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

    // Build NodeStyle from modifier data
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
    report_unsupported_effects(&node_layer);
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    let transformed_rect = apply_layer_to_rect(rect, rect, &node_layer);

    if transformed_rect.width <= 0.0 || transformed_rect.height <= 0.0 {
        return;
    }

    let content_clip_to_bounds = style.clip_to_bounds || node_layer.clip;
    let visual_clip = resolve_clip(
        parent_visual_clip,
        content_clip_to_bounds.then_some(transformed_rect),
    );

    if content_clip_to_bounds && visual_clip.is_none() {
        return;
    }

    let hit_clip = resolve_clip(
        parent_hit_clip,
        content_clip_to_bounds.then_some(transformed_rect),
    );

    // GraphicsLayer clipping clips content, but should not clip its own shadow.
    // Explicit clip-to-bounds modifiers still clip both.
    let shadow_clip = resolve_clip(
        parent_visual_clip,
        style.clip_to_bounds.then_some(transformed_rect),
    );
    push_layer_shadow(scene, &node_layer, rect, transformed_rect, shadow_clip);

    // Draw behind layer
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Behind,
        rect,
        size,
        &node_layer,
        visual_clip,
        scene,
    );

    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(
            resolved,
            layer_uniform_scale(&node_layer),
        ))
    });

    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), &node_layer);
        let local_rect = apply_layer_affine_to_rect(rect, rect, &node_layer);
        let quad = apply_layer_to_quad(rect, rect, &node_layer);
        scene.push_shape_with_geometry(
            quad_bounds(quad),
            local_rect,
            quad,
            brush,
            scaled_shape,
            visual_clip,
            BlendMode::SrcOver,
        );
    }

    // Render text content if present
    if let Some(value) = modifier_slices.text_content_rc() {
        let default_text_style = cranpose_ui::text::TextStyle::default();
        let text_style_ref = modifier_slices.text_style().unwrap_or(&default_text_style);

        let options = modifier_slices
            .text_layout_options()
            .unwrap_or_default()
            .normalized();
        let padding = style.padding;
        let max_width = (rect.width - padding.left - padding.right).max(0.0);
        let prepared = prepare_text_layout(
            value.as_ref(),
            text_style_ref,
            options,
            Some(max_width).filter(|w| w.is_finite() && *w > 0.0),
        );
        let draw_width = if options.overflow == TextOverflow::Visible {
            prepared.metrics.width
        } else {
            max_width
        };
        let text_rect = Rect {
            x: rect.x + padding.left,
            y: rect.y + padding.top,
            width: draw_width,
            height: prepared.metrics.height,
        };
        let transformed_text_rect = apply_layer_to_rect(text_rect, rect, &node_layer);
        let text_bounds_rect = Rect {
            x: rect.x + padding.left,
            y: rect.y + padding.top,
            width: max_width,
            height: (rect.height - padding.top - padding.bottom).max(0.0),
        };
        let transformed_text_bounds = apply_layer_to_rect(text_bounds_rect, rect, &node_layer);
        let text_clip = resolve_text_clip(options.overflow, visual_clip, transformed_text_bounds);

        // Extract color and font size
        let text_color = text_style_ref.color.unwrap_or(Color(1.0, 1.0, 1.0, 1.0));
        let font_size = match text_style_ref.font_size {
            cranpose_ui::text::TextUnit::Sp(v) => v,
            cranpose_ui::text::TextUnit::Em(v) => v * 14.0,
            cranpose_ui::text::TextUnit::Unspecified => 14.0,
        };

        if let Some(text_clip) = text_clip {
            scene.push_text(
                node_id,
                transformed_text_rect,
                Rc::from(prepared.text),
                apply_layer_to_color(text_color, &node_layer),
                text_style_ref.clone(),
                font_size,
                layer_uniform_scale(&node_layer),
                options,
                text_clip,
            );
        }
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
        size,
        &node_layer,
        visual_clip,
        scene,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_effect_support_matrix_is_explicit() {
        let blur = RenderEffect::blur(4.0);
        let offset = RenderEffect::offset(2.0, 3.0);
        let chain = blur.clone().then(offset.clone());

        assert!(!is_render_effect_supported(&blur));
        assert!(!is_render_effect_supported(&offset));
        assert!(!is_render_effect_supported(&chain));
    }

    #[test]
    fn fallback_detection_triggers_for_effects_and_offscreen() {
        let mut layer = GraphicsLayer::default();
        assert!(!layer_requires_effect_fallback(&layer));

        layer.render_effect = Some(RenderEffect::blur(4.0));
        assert!(layer_requires_effect_fallback(&layer));

        layer.render_effect = None;
        layer.backdrop_effect = Some(RenderEffect::offset(1.0, 2.0));
        assert!(layer_requires_effect_fallback(&layer));

        layer.backdrop_effect = None;
        layer.compositing_strategy = CompositingStrategy::Offscreen;
        assert!(layer_requires_effect_fallback(&layer));
    }

    #[test]
    fn shadow_geometry_has_visible_expansion_and_offsets() {
        let mut scene = Scene::new();
        let layer = GraphicsLayer {
            shadow_elevation: 10.0,
            ambient_shadow_color: Color(0.2, 0.3, 0.4, 0.8),
            spot_shadow_color: Color(0.7, 0.6, 0.5, 0.9),
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(8.0)),
            ..Default::default()
        };
        let bounds = Rect {
            x: 20.0,
            y: 30.0,
            width: 40.0,
            height: 24.0,
        };

        push_layer_shadow(&mut scene, &layer, bounds, bounds, None);

        assert!(
            scene.shapes.len() >= 12,
            "soft shadow should emit layered ambient + spot geometry"
        );

        let ambient = scene
            .shapes
            .iter()
            .min_by(|a, b| a.rect.x.partial_cmp(&b.rect.x).expect("finite x"))
            .expect("ambient shape expected");
        assert!(
            ambient.rect.x <= bounds.x - 6.0,
            "ambient shadow should clearly expand left"
        );
        assert!(
            ambient.rect.width >= bounds.width + 12.0,
            "ambient shadow should clearly expand width"
        );
        let ambient_peak_alpha = scene
            .shapes
            .iter()
            .filter_map(|shape| match &shape.brush {
                Brush::Solid(color) => Some(color.a()),
                _ => None,
            })
            .fold(0.0f32, f32::max);
        assert!(
            ambient_peak_alpha > 0.02,
            "ambient alpha should remain visible"
        );

        let spot = scene
            .shapes
            .iter()
            .max_by(|a, b| a.rect.y.partial_cmp(&b.rect.y).expect("finite y"))
            .expect("spot shape expected");
        assert!(
            spot.rect.y > bounds.y,
            "spot shadow should be offset downward from source bounds"
        );
        let Brush::Solid(spot_color) = &spot.brush else {
            panic!("spot shadow must use solid color");
        };
        assert!(spot_color.a() > 0.02, "spot alpha should remain visible");
    }

    #[test]
    fn graphics_layer_clip_is_not_reused_for_shadow_clip() {
        let bounds = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 18.0,
        };
        let content_clip = resolve_clip(None, Some(bounds));
        let shadow_clip = resolve_clip(None, None);
        assert_eq!(content_clip, Some(bounds));
        assert_eq!(
            shadow_clip, None,
            "graphics-layer clip should not clip layer shadow geometry"
        );
    }

    #[test]
    fn clip_to_bounds_clips_shadow_and_content() {
        let parent = Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 40.0,
        };
        let bounds = Rect {
            x: 20.0,
            y: 20.0,
            width: 30.0,
            height: 30.0,
        };
        let content_clip = resolve_clip(Some(parent), Some(bounds)).expect("content clip");
        let shadow_clip = resolve_clip(Some(parent), Some(bounds)).expect("shadow clip");
        assert_eq!(content_clip, shadow_clip);
        assert_eq!(
            content_clip,
            Rect {
                x: 20.0,
                y: 20.0,
                width: 20.0,
                height: 20.0,
            }
        );
    }

    #[test]
    fn resolve_text_clip_skips_when_intersection_is_empty() {
        let visual_clip = Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        });
        let text_bounds = Rect {
            x: 20.0,
            y: 20.0,
            width: 5.0,
            height: 5.0,
        };
        assert_eq!(
            resolve_text_clip(TextOverflow::Clip, visual_clip, text_bounds),
            None
        );
    }

    #[test]
    fn resolve_text_clip_visible_keeps_unbounded_draw() {
        let text_bounds = Rect {
            x: 20.0,
            y: 20.0,
            width: 5.0,
            height: 5.0,
        };
        assert_eq!(
            resolve_text_clip(TextOverflow::Visible, None, text_bounds),
            Some(None)
        );
    }
}
