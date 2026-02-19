//! Scene building pipeline - copies layout tree to render scene.
//! This module is copied from the pixels renderer to maintain compatibility.

use std::rc::Rc;

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::Brush;
use cranpose_ui::text::{
    resolve_text_direction, ResolvedTextDirection, TextAlign, TextDecoration, TextMotion, TextStyle,
};
use cranpose_ui::{
    measure_text, prepare_text_layout, LayoutBox, LayoutNode, LayoutNodeKind, SubcomposeLayoutNode,
    TextLayoutOptions, TextOverflow,
};
use cranpose_ui_graphics::{
    BlendMode, Color, CompositingStrategy, EdgeInsets, GraphicsLayer, LayerShape, Point, Rect,
    RenderEffect, RoundedCornerShape, Size,
};

use crate::scene::{BackdropLayer, ClickAction, DrawShape, EffectLayer, Scene, ShadowDraw};
use crate::text_raster::{rasterize_text_to_image, requires_rasterized_glyph_path};

// Re-use style functions from a local copy
mod style;
use style::{
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

#[derive(Clone)]
struct LayerIsolation {
    effect: Option<RenderEffect>,
    blend_mode: BlendMode,
    composite_alpha: f32,
}

fn effective_layer_isolation(layer: &GraphicsLayer) -> Option<LayerIsolation> {
    let has_effect = layer.render_effect.is_some();
    let has_layer_blend = layer.blend_mode != BlendMode::SrcOver;
    let requires_isolation = match layer.compositing_strategy {
        CompositingStrategy::Offscreen => true,
        CompositingStrategy::Auto => has_effect || has_layer_blend || layer.alpha < 1.0,
        CompositingStrategy::ModulateAlpha => has_effect || has_layer_blend,
    };

    if !requires_isolation {
        return None;
    }

    let composite_alpha = if layer.compositing_strategy == CompositingStrategy::ModulateAlpha {
        1.0
    } else {
        layer.alpha.clamp(0.0, 1.0)
    };

    Some(LayerIsolation {
        effect: layer.render_effect.clone(),
        blend_mode: layer.blend_mode,
        composite_alpha,
    })
}

fn layer_for_content(layer: &GraphicsLayer, isolation: Option<&LayerIsolation>) -> GraphicsLayer {
    let mut content = layer.clone();
    if isolation.is_some() && layer.compositing_strategy != CompositingStrategy::ModulateAlpha {
        content.alpha = 1.0;
    }
    content
}

fn rect_to_quad(rect: Rect) -> [[f32; 2]; 4] {
    [
        [rect.x, rect.y],
        [rect.x + rect.width, rect.y],
        [rect.x, rect.y + rect.height],
        [rect.x + rect.width, rect.y + rect.height],
    ]
}

fn shadow_shape(
    rect: Rect,
    color: Color,
    shape: Option<RoundedCornerShape>,
) -> (DrawShape, BlendMode) {
    (
        DrawShape {
            rect,
            local_rect: rect,
            quad: rect_to_quad(rect),
            brush: Brush::solid(color),
            shape,
            z_index: 0, // populated by Scene::push_shadow_draw()
            clip: None,
            blend_mode: BlendMode::SrcOver,
        },
        BlendMode::SrcOver,
    )
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
    let spread = (elevation * 0.24).max(0.8);
    let spot_offset_x = elevation * 0.18;
    let spot_offset_y = elevation * 0.62;
    let ambient_blur_radius = (elevation * 0.95).max(0.5);
    let spot_blur_radius = (elevation * 0.72).max(0.5);

    let resolved_shape = match layer.shape {
        LayerShape::Rectangle => None,
        LayerShape::Rounded(shape) => {
            let resolved = shape.resolve(layer_bounds.width, layer_bounds.height);
            Some(RoundedCornerShape::with_radii(scale_corner_radii(
                resolved, scale,
            )))
        }
    };

    let ambient_alpha = (layer.ambient_shadow_color.a() * 0.44).clamp(0.0, 1.0);
    if ambient_alpha > f32::EPSILON {
        let ambient = Color(
            layer.ambient_shadow_color.r(),
            layer.ambient_shadow_color.g(),
            layer.ambient_shadow_color.b(),
            ambient_alpha,
        );
        let ambient_rect = Rect {
            x: transformed_bounds.x - spread,
            y: transformed_bounds.y - spread,
            width: transformed_bounds.width + spread * 2.0,
            height: transformed_bounds.height + spread * 2.0,
        };
        scene.push_shadow_draw(ShadowDraw {
            shapes: vec![shadow_shape(ambient_rect, ambient, resolved_shape)],
            blur_radius: ambient_blur_radius,
            clip,
            z_index: 0, // populated by Scene::push_shadow_draw()
        });
    }

    let spot_alpha = (layer.spot_shadow_color.a() * 0.62).clamp(0.0, 1.0);
    if spot_alpha > f32::EPSILON {
        let spot = Color(
            layer.spot_shadow_color.r(),
            layer.spot_shadow_color.g(),
            layer.spot_shadow_color.b(),
            spot_alpha,
        );
        let spot_spread = spread * 0.72;
        let spot_rect = Rect {
            x: transformed_bounds.x + spot_offset_x - spot_spread,
            y: transformed_bounds.y + spot_offset_y - spot_spread,
            width: transformed_bounds.width + spot_spread * 2.0,
            height: transformed_bounds.height + spot_spread * 2.0,
        };
        scene.push_shadow_draw(ShadowDraw {
            shapes: vec![shadow_shape(spot_rect, spot, resolved_shape)],
            blur_radius: spot_blur_radius,
            clip,
            z_index: 0, // populated by Scene::push_shadow_draw()
        });
    }
}

#[allow(dead_code)]
pub(crate) fn render_layout_tree(root: &LayoutBox, scene: &mut Scene) {
    render_layout_tree_with_scale(root, scene, 1.0);
}

pub(crate) fn render_layout_tree_with_scale(root: &LayoutBox, scene: &mut Scene, scale: f32) {
    let root_layer = GraphicsLayer {
        scale,
        ..Default::default()
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
    let layer_isolation = effective_layer_isolation(&node_layer);
    let content_layer = layer_for_content(&node_layer, layer_isolation.as_ref());
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

    // Track z_start for layer events emitted by this node.
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
        &content_layer,
        visual_clip,
        scene,
    );

    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(
            resolved,
            layer_uniform_scale(&content_layer),
        ))
    });

    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), &content_layer);
        let local_rect = apply_layer_affine_to_rect(rect, rect, &content_layer);
        let quad = apply_layer_to_quad(rect, rect, &content_layer);
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
        let content_width = (rect.width - padding.left - padding.right).max(0.0);
        let measure_width = resolve_text_measure_width(content_width, padding, None, options);
        let prepared = prepare_text_layout(
            value.as_ref(),
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
            value.as_ref(),
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
        let transformed_text_bounds = apply_layer_to_rect(text_bounds_rect, rect, &content_layer);
        let text_clip = resolve_text_clip(options.overflow, visual_clip, transformed_text_bounds);

        // Extract font size from text style or default
        let font_size = text_style_ref.resolve_font_size(14.0);

        if let Some(text_clip) = text_clip {
            push_text_style_draws(
                scene,
                layout.node_id,
                rect,
                text_rect,
                &content_layer,
                prepared.text.as_str(),
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
            content_layer.clone(),
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
        &content_layer,
        visual_clip,
        scene,
    );

    // Record isolation layer if this node requires offscreen composition.
    if let Some(isolation) = layer_isolation {
        scene.effect_layers.push(EffectLayer {
            rect: transformed_rect,
            clip: visual_clip,
            effect: isolation.effect,
            blend_mode: isolation.blend_mode,
            composite_alpha: isolation.composite_alpha,
            z_start,
            z_end: scene.next_z,
        });
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

fn resolve_clip(parent_clip: Option<Rect>, requested_clip: Option<Rect>) -> Option<Rect> {
    match (parent_clip, requested_clip) {
        (Some(parent), Some(current)) => intersect_rect(parent, current),
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
    // Non-visible overflow requires a concrete clip intersection.
    // If empty, skip drawing rather than treating `None` as unbounded clip.
    resolve_clip(visual_clip, Some(pad_clip_rect(transformed_text_bounds))).map(Some)
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

fn snap_text_rect_for_motion(rect: Rect, text_style: &TextStyle) -> Rect {
    if text_style
        .paragraph_style
        .text_motion
        .unwrap_or(TextMotion::Static)
        == TextMotion::Static
    {
        return Rect {
            x: rect.x.round(),
            y: rect.y.round(),
            width: rect.width,
            height: rect.height,
        };
    }

    rect
}

#[allow(clippy::too_many_arguments)]
fn push_text_style_draws(
    scene: &mut Scene,
    node_id: NodeId,
    rect: Rect,
    text_rect: Rect,
    content_layer: &GraphicsLayer,
    text: &str,
    text_style: &TextStyle,
    font_size: f32,
    options: TextLayoutOptions,
    text_clip: Option<Rect>,
) {
    let text_scale = layer_uniform_scale(content_layer);
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
    let transformed_shifted_text_rect = apply_layer_to_rect(shifted_text_rect, rect, content_layer);
    let snapped_shifted_text_rect =
        snap_text_rect_for_motion(transformed_shifted_text_rect, text_style);

    if let Some(background) = text_style.span_style.background {
        let brush = apply_layer_to_brush(Brush::solid(background), content_layer);
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
    let transformed_text_color = apply_layer_to_color(text_color, content_layer);
    let mut transformed_text_style = text_style.clone();
    transformed_text_style.span_style.shadow = None;
    transformed_text_style.span_style.brush = text_style
        .span_style
        .brush
        .clone()
        .map(|brush| apply_layer_to_brush(brush, content_layer));
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
        let transformed_shadow_rect = snap_text_rect_for_motion(
            apply_layer_to_rect(shadow_rect, rect, content_layer),
            text_style,
        );
        let mut shadow_text_style = transformed_text_style.clone();
        shadow_text_style.span_style.brush = None;
        let shadow_z_start = scene.next_z;
        scene.push_text(
            node_id,
            transformed_shadow_rect,
            Rc::from(text),
            apply_layer_to_color(shadow.color, content_layer),
            shadow_text_style,
            font_size,
            text_scale,
            options,
            text_clip,
        );
        let blur_radius = shadow.blur_radius.max(0.0) * text_scale;
        if blur_radius > f32::EPSILON {
            let blur_margin = (blur_radius * 3.0).max(1.0);
            scene.effect_layers.push(EffectLayer {
                rect: Rect {
                    x: transformed_shadow_rect.x - blur_margin,
                    y: transformed_shadow_rect.y - blur_margin,
                    width: transformed_shadow_rect.width + blur_margin * 2.0,
                    height: transformed_shadow_rect.height + blur_margin * 2.0,
                },
                clip: text_clip,
                effect: Some(RenderEffect::blur(blur_radius)),
                blend_mode: BlendMode::SrcOver,
                composite_alpha: 1.0,
                z_start: shadow_z_start,
                z_end: scene.next_z,
            });
        }
    }

    push_text_decorations(
        scene,
        rect,
        shifted_text_rect,
        content_layer,
        text,
        text_style,
        &text_brush,
        text_clip,
    );

    if requires_rasterized_glyph_path(&transformed_text_style) {
        let image = rasterize_text_to_image(
            text,
            snapped_shifted_text_rect,
            &transformed_text_style,
            transformed_text_color,
            font_size,
            text_scale,
        )
        .expect("styled text raster path requires configured raster fonts");
        scene.push_image(
            snapped_shifted_text_rect,
            image,
            1.0,
            None,
            text_clip,
            None,
            BlendMode::SrcOver,
        );
        return;
    }

    scene.push_text(
        node_id,
        snapped_shifted_text_rect,
        Rc::from(text),
        transformed_text_color,
        transformed_text_style,
        font_size,
        text_scale,
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
    text: &str,
    text_style: &TextStyle,
    text_brush: &Brush,
    text_clip: Option<Rect>,
) {
    let Some(decoration) = text_style.span_style.text_decoration else {
        return;
    };
    if decoration == TextDecoration::NONE {
        return;
    }

    let font_size = text_style.resolve_font_size(14.0);
    let line_height = text_style.resolve_line_height(14.0, font_size).max(1.0);
    let thickness = (font_size * 0.06).clamp(1.0, line_height * 0.25);
    let brush = text_brush.clone();

    for (line_index, line) in text.split('\n').enumerate() {
        let line_width = measure_text(line, text_style)
            .width
            .min(text_rect.width)
            .max(0.0);
        if line_width <= 0.0 {
            continue;
        }

        let line_top = text_rect.y + line_index as f32 * line_height;

        if decoration.contains(TextDecoration::UNDERLINE) {
            let underline_rect = Rect {
                x: text_rect.x,
                y: line_top + line_height - thickness * 1.35,
                width: line_width,
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
                x: text_rect.x,
                y: line_top + line_height * 0.52 - thickness * 0.5,
                width: line_width,
                height: thickness,
            };
            let transformed = apply_layer_to_rect(strike_rect, rect, content_layer);
            scene.push_shape(
                transformed,
                brush.clone(),
                None,
                text_clip,
                BlendMode::SrcOver,
            );
        }
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
        scale,
        ..Default::default()
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
    let layer_isolation = effective_layer_isolation(&node_layer);
    let content_layer = layer_for_content(&node_layer, layer_isolation.as_ref());
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

    // Track z_start for layer events emitted by this node.
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
        &content_layer,
        visual_clip,
        scene,
    );

    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(
            resolved,
            layer_uniform_scale(&content_layer),
        ))
    });

    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), &content_layer);
        let local_rect = apply_layer_affine_to_rect(rect, rect, &content_layer);
        let quad = apply_layer_to_quad(rect, rect, &content_layer);
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
        let content_width = (rect.width - padding.left - padding.right).max(0.0);
        let measured_max_width = layout_state
            .measurement_constraints
            .max_width
            .is_finite()
            .then_some(layout_state.measurement_constraints.max_width);
        let measure_width =
            resolve_text_measure_width(content_width, padding, measured_max_width, options);
        let prepared = prepare_text_layout(
            value.as_ref(),
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
            value.as_ref(),
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
        let transformed_text_bounds = apply_layer_to_rect(text_bounds_rect, rect, &content_layer);
        let text_clip = resolve_text_clip(options.overflow, visual_clip, transformed_text_bounds);

        // Extract font size
        let font_size = text_style_ref.resolve_font_size(14.0);

        if let Some(text_clip) = text_clip {
            push_text_style_draws(
                scene,
                node_id,
                rect,
                text_rect,
                &content_layer,
                prepared.text.as_str(),
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
            content_layer.clone(),
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
        &content_layer,
        visual_clip,
        scene,
    );

    // Record isolation layer if this node requires offscreen composition.
    if let Some(isolation) = layer_isolation {
        scene.effect_layers.push(EffectLayer {
            rect: transformed_rect,
            clip: visual_clip,
            effect: isolation.effect,
            blend_mode: isolation.blend_mode,
            composite_alpha: isolation.composite_alpha,
            z_start,
            z_end: scene.next_z,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_test_fonts() {
        crate::text_raster::configure_raster_fonts(&[include_bytes!(
            "../../../../apps/desktop-demo/assets/Roboto-Light.ttf"
        ) as &[u8]]);
    }

    #[test]
    fn auto_alpha_triggers_isolation_with_composite_alpha() {
        let layer = GraphicsLayer {
            alpha: 0.5,
            compositing_strategy: CompositingStrategy::Auto,
            ..Default::default()
        };
        let isolation = effective_layer_isolation(&layer).expect("expected isolation");
        assert!(isolation.effect.is_none());
        assert!((isolation.composite_alpha - 0.5).abs() < 1e-6);

        let content = layer_for_content(&layer, Some(&isolation));
        assert!((content.alpha - 1.0).abs() < 1e-6);
    }

    #[test]
    fn modulate_alpha_keeps_in_place_alpha_without_offscreen() {
        let layer = GraphicsLayer {
            alpha: 0.5,
            compositing_strategy: CompositingStrategy::ModulateAlpha,
            ..Default::default()
        };
        assert!(effective_layer_isolation(&layer).is_none());
    }

    #[test]
    fn non_src_over_layer_blend_triggers_isolation() {
        let layer = GraphicsLayer {
            blend_mode: BlendMode::DstOut,
            compositing_strategy: CompositingStrategy::Auto,
            ..Default::default()
        };
        let isolation = effective_layer_isolation(&layer).expect("expected blend isolation");
        assert_eq!(isolation.blend_mode, BlendMode::DstOut);
        assert!((isolation.composite_alpha - 1.0).abs() < 1e-6);
    }

    #[test]
    fn offscreen_isolation_has_no_effect_payload() {
        let layer = GraphicsLayer {
            alpha: 1.0,
            compositing_strategy: CompositingStrategy::Offscreen,
            ..Default::default()
        };
        let isolation = effective_layer_isolation(&layer).expect("expected isolation");
        assert!(isolation.effect.is_none());
        assert!((isolation.composite_alpha - 1.0).abs() < 1e-6);
    }

    #[test]
    fn render_effect_forces_isolation_even_with_modulate_alpha() {
        let layer = GraphicsLayer {
            alpha: 0.4,
            compositing_strategy: CompositingStrategy::ModulateAlpha,
            render_effect: Some(RenderEffect::blur(4.0)),
            ..Default::default()
        };
        let isolation = effective_layer_isolation(&layer).expect("expected effect isolation");
        assert!(isolation.effect.is_some());
        // ModulateAlpha keeps alpha modulation in-content.
        assert!((isolation.composite_alpha - 1.0).abs() < 1e-6);
        let content = layer_for_content(&layer, Some(&isolation));
        assert!((content.alpha - layer.alpha).abs() < 1e-6);
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
            scene.shadow_draws.len() >= 2,
            "elevation shadow should emit ambient + spot blur draws"
        );

        let ambient = &scene.shadow_draws[0];
        assert!(
            ambient.blur_radius > 0.0,
            "ambient shadow should have a blur radius"
        );
        let ambient_shape = &ambient.shapes[0].0;
        assert!(
            ambient_shape.rect.x <= bounds.x - 2.0,
            "ambient shadow should clearly expand left"
        );
        assert!(
            ambient_shape.rect.width > bounds.width,
            "ambient shadow should clearly expand width"
        );
        let ambient_peak_alpha = match &ambient_shape.brush {
            Brush::Solid(color) => color.a(),
            _ => 0.0,
        };
        assert!(
            ambient_peak_alpha > 0.02,
            "ambient alpha should remain visible"
        );

        let spot = &scene.shadow_draws[1];
        assert!(spot.blur_radius > 0.0, "spot shadow should have blur");
        let spot_shape = &spot.shapes[0].0;
        assert!(
            spot_shape.rect.y > bounds.y,
            "spot shadow should be offset downward from source bounds"
        );
        let Brush::Solid(spot_color) = &spot_shape.brush else {
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

        let wrapped_by_content =
            prepare_text_layout(text, &style, options, Some(content_width)).text;
        assert!(
            wrapped_by_content.contains('\n'),
            "control check expected wrapping at content width: {wrapped_by_content:?}"
        );

        let measure_width =
            resolve_text_measure_width(content_width, padding, Some(180.0), options);
        let prepared = prepare_text_layout(text, &style, options, Some(measure_width));
        assert!(
            !prepared.text.contains('\n'),
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
        let prepared = prepare_text_layout(text, &style, options, Some(measure_width));
        assert!(
            prepared.text.contains('\n'),
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
            "Decorated shadow text",
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
        assert_eq!(scene.texts[0].color, Color(0.0, 0.0, 0.0, 0.95));
        assert_eq!(scene.texts[1].color, Color(0.9, 0.95, 1.0, 1.0));
        assert!(scene.texts[0].rect.x > scene.texts[1].rect.x);
        assert!(scene.texts[0].rect.y > scene.texts[1].rect.y);
        assert_eq!(
            scene.effect_layers.len(),
            1,
            "blurred text shadow uses effect layer"
        );
        let effect = scene.effect_layers[0]
            .effect
            .as_ref()
            .expect("shadow effect should be present");
        assert!(
            matches!(
                effect,
                RenderEffect::Blur {
                    radius_x,
                    radius_y,
                    ..
                } if (*radius_x - 3.0).abs() < f32::EPSILON && (*radius_y - 3.0).abs() < f32::EPSILON
            ),
            "shadow blur radius should map to blur effect: {effect:?}"
        );
    }

    #[test]
    fn push_text_style_draws_hard_shadow_does_not_emit_effect_layer() {
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                color: Some(Color(0.9, 0.95, 1.0, 1.0)),
                shadow: Some(cranpose_ui::text::Shadow {
                    color: Color(0.0, 0.0, 0.0, 0.95),
                    offset: Point::new(2.0, 2.0),
                    blur_radius: 0.0,
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

        push_text_style_draws(
            &mut scene,
            7 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            "Hard shadow",
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(scene.texts.len(), 2, "shadow + content text expected");
        assert!(
            scene.effect_layers.is_empty(),
            "hard shadow should not allocate blur effect layer"
        );
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
            "Decorated",
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
            "Shifted",
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
        init_test_fonts();
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
            "Gradient text",
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert!(
            scene.texts.is_empty(),
            "non-solid brush text should bypass glyphon color-only path"
        );
        assert_eq!(
            scene.images.len(),
            1,
            "non-solid brush text should rasterize to an image draw"
        );

        let image = &scene.images[0].image;
        let pixels = image.pixels();
        let mut max_red = 0.0f32;
        let mut min_red = 1.0f32;
        let mut max_blue = 0.0f32;
        let mut min_blue = 1.0f32;
        let mut ink_count = 0usize;
        for y in 0..image.height() {
            for x in 0..image.width() {
                let idx = ((y * image.width() + x) * 4) as usize;
                if pixels[idx + 3] == 0 {
                    continue;
                }
                let red = pixels[idx] as f32 / 255.0;
                let blue = pixels[idx + 2] as f32 / 255.0;
                max_red = max_red.max(red);
                min_red = min_red.min(red);
                max_blue = max_blue.max(blue);
                min_blue = min_blue.min(blue);
                ink_count += 1;
            }
        }
        assert!(
            ink_count > 0,
            "expected visible glyph pixels in rasterized image"
        );
        let red_span = max_red - min_red;
        let blue_span = max_blue - min_blue;
        let max_channel_delta = red_span.max(blue_span);
        assert!(
            max_channel_delta > 0.12,
            "gradient raster should vary visible color channels, red_span={red_span:.3}, blue_span={blue_span:.3}"
        );
    }

    #[test]
    fn push_text_style_draws_stroke_contract_rasterizes_to_image() {
        init_test_fonts();
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                draw_style: Some(cranpose_ui::text::TextDrawStyle::Stroke { width: 5.0 }),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 8.0,
            y: 20.0,
            width: 180.0,
            height: 32.0,
        };

        push_text_style_draws(
            &mut scene,
            8 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            "Stroke text",
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert!(
            scene.texts.is_empty(),
            "stroke text should bypass glyphon fill-only path"
        );
        assert_eq!(
            scene.images.len(),
            1,
            "stroke text should rasterize to an image draw"
        );
    }

    #[test]
    fn push_text_style_draws_text_motion_static_snaps_position() {
        let base_rect = Rect {
            x: 8.25,
            y: 10.75,
            width: 180.0,
            height: 28.0,
        };
        let static_style = cranpose_ui::TextStyle {
            paragraph_style: cranpose_ui::ParagraphStyle {
                text_motion: Some(cranpose_ui::text::TextMotion::Static),
                ..Default::default()
            },
            ..Default::default()
        };
        let animated_style = cranpose_ui::TextStyle {
            paragraph_style: cranpose_ui::ParagraphStyle {
                text_motion: Some(cranpose_ui::text::TextMotion::Animated),
                ..Default::default()
            },
            ..Default::default()
        };

        let mut static_scene = Scene::new();
        push_text_style_draws(
            &mut static_scene,
            9 as NodeId,
            base_rect,
            base_rect,
            &GraphicsLayer::default(),
            "Motion",
            &static_style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        let mut animated_scene = Scene::new();
        push_text_style_draws(
            &mut animated_scene,
            10 as NodeId,
            base_rect,
            base_rect,
            &GraphicsLayer::default(),
            "Motion",
            &animated_style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(static_scene.texts.len(), 1);
        assert_eq!(animated_scene.texts.len(), 1);
        assert_eq!(static_scene.texts[0].rect.x, base_rect.x.round());
        assert_eq!(static_scene.texts[0].rect.y, base_rect.y.round());
        assert!((animated_scene.texts[0].rect.x - base_rect.x).abs() < f32::EPSILON);
        assert!((animated_scene.texts[0].rect.y - base_rect.y).abs() < f32::EPSILON);
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
        let prepared = prepare_text_layout(text, &style, options, Some(measure_width));
        assert!(
            prepared.text.contains('\u{2026}'),
            "ellipsis should remain active: {:?}",
            prepared.text
        );
    }
}
