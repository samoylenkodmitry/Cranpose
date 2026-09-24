use std::rc::Rc;

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::{
    Brush,
    graph::{
        LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph,
        RenderNode, TextPrimitiveNode,
    },
    graph_scene::RenderDiagnostics,
    hit_graph::collect_hits_from_graph,
    layer_composition::local_content_layer,
    layer_shadow::layer_shadow_geometry,
    layer_transform::{apply_layer_affine_to_rect, apply_layer_to_rect, layer_uniform_scale},
    primitive_emit::{
        DrawPrimitiveSink, ImageDrawParams, PrimitiveClipSpace, ShapeDrawParams, TextDrawParams,
        draw_shape_params_for_primitive, emit_draw_primitive, resolve_clip, resolve_primitive_clip,
    },
};
#[cfg(test)]
use cranpose_ui::text::{ResolvedTextDirection, TextAlign, resolve_text_direction};
use cranpose_ui::{
    LayoutBox, TextLayoutOptions, measure_text,
    text::{TextDecoration, TextStyle},
};
use cranpose_ui_graphics::{
    BlendMode, Color, CompositingStrategy, DrawPrimitive, GraphicsLayer, LayerShape, Point, Rect,
    RenderEffect, RoundedCornerShape,
};

use crate::{
    scene::{RasterScene, Scene},
    style::{apply_layer_to_brush, apply_layer_to_color, combine_layers, scale_corner_radii},
};

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

struct RasterTraversalContext<'a> {
    parent_transform: ProjectiveTransform,
    parent_content_style: GraphicsLayer,
    parent_visual_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<Point>,
    inherited_translated_content_context: bool,
    diagnostics: &'a RenderDiagnostics,
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

fn report_unsupported_effects(layer: &GraphicsLayer, diagnostics: &RenderDiagnostics) {
    if layer_requires_effect_fallback(layer)
        && diagnostics.claim_warning_once("pixels.unsupported-layer-effect")
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
            snap_to_pixel_grid: false,
            brush: Brush::solid(color),
            shape,
            stroke: None,
            arc: None,
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

#[expect(clippy::too_many_arguments)]
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
        .map_or(0.0, |shift| -(shift.0 * font_size));
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
}

#[expect(clippy::too_many_arguments)]
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
                .map_or_else(|| text_brush.clone(), Brush::solid)
        });

        let line_top = text_rect.y;

        if decoration.contains(TextDecoration::UNDERLINE) {
            let underline_rect = text_decoration_rect(
                text_rect.x + current_offset,
                line_top + line_height - thickness * 1.35,
                span_width,
                thickness,
            );
            let transformed = apply_layer_to_rect(underline_rect, rect, content_layer);
            scene.push_pixel_snapped_shape(
                transformed,
                brush.clone(),
                None,
                text_clip,
                BlendMode::SrcOver,
            );
        }

        if decoration.contains(TextDecoration::LINE_THROUGH) {
            let strike_rect = text_decoration_rect(
                text_rect.x + current_offset,
                line_top + line_height * 0.52 - thickness * 0.5,
                span_width,
                thickness,
            );
            let transformed = apply_layer_to_rect(strike_rect, rect, content_layer);
            scene.push_pixel_snapped_shape(transformed, brush, None, text_clip, BlendMode::SrcOver);
        }

        current_offset += span_width;
    }
}

fn text_decoration_rect(x: f32, y: f32, width: f32, thickness: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height: thickness.ceil().max(1.0),
    }
}

#[cfg(test)]
use cranpose_render_common::scene_builder::expand_text_bounds_for_baseline_shift;

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

pub(crate) fn render_from_applier(applier: &mut MemoryApplier, root: NodeId, scene: &mut Scene) {
    let Some(graph) =
        cranpose_render_common::scene_builder::build_graph_from_applier(applier, root, 1.0)
    else {
        return;
    };
    collect_hits_from_graph(&graph.root, ProjectiveTransform::identity(), scene, None);
    scene.replace_graph(graph);
}

pub(crate) fn build_raster_scene(
    graph: &RenderGraph,
    diagnostics: &RenderDiagnostics,
) -> RasterScene {
    let mut scene = RasterScene::new();
    populate_draws_from_graph(
        &graph.root,
        &mut scene,
        RasterTraversalContext {
            parent_transform: ProjectiveTransform::identity(),
            parent_content_style: GraphicsLayer::default(),
            parent_visual_clip: None,
            inherited_translated_snap_anchor: None,
            inherited_translated_content_context: false,
            diagnostics,
        },
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
    scene: &mut RasterScene,
    context: RasterTraversalContext<'_>,
) {
    let diagnostics = context.diagnostics;
    let transform = layer.transform_to_parent.then(context.parent_transform);
    let mapping = raster_layer_mapping(layer, transform, context.parent_content_style);
    report_unsupported_effects(&layer.graphics_layer, diagnostics);

    if mapping.transformed_bounds.width <= 0.0 || mapping.transformed_bounds.height <= 0.0 {
        return;
    }

    let content_clip_to_bounds = layer.clip_to_bounds || layer.graphics_layer.clip;
    let visual_clip = resolve_clip(
        context.parent_visual_clip,
        content_clip_to_bounds.then_some(mapping.transformed_bounds),
    );
    let effective_translated_content_context =
        context.inherited_translated_content_context || layer.translated_content_context;
    let allow_rigid_snap = effective_translated_content_context || !layer.motion_context_animated;
    let boundary_snap_anchor = if !context.inherited_translated_content_context
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
    let translated_snap_anchor = context
        .inherited_translated_snap_anchor
        .or(boundary_snap_anchor);
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

    if visual_clip.is_some_and(|clip| clip.is_empty()) {
        return;
    }

    let shadow_clip = resolve_clip(
        context.parent_visual_clip,
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

    let mut deferred_draws: Vec<&RenderNode> = Vec::new();
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
                    deferred_draws.push(child);
                }
            },
            RenderNode::DrawRun(run) => match run.phase {
                PrimitivePhase::BeforeChildren => {
                    let primitive_context = PrimitiveRenderContext {
                        layer_bounds: mapping.layer_bounds,
                        node_layer: &mapping.raster_content_layer,
                        visual_clip,
                        motion_context_animated: layer.motion_context_animated,
                        content_offset_translation: effective_translated_content_context,
                        layer_snap_anchor,
                    };
                    render_graph_draw_run(scene, run, primitive_context);
                }
                PrimitivePhase::AfterChildren => {
                    deferred_draws.push(child);
                }
            },
            RenderNode::Layer(child_layer) => {
                populate_draws_from_graph(
                    child_layer,
                    scene,
                    RasterTraversalContext {
                        parent_transform: transform,
                        parent_content_style: mapping.content_style.clone(),
                        parent_visual_clip: visual_clip,
                        inherited_translated_snap_anchor: translated_snap_anchor,
                        inherited_translated_content_context: effective_translated_content_context,
                        diagnostics,
                    },
                );
            }
        }
    }

    for child in deferred_draws {
        let primitive_context = PrimitiveRenderContext {
            layer_bounds: mapping.layer_bounds,
            node_layer: &mapping.raster_content_layer,
            visual_clip,
            motion_context_animated: layer.motion_context_animated,
            content_offset_translation: effective_translated_content_context,
            layer_snap_anchor,
        };
        match child {
            RenderNode::Primitive(primitive) => {
                render_graph_primitive(scene, primitive, primitive_context);
            }
            RenderNode::DrawRun(run) => {
                render_graph_draw_run(scene, run, primitive_context);
            }
            RenderNode::Layer(_) => {}
        }
    }
}

fn render_graph_draw_run(
    scene: &mut RasterScene,
    run: &cranpose_render_common::graph::DrawRunNode,
    context: PrimitiveRenderContext<'_>,
) {
    let rect = context.layer_bounds.raster_rect();
    let counts_before = scene_counts(scene);
    for primitive in run.primitives() {
        push_draw_primitive(
            &primitive,
            rect,
            context.node_layer,
            context.visual_clip,
            scene,
            None,
            context.motion_context_animated || context.content_offset_translation,
        );
    }
    assign_snap_anchor_since(scene, counts_before, context.layer_snap_anchor);
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
            if effective_clip.is_some_and(|clip| clip.is_empty()) {
                return;
            }
            push_draw_primitive(
                &draw.primitive,
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
    if text_clip.is_some_and(|clip| clip.is_empty()) {
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

const DRAW_PRIMITIVE_TEXT_NODE_ID: cranpose_core::NodeId = 0;

pub(crate) fn push_draw_primitive(
    primitive: &DrawPrimitive,
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
            self.scene.push_shape_with_stroke_and_arc(
                params.rect,
                params.brush.into_brush(),
                params.shape,
                params.stroke,
                params.arc,
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
            shadow_primitive: &cranpose_ui_graphics::ShadowPrimitive,
            layer_bounds: Rect,
            layer: &GraphicsLayer,
            clip: Option<Rect>,
        ) {
            push_shadow_primitive(shadow_primitive, layer_bounds, layer, clip, self.scene);
        }

        fn push_text(&mut self, params: TextDrawParams) {
            self.scene.push_text(
                DRAW_PRIMITIVE_TEXT_NODE_ID,
                params.rect,
                params.text,
                params.color,
                params.text_style,
                params.font_size,
                params.scale,
                params.layout_options,
                params.clip,
            );
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
    shadow_prim: &cranpose_ui_graphics::ShadowPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    scene: &mut RasterScene,
) {
    fn shape_pair_for_primitive(
        prim: &DrawPrimitive,
        layer_bounds: Rect,
        layer: &GraphicsLayer,
        blend_mode: BlendMode,
    ) -> Option<(crate::scene::DrawShape, BlendMode)> {
        let params = draw_shape_params_for_primitive(prim, layer_bounds, layer, None, blend_mode)?;
        Some((
            crate::scene::DrawShape {
                rect: params.rect,
                snap_anchor: None,
                snap_to_pixel_grid: false,
                brush: params.brush.into_brush(),
                shape: params.shape,
                stroke: params.stroke,
                arc: params.arc,
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
            cutout,
            blur_radius,
            blend_mode,
        } => {
            let Some(shape_pair) =
                shape_pair_for_primitive(shape, layer_bounds, layer, *blend_mode)
            else {
                return;
            };
            let cutout_pair = match cutout {
                Some(cutout) => {
                    let Some(pair) =
                        shape_pair_for_primitive(cutout, layer_bounds, layer, BlendMode::DstOut)
                    else {
                        return;
                    };
                    Some(pair)
                }
                None => None,
            };
            let Some(cutout_pair) = cutout_pair else {
                push_blurred_shape_samples(scene, &shape_pair.0, shape_pair.1, clip, *blur_radius);
                return;
            };
            let samples = blur_samples(blur_radius.max(1.0));
            if samples.is_empty() {
                scene.push_shape(
                    shape_pair.0.rect,
                    shape_pair.0.brush,
                    shape_pair.0.shape,
                    clip,
                    shape_pair.1,
                );
                scene.push_shape(
                    cutout_pair.0.rect,
                    cutout_pair.0.brush,
                    cutout_pair.0.shape,
                    clip,
                    cutout_pair.1,
                );
                return;
            }
            for sample in samples.iter().rev() {
                scene.push_shape(
                    expanded_shape_rect(&shape_pair.0, sample.expansion),
                    scale_brush_alpha(shape_pair.0.brush.clone(), sample.weight),
                    shape_pair.0.shape,
                    clip,
                    shape_pair.1,
                );
                scene.push_shape(
                    expanded_shape_rect(&cutout_pair.0, sample.expansion),
                    scale_brush_alpha(cutout_pair.0.brush.clone(), sample.weight),
                    cutout_pair.0.shape,
                    clip,
                    cutout_pair.1,
                );
            }
        }
        cranpose_ui_graphics::ShadowPrimitive::Inner {
            fill,
            cutout,
            blur_radius,
            blend_mode,
            clip_rect,
        } => {
            let Some(fill_pair) = shape_pair_for_primitive(fill, layer_bounds, layer, *blend_mode)
            else {
                return;
            };
            let Some(cutout_pair) =
                shape_pair_for_primitive(cutout, layer_bounds, layer, BlendMode::DstOut)
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
#[path = "tests/pipeline_tests.rs"]
mod tests;
