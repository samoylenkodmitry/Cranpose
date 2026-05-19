use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::graph::{
    LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph,
    RenderNode, TextPrimitiveNode,
};
use cranpose_render_common::hit_graph::{
    collect_hits_from_graph as collect_common_hits, HitGraphSink,
};
use cranpose_render_common::layer_composition::local_content_layer;
use cranpose_render_common::layer_shadow::layer_shadow_geometry;
use cranpose_render_common::layer_transform::{
    apply_layer_affine_to_rect, apply_layer_to_rect, layer_uniform_scale,
};
use cranpose_render_common::primitive_emit::{
    draw_shape_params_for_primitive, emit_draw_primitive, resolve_clip, resolve_primitive_clip,
    DrawPrimitiveSink, ImageDrawParams, PrimitiveClipSpace, ShapeDrawParams,
};
use cranpose_render_common::Brush;
#[cfg(test)]
use cranpose_ui::prepare_text_layout;
#[cfg(test)]
use cranpose_ui::text::{resolve_text_direction, ResolvedTextDirection, TextAlign};
use cranpose_ui::text::{TextDecoration, TextStyle};
use cranpose_ui::{measure_text, LayoutBox, TextLayoutOptions};
#[cfg(test)]
use cranpose_ui::{EdgeInsets, TextOverflow};
use cranpose_ui_graphics::{
    BlendMode, Color, CompositingStrategy, DrawPrimitive, GraphicsLayer, LayerShape, Point, Rect,
    RenderEffect, RoundedCornerShape,
};

use crate::scene::{ClickAction, RasterScene, Scene};
use crate::style::{
    apply_layer_to_brush, apply_layer_to_color, combine_layers, scale_corner_radii,
};

#[cfg(test)]
const TEXT_CLIP_PAD: f32 = 1.0;

#[cfg(test)]
fn pad_clip_rect(rect: Rect) -> Rect {
    Rect {
        x: rect.x - TEXT_CLIP_PAD,
        y: rect.y - TEXT_CLIP_PAD,
        width: (rect.width + TEXT_CLIP_PAD * 2.0).max(0.0),
        height: (rect.height + TEXT_CLIP_PAD * 2.0).max(0.0),
    }
}

static REPORTED_UNSUPPORTED_PIXELS_EFFECTS: AtomicBool = AtomicBool::new(false);

fn graphics_layer_supports_rigid_snap(layer: &GraphicsLayer) -> bool {
    (layer.scale - 1.0).abs() <= f32::EPSILON
        && (layer.scale_x - 1.0).abs() <= f32::EPSILON
        && (layer.scale_y - 1.0).abs() <= f32::EPSILON
        && layer.rotation_x.abs() <= f32::EPSILON
        && layer.rotation_y.abs() <= f32::EPSILON
        && layer.rotation_z.abs() <= f32::EPSILON
}

fn rigid_snap_anchor(layer_bounds: Rect, layer: &GraphicsLayer) -> Option<Point> {
    if !graphics_layer_supports_rigid_snap(layer) {
        return None;
    }
    let mapped = apply_layer_affine_to_rect(layer_bounds, layer_bounds, layer);
    Some(Point::new(mapped.x, mapped.y))
}

#[derive(Clone, Copy)]
struct SceneCounts {
    shapes: usize,
    images: usize,
    texts: usize,
}

fn scene_counts(scene: &RasterScene) -> SceneCounts {
    SceneCounts {
        shapes: scene.shapes.len(),
        images: scene.images.len(),
        texts: scene.texts.len(),
    }
}

fn assign_snap_anchor_since(
    scene: &mut RasterScene,
    counts: SceneCounts,
    snap_anchor: Option<Point>,
) {
    let Some(snap_anchor) = snap_anchor else {
        return;
    };

    for shape in &mut scene.shapes[counts.shapes..] {
        shape.snap_anchor = Some(snap_anchor);
    }
    for image in &mut scene.images[counts.images..] {
        image.snap_anchor = Some(snap_anchor);
    }
    for text in &mut scene.texts[counts.texts..] {
        text.snap_anchor = Some(snap_anchor);
    }
}

#[derive(Clone, Copy)]
struct PrimitiveRenderContext<'a> {
    layer_bounds: RasterLayerBounds,
    node_layer: &'a GraphicsLayer,
    visual_clip: Option<Rect>,
    motion_context_animated: bool,
    content_offset_translation: bool,
    layer_snap_anchor: Option<Point>,
}

fn layer_contains_text_primitives(layer: &LayerNode) -> bool {
    layer.children.iter().any(|child| {
        matches!(
            child,
            RenderNode::Primitive(PrimitiveEntry {
                node: PrimitiveNode::Text(_),
                ..
            })
        )
    })
}

fn layer_contains_draw_primitives(layer: &LayerNode) -> bool {
    layer.children.iter().any(|child| {
        matches!(
            child,
            RenderNode::Primitive(PrimitiveEntry {
                node: PrimitiveNode::Draw(_),
                ..
            })
        )
    })
}

fn layer_needs_rigid_snap(layer: &LayerNode, translated_content_context: bool) -> bool {
    (translated_content_context
        && (layer_contains_draw_primitives(layer) || layer_contains_text_primitives(layer)))
        || (layer_contains_text_primitives(layer) && layer_contains_draw_primitives(layer))
}

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
}

fn blur_samples(blur_radius: f32) -> Vec<ShadowSample> {
    if blur_radius <= f32::EPSILON {
        return Vec::new();
    }

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
        samples.push(ShadowSample { expansion, weight });
        weight_sum += weight;
    }

    if weight_sum <= f32::EPSILON {
        return vec![ShadowSample {
            expansion: blur_radius,
            weight: 1.0,
        }];
    }

    for sample in &mut samples {
        sample.weight /= weight_sum;
    }

    samples
}

fn expanded_shape_rect(shape: &crate::scene::DrawShape, expansion: f32) -> Rect {
    Rect {
        x: shape.rect.x - expansion,
        y: shape.rect.y - expansion,
        width: (shape.rect.width + expansion * 2.0).max(0.0),
        height: (shape.rect.height + expansion * 2.0).max(0.0),
    }
}

fn push_blurred_shape_samples(
    scene: &mut RasterScene,
    shape: &crate::scene::DrawShape,
    blend_mode: BlendMode,
    clip: Option<Rect>,
    blur_radius: f32,
) {
    let samples = blur_samples(blur_radius.max(1.0));
    if samples.is_empty() {
        scene.push_shape(
            shape.rect,
            shape.brush.clone(),
            shape.shape,
            clip,
            blend_mode,
        );
        return;
    }

    for sample in samples.iter().rev() {
        scene.push_shape(
            expanded_shape_rect(shape, sample.expansion),
            scale_brush_alpha(shape.brush.clone(), sample.weight),
            shape.shape,
            clip,
            blend_mode,
        );
    }
}

fn scale_color_alpha(color: Color, alpha: f32) -> Color {
    Color(
        color.r(),
        color.g(),
        color.b(),
        (color.a() * alpha).clamp(0.0, 1.0),
    )
}

fn scale_brush_alpha(brush: Brush, alpha: f32) -> Brush {
    match brush {
        Brush::Solid(color) => Brush::solid(scale_color_alpha(color, alpha)),
        Brush::LinearGradient {
            mut colors,
            stops,
            start,
            end,
            tile_mode,
        } => {
            for color in &mut colors {
                *color = scale_color_alpha(*color, alpha);
            }
            Brush::LinearGradient {
                colors,
                stops,
                start,
                end,
                tile_mode,
            }
        }
        Brush::RadialGradient {
            mut colors,
            stops,
            center,
            radius,
            tile_mode,
        } => {
            for color in &mut colors {
                *color = scale_color_alpha(*color, alpha);
            }
            Brush::RadialGradient {
                colors,
                stops,
                center,
                radius,
                tile_mode,
            }
        }
        Brush::SweepGradient {
            mut colors,
            stops,
            center,
        } => {
            for color in &mut colors {
                *color = scale_color_alpha(*color, alpha);
            }
            Brush::SweepGradient {
                colors,
                stops,
                center,
            }
        }
    }
}

fn push_layer_shadow(
    scene: &mut RasterScene,
    layer: &GraphicsLayer,
    layer_bounds: RasterLayerBounds,
    transformed_bounds: Rect,
    clip: Option<Rect>,
) {
    let shadow_geometry = layer_shadow_geometry(layer, transformed_bounds);
    let scale = layer_uniform_scale(layer).max(0.1);
    let resolved_shape = match layer.shape {
        LayerShape::Rectangle => None,
        LayerShape::Rounded(shape) => {
            let resolved = shape.resolve(
                layer_bounds.local_bounds.width,
                layer_bounds.local_bounds.height,
            );
            Some(RoundedCornerShape::with_radii(scale_corner_radii(
                resolved, scale,
            )))
        }
    };

    fn shadow_shape(
        rect: Rect,
        color: Color,
        shape: Option<RoundedCornerShape>,
    ) -> crate::scene::DrawShape {
        crate::scene::DrawShape {
            rect,
            snap_anchor: None,
            brush: Brush::solid(color),
            shape,
            z_index: 0,
            clip: None,
            blend_mode: BlendMode::SrcOver,
        }
    }

    if let Some(ambient_pass) = shadow_geometry.ambient {
        let ambient = Color(
            layer.ambient_shadow_color.r(),
            layer.ambient_shadow_color.g(),
            layer.ambient_shadow_color.b(),
            ambient_pass.alpha,
        );
        push_blurred_shape_samples(
            scene,
            &shadow_shape(ambient_pass.rect, ambient, resolved_shape),
            BlendMode::SrcOver,
            clip,
            ambient_pass.blur_radius,
        );
    }

    if let Some(spot_pass) = shadow_geometry.spot {
        let spot = Color(
            layer.spot_shadow_color.r(),
            layer.spot_shadow_color.g(),
            layer.spot_shadow_color.b(),
            spot_pass.alpha,
        );
        push_blurred_shape_samples(
            scene,
            &shadow_shape(spot_pass.rect, spot, resolved_shape),
            BlendMode::SrcOver,
            clip,
            spot_pass.blur_radius,
        );
    }
}

pub(crate) fn render_layout_tree(root: &LayoutBox, scene: &mut Scene) {
    let graph = cranpose_render_common::scene_builder::build_graph_from_layout_tree(root, 1.0);
    collect_hits_from_graph(&graph.root, ProjectiveTransform::identity(), scene, None);
    scene.replace_graph(graph);
}

#[cfg(test)]
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

#[cfg(test)]
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
    scene: &mut RasterScene,
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
    scene: &mut RasterScene,
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

#[cfg(test)]
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

#[cfg(test)]
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
    let Some(graph) =
        cranpose_render_common::scene_builder::build_graph_from_applier(applier, root, 1.0)
    else {
        return;
    };
    collect_hits_from_graph(&graph.root, ProjectiveTransform::identity(), scene, None);
    scene.replace_graph(graph);
}

fn collect_hits_from_graph(
    layer: &LayerNode,
    parent_transform: ProjectiveTransform,
    scene: &mut Scene,
    parent_hit_clip: Option<Rect>,
) {
    struct SceneHitSink<'a> {
        scene: &'a mut Scene,
    }

    impl HitGraphSink for SceneHitSink<'_> {
        fn push_hit(
            &mut self,
            node_id: NodeId,
            capture_path: &[NodeId],
            geometry: cranpose_render_common::graph_scene::HitGeometry,
            shape: Option<RoundedCornerShape>,
            click_actions: &[Rc<dyn Fn(Point)>],
            pointer_inputs: &[Rc<dyn Fn(cranpose_foundation::PointerEvent)>],
        ) {
            self.scene.push_hit(
                node_id,
                capture_path.to_vec(),
                geometry,
                shape,
                click_actions
                    .iter()
                    .cloned()
                    .map(ClickAction::WithPoint)
                    .collect(),
                pointer_inputs.to_vec(),
            );
        }
    }

    let mut sink = SceneHitSink { scene };
    collect_common_hits(layer, parent_transform, &mut sink, parent_hit_clip);
}

pub(crate) fn build_raster_scene(graph: &RenderGraph) -> RasterScene {
    let mut scene = RasterScene::new();
    populate_draws_from_graph(
        &graph.root,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        &mut scene,
        None,
        None,
        false,
    );
    scene
}

#[derive(Clone, Copy)]
struct RasterLayerBounds {
    device_origin: Point,
    local_bounds: Rect,
}

impl RasterLayerBounds {
    fn from_transformed_bounds(transformed_bounds: Rect, local_bounds: Rect) -> Self {
        Self {
            device_origin: Point::new(transformed_bounds.x, transformed_bounds.y),
            local_bounds,
        }
    }

    // The software raster path anchors local primitive coordinates at a device-space origin and
    // applies one axis-aligned scale from there. This is not a world-space rect and should only
    // cross helper boundaries through this explicit conversion.
    fn raster_rect(self) -> Rect {
        Rect {
            x: self.device_origin.x,
            y: self.device_origin.y,
            width: self.local_bounds.width,
            height: self.local_bounds.height,
        }
    }
}

#[derive(Clone)]
struct RasterLayerMapping {
    layer_bounds: RasterLayerBounds,
    transformed_bounds: Rect,
    content_style: GraphicsLayer,
    raster_content_layer: GraphicsLayer,
    shadow_layer: GraphicsLayer,
}

fn raster_layer_scale(transformed_bounds: Rect, local_bounds: Rect) -> (f32, f32) {
    // The software raster path only models axis-aligned translation and scale. For rotated or
    // projective transforms this falls back to the transformed AABB, which is exact for the shared
    // Pixels/WGPU overlap but intentionally not a full rotation implementation.
    let scale_x = if local_bounds.width.abs() <= f32::EPSILON {
        1.0
    } else {
        transformed_bounds.width / local_bounds.width
    };
    let scale_y = if local_bounds.height.abs() <= f32::EPSILON {
        1.0
    } else {
        transformed_bounds.height / local_bounds.height
    };
    (scale_x.max(0.0), scale_y.max(0.0))
}

fn raster_layer_mapping(
    layer: &LayerNode,
    transform: ProjectiveTransform,
    parent_content_style: GraphicsLayer,
) -> RasterLayerMapping {
    let transformed_bounds = transform.bounds_for_rect(layer.local_bounds);
    let (scale_x, scale_y) = raster_layer_scale(transformed_bounds, layer.local_bounds);
    let content_style = combine_layers(
        parent_content_style,
        Some(local_content_layer(&layer.graphics_layer)),
    );
    let layer_bounds =
        RasterLayerBounds::from_transformed_bounds(transformed_bounds, layer.local_bounds);
    let raster_content_layer = GraphicsLayer {
        alpha: content_style.alpha,
        color_filter: content_style.color_filter,
        scale_x,
        scale_y,
        ..GraphicsLayer::default()
    };
    let shadow_layer = GraphicsLayer {
        scale_x,
        scale_y,
        shadow_elevation: layer.graphics_layer.shadow_elevation,
        ambient_shadow_color: layer.graphics_layer.ambient_shadow_color,
        spot_shadow_color: layer.graphics_layer.spot_shadow_color,
        shape: layer.graphics_layer.shape,
        ..GraphicsLayer::default()
    };

    RasterLayerMapping {
        layer_bounds,
        transformed_bounds,
        content_style,
        raster_content_layer,
        shadow_layer,
    }
}

fn populate_draws_from_graph(
    layer: &LayerNode,
    parent_transform: ProjectiveTransform,
    parent_content_style: GraphicsLayer,
    scene: &mut RasterScene,
    parent_visual_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<Point>,
    inherited_translated_content_context: bool,
) {
    let transform = layer.transform_to_parent.then(parent_transform);
    let mapping = raster_layer_mapping(layer, transform, parent_content_style);
    report_unsupported_effects(&layer.graphics_layer);

    if mapping.transformed_bounds.width <= 0.0 || mapping.transformed_bounds.height <= 0.0 {
        return;
    }

    let content_clip_to_bounds = layer.clip_to_bounds || layer.graphics_layer.clip;
    let visual_clip = resolve_clip(
        parent_visual_clip,
        content_clip_to_bounds.then_some(mapping.transformed_bounds),
    );
    let effective_translated_content_context =
        inherited_translated_content_context || layer.translated_content_context;
    let allow_rigid_snap = effective_translated_content_context || !layer.motion_context_animated;
    let boundary_snap_anchor = if !inherited_translated_content_context
        && layer.translated_content_context
        && allow_rigid_snap
    {
        rigid_snap_anchor(
            transform.bounds_for_rect(layer.local_bounds.translate(
                layer.translated_content_offset.x,
                layer.translated_content_offset.y,
            )),
            &mapping.raster_content_layer,
        )
    } else {
        None
    };
    let translated_snap_anchor = inherited_translated_snap_anchor.or(boundary_snap_anchor);
    let layer_snap_anchor = translated_snap_anchor.or_else(|| {
        if allow_rigid_snap && layer_needs_rigid_snap(layer, effective_translated_content_context) {
            rigid_snap_anchor(
                mapping.layer_bounds.raster_rect(),
                &mapping.raster_content_layer,
            )
        } else {
            None
        }
    });

    if content_clip_to_bounds && visual_clip.is_none() {
        return;
    }

    // GraphicsLayer clipping clips content, but should not clip its own shadow.
    // Explicit clip-to-bounds modifiers still clip both.
    let shadow_clip = resolve_clip(
        parent_visual_clip,
        layer
            .shadow_clip
            .map(|clip| transform.bounds_for_rect(clip)),
    );
    push_layer_shadow(
        scene,
        &mapping.shadow_layer,
        mapping.layer_bounds,
        mapping.transformed_bounds,
        shadow_clip,
    );

    let mut deferred_primitives = Vec::new();
    for child in &layer.children {
        match child {
            RenderNode::Primitive(primitive) => match primitive.phase {
                PrimitivePhase::BeforeChildren => {
                    let primitive_context = PrimitiveRenderContext {
                        layer_bounds: mapping.layer_bounds,
                        node_layer: &mapping.raster_content_layer,
                        visual_clip,
                        motion_context_animated: layer.motion_context_animated,
                        content_offset_translation: effective_translated_content_context,
                        layer_snap_anchor,
                    };
                    render_graph_primitive(scene, primitive, primitive_context);
                }
                PrimitivePhase::AfterChildren => {
                    deferred_primitives.push(primitive);
                }
            },
            RenderNode::Layer(child_layer) => {
                populate_draws_from_graph(
                    child_layer,
                    transform,
                    mapping.content_style.clone(),
                    scene,
                    visual_clip,
                    translated_snap_anchor,
                    effective_translated_content_context,
                );
            }
        }
    }

    for primitive in deferred_primitives {
        let primitive_context = PrimitiveRenderContext {
            layer_bounds: mapping.layer_bounds,
            node_layer: &mapping.raster_content_layer,
            visual_clip,
            motion_context_animated: layer.motion_context_animated,
            content_offset_translation: effective_translated_content_context,
            layer_snap_anchor,
        };
        render_graph_primitive(scene, primitive, primitive_context);
    }
}

fn render_graph_primitive(
    scene: &mut RasterScene,
    primitive: &PrimitiveEntry,
    context: PrimitiveRenderContext<'_>,
) {
    let rect = context.layer_bounds.raster_rect();
    let counts_before = scene_counts(scene);
    match &primitive.node {
        PrimitiveNode::Draw(draw) => {
            let effective_clip = resolve_primitive_clip(
                draw.clip,
                rect,
                context.node_layer,
                context.visual_clip,
                PrimitiveClipSpace::LayerTransformed,
            );
            if draw.clip.is_some() && effective_clip.is_none() {
                return;
            }
            push_draw_primitive(
                draw.primitive.clone(),
                rect,
                context.node_layer,
                effective_clip,
                scene,
                None,
                context.motion_context_animated || context.content_offset_translation,
            );
        }
        PrimitiveNode::Text(text) => {
            render_graph_text(
                scene,
                text,
                context.layer_bounds,
                context.node_layer,
                context.visual_clip,
            );
        }
    }
    assign_snap_anchor_since(scene, counts_before, context.layer_snap_anchor);
}

fn render_graph_text(
    scene: &mut RasterScene,
    text: &TextPrimitiveNode,
    layer_bounds: RasterLayerBounds,
    node_layer: &GraphicsLayer,
    visual_clip: Option<Rect>,
) {
    let rect = layer_bounds.raster_rect();
    let text_rect = Rect {
        x: rect.x + text.rect.x,
        y: rect.y + text.rect.y,
        width: text.rect.width,
        height: text.rect.height,
    };
    let text_clip = resolve_primitive_clip(
        text.clip,
        rect,
        node_layer,
        visual_clip,
        PrimitiveClipSpace::LayerTransformed,
    );
    if text.clip.is_some() && text_clip.is_none() {
        return;
    }

    push_text_style_draws(
        scene,
        text.node_id,
        rect,
        text_rect,
        node_layer,
        &text.text,
        &text.text_style,
        text.font_size,
        text.layout_options,
        text_clip,
    );
}

fn push_draw_primitive(
    primitive: DrawPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    scene: &mut RasterScene,
    blend_mode: Option<BlendMode>,
    motion_context_animated: bool,
) {
    struct SceneEmitter<'a> {
        scene: &'a mut RasterScene,
    }

    impl DrawPrimitiveSink for SceneEmitter<'_> {
        fn push_shape(&mut self, params: ShapeDrawParams) {
            self.scene.push_shape_with_geometry(
                params.rect,
                params.local_rect,
                params.quad,
                params.brush,
                params.shape,
                params.clip,
                params.blend_mode,
            );
        }

        fn push_image(&mut self, params: ImageDrawParams) {
            self.scene.push_image_with_geometry(
                params.rect,
                params.local_rect,
                params.quad,
                params.image,
                params.alpha,
                params.color_filter,
                params.sampling,
                params.clip,
                params.src_rect,
                params.blend_mode,
            );
        }

        fn push_shadow(
            &mut self,
            shadow_primitive: cranpose_ui_graphics::ShadowPrimitive,
            layer_bounds: Rect,
            layer: &GraphicsLayer,
            clip: Option<Rect>,
        ) {
            push_shadow_primitive(shadow_primitive, layer_bounds, layer, clip, self.scene);
        }
    }

    let mut emitter = SceneEmitter { scene };
    emit_draw_primitive(
        primitive,
        layer_bounds,
        layer,
        clip,
        &mut emitter,
        blend_mode,
        motion_context_animated,
    );
}

fn push_shadow_primitive(
    shadow_prim: cranpose_ui_graphics::ShadowPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    scene: &mut RasterScene,
) {
    fn shape_pair_for_primitive(
        prim: DrawPrimitive,
        layer_bounds: Rect,
        layer: &GraphicsLayer,
        blend_mode: BlendMode,
    ) -> Option<(crate::scene::DrawShape, BlendMode)> {
        let params = draw_shape_params_for_primitive(prim, layer_bounds, layer, None, blend_mode)?;
        Some((
            crate::scene::DrawShape {
                rect: params.rect,
                snap_anchor: None,
                brush: params.brush,
                shape: params.shape,
                z_index: 0,
                clip: params.clip,
                blend_mode: params.blend_mode,
            },
            params.blend_mode,
        ))
    }

    match shadow_prim {
        cranpose_ui_graphics::ShadowPrimitive::Drop {
            shape,
            blur_radius,
            blend_mode,
        } => {
            let Some(shape_pair) =
                shape_pair_for_primitive(*shape, layer_bounds, layer, blend_mode)
            else {
                return;
            };
            push_blurred_shape_samples(scene, &shape_pair.0, shape_pair.1, clip, blur_radius);
        }
        cranpose_ui_graphics::ShadowPrimitive::Inner {
            fill,
            cutout,
            blur_radius,
            blend_mode,
            clip_rect,
        } => {
            let Some(fill_pair) = shape_pair_for_primitive(*fill, layer_bounds, layer, blend_mode)
            else {
                return;
            };
            let Some(cutout_pair) =
                shape_pair_for_primitive(*cutout, layer_bounds, layer, BlendMode::DstOut)
            else {
                return;
            };
            let abs_clip = Rect {
                x: clip_rect.x + layer_bounds.x,
                y: clip_rect.y + layer_bounds.y,
                width: clip_rect.width,
                height: clip_rect.height,
            };
            let transformed_clip = apply_layer_to_rect(abs_clip, layer_bounds, layer);
            let effective_clip = clip.map_or(Some(transformed_clip), |parent_clip| {
                parent_clip.intersect(transformed_clip)
            });
            let samples = blur_samples(blur_radius.max(1.0));
            if samples.is_empty() {
                scene.push_shape(
                    fill_pair.0.rect,
                    fill_pair.0.brush,
                    fill_pair.0.shape,
                    effective_clip,
                    fill_pair.1,
                );
                scene.push_shape(
                    cutout_pair.0.rect,
                    cutout_pair.0.brush,
                    cutout_pair.0.shape,
                    effective_clip,
                    cutout_pair.1,
                );
                return;
            }

            for sample in samples.iter().rev() {
                scene.push_shape(
                    expanded_shape_rect(&fill_pair.0, sample.expansion),
                    scale_brush_alpha(fill_pair.0.brush.clone(), sample.weight),
                    fill_pair.0.shape,
                    effective_clip,
                    fill_pair.1,
                );
                scene.push_shape(
                    expanded_shape_rect(&cutout_pair.0, sample.expansion),
                    scale_brush_alpha(cutout_pair.0.brush.clone(), sample.weight),
                    cutout_pair.0.shape,
                    effective_clip,
                    cutout_pair.1,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::RasterScene;
    use cranpose_render_common::graph::{
        CachePolicy, DrawPrimitiveNode, IsolationReasons, LayerNode, PrimitiveEntry, PrimitiveNode,
        PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
    };
    use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
    use cranpose_ui_graphics::{CornerRadii, ImageBitmap, ImageSampling};

    fn snapped_text_leaf_root(animated: bool, translated_content_context: bool) -> RenderGraph {
        let text_leaf = LayerNode {
            node_id: Some(77),
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 48.0,
                height: 24.0,
            },
            transform_to_parent: ProjectiveTransform::translation(14.25, 16.5),
            motion_context_animated: animated,
            translated_content_context,
            translated_content_offset: Point::default(),
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: vec![
                RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(DrawPrimitiveNode {
                        primitive: DrawPrimitive::RoundRect {
                            rect: Rect {
                                x: 0.0,
                                y: 0.0,
                                width: 48.0,
                                height: 24.0,
                            },
                            brush: Brush::solid(Color(0.28, 0.30, 0.46, 0.88)),
                            radii: CornerRadii::uniform(6.0),
                        },
                        clip: None,
                    }),
                }),
                RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(DrawPrimitiveNode {
                        primitive: DrawPrimitive::Image {
                            rect: Rect {
                                x: 2.0,
                                y: 2.0,
                                width: 12.0,
                                height: 12.0,
                            },
                            image: ImageBitmap::from_rgba8(
                                2,
                                2,
                                vec![
                                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255,
                                    255,
                                ],
                            )
                            .expect("image"),
                            alpha: 1.0,
                            color_filter: None,
                            sampling: ImageSampling::Linear,
                            src_rect: None,
                        },
                        clip: None,
                    }),
                }),
                RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                        node_id: 77,
                        rect: Rect {
                            x: 6.0,
                            y: 4.0,
                            width: 36.0,
                            height: 16.0,
                        },
                        text: cranpose_ui::text::AnnotatedString::from("48 px"),
                        text_style: TextStyle::default(),
                        font_size: 14.0,
                        layout_options: TextLayoutOptions::default(),
                        clip: None,
                    })),
                }),
            ],
        };

        RenderGraph::new(LayerNode {
            node_id: None,
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 96.0,
                height: 64.0,
            },
            transform_to_parent: ProjectiveTransform::identity(),
            motion_context_animated: false,
            translated_content_context: false,
            translated_content_offset: Point::default(),
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: vec![RenderNode::Layer(Box::new(text_leaf))],
        })
    }

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
        let mut scene = RasterScene::new();
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
        let geometry = layer_shadow_geometry(&layer, bounds);
        let ambient_pass = geometry.ambient.expect("ambient pass");
        let spot_pass = geometry.spot.expect("spot pass");
        let ambient_samples = blur_samples(ambient_pass.blur_radius.max(1.0));
        let spot_samples = blur_samples(spot_pass.blur_radius.max(1.0));

        push_layer_shadow(
            &mut scene,
            &layer,
            RasterLayerBounds::from_transformed_bounds(bounds, bounds),
            bounds,
            None,
        );

        assert!(
            scene.shapes.len() == ambient_samples.len() + spot_samples.len(),
            "pixels shadow blur should emit one sample per shared ambient/spot pass"
        );

        let ambient = &scene.shapes[0];
        let ambient_expansion = ambient_samples
            .last()
            .expect("ambient blur samples")
            .expansion;
        assert_eq!(
            ambient.rect,
            Rect {
                x: ambient_pass.rect.x - ambient_expansion,
                y: ambient_pass.rect.y - ambient_expansion,
                width: ambient_pass.rect.width + ambient_expansion * 2.0,
                height: ambient_pass.rect.height + ambient_expansion * 2.0,
            }
        );

        let spot = &scene.shapes[ambient_samples.len()];
        let spot_expansion = spot_samples.last().expect("spot blur samples").expansion;
        assert_eq!(
            spot.rect,
            Rect {
                x: spot_pass.rect.x - spot_expansion,
                y: spot_pass.rect.y - spot_expansion,
                width: spot_pass.rect.width + spot_expansion * 2.0,
                height: spot_pass.rect.height + spot_expansion * 2.0,
            }
        );
        let spot_peak_alpha = scene.shapes[ambient_samples.len()..]
            .iter()
            .filter_map(|shape| match &shape.brush {
                Brush::Solid(color) => Some(color.a()),
                _ => None,
            })
            .fold(0.0f32, f32::max);
        assert!(spot_peak_alpha > 0.02, "spot alpha should remain visible");
    }

    #[test]
    fn build_raster_scene_uses_graph_transform_to_parent() {
        let graph = RenderGraph::new(LayerNode {
            node_id: None,
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 48.0,
            },
            transform_to_parent: ProjectiveTransform::identity(),
            motion_context_animated: false,
            translated_content_context: false,
            translated_content_offset: Point::default(),
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: vec![RenderNode::Layer(Box::new(LayerNode {
                node_id: None,
                local_bounds: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 24.0,
                    height: 18.0,
                },
                transform_to_parent: ProjectiveTransform::translation(17.0, 11.0),
                motion_context_animated: false,
                translated_content_context: false,
                translated_content_offset: Point::default(),
                graphics_layer: GraphicsLayer::default(),
                clip_to_bounds: false,
                shadow_clip: None,
                hit_test: None,
                has_hit_targets: false,
                isolation: IsolationReasons::default(),
                cache_policy: CachePolicy::None,
                cache_hashes: LayerRasterCacheHashes::default(),
                cache_hashes_valid: false,
                children: vec![RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(DrawPrimitiveNode {
                        primitive: DrawPrimitive::Rect {
                            rect: Rect {
                                x: 4.0,
                                y: 3.0,
                                width: 8.0,
                                height: 6.0,
                            },
                            brush: Brush::solid(Color::WHITE),
                        },
                        clip: None,
                    }),
                })],
            }))],
        });

        let scene = build_raster_scene(&graph);
        let [shape] = scene.shapes.as_slice() else {
            panic!("expected exactly one translated child shape");
        };
        assert_eq!(
            shape.rect,
            Rect {
                x: 21.0,
                y: 14.0,
                width: 8.0,
                height: 6.0,
            }
        );
    }

    #[test]
    fn direct_text_leaf_snaps_modifier_background_and_text_with_one_anchor() {
        let scene = build_raster_scene(&snapped_text_leaf_root(false, false));

        assert_eq!(scene.shapes.len(), 1);
        assert_eq!(scene.images.len(), 1);
        assert_eq!(scene.texts.len(), 1);
        let expected_anchor = Some(Point::new(14.25, 16.5));
        assert_eq!(scene.shapes[0].snap_anchor, expected_anchor);
        assert_eq!(scene.images[0].snap_anchor, expected_anchor);
        assert_eq!(scene.texts[0].snap_anchor, expected_anchor);
    }

    #[test]
    fn animated_translated_content_text_leaf_uses_content_snap() {
        let scene = build_raster_scene(&snapped_text_leaf_root(true, true));

        assert_eq!(scene.shapes.len(), 1);
        assert_eq!(scene.images.len(), 1);
        assert_eq!(scene.texts.len(), 1);
        let expected_anchor = Some(Point::new(14.25, 16.5));
        assert_eq!(
            scene.shapes[0].snap_anchor, expected_anchor,
            "active scroll shapes should render with the same content-origin snap phase as settled content"
        );
        assert_eq!(
            scene.images[0].snap_anchor, expected_anchor,
            "active scroll images should render with the same content-origin snap phase as settled content"
        );
        assert_eq!(
            scene.texts[0].snap_anchor, expected_anchor,
            "active scroll text should render with the same content-origin snap phase as settled content"
        );
    }

    #[test]
    fn rested_translated_content_text_leaf_snaps_for_crisp_scroll_rest() {
        let scene = build_raster_scene(&snapped_text_leaf_root(false, true));

        assert_eq!(scene.shapes.len(), 1);
        assert_eq!(scene.images.len(), 1);
        assert_eq!(scene.texts.len(), 1);
        let expected_anchor = Some(Point::new(14.25, 16.5));
        assert_eq!(
            scene.shapes[0].snap_anchor, expected_anchor,
            "rested scroll content should snap back to device pixels"
        );
        assert_eq!(
            scene.images[0].snap_anchor, expected_anchor,
            "rested scroll images should snap back to device pixels"
        );
        assert_eq!(
            scene.texts[0].snap_anchor, expected_anchor,
            "rested scroll text should snap back to device pixels"
        );
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
        let mut scene = RasterScene::new();
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
        let mut scene = RasterScene::new();
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
        let mut scene = RasterScene::new();
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
        let mut scene = RasterScene::new();
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

    #[test]
    fn drop_shadow_primitive_blur_emits_multiple_samples() {
        let mut scene = RasterScene::new();
        let layer_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 24.0,
            height: 16.0,
        };

        push_shadow_primitive(
            cranpose_ui_graphics::ShadowPrimitive::Drop {
                shape: Box::new(DrawPrimitive::Rect {
                    rect: Rect {
                        x: 2.0,
                        y: 3.0,
                        width: 12.0,
                        height: 8.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                }),
                blur_radius: 6.0,
                blend_mode: BlendMode::SrcOver,
            },
            layer_bounds,
            &GraphicsLayer::default(),
            None,
            &mut scene,
        );

        assert!(
            scene.shapes.len() > 1,
            "blurred drop shadow should emit multiple weighted samples"
        );
    }

    #[test]
    fn inner_shadow_primitive_blur_emits_multiple_samples() {
        let mut scene = RasterScene::new();
        let layer_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 24.0,
            height: 16.0,
        };

        push_shadow_primitive(
            cranpose_ui_graphics::ShadowPrimitive::Inner {
                fill: Box::new(DrawPrimitive::Rect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 24.0,
                        height: 16.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                }),
                cutout: Box::new(DrawPrimitive::Rect {
                    rect: Rect {
                        x: 3.0,
                        y: 4.0,
                        width: 12.0,
                        height: 6.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                }),
                blur_radius: 5.0,
                blend_mode: BlendMode::SrcOver,
                clip_rect: layer_bounds,
            },
            layer_bounds,
            &GraphicsLayer::default(),
            None,
            &mut scene,
        );

        assert!(
            scene.shapes.len() > 2,
            "blurred inner shadow should emit repeated fill/cutout samples"
        );
    }
}
