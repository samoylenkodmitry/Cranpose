//! Scene building pipeline - copies layout tree to render scene.
//! This module is copied from the pixels renderer to maintain compatibility.

use std::{ops::Range, rc::Rc};

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::Brush;
use cranpose_ui::text::{
    resolve_text_direction, ResolvedTextDirection, TextAlign, TextDecoration, TextDrawStyle,
    TextMotion, TextStyle,
};
use cranpose_ui::{
    layout_text, measure_text, prepare_text_layout, LayoutBox, LayoutNode, LayoutNodeKind,
    SubcomposeLayoutNode, TextLayoutOptions, TextOverflow,
};
use cranpose_ui_graphics::{
    BlendMode, Color, CompositingStrategy, EdgeInsets, GraphicsLayer, LayerShape, Point, Rect,
    RenderEffect, RoundedCornerShape, RuntimeShader, Size, TileMode,
};

use crate::scene::{
    BackdropLayer, ClickAction, DrawShape, EffectLayer, Scene, ShadowDraw, TextDraw,
};

// Re-use style functions from a local copy
mod style;
use style::{
    apply_draw_commands, apply_layer_affine_to_rect, apply_layer_to_brush, apply_layer_to_color,
    apply_layer_to_quad, apply_layer_to_rect, combine_layers, layer_uniform_scale, quad_bounds,
    scale_corner_radii, DrawPlacement, NodeStyle,
};

const TEXT_CLIP_PAD: f32 = 1.0;
const GPU_TEXT_BRUSH_EFFECT_MAX_STOPS: usize = 16;
const GPU_TEXT_BRUSH_EFFECT_FIRST_STOP_SLOT: usize = 8;
const DECORATION_SEGMENT_MERGE_EPSILON: f32 = 0.75;
const GPU_TEXT_BRUSH_EFFECT_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn fullscreen_vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;
    let x = f32(i32(vertex_index & 1u) * 2 - 1);
    let y = f32(i32(vertex_index >> 1u) * 2 - 1);
    output.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    output.position = vec4<f32>(x, y, 0.0, 1.0);
    return output;
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;

const BRUSH_LINEAR: u32 = 0u;
const BRUSH_RADIAL: u32 = 1u;
const BRUSH_SWEEP: u32 = 2u;
const TILE_CLAMP: u32 = 0u;
const TILE_REPEAT: u32 = 1u;
const TILE_MIRROR: u32 = 2u;
const TILE_DECAL: u32 = 3u;
const MAX_STOPS: u32 = 16u;
const DRAW_FILL: u32 = 0u;
const DRAW_STROKE: u32 = 1u;
const OUTLINE_SAMPLE_COUNT: u32 = 8u;

struct GradientSample {
    t: f32,
    valid: bool,
}

fn uniform_u32(value: f32) -> u32 {
    return u32(max(round(value), 0.0));
}

fn stop_color(index: u32) -> vec4<f32> {
    return u[8u + index * 2u];
}

fn stop_position(index: u32) -> f32 {
    return u[8u + index * 2u + 1u].x;
}

fn remap_gradient_t(raw_t: f32, tile_mode: u32) -> GradientSample {
    if (tile_mode == TILE_REPEAT) {
        let repeated = raw_t - floor(raw_t);
        return GradientSample(repeated, true);
    }

    if (tile_mode == TILE_MIRROR) {
        let wrapped = raw_t - floor(raw_t * 0.5) * 2.0;
        let mirrored = select(wrapped, 2.0 - wrapped, wrapped > 1.0);
        return GradientSample(clamp(mirrored, 0.0, 1.0), true);
    }

    if (tile_mode == TILE_DECAL) {
        let valid = raw_t >= 0.0 && raw_t <= 1.0;
        return GradientSample(clamp(raw_t, 0.0, 1.0), valid);
    }

    return GradientSample(clamp(raw_t, 0.0, 1.0), true);
}

fn sample_gradient(t: f32, stop_count: u32) -> vec4<f32> {
    if (stop_count == 0u) {
        return vec4<f32>(0.0);
    }
    if (stop_count == 1u) {
        return stop_color(0u);
    }

    let first = stop_color(0u);
    let first_t = stop_position(0u);
    if (t <= first_t) {
        return first;
    }

    for (var i: u32 = 0u; i + 1u < MAX_STOPS; i = i + 1u) {
        if (i + 1u >= stop_count) {
            break;
        }
        let current = stop_color(i);
        let next = stop_color(i + 1u);
        let current_t = stop_position(i);
        let next_t = stop_position(i + 1u);

        if (t <= next_t) {
            let span = max(next_t - current_t, 0.00001);
            let frac = clamp((t - current_t) / span, 0.0, 1.0);
            return mix(current, next, frac);
        }
    }

    return stop_color(stop_count - 1u);
}

fn evaluate_brush(local: vec2<f32>) -> vec4<f32> {
    let brush_type = uniform_u32(u[0].x);
    let stop_count = min(uniform_u32(u[0].y), MAX_STOPS);
    let tile_mode = uniform_u32(u[0].z);
    if (stop_count == 0u) {
        return vec4<f32>(0.0);
    }

    if (brush_type == BRUSH_LINEAR) {
        let start = u[2].xy;
        let end = u[2].zw;
        let delta = end - start;
        let denom = max(dot(delta, delta), 0.00001);
        let raw_t = dot(local - start, delta) / denom;
        let sample = remap_gradient_t(raw_t, tile_mode);
        if (!sample.valid) {
            return vec4<f32>(0.0);
        }
        return sample_gradient(sample.t, stop_count);
    }

    if (brush_type == BRUSH_RADIAL) {
        let center = u[3].xy;
        let radius = max(u[3].z, 0.00001);
        let raw_t = distance(local, center) / radius;
        let sample = remap_gradient_t(raw_t, tile_mode);
        if (!sample.valid) {
            return vec4<f32>(0.0);
        }
        return sample_gradient(sample.t, stop_count);
    }

    if (brush_type == BRUSH_SWEEP) {
        let center = u[4].xy;
        let delta = local - center;
        let angle = atan2(delta.y, delta.x);
        let raw_t = angle / (2.0 * 3.14159265358979) + 0.5;
        let sample = remap_gradient_t(raw_t, TILE_CLAMP);
        if (!sample.valid) {
            return vec4<f32>(0.0);
        }
        return sample_gradient(sample.t, stop_count);
    }

    return sample_gradient(0.0, stop_count);
}

fn sample_mask_alpha(uv: vec2<f32>) -> f32 {
    return textureSample(input_texture, input_sampler, uv).a;
}

fn max_dilated_alpha(uv: vec2<f32>, texel: vec2<f32>, radius_px: f32, center_alpha: f32) -> f32 {
    let radius = max(radius_px, 0.0);
    if (radius <= 0.0) {
        return center_alpha;
    }

    let half_radius = radius * 0.5;
    var max_alpha = center_alpha;
    let directions = array<vec2<f32>, 8>(
        vec2<f32>(1.0, 0.0),
        vec2<f32>(-1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, -1.0),
        vec2<f32>(0.70710677, 0.70710677),
        vec2<f32>(-0.70710677, 0.70710677),
        vec2<f32>(0.70710677, -0.70710677),
        vec2<f32>(-0.70710677, -0.70710677),
    );

    for (var i: u32 = 0u; i < OUTLINE_SAMPLE_COUNT; i = i + 1u) {
        let dir = directions[i];
        max_alpha = max(max_alpha, sample_mask_alpha(uv + dir * texel * radius));
        max_alpha = max(max_alpha, sample_mask_alpha(uv + dir * texel * half_radius));
    }

    return max_alpha;
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(input_texture, input_sampler, input.uv);
    let draw_mode = uniform_u32(u[5].x);
    let fill_alpha = sampled.a;
    if (draw_mode == DRAW_FILL && fill_alpha <= 0.0) {
        return vec4<f32>(0.0);
    }

    // Renderer-reserved slot 62 stores layer pixel rect: x, y, width, height.
    let layer_rect = u[62];
    let layer_size = max(layer_rect.zw, vec2<f32>(0.00001));
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let pixel = input.uv * tex_size;
    let local_uv = (pixel - layer_rect.xy) / layer_size;
    let logical_size = max(u[1].xy, vec2<f32>(0.00001));
    let local = local_uv * logical_size;
    let local_to_px = layer_size / logical_size;
    let local_to_px_avg = max((local_to_px.x + local_to_px.y) * 0.5, 0.00001);
    let stroke_width_local = max(u[5].y, 0.0);
    let stroke_radius_px = stroke_width_local * local_to_px_avg * 0.5;
    let texel = vec2<f32>(1.0) / max(tex_size, vec2<f32>(1.0));
    let outline_alpha = max_dilated_alpha(input.uv, texel, stroke_radius_px, fill_alpha);
    let stroke_alpha = max(outline_alpha - fill_alpha, 0.0);
    let material_alpha = select(fill_alpha, stroke_alpha, draw_mode == DRAW_STROKE);
    if (material_alpha <= 0.0) {
        return vec4<f32>(0.0);
    }

    let brush = evaluate_brush(local);
    let alpha_multiplier = clamp(u[0].w, 0.0, 1.0);
    let out_alpha = material_alpha * brush.a * alpha_multiplier;
    return vec4<f32>(brush.rgb * out_alpha, out_alpha);
}
"#;

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
            texts: vec![],
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
            texts: vec![],
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
        let transformed_text_bounds =
            apply_layer_to_rect(expanded_text_bounds, rect, &content_layer);
        let text_clip = resolve_text_clip(options.overflow, visual_clip, transformed_text_bounds);

        if let Some(text_clip) = text_clip {
            push_text_style_draws(
                scene,
                layout.node_id,
                rect,
                text_rect,
                &content_layer,
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

fn tile_mode_to_shader_uniform(tile_mode: TileMode) -> f32 {
    match tile_mode {
        TileMode::Clamp => 0.0,
        TileMode::Repeated => 1.0,
        TileMode::Mirror => 2.0,
        TileMode::Decal => 3.0,
    }
}

fn normalized_gradient_stops(color_count: usize, stops: Option<&[f32]>) -> Vec<f32> {
    if let Some(explicit) = stops.filter(|values| values.len() == color_count) {
        return explicit.to_vec();
    }
    if color_count <= 1 {
        return vec![0.0; color_count];
    }
    (0..color_count)
        .map(|index| index as f32 / (color_count - 1) as f32)
        .collect()
}

fn set_shader_vec4(shader: &mut RuntimeShader, slot: usize, values: [f32; 4]) {
    shader.set_float4(slot * 4, values[0], values[1], values[2], values[3]);
}

fn resolve_gradient_component(extent: f32, value: f32) -> f32 {
    if value.is_finite() {
        value
    } else if value.is_sign_positive() {
        extent.max(0.0)
    } else {
        0.0
    }
}

const GPU_TEXT_BRUSH_KIND_LINEAR: f32 = 0.0;
const GPU_TEXT_BRUSH_KIND_RADIAL: f32 = 1.0;
const GPU_TEXT_BRUSH_KIND_SWEEP: f32 = 2.0;
const GPU_TEXT_BRUSH_KIND_SOLID: f32 = 3.0;
const GPU_TEXT_BRUSH_EFFECT_MATERIAL_SLOT: usize = 5;
const GPU_TEXT_DRAW_MODE_FILL: f32 = 0.0;
const GPU_TEXT_DRAW_MODE_STROKE: f32 = 1.0;

#[derive(Clone, PartialEq)]
enum GpuTextDrawMode {
    Fill,
    Stroke { width: f32 },
}

#[derive(Clone, PartialEq)]
struct GpuTextMaterial {
    brush: Brush,
    alpha_multiplier: f32,
    draw_mode: GpuTextDrawMode,
}

#[derive(Clone)]
struct GpuTextMaterialBatch {
    material: GpuTextMaterial,
    visible_ranges: Vec<Range<usize>>,
}

fn gpu_text_material_for_style(
    text_style: &TextStyle,
    fallback_color: Color,
    text_scale: f32,
) -> GpuTextMaterial {
    let scale = if text_scale.is_finite() && text_scale > 0.0 {
        text_scale
    } else {
        1.0
    };
    let draw_mode = match text_style.span_style.draw_style {
        Some(TextDrawStyle::Stroke { width }) if width.is_finite() && width > 0.0 => {
            GpuTextDrawMode::Stroke {
                width: width * scale,
            }
        }
        _ => GpuTextDrawMode::Fill,
    };

    let brush = text_style
        .span_style
        .brush
        .clone()
        .unwrap_or_else(|| Brush::solid(fallback_color));
    let alpha_multiplier = if text_style.span_style.brush.is_some() {
        text_style.span_style.alpha.unwrap_or(1.0)
    } else {
        1.0
    };

    GpuTextMaterial {
        brush,
        alpha_multiplier,
        draw_mode,
    }
}

fn build_gpu_text_effect(material: &GpuTextMaterial, text_rect: Rect) -> Option<RenderEffect> {
    if !text_rect.width.is_finite()
        || !text_rect.height.is_finite()
        || text_rect.width <= 0.0
        || text_rect.height <= 0.0
    {
        return None;
    }

    let mut shader = RuntimeShader::new(GPU_TEXT_BRUSH_EFFECT_SHADER);
    let logical_width = text_rect.width.max(f32::EPSILON);
    let logical_height = text_rect.height.max(f32::EPSILON);
    set_shader_vec4(&mut shader, 1, [logical_width, logical_height, 0.0, 0.0]);

    let (draw_mode, stroke_width) = match material.draw_mode {
        GpuTextDrawMode::Fill => (GPU_TEXT_DRAW_MODE_FILL, 0.0),
        GpuTextDrawMode::Stroke { width } => (GPU_TEXT_DRAW_MODE_STROKE, width.max(0.0)),
    };
    set_shader_vec4(
        &mut shader,
        GPU_TEXT_BRUSH_EFFECT_MATERIAL_SLOT,
        [draw_mode, stroke_width, 0.0, 0.0],
    );

    let alpha = if material.alpha_multiplier.is_finite() {
        material.alpha_multiplier.clamp(0.0, 1.0)
    } else {
        1.0
    };

    let (brush_type, colors, stops, tile_mode) = match &material.brush {
        Brush::LinearGradient {
            colors,
            stops,
            start,
            end,
            tile_mode,
        } => {
            let resolved_start_x = resolve_gradient_component(logical_width, start.x);
            let resolved_start_y = resolve_gradient_component(logical_height, start.y);
            let resolved_end_x = resolve_gradient_component(logical_width, end.x);
            let resolved_end_y = resolve_gradient_component(logical_height, end.y);
            set_shader_vec4(
                &mut shader,
                2,
                [
                    resolved_start_x,
                    resolved_start_y,
                    resolved_end_x,
                    resolved_end_y,
                ],
            );
            (
                GPU_TEXT_BRUSH_KIND_LINEAR,
                colors,
                stops.as_deref(),
                *tile_mode,
            )
        }
        Brush::RadialGradient {
            colors,
            stops,
            center,
            radius,
            tile_mode,
        } => {
            set_shader_vec4(&mut shader, 3, [center.x, center.y, *radius, 0.0]);
            (
                GPU_TEXT_BRUSH_KIND_RADIAL,
                colors,
                stops.as_deref(),
                *tile_mode,
            )
        }
        Brush::SweepGradient {
            colors,
            stops,
            center,
        } => {
            set_shader_vec4(&mut shader, 4, [center.x, center.y, 0.0, 0.0]);
            (
                GPU_TEXT_BRUSH_KIND_SWEEP,
                colors,
                stops.as_deref(),
                TileMode::Clamp,
            )
        }
        Brush::Solid(color) => {
            set_shader_vec4(
                &mut shader,
                GPU_TEXT_BRUSH_EFFECT_FIRST_STOP_SLOT,
                [color.r(), color.g(), color.b(), color.a()],
            );
            shader.set_float((GPU_TEXT_BRUSH_EFFECT_FIRST_STOP_SLOT + 1) * 4, 0.0);
            set_shader_vec4(
                &mut shader,
                0,
                [
                    GPU_TEXT_BRUSH_KIND_SOLID,
                    1.0,
                    tile_mode_to_shader_uniform(TileMode::Clamp),
                    alpha,
                ],
            );
            return Some(RenderEffect::runtime_shader(shader));
        }
    };

    let stop_count = colors.len();
    if stop_count == 0 || stop_count > GPU_TEXT_BRUSH_EFFECT_MAX_STOPS {
        return None;
    }

    set_shader_vec4(
        &mut shader,
        0,
        [
            brush_type,
            stop_count as f32,
            tile_mode_to_shader_uniform(tile_mode),
            alpha,
        ],
    );

    let resolved_stops = normalized_gradient_stops(stop_count, stops);
    for (index, color) in colors.iter().enumerate() {
        let color_slot = GPU_TEXT_BRUSH_EFFECT_FIRST_STOP_SLOT + index * 2;
        set_shader_vec4(
            &mut shader,
            color_slot,
            [color.r(), color.g(), color.b(), color.a()],
        );
        shader.set_float((color_slot + 1) * 4, resolved_stops[index]);
    }

    Some(RenderEffect::runtime_shader(shader))
}

fn gpu_text_effect_for_style(
    text_style: &TextStyle,
    text_rect: Rect,
    fallback_color: Color,
    text_scale: f32,
) -> Option<RenderEffect> {
    let uses_non_solid_brush = matches!(
        text_style.span_style.brush,
        Some(
            Brush::LinearGradient { .. }
                | Brush::RadialGradient { .. }
                | Brush::SweepGradient { .. }
        )
    );
    let uses_stroke = matches!(
        text_style.span_style.draw_style,
        Some(TextDrawStyle::Stroke { width }) if width.is_finite() && width > 0.0
    );
    if !uses_non_solid_brush && !uses_stroke {
        return None;
    }

    let material = gpu_text_material_for_style(text_style, fallback_color, text_scale);
    build_gpu_text_effect(&material, text_rect)
}

fn span_has_foreground_override(span_style: &cranpose_ui::text::SpanStyle) -> bool {
    span_style.color.is_some()
        || span_style.brush.is_some()
        || span_style.alpha.is_some()
        || span_style.draw_style.is_some()
}

fn text_has_span_foreground_overrides(text: &cranpose_ui::text::AnnotatedString) -> bool {
    text.span_styles
        .iter()
        .any(|span| span_has_foreground_override(&span.item))
}

fn text_for_gpu_mask(
    text: &cranpose_ui::text::AnnotatedString,
) -> cranpose_ui::text::AnnotatedString {
    if text.span_styles.is_empty() {
        return text.clone();
    }

    let mut mask_text = text.clone();
    for span in &mut mask_text.span_styles {
        span.item.color = None;
        span.item.brush = None;
        span.item.alpha = None;
        span.item.draw_style = None;
        span.item.shadow = None;
    }
    mask_text
}

fn text_with_layer_transformed_span_paint(
    text: &cranpose_ui::text::AnnotatedString,
    content_layer: &GraphicsLayer,
) -> cranpose_ui::text::AnnotatedString {
    if text.span_styles.is_empty() {
        return text.clone();
    }

    let mut transformed_text = text.clone();
    for span in &mut transformed_text.span_styles {
        span.item.color = span
            .item
            .color
            .map(|color| apply_layer_to_color(color, content_layer));
        span.item.brush = span
            .item
            .brush
            .clone()
            .map(|brush| apply_layer_to_brush(brush, content_layer));
    }
    transformed_text
}

fn merged_span_style_for_range(
    text: &cranpose_ui::text::AnnotatedString,
    base_span_style: &cranpose_ui::text::SpanStyle,
    start: usize,
    end: usize,
) -> cranpose_ui::text::SpanStyle {
    let mut merged_style = base_span_style.clone();
    for span in &text.span_styles {
        if span.range.start <= start && span.range.end >= end {
            merged_style = merged_style.merge(&span.item);
        }
    }
    merged_style
}

fn gpu_text_material_batches_for_text(
    text: &cranpose_ui::text::AnnotatedString,
    text_style: &TextStyle,
    fallback_color: Color,
    text_scale: f32,
) -> Vec<GpuTextMaterialBatch> {
    let mut batches: Vec<GpuTextMaterialBatch> = Vec::new();
    for window in text.span_boundaries().windows(2) {
        let start = window[0];
        let end = window[1];
        if start == end {
            continue;
        }

        let mut range_style = text_style.clone();
        range_style.span_style =
            merged_span_style_for_range(text, &text_style.span_style, start, end);
        let material = gpu_text_material_for_style(&range_style, fallback_color, text_scale);
        if let Some(last_batch) = batches.last_mut() {
            if last_batch.material == material {
                if let Some(last_range) = last_batch.visible_ranges.last_mut() {
                    if last_range.end == start {
                        last_range.end = end;
                    } else {
                        last_batch.visible_ranges.push(start..end);
                    }
                } else {
                    last_batch.visible_ranges.push(start..end);
                }
                continue;
            }
        }

        batches.push(GpuTextMaterialBatch {
            material,
            visible_ranges: std::iter::once(start..end).collect(),
        });
    }
    batches
}

fn text_for_gpu_mask_batch(
    text: &cranpose_ui::text::AnnotatedString,
    visible_ranges: &[Range<usize>],
) -> cranpose_ui::text::AnnotatedString {
    let mut mask_text = text_for_gpu_mask(text);
    let visible_style = cranpose_ui::text::SpanStyle {
        color: Some(Color::WHITE),
        ..Default::default()
    };
    for range in visible_ranges {
        if range.start >= range.end {
            continue;
        }
        mask_text.span_styles.push(cranpose_ui::text::RangeStyle {
            item: visible_style.clone(),
            range: range.clone(),
        });
    }
    mask_text
}

#[allow(clippy::too_many_arguments)]
fn push_span_gpu_text_material_draws(
    scene: &mut Scene,
    node_id: NodeId,
    text_rect: Rect,
    content_layer: &GraphicsLayer,
    text: &cranpose_ui::text::AnnotatedString,
    transformed_text_style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    text_scale: f32,
    options: TextLayoutOptions,
    text_clip: Option<Rect>,
) -> bool {
    let transformed_span_text = text_with_layer_transformed_span_paint(text, content_layer);
    let material_batches = gpu_text_material_batches_for_text(
        &transformed_span_text,
        transformed_text_style,
        fallback_color,
        text_scale,
    );
    if material_batches.is_empty() {
        return false;
    }

    let mut batch_effects = Vec::with_capacity(material_batches.len());
    for batch in material_batches {
        let Some(effect) = build_gpu_text_effect(&batch.material, text_rect) else {
            return false;
        };
        batch_effects.push((batch.visible_ranges, effect));
    }

    let mut mask_text_style = transformed_text_style.clone();
    mask_text_style.span_style.brush = None;
    mask_text_style.span_style.alpha = None;
    mask_text_style.span_style.color = Some(Color(1.0, 1.0, 1.0, 0.0));
    mask_text_style.span_style.draw_style = Some(TextDrawStyle::Fill);

    for (visible_ranges, effect) in batch_effects {
        let z_start = scene.next_z;
        let mask_text = text_for_gpu_mask_batch(text, &visible_ranges);
        scene.push_text(
            node_id,
            text_rect,
            Rc::new(mask_text),
            Color(1.0, 1.0, 1.0, 0.0),
            mask_text_style.clone(),
            font_size,
            text_scale,
            options,
            text_clip,
        );
        scene.effect_layers.push(EffectLayer {
            rect: text_rect,
            clip: text_clip,
            effect: Some(effect),
            blend_mode: BlendMode::SrcOver,
            composite_alpha: 1.0,
            z_start,
            z_end: scene.next_z,
        });
    }

    true
}

#[allow(clippy::too_many_arguments)]
fn push_text_style_draws(
    scene: &mut Scene,
    node_id: NodeId,
    rect: Rect,
    text_rect: Rect,
    content_layer: &GraphicsLayer,
    text: &cranpose_ui::text::AnnotatedString,
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
    let text_brush = text_style
        .span_style
        .brush
        .clone()
        .unwrap_or_else(|| Brush::solid(text_color));
    let mut transformed_text_style = text_style.clone();
    transformed_text_style.span_style.shadow = None;
    transformed_text_style.span_style.brush = text_style
        .span_style
        .brush
        .clone()
        .map(|brush| apply_layer_to_brush(brush, content_layer));

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
        let blur_radius = shadow.blur_radius.max(0.0) * text_scale;

        let shadow_text = TextDraw {
            node_id,
            rect: transformed_shadow_rect,
            text: Rc::new(text.clone()),
            color: apply_layer_to_color(shadow.color, content_layer),
            text_style: shadow_text_style,
            font_size,
            scale: text_scale,
            layout_options: options,
            z_index: 0,
            clip: text_clip,
        };

        scene.push_shadow_draw(ShadowDraw {
            shapes: vec![],
            texts: vec![shadow_text],
            blur_radius,
            clip: text_clip,
            z_index: 0,
        });
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

    let has_span_foreground_overrides = text_has_span_foreground_overrides(text);
    if has_span_foreground_overrides
        && push_span_gpu_text_material_draws(
            scene,
            node_id,
            snapped_shifted_text_rect,
            content_layer,
            text,
            &transformed_text_style,
            transformed_text_color,
            font_size,
            text_scale,
            options,
            text_clip,
        )
    {
        return;
    }

    if !has_span_foreground_overrides {
        if let Some(effect) = gpu_text_effect_for_style(
            &transformed_text_style,
            snapped_shifted_text_rect,
            transformed_text_color,
            text_scale,
        ) {
            let z_start = scene.next_z;
            let mut mask_text_style = transformed_text_style.clone();
            mask_text_style.span_style.brush = None;
            mask_text_style.span_style.alpha = None;
            mask_text_style.span_style.color = Some(Color::WHITE);
            mask_text_style.span_style.draw_style = Some(TextDrawStyle::Fill);
            let mask_text = text_for_gpu_mask(text);

            scene.push_text(
                node_id,
                snapped_shifted_text_rect,
                Rc::new(mask_text),
                Color::WHITE,
                mask_text_style,
                font_size,
                text_scale,
                options,
                text_clip,
            );

            scene.effect_layers.push(EffectLayer {
                rect: snapped_shifted_text_rect,
                clip: text_clip,
                effect: Some(effect),
                blend_mode: BlendMode::SrcOver,
                composite_alpha: 1.0,
                z_start,
                z_end: scene.next_z,
            });
            return;
        }
    }

    scene.push_text(
        node_id,
        snapped_shifted_text_rect,
        Rc::new(text.clone()),
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
    annotated_text: &cranpose_ui::text::AnnotatedString,
    global_style: &TextStyle,
    text_brush: &Brush,
    text_clip: Option<Rect>,
) {
    if annotated_text.is_empty() || !text_has_visible_decoration(annotated_text, global_style) {
        return;
    }

    let layout = layout_text(annotated_text, global_style);
    let mut segments =
        decoration_segments_from_glyph_layouts(annotated_text, global_style, &layout);
    if segments.is_empty() {
        segments = decoration_segments_from_logical_lines(annotated_text, global_style);
    }

    for segment in segments {
        let Some(decoration) = segment.span_style.text_decoration else {
            continue;
        };
        if decoration == TextDecoration::NONE {
            continue;
        }

        let span_width = segment.width();
        if span_width <= 0.0 {
            continue;
        }

        let line_height = segment.line_height.max(1.0);
        let font_size = segment.span_style.resolve_font_size(14.0);
        let thickness = (font_size * 0.06).clamp(1.0, line_height * 0.25);
        let brush = decoration_brush_for_span(&segment.span_style, text_brush, content_layer);
        let line_top = text_rect.y + segment.line_top;
        let segment_x = text_rect.x + segment.x_start;

        if decoration.contains(TextDecoration::UNDERLINE) {
            let underline_rect = Rect {
                x: segment_x,
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
                x: segment_x,
                y: line_top + line_height * 0.52 - thickness * 0.5,
                width: span_width,
                height: thickness,
            };
            let transformed = apply_layer_to_rect(strike_rect, rect, content_layer);
            scene.push_shape(transformed, brush, None, text_clip, BlendMode::SrcOver);
        }
    }
}

fn text_has_visible_decoration(
    text: &cranpose_ui::text::AnnotatedString,
    global_style: &TextStyle,
) -> bool {
    if global_style
        .span_style
        .text_decoration
        .is_some_and(|decoration| decoration != TextDecoration::NONE)
    {
        return true;
    }

    text.span_styles.iter().any(|span| {
        span.item
            .text_decoration
            .is_some_and(|decoration| decoration != TextDecoration::NONE)
    })
}

#[derive(Clone, Debug, PartialEq)]
struct DecorationVisualSegment {
    line_index: usize,
    line_top: f32,
    line_height: f32,
    x_start: f32,
    x_end: f32,
    span_style: cranpose_ui::text::SpanStyle,
}

impl DecorationVisualSegment {
    fn width(&self) -> f32 {
        (self.x_end - self.x_start).max(0.0)
    }
}

fn decoration_segments_from_glyph_layouts(
    text: &cranpose_ui::text::AnnotatedString,
    global_style: &TextStyle,
    layout: &cranpose_ui::text_layout_result::TextLayoutResult,
) -> Vec<DecorationVisualSegment> {
    let mut glyph_layouts: Vec<_> = layout
        .glyph_layouts()
        .iter()
        .copied()
        .filter(|glyph| glyph.end_offset > glyph.start_offset && glyph.width.is_finite())
        .collect();
    if glyph_layouts.is_empty() {
        return Vec::new();
    }

    glyph_layouts.sort_by(|a, b| {
        a.line_index
            .cmp(&b.line_index)
            .then_with(|| a.x.total_cmp(&b.x))
            .then_with(|| a.start_offset.cmp(&b.start_offset))
            .then_with(|| a.end_offset.cmp(&b.end_offset))
    });

    let text_len = text.text.len();
    let mut segments: Vec<DecorationVisualSegment> = Vec::new();
    for glyph in glyph_layouts {
        let start = glyph.start_offset.min(text_len);
        let end = glyph.end_offset.min(text_len);
        if start >= end {
            continue;
        }

        let merged_style = merged_span_style_for_range(text, &global_style.span_style, start, end);
        let Some(decoration) = merged_style.text_decoration else {
            continue;
        };
        if decoration == TextDecoration::NONE {
            continue;
        }

        let glyph_start_x = glyph.x;
        let glyph_end_x = (glyph.x + glyph.width.max(0.0)).max(glyph_start_x);
        if glyph_end_x <= glyph_start_x {
            continue;
        }

        if let Some(last) = segments.last_mut() {
            let same_line = last.line_index == glyph.line_index;
            let same_style = last.span_style == merged_style;
            let same_vertical_band =
                (last.line_top - glyph.y).abs() <= DECORATION_SEGMENT_MERGE_EPSILON;
            let touching = glyph_start_x <= last.x_end + DECORATION_SEGMENT_MERGE_EPSILON;
            if same_line && same_style && same_vertical_band && touching {
                last.x_end = last.x_end.max(glyph_end_x);
                last.line_height = last.line_height.max(glyph.height.max(1.0));
                continue;
            }
        }

        segments.push(DecorationVisualSegment {
            line_index: glyph.line_index,
            line_top: glyph.y,
            line_height: glyph.height.max(1.0),
            x_start: glyph_start_x,
            x_end: glyph_end_x,
            span_style: merged_style,
        });
    }

    segments
}

fn decoration_segments_from_logical_lines(
    text: &cranpose_ui::text::AnnotatedString,
    global_style: &TextStyle,
) -> Vec<DecorationVisualSegment> {
    let line_height = measure_text(text, global_style).line_height.max(1.0);
    let mut line_top = 0.0;
    let mut line_index = 0usize;
    let mut segments: Vec<DecorationVisualSegment> = Vec::new();

    for line in split_annotated_lines_for_decorations(text) {
        let mut current_offset = 0.0;
        for window in line.span_boundaries().windows(2) {
            let start = window[0];
            let end = window[1];
            if start == end {
                continue;
            }

            let merged_style =
                merged_span_style_for_range(&line, &global_style.span_style, start, end);
            let mut span_text_style = global_style.clone();
            span_text_style.span_style = merged_style.clone();
            let span_width = measure_text(&line.subsequence(start..end), &span_text_style)
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

            let x_start = current_offset;
            let x_end = current_offset + span_width;
            if let Some(last) = segments.last_mut() {
                let same_line = last.line_index == line_index;
                let same_style = last.span_style == merged_style;
                let touching = x_start <= last.x_end + DECORATION_SEGMENT_MERGE_EPSILON;
                if same_line && same_style && touching {
                    last.x_end = last.x_end.max(x_end);
                    current_offset += span_width;
                    continue;
                }
            }

            segments.push(DecorationVisualSegment {
                line_index,
                line_top,
                line_height,
                x_start,
                x_end,
                span_style: merged_style,
            });
            current_offset += span_width;
        }
        line_index = line_index.saturating_add(1);
        line_top += line_height;
    }

    segments
}

fn split_annotated_lines_for_decorations(
    text: &cranpose_ui::text::AnnotatedString,
) -> Vec<cranpose_ui::text::AnnotatedString> {
    if text.text.is_empty() {
        return vec![cranpose_ui::text::AnnotatedString::from("")];
    }

    let mut lines = Vec::new();
    let mut start = 0usize;
    for (idx, ch) in text.text.char_indices() {
        if ch == '\n' {
            lines.push(text.subsequence(start..idx));
            start = idx + ch.len_utf8();
        }
    }
    lines.push(text.subsequence(start..text.text.len()));
    lines
}

fn resolved_alpha_multiplier(alpha: Option<f32>) -> f32 {
    match alpha {
        Some(value) if value.is_finite() => value.clamp(0.0, 1.0),
        _ => 1.0,
    }
}

fn color_with_alpha_multiplier(color: Color, alpha_multiplier: f32) -> Color {
    Color(
        color.r(),
        color.g(),
        color.b(),
        (color.a() * alpha_multiplier).clamp(0.0, 1.0),
    )
}

fn brush_with_alpha_multiplier(brush: Brush, alpha_multiplier: f32) -> Brush {
    match brush {
        Brush::Solid(color) => Brush::solid(color_with_alpha_multiplier(color, alpha_multiplier)),
        Brush::LinearGradient {
            colors,
            stops,
            start,
            end,
            tile_mode,
        } => Brush::LinearGradient {
            colors: colors
                .into_iter()
                .map(|color| color_with_alpha_multiplier(color, alpha_multiplier))
                .collect(),
            stops,
            start,
            end,
            tile_mode,
        },
        Brush::RadialGradient {
            colors,
            stops,
            center,
            radius,
            tile_mode,
        } => Brush::RadialGradient {
            colors: colors
                .into_iter()
                .map(|color| color_with_alpha_multiplier(color, alpha_multiplier))
                .collect(),
            stops,
            center,
            radius,
            tile_mode,
        },
        Brush::SweepGradient {
            colors,
            stops,
            center,
        } => Brush::SweepGradient {
            colors: colors
                .into_iter()
                .map(|color| color_with_alpha_multiplier(color, alpha_multiplier))
                .collect(),
            stops,
            center,
        },
    }
}

fn decoration_brush_for_span(
    merged_style: &cranpose_ui::text::SpanStyle,
    fallback_brush: &Brush,
    content_layer: &GraphicsLayer,
) -> Brush {
    let brush = merged_style
        .brush
        .clone()
        .or_else(|| merged_style.color.map(Brush::solid))
        .unwrap_or_else(|| fallback_brush.clone());
    let alpha_multiplier = resolved_alpha_multiplier(merged_style.alpha);
    apply_layer_to_brush(
        brush_with_alpha_multiplier(brush, alpha_multiplier),
        content_layer,
    )
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
        let transformed_text_bounds =
            apply_layer_to_rect(expanded_text_bounds, rect, &content_layer);
        let text_clip = resolve_text_clip(options.overflow, visual_clip, transformed_text_bounds);

        if let Some(text_clip) = text_clip {
            push_text_style_draws(
                scene,
                node_id,
                rect,
                text_rect,
                &content_layer,
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
    use cranpose_ui::text_layout_result::{
        GlyphLayout, LineLayout, TextLayoutData, TextLayoutResult,
    };

    fn synthetic_text_layout(
        text: &str,
        line_height: f32,
        lines: Vec<LineLayout>,
        glyph_layouts: Vec<GlyphLayout>,
    ) -> TextLayoutResult {
        let mut glyph_x_positions = Vec::new();
        let mut char_to_byte = Vec::new();
        for (byte_offset, _) in text.char_indices() {
            glyph_x_positions.push(0.0);
            char_to_byte.push(byte_offset);
        }
        glyph_x_positions.push(0.0);
        char_to_byte.push(text.len());

        let width = glyph_layouts
            .iter()
            .map(|glyph| (glyph.x + glyph.width).max(0.0))
            .fold(0.0, f32::max);
        let height = lines
            .iter()
            .map(|line| (line.y + line.height).max(0.0))
            .fold(line_height.max(0.0), f32::max);

        TextLayoutResult::new(
            text,
            TextLayoutData {
                width,
                height,
                line_height,
                glyph_x_positions,
                char_to_byte,
                lines,
                glyph_layouts,
            },
        )
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
    fn text_has_visible_decoration_detects_global_and_span_styles() {
        let plain_style = cranpose_ui::TextStyle::default();
        let plain_text = cranpose_ui::text::AnnotatedString::from("Plain");
        assert!(!text_has_visible_decoration(&plain_text, &plain_style));

        let decorated_global_style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                text_decoration: Some(cranpose_ui::text::TextDecoration::UNDERLINE),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(text_has_visible_decoration(
            &plain_text,
            &decorated_global_style
        ));

        let span_decorated_text = cranpose_ui::text::AnnotatedString::builder()
            .push_style(cranpose_ui::text::SpanStyle {
                text_decoration: Some(cranpose_ui::text::TextDecoration::LINE_THROUGH),
                ..Default::default()
            })
            .append("Span")
            .pop()
            .to_annotated_string();
        assert!(text_has_visible_decoration(
            &span_decorated_text,
            &plain_style
        ));
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

        assert_eq!(scene.texts.len(), 1, "content text expected");
        assert_eq!(scene.shadow_draws.len(), 1, "shadow draw expected");
        assert_eq!(scene.shadow_draws[0].texts.len(), 1, "shadow text expected");
        assert_eq!(
            scene.shadow_draws[0].texts[0].color,
            Color(0.0, 0.0, 0.0, 0.95)
        );
        assert_eq!(scene.texts[0].color, Color(0.9, 0.95, 1.0, 1.0));
        assert!(scene.shadow_draws[0].texts[0].rect.x > scene.texts[0].rect.x);
        assert!(scene.shadow_draws[0].texts[0].rect.y > scene.texts[0].rect.y);
        assert!(
            scene.effect_layers.is_empty(),
            "blurred text shadow does not use effect layer"
        );
        assert_eq!(
            scene.shadow_draws[0].blur_radius, 3.0,
            "shadow uses blurred shadow draw radius"
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
            &cranpose_ui::text::AnnotatedString::from("Hard shadow"),
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(scene.texts.len(), 1, "content text expected");
        assert_eq!(scene.shadow_draws.len(), 1, "shadow draw expected");
        assert_eq!(scene.shadow_draws[0].texts.len(), 1, "shadow text expected");
        assert_eq!(
            scene.shadow_draws[0].blur_radius, 0.0,
            "hard shadow blur radius"
        );
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
    fn push_text_style_draws_emits_multiline_decoration_shapes_per_visual_line() {
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                text_decoration: Some(cranpose_ui::text::TextDecoration::UNDERLINE),
                ..Default::default()
            },
            ..Default::default()
        };
        let text = cranpose_ui::text::AnnotatedString::from("One\nTwo\nThree");
        let rect = Rect {
            x: 8.0,
            y: 10.0,
            width: 220.0,
            height: 72.0,
        };

        push_text_style_draws(
            &mut scene,
            22 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &text,
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(scene.shapes.len(), 3, "one underline per visual line");
        let line_height = measure_text(&text, &style).line_height.max(1.0);
        let mut ys: Vec<f32> = scene.shapes.iter().map(|shape| shape.rect.y).collect();
        ys.sort_by(|a, b| a.total_cmp(b));
        assert!(ys[1] > ys[0], "second underline should be below first line");
        assert!(ys[2] > ys[1], "third underline should be below second line");
        assert!(
            ((ys[1] - ys[0]) - line_height).abs() <= line_height * 0.35,
            "line 1->2 decoration delta should follow measured line height"
        );
        assert!(
            ((ys[2] - ys[1]) - line_height).abs() <= line_height * 0.35,
            "line 2->3 decoration delta should follow measured line height"
        );
    }

    #[test]
    fn decoration_segments_from_glyph_layouts_line_through_preserves_bidi_visual_order() {
        let style = cranpose_ui::TextStyle::default();
        let text = cranpose_ui::text::AnnotatedString::builder()
            .push_style(cranpose_ui::text::SpanStyle {
                color: Some(Color::RED),
                text_decoration: Some(cranpose_ui::text::TextDecoration::LINE_THROUGH),
                ..Default::default()
            })
            .append("A")
            .pop()
            .push_style(cranpose_ui::text::SpanStyle {
                color: Some(Color(0.0, 0.8, 0.0, 1.0)),
                text_decoration: Some(cranpose_ui::text::TextDecoration::LINE_THROUGH),
                ..Default::default()
            })
            .append("B")
            .pop()
            .push_style(cranpose_ui::text::SpanStyle {
                color: Some(Color::BLUE),
                text_decoration: Some(cranpose_ui::text::TextDecoration::LINE_THROUGH),
                ..Default::default()
            })
            .append("C")
            .pop()
            .to_annotated_string();

        let layout = synthetic_text_layout(
            "ABC",
            12.0,
            vec![LineLayout {
                start_offset: 0,
                end_offset: 3,
                y: 0.0,
                height: 12.0,
            }],
            vec![
                GlyphLayout {
                    line_index: 0,
                    start_offset: 0,
                    end_offset: 1,
                    x: 20.0,
                    y: 0.0,
                    width: 10.0,
                    height: 12.0,
                },
                GlyphLayout {
                    line_index: 0,
                    start_offset: 1,
                    end_offset: 2,
                    x: 10.0,
                    y: 0.0,
                    width: 10.0,
                    height: 12.0,
                },
                GlyphLayout {
                    line_index: 0,
                    start_offset: 2,
                    end_offset: 3,
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 12.0,
                },
            ],
        );

        let segments = decoration_segments_from_glyph_layouts(&text, &style, &layout);
        assert_eq!(
            segments.len(),
            3,
            "each differently styled glyph should emit one segment"
        );

        let red = segments
            .iter()
            .find(|segment| segment.span_style.color == Some(Color::RED))
            .expect("red segment");
        let green = segments
            .iter()
            .find(|segment| segment.span_style.color == Some(Color(0.0, 0.8, 0.0, 1.0)))
            .expect("green segment");
        let blue = segments
            .iter()
            .find(|segment| segment.span_style.color == Some(Color::BLUE))
            .expect("blue segment");

        assert!((red.x_start - 20.0).abs() < f32::EPSILON);
        assert!((green.x_start - 10.0).abs() < f32::EPSILON);
        assert!((blue.x_start - 0.0).abs() < f32::EPSILON);
        assert!(
            blue.x_start < green.x_start && green.x_start < red.x_start,
            "segments must follow visual x-order instead of logical span order"
        );
    }

    #[test]
    fn decoration_segments_from_glyph_layouts_multiline_line_through_keeps_visual_line_boxes() {
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                text_decoration: Some(cranpose_ui::text::TextDecoration::LINE_THROUGH),
                ..Default::default()
            },
            paragraph_style: cranpose_ui::ParagraphStyle {
                text_align: cranpose_ui::text::TextAlign::End,
                ..Default::default()
            },
        };
        let text = cranpose_ui::text::AnnotatedString::from("AB\nCD");
        let layout = synthetic_text_layout(
            "AB\nCD",
            12.0,
            vec![
                LineLayout {
                    start_offset: 0,
                    end_offset: 2,
                    y: 0.0,
                    height: 12.0,
                },
                LineLayout {
                    start_offset: 3,
                    end_offset: 5,
                    y: 22.0,
                    height: 18.0,
                },
            ],
            vec![
                GlyphLayout {
                    line_index: 0,
                    start_offset: 0,
                    end_offset: 1,
                    x: 28.0,
                    y: 0.0,
                    width: 10.0,
                    height: 12.0,
                },
                GlyphLayout {
                    line_index: 0,
                    start_offset: 1,
                    end_offset: 2,
                    x: 38.0,
                    y: 0.0,
                    width: 10.0,
                    height: 12.0,
                },
                GlyphLayout {
                    line_index: 1,
                    start_offset: 3,
                    end_offset: 4,
                    x: 6.0,
                    y: 22.0,
                    width: 10.0,
                    height: 18.0,
                },
                GlyphLayout {
                    line_index: 1,
                    start_offset: 4,
                    end_offset: 5,
                    x: 16.0,
                    y: 22.0,
                    width: 10.0,
                    height: 18.0,
                },
            ],
        );

        let segments = decoration_segments_from_glyph_layouts(&text, &style, &layout);
        assert_eq!(
            segments.len(),
            2,
            "adjacent same-style glyphs should merge to one segment per visual line"
        );
        assert!((segments[0].x_start - 28.0).abs() < f32::EPSILON);
        assert!((segments[0].x_end - 48.0).abs() < f32::EPSILON);
        assert!((segments[0].line_top - 0.0).abs() < f32::EPSILON);
        assert!((segments[0].line_height - 12.0).abs() < f32::EPSILON);
        assert!((segments[1].x_start - 6.0).abs() < f32::EPSILON);
        assert!((segments[1].x_end - 26.0).abs() < f32::EPSILON);
        assert!((segments[1].line_top - 22.0).abs() < f32::EPSILON);
        assert!((segments[1].line_height - 18.0).abs() < f32::EPSILON);
        assert!(
            segments[1].line_top - segments[0].line_top > 20.0,
            "line-through boxes should preserve measured wrapped line spacing"
        );
    }

    #[test]
    fn push_text_style_draws_resolves_decoration_brush_with_span_alpha_and_layer_alpha() {
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle::default();
        let text = cranpose_ui::text::AnnotatedString::builder()
            .push_style(cranpose_ui::text::SpanStyle {
                color: Some(Color::RED),
                alpha: Some(0.4),
                text_decoration: Some(cranpose_ui::text::TextDecoration::UNDERLINE),
                ..Default::default()
            })
            .append("Tinted")
            .pop()
            .to_annotated_string();
        let rect = Rect {
            x: 8.0,
            y: 10.0,
            width: 180.0,
            height: 28.0,
        };
        let layer = GraphicsLayer {
            alpha: 0.5,
            ..Default::default()
        };

        push_text_style_draws(
            &mut scene,
            23 as NodeId,
            rect,
            rect,
            &layer,
            &text,
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(scene.shapes.len(), 1, "one underline expected");
        let Brush::Solid(color) = scene.shapes[0].brush else {
            panic!("span color decoration should resolve to solid brush");
        };
        assert!((color.r() - 1.0).abs() < 1e-6);
        assert!(color.g() < 1e-6);
        assert!(color.b() < 1e-6);
        assert!(
            (color.a() - 0.2).abs() < 1e-3,
            "span alpha and layer alpha should both modulate decoration alpha"
        );
    }

    #[test]
    fn push_text_style_draws_baseline_shift_offsets_decoration_y_position() {
        let mut base_scene = Scene::new();
        let mut shifted_scene = Scene::new();
        let base_style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                text_decoration: Some(cranpose_ui::text::TextDecoration::UNDERLINE),
                ..Default::default()
            },
            ..Default::default()
        };
        let shifted_style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                baseline_shift: Some(cranpose_ui::text::BaselineShift::SUPERSCRIPT),
                text_decoration: Some(cranpose_ui::text::TextDecoration::UNDERLINE),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 8.0,
            y: 18.0,
            width: 180.0,
            height: 28.0,
        };
        let text = cranpose_ui::text::AnnotatedString::from("Shifted");

        push_text_style_draws(
            &mut base_scene,
            24 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &text,
            &base_style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );
        push_text_style_draws(
            &mut shifted_scene,
            25 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &text,
            &shifted_style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(base_scene.shapes.len(), 1, "base underline expected");
        assert_eq!(shifted_scene.shapes.len(), 1, "shifted underline expected");
        assert!(
            shifted_scene.shapes[0].rect.y < base_scene.shapes[0].rect.y,
            "baseline shift should move decoration geometry with shifted text"
        );
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
    fn push_text_style_draws_non_solid_brush_contract_uses_gpu_shader_mask() {
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

        assert_eq!(
            scene.texts.len(),
            1,
            "gradient text should use glyphon text mask"
        );
        assert_eq!(
            scene.effect_layers.len(),
            1,
            "gradient text should be wrapped in a shader effect layer"
        );
        assert_eq!(
            scene.images.len(),
            0,
            "gradient text should not fall back to software raster image draws"
        );

        let layer = &scene.effect_layers[0];
        assert_eq!(layer.z_start + 1, layer.z_end);
        let Some(RenderEffect::Shader { shader }) = layer.effect.as_ref() else {
            panic!("expected runtime shader effect for non-solid brush text");
        };
        let uniforms = shader.uniforms();

        let uniform = |index: usize| uniforms.get(index).copied().unwrap_or(0.0);
        assert!((uniform(0) - 0.0).abs() < f32::EPSILON, "linear brush type");
        assert!(
            (uniform(1) - 2.0).abs() < f32::EPSILON,
            "two gradient stops"
        );

        let stop0_color_index = GPU_TEXT_BRUSH_EFFECT_FIRST_STOP_SLOT * 4;
        let stop1_color_index = (GPU_TEXT_BRUSH_EFFECT_FIRST_STOP_SLOT + 2) * 4;
        assert!(
            uniform(stop0_color_index) > 0.95 && uniform(stop0_color_index + 2) < 0.05,
            "first stop should remain red-dominant in shader uniforms"
        );
        assert!(
            uniform(stop1_color_index + 2) > 0.95 && uniform(stop1_color_index) < 0.05,
            "second stop should remain blue-dominant in shader uniforms"
        );
    }

    #[test]
    fn push_text_style_draws_default_linear_gradient_fill_resolves_infinite_endpoints() {
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                brush: Some(Brush::linear_gradient(vec![
                    Color(0.42, 0.94, 1.0, 1.0),
                    Color(1.0, 0.76, 0.54, 1.0),
                ])),
                draw_style: Some(cranpose_ui::text::TextDrawStyle::Fill),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 8.0,
            y: 20.0,
            width: 220.0,
            height: 32.0,
        };

        push_text_style_draws(
            &mut scene,
            13 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &cranpose_ui::text::AnnotatedString::from("Gradient fill"),
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(
            scene.images.len(),
            0,
            "gradient fill should not route through software image rendering"
        );
        assert_eq!(
            scene.effect_layers.len(),
            1,
            "gradient fill should emit runtime shader effect layer"
        );
        let Some(RenderEffect::Shader { shader }) = scene.effect_layers[0].effect.as_ref() else {
            panic!("expected runtime shader effect for gradient fill");
        };

        let uniforms = shader.uniforms();
        let uniform = |index: usize| uniforms.get(index).copied().unwrap_or(0.0);
        let start_x = uniform(8);
        let start_y = uniform(9);
        let end_x = uniform(10);
        let end_y = uniform(11);

        assert!(start_x.is_finite() && start_y.is_finite());
        assert!(end_x.is_finite() && end_y.is_finite());
        assert!(end_x > start_x, "x endpoint should span gradient range");
        assert!(end_y > start_y, "y endpoint should span gradient range");
    }

    #[test]
    fn push_text_style_draws_stroke_contract_uses_gpu_shader_mask() {
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
            &cranpose_ui::text::AnnotatedString::from("Stroke text"),
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(
            scene.texts.len(),
            1,
            "stroke should emit a glyph mask text draw"
        );
        assert_eq!(
            scene.effect_layers.len(),
            1,
            "stroke should route through runtime shader effect layer"
        );
        assert_eq!(
            scene.images.len(),
            0,
            "stroke should not render via software image fallback"
        );
        let Some(RenderEffect::Shader { shader }) = scene.effect_layers[0].effect.as_ref() else {
            panic!("expected runtime shader effect for stroke text");
        };
        let uniforms = shader.uniforms();
        let uniform = |index: usize| uniforms.get(index).copied().unwrap_or(0.0);
        let material_slot = GPU_TEXT_BRUSH_EFFECT_MATERIAL_SLOT * 4;
        assert_eq!(
            uniform(material_slot),
            GPU_TEXT_DRAW_MODE_STROKE,
            "stroke path should set stroke draw mode in shader uniforms"
        );
        assert!(
            (uniform(material_slot + 1) - 5.0).abs() < f32::EPSILON,
            "stroke width should be packed into shader uniforms"
        );
    }

    #[test]
    fn push_text_style_draws_stroke_material_scales_width_with_layer_scale() {
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                draw_style: Some(cranpose_ui::text::TextDrawStyle::Stroke { width: 3.0 }),
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
        let scaled_layer = GraphicsLayer {
            scale: 2.0,
            ..Default::default()
        };

        push_text_style_draws(
            &mut scene,
            18 as NodeId,
            rect,
            rect,
            &scaled_layer,
            &cranpose_ui::text::AnnotatedString::from("Scaled stroke"),
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(
            scene.effect_layers.len(),
            1,
            "expected one shader effect layer"
        );
        let Some(RenderEffect::Shader { shader }) = scene.effect_layers[0].effect.as_ref() else {
            panic!("expected runtime shader effect for scaled stroke");
        };
        let uniforms = shader.uniforms();
        let uniform = |index: usize| uniforms.get(index).copied().unwrap_or(0.0);
        let material_slot = GPU_TEXT_BRUSH_EFFECT_MATERIAL_SLOT * 4;
        assert_eq!(uniform(material_slot), GPU_TEXT_DRAW_MODE_STROKE);
        assert!(
            (uniform(material_slot + 1) - 6.0).abs() < f32::EPSILON,
            "stroke width should scale with graphics layer scale"
        );
    }

    #[test]
    fn push_text_style_draws_gradient_stroke_contract_uses_gpu_shader_mask() {
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                brush: Some(Brush::linear_gradient_range(
                    vec![Color(0.2, 0.9, 1.0, 1.0), Color(1.0, 0.7, 0.5, 1.0)],
                    Point::new(0.0, 0.0),
                    Point::new(180.0, 0.0),
                )),
                draw_style: Some(cranpose_ui::text::TextDrawStyle::Stroke { width: 2.8 }),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 8.0,
            y: 20.0,
            width: 220.0,
            height: 32.0,
        };

        push_text_style_draws(
            &mut scene,
            12 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &cranpose_ui::text::AnnotatedString::from("Gradient stroke"),
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(
            scene.texts.len(),
            1,
            "gradient + stroke should emit a glyph mask text draw"
        );
        assert_eq!(
            scene.effect_layers.len(),
            1,
            "gradient + stroke should emit one runtime shader effect layer"
        );
        assert_eq!(
            scene.images.len(),
            0,
            "gradient + stroke should not use software text image path"
        );
        let Some(RenderEffect::Shader { shader }) = scene.effect_layers[0].effect.as_ref() else {
            panic!("expected runtime shader effect for gradient stroke");
        };
        let uniforms = shader.uniforms();
        let uniform = |index: usize| uniforms.get(index).copied().unwrap_or(0.0);
        let material_slot = GPU_TEXT_BRUSH_EFFECT_MATERIAL_SLOT * 4;
        assert_eq!(
            uniform(material_slot),
            GPU_TEXT_DRAW_MODE_STROKE,
            "gradient + stroke should keep stroke draw mode"
        );
    }

    #[test]
    fn push_text_style_draws_span_gradient_without_paint_override_uses_gpu_shader_mask() {
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                brush: Some(Brush::linear_gradient_range(
                    vec![Color(0.15, 0.9, 1.0, 1.0), Color(1.0, 0.65, 0.45, 1.0)],
                    Point::new(0.0, 0.0),
                    Point::new(200.0, 0.0),
                )),
                ..Default::default()
            },
            ..Default::default()
        };
        let text = cranpose_ui::text::AnnotatedString::builder()
            .append("GPU ")
            .push_style(cranpose_ui::text::SpanStyle {
                font_weight: Some(cranpose_ui::text::FontWeight::BOLD),
                ..Default::default()
            })
            .append("Mask")
            .pop()
            .to_annotated_string();
        let rect = Rect {
            x: 8.0,
            y: 20.0,
            width: 220.0,
            height: 32.0,
        };

        push_text_style_draws(
            &mut scene,
            19 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &text,
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(
            scene.images.len(),
            0,
            "span gradient should not use image path"
        );
        assert_eq!(scene.texts.len(), 1, "expected one mask text draw");
        assert_eq!(
            scene.effect_layers.len(),
            1,
            "span gradient without paint overrides should use gpu effect path"
        );
        let mask_text = &scene.texts[0].text;
        assert!(
            mask_text
                .span_styles
                .iter()
                .all(|span| span.item.color.is_none()
                    && span.item.brush.is_none()
                    && span.item.alpha.is_none()
                    && span.item.draw_style.is_none()),
            "mask text span styles should clear paint fields for uniform material sampling"
        );
        assert!(
            mask_text
                .span_styles
                .iter()
                .any(|span| span.item.font_weight == Some(cranpose_ui::text::FontWeight::BOLD)),
            "mask text should preserve non-paint span styling"
        );
    }

    #[test]
    fn push_text_style_draws_span_gradient_with_paint_override_uses_gpu_shader_mask_batches() {
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                brush: Some(Brush::linear_gradient_range(
                    vec![Color(0.15, 0.9, 1.0, 1.0), Color(1.0, 0.65, 0.45, 1.0)],
                    Point::new(0.0, 0.0),
                    Point::new(200.0, 0.0),
                )),
                ..Default::default()
            },
            ..Default::default()
        };
        let text = cranpose_ui::text::AnnotatedString::builder()
            .append("GPU ")
            .push_style(cranpose_ui::text::SpanStyle {
                color: Some(Color::RED),
                font_weight: Some(cranpose_ui::text::FontWeight::BOLD),
                ..Default::default()
            })
            .append("Mask")
            .pop()
            .to_annotated_string();
        let rect = Rect {
            x: 8.0,
            y: 20.0,
            width: 220.0,
            height: 32.0,
        };

        push_text_style_draws(
            &mut scene,
            20 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &text,
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(scene.images.len(), 0, "no software fallback should be used");
        assert!(
            scene.texts.len() >= 2,
            "span paint overrides should split into per-material mask draws"
        );
        assert_eq!(
            scene.texts.len(),
            scene.effect_layers.len(),
            "each mask draw should be wrapped by one runtime shader layer"
        );
        assert!(
            scene
                .effect_layers
                .iter()
                .all(|layer| layer.z_start + 1 == layer.z_end),
            "each per-span material layer should isolate exactly one mask draw"
        );
        assert!(
            scene
                .texts
                .iter()
                .all(|draw| draw.color == Color(1.0, 1.0, 1.0, 0.0)),
            "per-material mask draws should default to transparent non-target glyphs"
        );
        assert!(
            scene
                .texts
                .iter()
                .all(
                    |draw| draw.text_style.span_style.color == Some(Color(1.0, 1.0, 1.0, 0.0))
                        && draw.text_style.span_style.brush.is_none()
                        && draw.text_style.span_style.alpha.is_none()
                        && draw.text_style.span_style.draw_style == Some(TextDrawStyle::Fill)
                ),
            "mask text style should force fill-mode alpha masks for each batch"
        );
        assert!(
            scene.texts.iter().all(|draw| draw
                .text
                .span_styles
                .iter()
                .any(|span| span.item.color == Some(Color::WHITE))),
            "each batch should include explicit visible-range white mask spans"
        );
        assert!(
            scene.texts.iter().all(|draw| draw
                .text
                .span_styles
                .iter()
                .any(|span| span.item.font_weight == Some(cranpose_ui::text::FontWeight::BOLD))),
            "mask text should preserve non-paint span styling"
        );
        assert!(
            scene.texts.iter().all(|draw| draw
                .text
                .span_styles
                .iter()
                .all(|span| span.item.color != Some(Color::RED))),
            "original span paint overrides should not leak directly into mask attrs"
        );

        let mut brush_kinds = Vec::new();
        for layer in &scene.effect_layers {
            let Some(RenderEffect::Shader { shader }) = layer.effect.as_ref() else {
                panic!("expected runtime shader effect for span material batch");
            };
            brush_kinds.push(shader.uniforms().first().copied().unwrap_or_default());
        }
        assert_eq!(
            brush_kinds,
            vec![GPU_TEXT_BRUSH_KIND_LINEAR, GPU_TEXT_BRUSH_KIND_SOLID],
            "global gradient and span solid override should map to separate shader batches"
        );
    }

    #[test]
    fn push_text_style_draws_adjacent_span_paint_overrides_batch_same_material() {
        let mut scene = Scene::new();
        let style = cranpose_ui::TextStyle::default();
        let text = cranpose_ui::text::AnnotatedString::builder()
            .push_style(cranpose_ui::text::SpanStyle {
                color: Some(Color::RED),
                ..Default::default()
            })
            .append("GP")
            .pop()
            .push_style(cranpose_ui::text::SpanStyle {
                color: Some(Color::RED),
                ..Default::default()
            })
            .append("U!")
            .pop()
            .to_annotated_string();
        let rect = Rect {
            x: 8.0,
            y: 20.0,
            width: 220.0,
            height: 32.0,
        };

        push_text_style_draws(
            &mut scene,
            21 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &text,
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert_eq!(scene.images.len(), 0, "no software fallback should be used");
        assert_eq!(
            scene.texts.len(),
            1,
            "adjacent equal-material spans should batch into one mask draw"
        );
        assert_eq!(
            scene.effect_layers.len(),
            1,
            "adjacent equal-material spans should share one runtime shader pass"
        );
        assert_eq!(
            scene.effect_layers[0].z_start + 1,
            scene.effect_layers[0].z_end
        );
        let Some(RenderEffect::Shader { shader }) = scene.effect_layers[0].effect.as_ref() else {
            panic!("expected runtime shader effect for solid span batch");
        };
        let uniforms = shader.uniforms();
        let uniform = |index: usize| uniforms.get(index).copied().unwrap_or(0.0);
        assert_eq!(uniform(0), GPU_TEXT_BRUSH_KIND_SOLID);
        assert_eq!(scene.texts[0].color, Color(1.0, 1.0, 1.0, 0.0));
        assert_eq!(
            scene.texts[0].text_style.span_style.color,
            Some(Color(1.0, 1.0, 1.0, 0.0))
        );
        assert!(
            scene.texts[0]
                .text
                .span_styles
                .iter()
                .any(|span| span.item.color == Some(Color::WHITE)),
            "batched mask text should contain white-visible range spans"
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
        let static_style =
            cranpose_ui::TextStyle::from_paragraph_style(cranpose_ui::ParagraphStyle {
                text_motion: Some(cranpose_ui::text::TextMotion::Static),
                ..Default::default()
            });
        let animated_style =
            cranpose_ui::TextStyle::from_paragraph_style(cranpose_ui::ParagraphStyle {
                text_motion: Some(cranpose_ui::text::TextMotion::Animated),
                ..Default::default()
            });

        let mut static_scene = Scene::new();
        push_text_style_draws(
            &mut static_scene,
            9 as NodeId,
            base_rect,
            base_rect,
            &GraphicsLayer::default(),
            &cranpose_ui::text::AnnotatedString::from("Motion"),
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
            &cranpose_ui::text::AnnotatedString::from("Motion"),
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
    fn push_text_style_draws_gradient_static_text_snaps_mask_and_effect_bounds() {
        let mut scene = Scene::new();
        let base_rect = Rect {
            x: 8.25,
            y: 10.75,
            width: 180.2,
            height: 28.4,
        };
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                brush: Some(Brush::linear_gradient_range(
                    vec![Color(0.2, 0.9, 1.0, 1.0), Color(1.0, 0.7, 0.5, 1.0)],
                    Point::new(0.0, 0.0),
                    Point::new(180.0, 0.0),
                )),
                ..Default::default()
            },
            paragraph_style: cranpose_ui::ParagraphStyle {
                text_motion: Some(cranpose_ui::text::TextMotion::Static),
                ..Default::default()
            },
        };

        push_text_style_draws(
            &mut scene,
            11 as NodeId,
            base_rect,
            base_rect,
            &GraphicsLayer::default(),
            &cranpose_ui::text::AnnotatedString::from("Gradient static"),
            &style,
            14.0,
            TextLayoutOptions::default(),
            None,
        );

        assert!(
            scene.images.is_empty(),
            "gradient text should not use rasterized image fallback"
        );
        assert_eq!(
            scene.texts.len(),
            1,
            "gradient should emit a glyphon mask text draw"
        );
        assert_eq!(
            scene.effect_layers.len(),
            1,
            "gradient should emit one runtime shader effect layer"
        );

        let text_draw = &scene.texts[0];
        assert_eq!(text_draw.rect.x, base_rect.x.round());
        assert_eq!(text_draw.rect.y, base_rect.y.round());

        let effect_layer = &scene.effect_layers[0];
        assert_eq!(effect_layer.rect.x, base_rect.x.round());
        assert_eq!(effect_layer.rect.y, base_rect.y.round());
        assert!((effect_layer.rect.width - base_rect.width).abs() < 1e-3);
        assert!((effect_layer.rect.height - base_rect.height).abs() < 1e-3);
        let Some(RenderEffect::Shader { .. }) = effect_layer.effect.as_ref() else {
            panic!("gradient text should use runtime shader effect");
        };
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
