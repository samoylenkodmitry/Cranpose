use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::Brush;
use cranpose_ui::text::{
    resolve_text_direction, ResolvedTextDirection, TextAlign, TextDecoration, TextStyle,
};
use cranpose_ui::{
    measure_text, prepare_text_layout, LayoutBox, LayoutNode, LayoutNodeKind, SubcomposeLayoutNode,
    TextLayoutOptions, TextOverflow,
};
use cranpose_ui_graphics::{
    BlendMode, Color, CompositingStrategy, EdgeInsets, GraphicsLayer, LayerShape, Point, Rect,
    RenderEffect, RoundedCornerShape, Size,
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
    if let Some(value) = layout.node_data.modifier_slices().annotated_string() {
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
        let content_width = (rect.width - padding.left - padding.right).max(0.0);
        let measure_width = resolve_text_measure_width(content_width, padding, None, options);
        let prepared = prepare_text_layout(
            &value,
            text_style_ref,
            options,
            Some(measure_width).filter(|w| w.is_finite() && *w > 0.0),
        );
        let draw_width = if options.overflow == TextOverflow::Visible {
            prepared.metrics.width
        } else {
            content_width
        };
        let alignment_offset = resolve_text_horizontal_offset(
            text_style_ref,
            value.text.as_str(),
            content_width,
            prepared.metrics.width,
        );
        let text_rect = Rect {
            x: rect.x + padding.left + alignment_offset,
            y: rect.y + padding.top,
            width: draw_width,
            height: prepared.metrics.height,
        };
        let text_bounds_rect = Rect {
            x: rect.x + padding.left,
            y: rect.y + padding.top,
            width: content_width,
            height: (rect.height - padding.top - padding.bottom).max(0.0),
        };
        // Extract font size from text style or default
        let font_size = text_style_ref.resolve_font_size(14.0);
        let expanded_text_bounds =
            expand_text_bounds_for_baseline_shift(text_bounds_rect, text_style_ref, font_size);
        let transformed_text_bounds = apply_layer_to_rect(expanded_text_bounds, rect, &node_layer);
        let text_clip = resolve_text_clip(options.overflow, visual_clip, transformed_text_bounds);

        if let Some(text_clip) = text_clip {
            push_text_style_draws(
                scene,
                layout.node_id,
                rect,
                text_rect,
                &node_layer,
                &prepared.text,
                text_style_ref,
                font_size,
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

fn resolve_text_color_without_gradient_fallback(text_style: &TextStyle, default: Color) -> Color {
    let mut color = text_style
        .span_style
        .color
        .or(match text_style.span_style.brush.as_ref() {
            Some(Brush::Solid(color)) => Some(*color),
            _ => None,
        })
        .unwrap_or(default);
    if let Some(alpha) = text_style.span_style.alpha {
        color.3 *= alpha.clamp(0.0, 1.0);
    }
    color
}

#[allow(clippy::too_many_arguments)]
fn push_text_style_draws(
    scene: &mut Scene,
    node_id: NodeId,
    rect: Rect,
    text_rect: Rect,
    node_layer: &GraphicsLayer,
    text: &cranpose_ui::text::AnnotatedString,
    text_style: &TextStyle,
    font_size: f32,
    options: TextLayoutOptions,
    text_clip: Option<Rect>,
) {
    let baseline_shift_px = text_style
        .span_style
        .baseline_shift
        .filter(|shift| shift.is_specified())
        .map(|shift| -(shift.0 * font_size))
        .unwrap_or(0.0);
    let shifted_text_rect = Rect {
        x: text_rect.x,
        y: text_rect.y + baseline_shift_px,
        width: text_rect.width,
        height: text_rect.height,
    };
    let transformed_shifted_text_rect = apply_layer_to_rect(shifted_text_rect, rect, node_layer);

    if let Some(background) = text_style.span_style.background {
        let brush = apply_layer_to_brush(Brush::solid(background), node_layer);
        scene.push_shape(
            transformed_shifted_text_rect,
            brush,
            None,
            text_clip,
            BlendMode::SrcOver,
        );
    }

    let text_color =
        resolve_text_color_without_gradient_fallback(text_style, Color(1.0, 1.0, 1.0, 1.0));
    let transformed_text_color = apply_layer_to_color(text_color, node_layer);
    let mut transformed_text_style = text_style.clone();
    transformed_text_style.span_style.shadow = None;
    transformed_text_style.span_style.brush = text_style
        .span_style
        .brush
        .clone()
        .map(|brush| apply_layer_to_brush(brush, node_layer));
    let text_brush = transformed_text_style
        .span_style
        .brush
        .clone()
        .unwrap_or_else(|| Brush::solid(transformed_text_color));

    if let Some(shadow) = text_style.span_style.shadow {
        let shadow_rect = Rect {
            x: shifted_text_rect.x + shadow.offset.x,
            y: shifted_text_rect.y + shadow.offset.y,
            width: shifted_text_rect.width,
            height: shifted_text_rect.height,
        };
        let transformed_shadow_rect = apply_layer_to_rect(shadow_rect, rect, node_layer);
        let transformed_shadow_color = apply_layer_to_color(shadow.color, node_layer);
        let mut shadow_text_style = transformed_text_style.clone();
        shadow_text_style.span_style.brush = None;
        shadow_text_style.span_style.shadow = Some(cranpose_ui::text::Shadow {
            color: transformed_shadow_color,
            offset: Point::new(0.0, 0.0),
            blur_radius: shadow.blur_radius,
        });
        scene.push_text(
            node_id,
            transformed_shadow_rect,
            Rc::new(text.clone()),
            Color::TRANSPARENT,
            shadow_text_style,
            font_size,
            layer_uniform_scale(node_layer),
            options,
            text_clip,
        );
    }

    push_text_decorations(
        scene,
        rect,
        shifted_text_rect,
        node_layer,
        text,
        text_style,
        &text_brush,
        text_clip,
    );

    scene.push_text(
        node_id,
        transformed_shifted_text_rect,
        Rc::new(text.clone()),
        transformed_text_color,
        transformed_text_style,
        font_size,
        layer_uniform_scale(node_layer),
        options,
        text_clip,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_text_decorations(
    scene: &mut Scene,
    rect: Rect,
    text_rect: Rect,
    content_layer: &GraphicsLayer,
    annotated_text: &cranpose_ui::text::AnnotatedString,
    global_style: &TextStyle,
    text_brush: &Brush,
    text_clip: Option<Rect>,
) {
    if annotated_text.is_empty() {
        return;
    }

    let boundaries = annotated_text.span_boundaries();
    let text_str = annotated_text.text.as_str();

    let mut current_offset: f32 = 0.0;

    for window in boundaries.windows(2) {
        let start = window[0];
        let end = window[1];
        if start == end {
            continue;
        }

        let slice = &text_str[start..end];
        let mut merged_style = global_style.span_style.clone();
        for span in &annotated_text.span_styles {
            if span.range.start <= start && span.range.end >= end {
                merged_style = merged_style.merge(&span.item);
            }
        }

        let mut span_text_style = global_style.clone();
        span_text_style.span_style = merged_style.clone();

        let span_width = measure_text(
            &cranpose_ui::text::AnnotatedString::from(slice),
            &span_text_style,
        )
        .width
        .max(0.0);

        let Some(decoration) = merged_style.text_decoration else {
            current_offset += span_width;
            continue;
        };

        if decoration == TextDecoration::NONE || span_width <= 0.0 {
            current_offset += span_width;
            continue;
        }

        let font_size = span_text_style.resolve_font_size(14.0);
        let line_height = span_text_style
            .resolve_line_height(14.0, font_size * 1.4)
            .max(1.0);
        let thickness = (font_size * 0.06).clamp(1.0, line_height * 0.25);
        let brush = merged_style.brush.clone().unwrap_or_else(|| {
            merged_style
                .color
                .map(Brush::solid)
                .unwrap_or_else(|| text_brush.clone())
        });

        // Using y for single line since we don't map wrapping correctly without layout runs yet
        let line_top = text_rect.y;

        if decoration.contains(TextDecoration::UNDERLINE) {
            let underline_rect = Rect {
                x: text_rect.x + current_offset,
                y: line_top + line_height - thickness * 1.35,
                width: span_width,
                height: thickness,
            };
            let transformed = apply_layer_to_rect(underline_rect, rect, content_layer);
            scene.push_shape(
                transformed,
                brush.clone(),
                None,
                text_clip,
                BlendMode::SrcOver,
            );
        }

        if decoration.contains(TextDecoration::LINE_THROUGH) {
            let strike_rect = Rect {
                x: text_rect.x + current_offset,
                y: line_top + line_height * 0.52 - thickness * 0.5,
                width: span_width,
                height: thickness,
            };
            let transformed = apply_layer_to_rect(strike_rect, rect, content_layer);
            scene.push_shape(transformed, brush, None, text_clip, BlendMode::SrcOver);
        }

        current_offset += span_width;
    }
}

fn resolve_text_measure_width(
    content_width: f32,
    padding: EdgeInsets,
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

fn resolve_text_horizontal_offset(
    style: &TextStyle,
    text: &str,
    content_width: f32,
    measured_width: f32,
) -> f32 {
    let available_width = content_width.max(0.0);
    let remaining = (available_width - measured_width.max(0.0)).max(0.0);
    let paragraph_style = &style.paragraph_style;
    let direction = resolve_text_direction(text, Some(paragraph_style.text_direction));
    match paragraph_style.text_align {
        TextAlign::Left => 0.0,
        TextAlign::Right => remaining,
        TextAlign::Center => remaining * 0.5,
        TextAlign::Justify => 0.0,
        TextAlign::Start => match direction {
            ResolvedTextDirection::Ltr => 0.0,
            ResolvedTextDirection::Rtl => remaining,
        },
        TextAlign::End => match direction {
            ResolvedTextDirection::Ltr => remaining,
            ResolvedTextDirection::Rtl => 0.0,
        },
        TextAlign::Unspecified => match direction {
            ResolvedTextDirection::Ltr => 0.0,
            ResolvedTextDirection::Rtl => remaining,
        },
    }
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
    if let Some(value) = modifier_slices.annotated_string() {
        let default_text_style = cranpose_ui::text::TextStyle::default();
        let text_style_ref = modifier_slices.text_style().unwrap_or(&default_text_style);

        let options = modifier_slices
            .text_layout_options()
            .unwrap_or_default()
            .normalized();
        let padding = style.padding;
        let content_width = (rect.width - padding.left - padding.right).max(0.0);
        let measured_max_width = layout_state
            .measurement_constraints
            .max_width
            .is_finite()
            .then_some(layout_state.measurement_constraints.max_width);
        let measure_width =
            resolve_text_measure_width(content_width, padding, measured_max_width, options);
        let prepared = prepare_text_layout(
            &value,
            text_style_ref,
            options,
            Some(measure_width).filter(|w| w.is_finite() && *w > 0.0),
        );
        let draw_width = if options.overflow == TextOverflow::Visible {
            prepared.metrics.width
        } else {
            content_width
        };
        let alignment_offset = resolve_text_horizontal_offset(
            text_style_ref,
            value.text.as_str(),
            content_width,
            prepared.metrics.width,
        );
        let text_rect = Rect {
            x: rect.x + padding.left + alignment_offset,
            y: rect.y + padding.top,
            width: draw_width,
            height: prepared.metrics.height,
        };
        let text_bounds_rect = Rect {
            x: rect.x + padding.left,
            y: rect.y + padding.top,
            width: content_width,
            height: (rect.height - padding.top - padding.bottom).max(0.0),
        };
        // Extract font size
        let font_size = text_style_ref.resolve_font_size(14.0);
        let expanded_text_bounds =
            expand_text_bounds_for_baseline_shift(text_bounds_rect, text_style_ref, font_size);
        let transformed_text_bounds = apply_layer_to_rect(expanded_text_bounds, rect, &node_layer);
        let text_clip = resolve_text_clip(options.overflow, visual_clip, transformed_text_bounds);

        if let Some(text_clip) = text_clip {
            push_text_style_draws(
                scene,
                node_id,
                rect,
                text_rect,
                &node_layer,
                &prepared.text,
                text_style_ref,
                font_size,
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

    #[test]
    fn expand_text_bounds_for_baseline_shift_superscript_extends_top() {
        let style = TextStyle {
            span_style: cranpose_ui::text::SpanStyle {
                baseline_shift: Some(cranpose_ui::text::BaselineShift::SUPERSCRIPT),
                ..Default::default()
            },
            ..Default::default()
        };
        let text_bounds = Rect {
            x: 20.0,
            y: 20.0,
            width: 50.0,
            height: 18.0,
        };
        let expanded = expand_text_bounds_for_baseline_shift(text_bounds, &style, 20.0);
        assert!(expanded.y < text_bounds.y);
        assert!(expanded.height > text_bounds.height);
        assert_eq!(
            expanded.y + expanded.height,
            text_bounds.y + text_bounds.height
        );
    }

    #[test]
    fn resolve_text_measure_width_expands_for_multiline_clip_text() {
        let padding = EdgeInsets {
            left: 4.0,
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
        };
        let width =
            resolve_text_measure_width(130.0, padding, Some(180.0), TextLayoutOptions::default());
        assert!((width - 172.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_text_measure_width_caps_single_line_measurements() {
        let padding = EdgeInsets {
            left: 4.0,
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
        };
        let width = resolve_text_measure_width(
            130.0,
            padding,
            Some(180.0),
            TextLayoutOptions {
                overflow: TextOverflow::Ellipsis,
                soft_wrap: false,
                max_lines: 1,
                min_lines: 1,
            },
        );
        assert!((width - 130.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_text_measure_width_respects_tighter_measurement_constraint() {
        let padding = EdgeInsets {
            left: 4.0,
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
        };
        let width =
            resolve_text_measure_width(130.0, padding, Some(100.0), TextLayoutOptions::default());
        assert!((width - 92.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_text_measure_width_falls_back_to_content_width_without_constraint() {
        let padding = EdgeInsets {
            left: 4.0,
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
        };
        let width = resolve_text_measure_width(130.0, padding, None, TextLayoutOptions::default());
        assert!((width - 130.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_text_measure_width_keeps_content_width_for_finite_max_lines() {
        let padding = EdgeInsets {
            left: 4.0,
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
        };
        let options = TextLayoutOptions {
            max_lines: 4,
            ..TextLayoutOptions::default()
        };
        let width = resolve_text_measure_width(130.0, padding, Some(180.0), options);
        assert!((width - 130.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_text_horizontal_offset_centers_text() {
        let style = cranpose_ui::TextStyle {
            paragraph_style: cranpose_ui::ParagraphStyle {
                text_align: cranpose_ui::text::TextAlign::Center,
                ..Default::default()
            },
            ..Default::default()
        };
        let offset = resolve_text_horizontal_offset(&style, "hello", 120.0, 80.0);
        assert!((offset - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_text_horizontal_offset_uses_rtl_start() {
        let style = cranpose_ui::TextStyle {
            paragraph_style: cranpose_ui::ParagraphStyle {
                text_align: cranpose_ui::text::TextAlign::Start,
                text_direction: cranpose_ui::text::TextDirection::Rtl,
                ..Default::default()
            },
            ..Default::default()
        };
        let offset = resolve_text_horizontal_offset(&style, "hello", 120.0, 80.0);
        assert!((offset - 40.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_text_horizontal_offset_uses_start_for_unspecified_align() {
        let style = cranpose_ui::TextStyle {
            paragraph_style: cranpose_ui::ParagraphStyle {
                text_align: cranpose_ui::text::TextAlign::Unspecified,
                text_direction: cranpose_ui::text::TextDirection::Rtl,
                ..Default::default()
            },
            ..Default::default()
        };
        let offset = resolve_text_horizontal_offset(&style, "hello", 120.0, 80.0);
        assert!((offset - 40.0).abs() < f32::EPSILON);
    }

    #[test]
    fn measurement_constraint_width_prevents_spurious_wrap() {
        let padding = EdgeInsets {
            left: 4.0,
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
        };
        let text = "Dynamic Modifiers";
        let style = cranpose_ui::TextStyle::default();
        let options = cranpose_ui::TextLayoutOptions::default();
        let content_width = 130.0;

        let wrapped_by_content = prepare_text_layout(
            &cranpose_ui::text::AnnotatedString::from(text),
            &style,
            options,
            Some(content_width),
        )
        .text;
        assert!(
            wrapped_by_content.text.contains('\n'),
            "control check expected wrapping at content width: {wrapped_by_content:?}"
        );

        let measure_width =
            resolve_text_measure_width(content_width, padding, Some(180.0), options);
        let prepared = prepare_text_layout(
            &cranpose_ui::text::AnnotatedString::from(text),
            &style,
            options,
            Some(measure_width),
        );
        assert!(
            !prepared.text.text.contains('\n'),
            "measurement width should prevent synthetic wrap: {:?}",
            prepared.text
        );
    }

    #[test]
    fn finite_max_lines_keeps_wrap_points_under_content_width() {
        let padding = EdgeInsets {
            left: 4.0,
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
        };
        let text = "This paragraph demonstrates textIndent lineHeight lineBreak";
        let style = cranpose_ui::TextStyle::default();
        let options = cranpose_ui::TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: 4,
            min_lines: 1,
        };
        let content_width = 130.0;
        let measure_width =
            resolve_text_measure_width(content_width, padding, Some(180.0), options);
        let prepared = prepare_text_layout(
            &cranpose_ui::text::AnnotatedString::from(text),
            &style,
            options,
            Some(measure_width),
        );
        assert!(
            prepared.text.text.contains('\n'),
            "finite max_lines should keep constrained wrapping: {:?}",
            prepared.text
        );
    }

    #[test]
    fn push_text_style_draws_emits_background_shadow_and_main_text() {
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                color: Some(Color(0.9, 0.95, 1.0, 1.0)),
                background: Some(Color(0.2, 0.3, 0.52, 0.55)),
                shadow: Some(cranpose_ui::text::Shadow {
                    color: Color(0.0, 0.0, 0.0, 0.95),
                    offset: Point::new(2.0, 2.0),
                    blur_radius: 3.0,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 8.0,
            y: 10.0,
            width: 180.0,
            height: 28.0,
        };
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 200.0,
        };

        push_text_style_draws(
            &mut scene,
            7 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &cranpose_ui::text::AnnotatedString::from("Decorated shadow text"),
            &style,
            14.0,
            TextLayoutOptions::default(),
            Some(clip),
        );

        assert_eq!(
            scene.shapes.len(),
            1,
            "span background should emit one shape"
        );
        let Brush::Solid(background) = &scene.shapes[0].brush else {
            panic!("background draw should use a solid brush");
        };
        assert_eq!(*background, Color(0.2, 0.3, 0.52, 0.55));

        assert_eq!(scene.texts.len(), 2, "shadow + content text expected");
        assert_eq!(scene.texts[0].color, Color::TRANSPARENT);
        let shadow_style = scene.texts[0]
            .text_style
            .span_style
            .shadow
            .expect("shadow draw should carry style shadow");
        assert_eq!(shadow_style.color, Color(0.0, 0.0, 0.0, 0.95));
        assert_eq!(shadow_style.offset, Point::new(0.0, 0.0));
        assert!((shadow_style.blur_radius - 3.0).abs() < f32::EPSILON);
        assert_eq!(scene.texts[1].color, Color(0.9, 0.95, 1.0, 1.0));
        assert!(scene.texts[0].rect.x > scene.texts[1].rect.x);
        assert!(scene.texts[0].rect.y > scene.texts[1].rect.y);
    }

    #[test]
    fn push_text_style_draws_emits_decoration_shapes() {
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                color: Some(Color(0.9, 0.95, 1.0, 1.0)),
                text_decoration: Some(
                    cranpose_ui::text::TextDecoration::UNDERLINE
                        .combine(cranpose_ui::text::TextDecoration::LINE_THROUGH),
                ),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 8.0,
            y: 10.0,
            width: 180.0,
            height: 28.0,
        };

        push_text_style_draws(
            &mut scene,
            7 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &cranpose_ui::text::AnnotatedString::from("Decorated"),
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(scene.shapes.len(), 2, "underline + line-through expected");
        assert_eq!(scene.texts.len(), 1, "main text expected");
    }

    #[test]
    fn push_text_style_draws_applies_baseline_shift() {
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                color: Some(Color(0.9, 0.95, 1.0, 1.0)),
                baseline_shift: Some(cranpose_ui::text::BaselineShift::SUPERSCRIPT),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 8.0,
            y: 20.0,
            width: 180.0,
            height: 28.0,
        };

        push_text_style_draws(
            &mut scene,
            7 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &cranpose_ui::text::AnnotatedString::from("Shifted"),
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(scene.texts.len(), 1);
        assert!(
            scene.texts[0].rect.y < rect.y,
            "superscript baseline shift should move text up"
        );
    }

    #[test]
    fn push_text_style_draws_non_solid_brush_contract_does_not_fallback_to_first_stop() {
        let mut scene = Scene::new();
        let first_stop = Color(1.0, 0.0, 0.0, 1.0);
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                brush: Some(Brush::linear_gradient_range(
                    vec![first_stop, Color(0.0, 0.0, 1.0, 1.0)],
                    Point::new(0.0, 0.0),
                    Point::new(180.0, 0.0),
                )),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 8.0,
            y: 20.0,
            width: 180.0,
            height: 28.0,
        };

        push_text_style_draws(
            &mut scene,
            7 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &cranpose_ui::text::AnnotatedString::from("Gradient text"),
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(scene.texts.len(), 1);
        assert_ne!(
            scene.texts[0].color, first_stop,
            "non-solid brush text should not degrade to first-stop fallback color"
        );
    }

    #[test]
    fn single_line_overflow_keeps_content_width_for_ellipsis() {
        let padding = EdgeInsets {
            left: 4.0,
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
        };
        let text = "Overflow sample: Supercalifragilisticexpialidocious";
        let style = cranpose_ui::TextStyle::default();
        let options = TextLayoutOptions {
            overflow: TextOverflow::Ellipsis,
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        };
        let content_width = 130.0;
        let measure_width =
            resolve_text_measure_width(content_width, padding, Some(180.0), options);
        let prepared = prepare_text_layout(
            &cranpose_ui::text::AnnotatedString::from(text),
            &style,
            options,
            Some(measure_width),
        );
        assert!(
            prepared.text.text.contains('\u{2026}'),
            "ellipsis should remain active: {:?}",
            prepared.text
        );
    }
}
