use crate::effect_renderer::CompositeSampleMode;
use crate::pipeline::{
    emitted_scene_bounds, push_draw_primitive, push_layer_shadow, push_text_style_draws,
    scene_emission_counts, SceneEmissionCounts, TextLayoutResolver, UiTextLayoutResolver,
};
use crate::scene::{
    BackdropLayer, CompositorScene, DrawOp, DrawOpKind, DrawShape, EffectLayer, ImageDraw,
    SceneCapacityHint, ShadowDraw, SnapAnchor, TextDraw,
};
use crate::surface_plan::{
    composite_sample_mode_for_requirements, effective_surface_requirements, layer_cache_key,
    layer_contains_descendant_backdrop, layer_needs_rigid_snap, layer_surface_requirements_cached,
    translated_content_axes_for_layer, LayerSurfaceRequirements, TranslatedContentAxes,
    TranslationRenderContext,
};
use crate::surface_requirements::{SurfaceRequirement, SurfaceRequirementSet};
use cranpose_core::collections::map::HashMap;
use cranpose_render_common::geometry::{expand_blurred_rect, union_rect};
use cranpose_render_common::graph::DrawRunNode;
use cranpose_render_common::graph::{
    quad_bounds, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform,
    RenderNode,
};
use cranpose_render_common::layer_composition::local_content_layer_for;
use cranpose_render_common::primitive_emit::{
    arc_shape_params, rect_shape_params, resolve_clip, resolve_primitive_clip,
    round_rect_shape_params, PrimitiveClipSpace, ShapeDrawParams,
};
use cranpose_ui_graphics::{BlendMode, Brush, DrawPrimitive, GraphicsLayer, Point, Rect};

const NORMALIZED_SCENE_AFFINE_TOLERANCE: f32 = 1e-4;
const MOTION_STABLE_CAPTURE_MIN_LEADING_GUARD: f32 = 64.0;
const MOTION_STABLE_CAPTURE_MAX_LEADING_GUARD: f32 = 2048.0;
const MOTION_STABLE_CAPTURE_LEADING_VIEWPORTS: f32 = 3.0;
const MOTION_STABLE_CAPTURE_CLIPPED_LEADING_VIEWPORTS: f32 = 0.25;
const MOTION_STABLE_CAPTURE_CLIPPED_MAX_LEADING_GUARD: f32 = 256.0;
const TRANSLATED_LOCAL_CAPTURE_STABLE_GUARD: f32 = 64.0;
const MOTION_STABLE_CAPTURE_CROSS_AXIS_LEADING_GUARD: f32 = 96.0;
const CLIPPED_TEXT_PREWARM_VIEWPORT_MULTIPLIER: f32 = 2.0;

pub(crate) struct ChildLayerComposite<'a> {
    pub(crate) z_index: usize,
    pub(crate) layer: &'a LayerNode,
    pub(crate) logical_rect: Rect,
    pub(crate) dest_quad: [[f32; 2]; 4],
    pub(crate) snap_anchor: Option<SnapAnchor>,
    pub(crate) backdrop_rect: Rect,
    pub(crate) visual_clip: Option<Rect>,
    pub(crate) surface_clip: Option<Rect>,
    pub(crate) shadow_draws: Vec<ShadowDraw>,
    pub(crate) needs_nested_underlay: bool,
}

#[derive(Clone)]
pub(crate) struct ResolvedChildSurfaceComposite {
    pub(crate) logical_rect: Rect,
    pub(crate) dest_quad: [[f32; 2]; 4],
    pub(crate) snap_anchor: Option<SnapAnchor>,
    pub(crate) backdrop_rect: Rect,
    pub(crate) surface_clip: Option<Rect>,
    pub(crate) shadow_draws: Vec<ShadowDraw>,
}

pub(crate) struct CollectedLayer<'a> {
    pub(crate) scene: CompositorScene,
    pub(crate) child_layers: Vec<ChildLayerComposite<'a>>,
}

pub(crate) fn visible_draw_rect(rect: Rect, clip: Option<Rect>) -> Option<Rect> {
    match clip {
        Some(clip) => rect.intersect(clip),
        None => Some(rect),
    }
}

fn expand_rect(rect: Rect, margin_x: f32, margin_y: f32) -> Rect {
    Rect {
        x: rect.x - margin_x,
        y: rect.y - margin_y,
        width: rect.width + margin_x * 2.0,
        height: rect.height + margin_y * 2.0,
    }
}

fn layer_contains_text_primitive(layer: &LayerNode) -> bool {
    layer.children.iter().any(|child| match child {
        RenderNode::Primitive(PrimitiveEntry {
            node: PrimitiveNode::Text(_),
            ..
        }) => true,
        RenderNode::Layer(child_layer) => layer_contains_text_primitive(child_layer),
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
    })
}

fn clipped_layer_should_collect_for_text_prewarm(
    layer: &LayerNode,
    layer_bounds: Rect,
    clip: Rect,
) -> bool {
    if !layer_contains_text_primitive(layer) {
        return false;
    }
    rect_should_collect_for_text_prewarm(layer_bounds, clip)
}

fn rect_should_collect_for_text_prewarm(rect: Rect, clip: Rect) -> bool {
    let prewarm_clip = expand_rect(
        clip,
        clip.width * CLIPPED_TEXT_PREWARM_VIEWPORT_MULTIPLIER,
        clip.height * CLIPPED_TEXT_PREWARM_VIEWPORT_MULTIPLIER,
    );
    rect.intersect(prewarm_clip).is_some()
}

fn scene_bounds_with_clip(scene: &CompositorScene, apply_clip: bool) -> Option<Rect> {
    let mut bounds = None;
    for shape in &scene.shapes {
        let rect = if apply_clip {
            visible_draw_rect(shape.rect, shape.clip)
        } else {
            Some(shape.rect)
        };
        if let Some(visible) = rect {
            bounds = union_rect(bounds, visible);
        }
    }
    for image in &scene.images {
        let rect = if apply_clip {
            visible_draw_rect(image.rect, image.clip)
        } else {
            Some(image.rect)
        };
        if let Some(visible) = rect {
            bounds = union_rect(bounds, visible);
        }
    }
    for text in &scene.texts {
        let rect = if apply_clip {
            visible_draw_rect(text.rect, text.clip)
        } else {
            Some(text.rect)
        };
        if let Some(visible) = rect {
            bounds = union_rect(bounds, visible);
        }
    }
    if let Some(shadow_bounds) = shadow_draws_bounds_with_clip(&scene.shadow_draws, apply_clip) {
        bounds = union_rect(bounds, shadow_bounds);
    }
    for layer in &scene.effect_layers {
        let rect = if apply_clip
            || layer
                .requirements
                .contains(SurfaceRequirement::MotionStableCapture)
        {
            visible_draw_rect(layer.rect, layer.clip)
        } else {
            Some(layer.rect)
        };
        if let Some(visible) = rect {
            bounds = union_rect(bounds, visible);
        }
    }
    for layer in &scene.backdrop_layers {
        let rect = if apply_clip {
            visible_draw_rect(layer.rect, layer.clip)
        } else {
            Some(layer.rect)
        };
        if let Some(visible) = rect {
            bounds = union_rect(bounds, visible);
        }
    }
    bounds
}

pub(crate) fn scene_bounds(scene: &CompositorScene) -> Option<Rect> {
    scene_bounds_with_clip(scene, true)
}

fn scene_capture_bounds(scene: &CompositorScene) -> Option<Rect> {
    scene_bounds_with_clip(scene, false)
}

fn shadow_draws_bounds_with_clip(shadow_draws: &[ShadowDraw], apply_clip: bool) -> Option<Rect> {
    let mut bounds = None;
    for shadow in shadow_draws {
        let mut shadow_bounds = None;
        for (shape, _) in &shadow.shapes {
            shadow_bounds = union_rect(shadow_bounds, shape.rect);
        }
        for text in &shadow.texts {
            shadow_bounds = union_rect(shadow_bounds, text.rect);
        }
        if let Some(shadow_bounds) = shadow_bounds {
            let clip = apply_clip.then_some(shadow.clip).flatten();
            if let Some(expanded) = expand_blurred_rect(shadow_bounds, shadow.blur_radius, clip) {
                bounds = union_rect(bounds, expanded);
            }
        }
    }
    bounds
}

fn shadow_draws_bounds(shadow_draws: &[ShadowDraw]) -> Option<Rect> {
    shadow_draws_bounds_with_clip(shadow_draws, true)
}

pub(crate) fn collected_layer_bounds(
    scene: &CompositorScene,
    child_layers: &[ChildLayerComposite<'_>],
    apply_clip: bool,
) -> Option<Rect> {
    let mut bounds = if apply_clip {
        scene_bounds(scene)
    } else {
        scene_capture_bounds(scene)
    };
    for child in child_layers {
        let child_bounds = quad_bounds(child.dest_quad);
        let rect = if apply_clip {
            visible_draw_rect(child_bounds, child.visual_clip)
        } else {
            Some(child_bounds)
        };
        if let Some(visible) = rect {
            bounds = union_rect(bounds, visible);
        }
        let shadow_bounds = if apply_clip {
            shadow_draws_bounds(&child.shadow_draws)
        } else {
            shadow_draws_bounds_with_clip(&child.shadow_draws, false)
        };
        if let Some(shadow_bounds) = shadow_bounds {
            bounds = union_rect(bounds, shadow_bounds);
        }
    }
    bounds
}

fn hidden_content_precedes_visible_bounds(visible_bounds: Rect, full_bounds: Rect) -> bool {
    full_bounds.x < visible_bounds.x - NORMALIZED_SCENE_AFFINE_TOLERANCE
        || full_bounds.y < visible_bounds.y - NORMALIZED_SCENE_AFFINE_TOLERANCE
}

fn leading_capture_guard(extent: f32) -> f32 {
    (extent * MOTION_STABLE_CAPTURE_LEADING_VIEWPORTS).clamp(
        MOTION_STABLE_CAPTURE_MIN_LEADING_GUARD,
        MOTION_STABLE_CAPTURE_MAX_LEADING_GUARD,
    )
}

fn clipped_leading_capture_guard(extent: f32) -> f32 {
    (extent * MOTION_STABLE_CAPTURE_CLIPPED_LEADING_VIEWPORTS).clamp(
        MOTION_STABLE_CAPTURE_MIN_LEADING_GUARD,
        MOTION_STABLE_CAPTURE_CLIPPED_MAX_LEADING_GUARD,
    )
}

fn stable_deep_leading_axis(
    visible_start: f32,
    visible_extent: f32,
    full_start: f32,
) -> (f32, f32) {
    let guard = leading_capture_guard(visible_extent);
    stable_deep_leading_axis_with_guard(visible_start, visible_extent, full_start, guard)
}

fn stable_deep_leading_axis_with_guard(
    visible_start: f32,
    visible_extent: f32,
    _full_start: f32,
    guard: f32,
) -> (f32, f32) {
    let visible_end = visible_start + visible_extent;
    let guarded_start = visible_start - guard;
    (guarded_start, visible_end)
}

fn leading_capture_bounds(visible_bounds: Rect, full_bounds: Rect) -> Rect {
    let (left, right) = if full_bounds.x < visible_bounds.x - NORMALIZED_SCENE_AFFINE_TOLERANCE {
        stable_deep_leading_axis(visible_bounds.x, visible_bounds.width, full_bounds.x)
    } else {
        (visible_bounds.x, visible_bounds.x + visible_bounds.width)
    };
    let (top, bottom) = if full_bounds.y < visible_bounds.y - NORMALIZED_SCENE_AFFINE_TOLERANCE {
        stable_deep_leading_axis(visible_bounds.y, visible_bounds.height, full_bounds.y)
    } else {
        (visible_bounds.y, visible_bounds.y + visible_bounds.height)
    };

    Rect {
        x: left,
        y: top,
        width: (right - left).max(0.0),
        height: (bottom - top).max(0.0),
    }
}

fn leading_axis_capture_bounds_with_guard(
    visible_start: f32,
    visible_extent: f32,
    full_start: f32,
    guard: f32,
) -> (f32, f32) {
    if full_start < visible_start - NORMALIZED_SCENE_AFFINE_TOLERANCE {
        stable_deep_leading_axis_with_guard(visible_start, visible_extent, full_start, guard)
    } else {
        (visible_start, visible_start + visible_extent)
    }
}

fn stable_axis_reference(
    visible_start: f32,
    visible_extent: f32,
    clip_start: Option<f32>,
    clip_extent: Option<f32>,
) -> (f32, f32) {
    match (clip_start, clip_extent) {
        (Some(start), Some(extent)) if extent.is_finite() && extent > 0.0 => (start, extent),
        _ => (visible_start, visible_extent),
    }
}

fn fixed_cross_axis_capture_bounds(
    visible_start: f32,
    visible_extent: f32,
    clip_start: Option<f32>,
    clip_extent: Option<f32>,
) -> (f32, f32) {
    match (clip_start, clip_extent) {
        (Some(start), Some(extent)) if extent.is_finite() && extent > 0.0 => (
            start - MOTION_STABLE_CAPTURE_CROSS_AXIS_LEADING_GUARD,
            start + extent,
        ),
        _ => (visible_start, visible_start + visible_extent),
    }
}

fn translated_capture_bounds(
    visible_bounds: Rect,
    full_bounds: Rect,
    clip: Option<Rect>,
    preserve_leading_x: bool,
    preserve_leading_y: bool,
) -> Rect {
    let (left, right) = if preserve_leading_x {
        let (start, extent) = stable_axis_reference(
            visible_bounds.x,
            visible_bounds.width,
            clip.map(|clip| clip.x),
            clip.map(|clip| clip.width),
        );
        let guard = if clip.is_some() {
            clipped_leading_capture_guard(extent)
        } else {
            leading_capture_guard(extent)
        };
        leading_axis_capture_bounds_with_guard(start, extent, full_bounds.x, guard)
    } else if preserve_leading_y {
        fixed_cross_axis_capture_bounds(
            visible_bounds.x,
            visible_bounds.width,
            clip.map(|clip| clip.x),
            clip.map(|clip| clip.width),
        )
    } else {
        (visible_bounds.x, visible_bounds.x + visible_bounds.width)
    };
    let (top, bottom) = if preserve_leading_y {
        let (start, extent) = stable_axis_reference(
            visible_bounds.y,
            visible_bounds.height,
            clip.map(|clip| clip.y),
            clip.map(|clip| clip.height),
        );
        let guard = if clip.is_some() {
            clipped_leading_capture_guard(extent)
        } else {
            leading_capture_guard(extent)
        };
        leading_axis_capture_bounds_with_guard(start, extent, full_bounds.y, guard)
    } else if preserve_leading_x {
        fixed_cross_axis_capture_bounds(
            visible_bounds.y,
            visible_bounds.height,
            clip.map(|clip| clip.y),
            clip.map(|clip| clip.height),
        )
    } else {
        (visible_bounds.y, visible_bounds.y + visible_bounds.height)
    };

    Rect {
        x: left,
        y: top,
        width: (right - left).max(0.0),
        height: (bottom - top).max(0.0),
    }
}

fn translated_local_capture_bounds(
    visible_bounds: Rect,
    full_bounds: Rect,
    clip: Option<Rect>,
    stabilize_x: bool,
    stabilize_y: bool,
) -> Rect {
    let stable_guard_x = TRANSLATED_LOCAL_CAPTURE_STABLE_GUARD.min(visible_bounds.width);
    let stable_guard_y = TRANSLATED_LOCAL_CAPTURE_STABLE_GUARD.min(visible_bounds.height);
    let (left, right) = if stabilize_x {
        let (start, extent) = stable_axis_reference(
            visible_bounds.x,
            visible_bounds.width,
            clip.map(|clip| clip.x),
            clip.map(|clip| clip.width),
        );
        if full_bounds.x < start - NORMALIZED_SCENE_AFFINE_TOLERANCE {
            stable_deep_leading_axis_with_guard(start, extent, full_bounds.x, stable_guard_x)
        } else {
            (start, start + extent)
        }
    } else if stabilize_y {
        fixed_cross_axis_capture_bounds(
            visible_bounds.x,
            visible_bounds.width,
            clip.map(|clip| clip.x),
            clip.map(|clip| clip.width),
        )
    } else {
        (visible_bounds.x, visible_bounds.x + visible_bounds.width)
    };
    let (top, bottom) = if stabilize_y {
        let (start, extent) = stable_axis_reference(
            visible_bounds.y,
            visible_bounds.height,
            clip.map(|clip| clip.y),
            clip.map(|clip| clip.height),
        );
        if full_bounds.y < start - NORMALIZED_SCENE_AFFINE_TOLERANCE {
            stable_deep_leading_axis_with_guard(start, extent, full_bounds.y, stable_guard_y)
        } else {
            (start, start + extent)
        }
    } else if stabilize_x {
        fixed_cross_axis_capture_bounds(
            visible_bounds.y,
            visible_bounds.height,
            clip.map(|clip| clip.y),
            clip.map(|clip| clip.height),
        )
    } else {
        (visible_bounds.y, visible_bounds.y + visible_bounds.height)
    };

    Rect {
        x: left,
        y: top,
        width: (right - left).max(0.0),
        height: (bottom - top).max(0.0),
    }
}

fn combined_capture_clip(layer_clip: Option<Rect>, capture_clip: Option<Rect>) -> Option<Rect> {
    match (layer_clip, capture_clip) {
        (Some(layer_clip), Some(capture_clip)) => layer_clip.intersect(capture_clip),
        (Some(clip), None) | (None, Some(clip)) => Some(clip),
        (None, None) => None,
    }
}

pub(crate) fn motion_stable_capture_bounds(
    layer: &LayerNode,
    scene: &CompositorScene,
    child_layers: &[ChildLayerComposite<'_>],
    requirements: SurfaceRequirementSet,
    translated_content_axes: TranslatedContentAxes,
    capture_clip_override: Option<Rect>,
) -> Option<Rect> {
    let capture_clip = combined_capture_clip(layer.clip_rect(), capture_clip_override);
    let visible_bounds = collected_layer_bounds(scene, child_layers, true)
        .and_then(|bounds| visible_draw_rect(bounds, capture_clip));
    if !requirements.contains(SurfaceRequirement::MotionStableCapture) || layer.backdrop().is_some()
    {
        return visible_bounds;
    }

    let full_bounds = collected_layer_bounds(scene, child_layers, false);
    match (visible_bounds, full_bounds) {
        (Some(visible_bounds), Some(full_bounds))
            if hidden_content_precedes_visible_bounds(visible_bounds, full_bounds) =>
        {
            let translated_content_axes =
                translated_content_axes.union(translated_content_axes_for_layer(layer));
            let preserve_leading_x = translated_content_axes.x;
            let preserve_leading_y = translated_content_axes.y;
            if preserve_leading_x || preserve_leading_y {
                Some(translated_capture_bounds(
                    visible_bounds,
                    full_bounds,
                    capture_clip,
                    preserve_leading_x,
                    preserve_leading_y,
                ))
            } else {
                Some(leading_capture_bounds(visible_bounds, full_bounds))
            }
        }
        _ => visible_bounds.or(full_bounds),
    }
}

fn graphics_layer_supports_rigid_snap(layer: &GraphicsLayer) -> bool {
    (layer.scale - 1.0).abs() <= NORMALIZED_SCENE_AFFINE_TOLERANCE
        && (layer.scale_x - 1.0).abs() <= NORMALIZED_SCENE_AFFINE_TOLERANCE
        && (layer.scale_y - 1.0).abs() <= NORMALIZED_SCENE_AFFINE_TOLERANCE
        && layer.rotation_x.abs() <= NORMALIZED_SCENE_AFFINE_TOLERANCE
        && layer.rotation_y.abs() <= NORMALIZED_SCENE_AFFINE_TOLERANCE
        && layer.rotation_z.abs() <= NORMALIZED_SCENE_AFFINE_TOLERANCE
}

fn rigid_snap_anchor(layer_bounds: Rect, layer: &GraphicsLayer) -> Option<SnapAnchor> {
    if !graphics_layer_supports_rigid_snap(layer) {
        return None;
    }
    let mapped = cranpose_render_common::layer_transform::apply_layer_affine_to_rect(
        layer_bounds,
        layer_bounds,
        layer,
    );
    Some(SnapAnchor::rigid(Point::new(mapped.x, mapped.y)))
}

fn surface_composite_needs_rigid_snap(
    requirements: LayerSurfaceRequirements,
    translated_content_context: bool,
    surface_capture_active: bool,
) -> bool {
    composite_sample_mode_for_requirements(
        translated_content_context,
        surface_capture_active,
        requirements,
    ) == CompositeSampleMode::Box4
}

#[derive(Clone, Copy)]
struct SceneCounts {
    shapes: usize,
    images: usize,
    texts: usize,
    shadow_draws: usize,
    effect_layers: usize,
}

fn scene_counts(scene: &CompositorScene) -> SceneCounts {
    SceneCounts {
        shapes: scene.shapes.len(),
        images: scene.images.len(),
        texts: scene.texts.len(),
        shadow_draws: scene.shadow_draws.len(),
        effect_layers: scene.effect_layers.len(),
    }
}

fn assign_snap_anchor_since(
    scene: &mut CompositorScene,
    counts: SceneCounts,
    snap_anchor: Option<SnapAnchor>,
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
    for shadow in &mut scene.shadow_draws[counts.shadow_draws..] {
        for (shape, _) in &mut shadow.shapes {
            shape.snap_anchor = Some(snap_anchor);
        }
        for text in &mut shadow.texts {
            text.snap_anchor = Some(snap_anchor);
        }
    }
    for layer in &mut scene.effect_layers[counts.effect_layers..] {
        layer.snap_anchor = Some(snap_anchor);
    }
}

fn mark_translated_text_since(
    scene: &mut CompositorScene,
    counts: SceneCounts,
    translated_content_context: bool,
) {
    if !translated_content_context {
        return;
    }

    for text in &mut scene.texts[counts.texts..] {
        text.translated_content_context = true;
    }
}

fn mark_motion_stable_effect_layers_since(
    scene: &mut CompositorScene,
    counts: SceneCounts,
    translated_content_context: bool,
) {
    if !translated_content_context {
        return;
    }

    for layer in &mut scene.effect_layers[counts.effect_layers..] {
        layer
            .requirements
            .insert(SurfaceRequirement::MotionStableCapture);
    }
}

#[derive(Clone, Copy)]
struct TranslatedLocalPictureState {
    counts: SceneEmissionCounts,
    z_start: usize,
    stabilize_x: bool,
    stabilize_y: bool,
}

fn flush_translated_local_picture(
    scene: &mut CompositorScene,
    state: &mut Option<TranslatedLocalPictureState>,
    clip: Option<Rect>,
    snap_anchor: Option<SnapAnchor>,
) {
    let Some(current) = *state else {
        return;
    };
    let z_end = scene.next_z;
    if z_end > current.z_start {
        if let Some(surface_rect) = emitted_scene_bounds(scene, current.counts) {
            let surface_rect = match visible_draw_rect(surface_rect, clip) {
                Some(visible_rect) => translated_local_capture_bounds(
                    visible_rect,
                    surface_rect,
                    clip,
                    current.stabilize_x,
                    current.stabilize_y,
                ),
                None => return,
            };
            scene.push_effect_layer_with_requirements(
                surface_rect,
                clip,
                None,
                cranpose_ui_graphics::BlendMode::SrcOver,
                1.0,
                current.z_start,
                z_end,
                SurfaceRequirementSet::default().with(SurfaceRequirement::MotionStableCapture),
            );
            if let Some(layer) = scene.effect_layers.last_mut() {
                layer.snap_anchor = snap_anchor;
            }
        }
    }
    *state = Some(TranslatedLocalPictureState {
        counts: scene_emission_counts(scene),
        z_start: scene.next_z,
        stabilize_x: current.stabilize_x,
        stabilize_y: current.stabilize_y,
    });
}

pub(crate) fn resolved_child_surface_composite(
    child: &ChildLayerComposite<'_>,
) -> ResolvedChildSurfaceComposite {
    ResolvedChildSurfaceComposite {
        logical_rect: child.logical_rect,
        dest_quad: child.dest_quad,
        snap_anchor: child.snap_anchor,
        backdrop_rect: child.backdrop_rect,
        surface_clip: child.surface_clip,
        shadow_draws: child.shadow_draws.clone(),
    }
}

struct LocalPrimitiveContext<'a> {
    layer_bounds: Rect,
    local_layer: &'a GraphicsLayer,
    visual_clip: Option<Rect>,
    motion_context_animated: bool,
    content_offset_translation: bool,
    translated_text_motion: bool,
    draw_snap_anchor: Option<SnapAnchor>,
    text_snap_anchor: Option<SnapAnchor>,
}

use crate::run_entry::ShapeRunEntry;

/// Runs the shared per-variant emit math for one run entry, returning the
/// shape params instead of touching the scene. `None` means the draw resolved
/// away (fully clipped or degenerate), exactly the cases where the
/// per-primitive path emits nothing.
fn emit_shape_run_entry(
    entry: &ShapeRunEntry<'_>,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    visual_clip: Option<Rect>,
    motion_context_animated: bool,
) -> Option<ShapeDrawParams> {
    let clip = resolve_primitive_clip(
        entry.clip,
        layer_bounds,
        layer,
        visual_clip,
        PrimitiveClipSpace::Local,
    );
    if entry.clip.is_some() && clip.is_none() {
        return None;
    }
    match entry.primitive() {
        DrawPrimitive::Rect {
            rect,
            brush,
            stroke,
        } => rect_shape_params(
            *rect,
            brush,
            *stroke,
            layer_bounds,
            layer,
            clip,
            entry.blend_mode,
            motion_context_animated,
        ),
        DrawPrimitive::RoundRect {
            rect,
            brush,
            radii,
            stroke,
        } => round_rect_shape_params(
            *rect,
            brush,
            *radii,
            *stroke,
            layer_bounds,
            layer,
            clip,
            entry.blend_mode,
            motion_context_animated,
        ),
        DrawPrimitive::Arc {
            rect,
            brush,
            center,
            radius,
            start_angle,
            sweep_angle,
            stroke,
            inner_radius,
        } => arc_shape_params(
            *rect,
            brush,
            *center,
            *radius,
            *start_angle,
            *sweep_angle,
            *stroke,
            *inner_radius,
            layer_bounds,
            layer,
            clip,
            entry.blend_mode,
            motion_context_animated,
        ),
        // ShapeRunEntry::new admits no other variant.
        _ => unreachable!("shape run entries only hold shape primitives"),
    }
}

/// Whether a large flush fans out is decided by measurement — see
/// [`crate::cost_tuner::CostTuner`]. 2048 entries is the floor; a serial
/// run projected under 2 ms is never worth the spawn wave.
#[cfg(not(target_arch = "wasm32"))]
static SHAPE_RUN_TUNER: crate::cost_tuner::CostTuner =
    crate::cost_tuner::CostTuner::new("shape-emit", 2048, 2_000_000);

#[cfg(test)]
static FORCE_SHAPE_RUN_PARALLEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Test-only override that routes every flush through the parallel path,
/// bypassing the size gate — the equivalence test uses it to exercise the
/// fan-out on a scene small enough to assert against exactly.
#[cfg(test)]
pub(crate) fn force_shape_run_parallel_for_tests(on: bool) {
    FORCE_SHAPE_RUN_PARALLEL.store(on, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    /// Slot buffer for the parallel emit path, kept across frames so a 15k
    /// run does not remap megabytes of scratch every flush.
    static SHAPE_RUN_SCRATCH: std::cell::RefCell<Vec<Option<ShapeDrawParams>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Emits an accumulated run of consecutive shape draws into the scene.
///
/// Emission order — and therefore z order — matches the per-primitive path
/// exactly. Params go from the builder straight into the scene, skipping the
/// per-item scene bookkeeping `push_local_primitive` pays, and the shared
/// snap anchor is applied once at the end.
///
/// On native targets a large run may fan the emit math out across scoped
/// threads: workers write `Option<ShapeDrawParams>` into disjoint slots of a
/// reused scratch buffer and the main thread pushes the results in entry
/// order, so z order and snap anchoring are byte-identical to the serial
/// path. Whether the fan-out actually runs is decided by measured cost — see
/// [`crate::cost_tuner::CostTuner`] — because the same spawn wave that pays for itself
/// many times over on a watch-class in-order core measurably loses on a big
/// phone core.
fn flush_shape_run(
    local_scene: &mut CompositorScene,
    run: &mut Vec<ShapeRunEntry<'_>>,
    context: &LocalPrimitiveContext<'_>,
) {
    if run.is_empty() {
        return;
    }
    let counts_before = scene_counts(local_scene);
    let motion = context.motion_context_animated || context.content_offset_translation;

    #[cfg(not(target_arch = "wasm32"))]
    {
        if try_shape_replay(local_scene, run, context, motion) {
            // The replay driver emitted the whole run itself — retained ops
            // for stable segments, ordinary draws for the rest. Replay
            // engages only for anchor-free contexts, so the skipped
            // snap-anchor pass below had nothing to do anyway.
            return;
        }
        let entries = run.len();
        #[allow(unused_mut)]
        let mut parallel = SHAPE_RUN_TUNER.choose_parallel(entries)
            && crate::render::shape_convert_worker_count() > 1;
        #[cfg(test)]
        {
            parallel |= FORCE_SHAPE_RUN_PARALLEL.load(std::sync::atomic::Ordering::Relaxed);
        }
        let started = web_time::Instant::now();
        if parallel {
            flush_shape_run_parallel(local_scene, run, context, motion);
        } else {
            flush_shape_run_serial(local_scene, run, context, motion);
        }
        SHAPE_RUN_TUNER.record(parallel, entries, started.elapsed().as_nanos() as u64);
    }
    #[cfg(target_arch = "wasm32")]
    flush_shape_run_serial(local_scene, run, context, motion);

    // The whole run shares one layer context, so applying the anchor once at
    // the end lands on exactly the shapes the per-primitive path would have
    // anchored one at a time.
    assign_snap_anchor_since(local_scene, counts_before, context.draw_snap_anchor);
}

fn flush_shape_run_serial(
    local_scene: &mut CompositorScene,
    run: &mut Vec<ShapeRunEntry<'_>>,
    context: &LocalPrimitiveContext<'_>,
    motion: bool,
) {
    for entry in run.drain(..) {
        if let Some(params) = emit_shape_run_entry(
            &entry,
            context.layer_bounds,
            context.local_layer,
            context.visual_clip,
            motion,
        ) {
            push_shape_params(local_scene, params);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn flush_shape_run_parallel(
    local_scene: &mut CompositorScene,
    run: &mut Vec<ShapeRunEntry<'_>>,
    context: &LocalPrimitiveContext<'_>,
    motion: bool,
) {
    let layer_bounds = context.layer_bounds;
    let layer = context.local_layer;
    let visual_clip = context.visual_clip;
    SHAPE_RUN_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        crate::worker_pool::frame_pool().map_fill(run, &mut scratch, |entry| {
            emit_shape_run_entry(entry, layer_bounds, layer, visual_clip, motion)
        });
        for slot in scratch.iter_mut() {
            if let Some(params) = slot.take() {
                push_shape_params(local_scene, params);
            }
        }
    });
    run.clear();
}

#[cfg(not(target_arch = "wasm32"))]
use crate::shape_replay::{
    anchor_compatible, anchor_transform, anchor_transform_pinned, context_fingerprint,
    entries_bounds, is_circle, layer_supports_replay, match_entry, rect_contains, stroke_width,
    transforms_group, EntryMatch, PendingBrushPatch, PendingCapture, ReplayPhase, ReplaySegment,
    SegmentTransform, ShapeReplayState, SnapshotEntry, SnapshotShape, ANCHOR_PROBE_ENTRIES,
    MAX_RETAINED_OPS, MAX_SEGMENT_ENTRIES, MIN_REPLAY_RUN_ENTRIES, MIN_SEGMENT_ENTRIES,
    RESYNC_WINDOW, SHAPE_REPLAY,
};

/// The replay-relevant view of one run entry: its similarity-checkable
/// geometry and its (borrowed) brush. `None` marks shapes replay cannot
/// carry — plain rects, non-circular round-rects, per-draw clips, non-SrcOver
/// blends — which break segments wherever they sit.
#[cfg(not(target_arch = "wasm32"))]
fn replay_entry_view<'a>(entry: &ShapeRunEntry<'a>) -> Option<(SnapshotShape, &'a Brush)> {
    if entry.clip.is_some() || entry.blend_mode != BlendMode::SrcOver {
        return None;
    }
    match entry.primitive() {
        DrawPrimitive::Arc {
            brush,
            center,
            radius,
            start_angle,
            sweep_angle,
            stroke,
            inner_radius,
            ..
        } => Some((
            SnapshotShape::Arc {
                center: *center,
                radius: *radius,
                inner_radius: *inner_radius,
                start_angle: *start_angle,
                sweep_angle: *sweep_angle,
                stroke_width: stroke_width(stroke),
            },
            brush,
        )),
        DrawPrimitive::RoundRect {
            rect,
            brush,
            radii,
            stroke,
        } => {
            if !is_circle(*rect, *radii) {
                return None;
            }
            Some((
                SnapshotShape::Circle {
                    center: Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5),
                    diameter: rect.width,
                    stroke_width: stroke_width(stroke),
                },
                brush,
            ))
        }
        _ => None,
    }
}

/// The shared rotation/scale pivot of a run: the first arc's center. Every
/// arc must agree with it (verification enforces that); a run without arcs
/// never engages replay.
#[cfg(not(target_arch = "wasm32"))]
fn detect_center(views: &[Option<(SnapshotShape, &Brush)>]) -> Option<Point> {
    views.iter().flatten().find_map(|(shape, _)| match shape {
        SnapshotShape::Arc { center, .. } => Some(*center),
        _ => None,
    })
}

/// Serially emits a span of run entries through the ordinary per-entry path.
#[cfg(not(target_arch = "wasm32"))]
fn emit_entries_range(
    local_scene: &mut CompositorScene,
    entries: &[ShapeRunEntry<'_>],
    context: &LocalPrimitiveContext<'_>,
    motion: bool,
) {
    for entry in entries {
        if let Some(params) = emit_shape_run_entry(
            entry,
            context.layer_bounds,
            context.local_layer,
            context.visual_clip,
            motion,
        ) {
            push_shape_params(local_scene, params);
        }
    }
}

/// Consults the similarity-replay state for this run. `true` means the run
/// was fully emitted here — retained ops for replayed segments, ordinary
/// draws for everything else — and the caller must not emit it again. `false`
/// hands the run to the normal tuned path unchanged; snapshot bookkeeping may
/// still have happened on the way through.
#[cfg(not(target_arch = "wasm32"))]
fn try_shape_replay(
    local_scene: &mut CompositorScene,
    run: &mut Vec<ShapeRunEntry<'_>>,
    context: &LocalPrimitiveContext<'_>,
    motion: bool,
) -> bool {
    SHAPE_REPLAY.with(|state| {
        let mut state = state.borrow_mut();
        try_shape_replay_inner(&mut state, local_scene, run, context, motion)
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn try_shape_replay_inner(
    state: &mut ShapeReplayState,
    local_scene: &mut CompositorScene,
    run: &mut Vec<ShapeRunEntry<'_>>,
    context: &LocalPrimitiveContext<'_>,
    motion: bool,
) -> bool {
    if !state.supported || state.big_run_claimed || run.len() < MIN_REPLAY_RUN_ENTRIES {
        return false;
    }
    state.big_run_claimed = true;

    if context.draw_snap_anchor.is_some() || !layer_supports_replay(context.local_layer) {
        state.retire_all();
        return false;
    }
    let fingerprint = context_fingerprint(
        context.layer_bounds,
        context.visual_clip,
        context.local_layer.alpha,
        motion,
    );
    if state.rebuild_pending
        || (state.phase != ReplayPhase::Idle && fingerprint != state.fingerprint)
    {
        state.retire_all();
    }

    if state.phase != ReplayPhase::Active && state.frame.is_multiple_of(300) && state.frame > 0 {
        // A healthy bootstrap finishes in two frames; seeing this line at
        // all means capture keeps failing, and silence would hide it.
        log::warn!(
            "[similarity-replay] bootstrap stalled at frame {}: phase {:?}, run {} entries, snapshot {}",
            state.frame,
            state.phase,
            run.len(),
            state.snapshot.len(),
        );
    }

    match state.phase {
        ReplayPhase::Idle => {
            take_run_snapshot(state, run, fingerprint);
            false
        }
        ReplayPhase::Snapshotted => partition_and_capture(state, local_scene, run, context, motion),
        ReplayPhase::Active => replay_run(state, local_scene, run, context, motion),
    }
}

/// Frame 0 of a replay cycle: clone the run so the next frame's diff can
/// find what moves together. Stays `Idle` when the run has no arc to pin the
/// pivot.
#[cfg(not(target_arch = "wasm32"))]
fn take_run_snapshot(state: &mut ShapeReplayState, run: &[ShapeRunEntry<'_>], fingerprint: u64) {
    let views: Vec<Option<(SnapshotShape, &Brush)>> = run.iter().map(replay_entry_view).collect();
    let Some(center) = detect_center(&views) else {
        return;
    };
    state.snapshot = views
        .iter()
        .map(|view| {
            view.as_ref().map(|(shape, brush)| SnapshotEntry {
                shape: *shape,
                brush: (*brush).clone(),
            })
        })
        .collect();
    state.center = center;
    state.fingerprint = fingerprint;
    state.phase = ReplayPhase::Snapshotted;
}

/// Pairs current-run entries with snapshot entries, tolerating bounded
/// insertions and deletions. At 60 fps consecutive frames usually carry the
/// same entry list, but a slow device steps the simulation far enough per
/// frame that entities spawn and die between EVERY frame pair — a strict
/// equal-length lockstep never bootstraps exactly where replay is needed
/// most. Pairing is structural only; the partition's transform-consistency
/// check is what decides whether a pair actually moved together, so a wrong
/// pairing costs a chain, never a wrong capture.
#[cfg(not(target_arch = "wasm32"))]
fn align_snapshot(
    snapshot: &[Option<SnapshotEntry>],
    views: &[Option<(SnapshotShape, &Brush)>],
) -> Vec<Option<usize>> {
    const RESYNC_SPAN: usize = 48;
    const MAX_RESYNC_EVENTS: usize = 512;
    let pair = |i: usize, j: usize| -> bool {
        match (&views[i], &snapshot[j]) {
            (Some((shape, _)), Some(snap)) => anchor_compatible(shape, &snap.shape),
            (None, None) => true,
            _ => false,
        }
    };
    let mut aligned = vec![None; views.len()];
    let (mut i, mut j) = (0usize, 0usize);
    let mut events = 0usize;
    while i < views.len() && j < snapshot.len() {
        if pair(i, j) {
            aligned[i] = Some(j);
            i += 1;
            j += 1;
            continue;
        }
        events += 1;
        if events > MAX_RESYNC_EVENTS {
            // This is not churn, the structure is gone; an empty alignment
            // makes the caller restart from a fresh snapshot.
            return vec![None; views.len()];
        }
        let mut resynced = false;
        'search: for total in 1..=RESYNC_SPAN {
            for di in 0..=total {
                let dj = total - di;
                if i + di < views.len() && j + dj < snapshot.len() && pair(i + di, j + dj) {
                    i += di;
                    j += dj;
                    resynced = true;
                    break 'search;
                }
            }
        }
        if !resynced {
            i += 1;
            j += 1;
        }
    }
    aligned
}

/// Splits the run into maximal chains of consecutive entries that moved from
/// the snapshot by one shared similarity transform. Chains shorter than
/// [`MIN_SEGMENT_ENTRIES`] stay dynamic.
#[cfg(not(target_arch = "wasm32"))]
fn partition_chains(
    snapshot: &[Option<SnapshotEntry>],
    aligned: &[Option<usize>],
    views: &[Option<(SnapshotShape, &Brush)>],
    center: Point,
) -> Vec<(usize, usize)> {
    let mut chains = Vec::new();
    let mut i = 0;
    while i < views.len() {
        let (Some(snap), Some((current, brush))) =
            (aligned[i].and_then(|j| snapshot[j].as_ref()), &views[i])
        else {
            i += 1;
            continue;
        };
        // A chain anchor must pin rotation itself: an on-pivot circle as
        // anchor would freeze the chain's angle at zero and fail rotating
        // content behind it.
        let Some((t, true)) = anchor_transform_pinned(current, &snap.shape, center) else {
            i += 1;
            continue;
        };
        if matches!(
            match_entry(current, brush, snap, center, t),
            EntryMatch::Mismatch
        ) {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        while end < views.len() {
            let (Some(snap), Some((current, brush))) =
                (aligned[end].and_then(|j| snapshot[j].as_ref()), &views[end])
            else {
                break;
            };
            // Grouping is strict where verification is lenient: the entry's
            // own implied transform must sit on the anchor's, or the pair
            // belongs to differently-moving rings whose divergence from the
            // capture only grows.
            let Some((entry_t, pinned)) = anchor_transform_pinned(current, &snap.shape, center)
            else {
                break;
            };
            if !transforms_group(entry_t, pinned, t) {
                break;
            }
            if matches!(
                match_entry(current, brush, snap, center, t),
                EntryMatch::Mismatch
            ) {
                break;
            }
            end += 1;
        }
        if end - start >= MIN_SEGMENT_ENTRIES {
            // Long chains split into bounded captures so one entry going
            // dynamic later cannot take a five-figure span with it.
            let mut piece_start = start;
            while piece_start < end {
                let piece_end = (piece_start + MAX_SEGMENT_ENTRIES).min(end);
                if piece_end - piece_start >= MIN_SEGMENT_ENTRIES {
                    chains.push((piece_start, piece_end));
                }
                piece_start = piece_end;
            }
        }
        i = end.max(i + 1);
    }
    chains
}

/// Frame 1 of a replay cycle: diff against the snapshot, partition stable
/// chains, emit the whole run serially while recording exactly which scene
/// shapes each chain produced, and queue those ranges for GPU capture. The
/// run renders normally this frame; replay starts on the next.
#[cfg(not(target_arch = "wasm32"))]
fn partition_and_capture(
    state: &mut ShapeReplayState,
    local_scene: &mut CompositorScene,
    run: &mut Vec<ShapeRunEntry<'_>>,
    context: &LocalPrimitiveContext<'_>,
    motion: bool,
) -> bool {
    let views = build_replay_views(run);
    let aligned = align_snapshot(&state.snapshot, &views);
    let chains = partition_chains(&state.snapshot, &aligned, &views, state.center);
    if chains.is_empty() {
        let fingerprint = state.fingerprint;
        state.phase = ReplayPhase::Idle;
        state.snapshot.clear();
        take_run_snapshot(state, run, fingerprint);
        return false;
    }

    struct ChainTrack {
        start_entry: usize,
        end_entry: usize,
        shape_start: usize,
        bounds: Option<Rect>,
        alive: bool,
    }
    let capture_clip = context.visual_clip;
    let mut tracks: Vec<ChainTrack> = Vec::with_capacity(chains.len());
    let mut active: Option<ChainTrack> = None;
    let mut chain_cursor = 0usize;
    for (i, entry) in run.iter().enumerate() {
        if chain_cursor < chains.len() && chains[chain_cursor].0 == i {
            active = Some(ChainTrack {
                start_entry: i,
                end_entry: chains[chain_cursor].1,
                shape_start: local_scene.shapes.len(),
                bounds: None,
                alive: true,
            });
        }
        let shapes_before = local_scene.shapes.len();
        let mut emitted_rect = None;
        if let Some(params) = emit_shape_run_entry(
            entry,
            context.layer_bounds,
            context.local_layer,
            context.visual_clip,
            motion,
        ) {
            emitted_rect = Some(params.rect);
            push_shape_params(local_scene, params);
        }
        if let Some(track) = &mut active {
            if track.alive {
                // A replayable chain entry must map to exactly one scene
                // shape that its ambient clip does not cut — the retained
                // batch replays shapes whole or not at all.
                let one_shape = local_scene.shapes.len() == shapes_before + 1;
                let clip_ok = match (capture_clip, emitted_rect) {
                    (Some(clip), Some(rect)) => rect_contains(clip, rect),
                    (None, Some(_)) => true,
                    (_, None) => false,
                };
                if one_shape && clip_ok {
                    let rect = emitted_rect.expect("checked above");
                    track.bounds = union_rect(track.bounds, rect);
                } else {
                    track.alive = false;
                }
            }
            if i + 1 == track.end_entry {
                tracks.push(active.take().expect("active chain track"));
                chain_cursor += 1;
            }
        }
    }

    state.segments.clear();
    state.pending_captures.clear();
    for track in tracks {
        let Some(bounds) = track.bounds else { continue };
        if !track.alive {
            continue;
        }
        let entries: Vec<SnapshotEntry> = (track.start_entry..track.end_entry)
            .map(|i| {
                let (shape, brush) = views[i].as_ref().expect("chain entries are replayable");
                SnapshotEntry {
                    shape: *shape,
                    brush: (*brush).clone(),
                }
            })
            .collect();
        let segment = state.segments.len();
        state.segments.push(ReplaySegment {
            entries,
            bounds,
            slot: None,
            slot_offset: 0,
        });
        state.pending_captures.push(PendingCapture {
            segment,
            shape_start: track.shape_start,
            shape_count: track.end_entry - track.start_entry,
        });
    }

    if state.segments.is_empty() {
        let fingerprint = state.fingerprint;
        state.phase = ReplayPhase::Idle;
        state.snapshot.clear();
        take_run_snapshot(state, run, fingerprint);
    } else {
        static FIRST_CAPTURE: std::sync::Once = std::sync::Once::new();
        FIRST_CAPTURE.call_once(|| {
            let retained: usize = state.segments.iter().map(|s| s.entries.len()).sum();
            log::warn!(
                "[similarity-replay] first capture: {} segments retain {} of {} entries",
                state.segments.len(),
                retained,
                run.len(),
            );
        });
        state.capture_clip = capture_clip;
        state.captured_root_scale = Some(state.root_scale);
        state.snapshot.clear();
        state.phase = ReplayPhase::Active;
    }
    run.clear();
    true
}

/// A partial verification deeper than this is trusted as the segment's true
/// anchor — the mismatch marks real changed content, not a false anchor —
/// and reported for a split instead of being scanned past.
#[cfg(not(target_arch = "wasm32"))]
const TRUSTED_PARTIAL_DEPTH: usize = 64;

#[cfg(not(target_arch = "wasm32"))]
enum LocateOutcome {
    /// Whole segment verified; `patches` are the recolored entry indices.
    Match {
        anchor: usize,
        t: SegmentTransform,
        patches: Vec<usize>,
    },
    /// The anchor is trustworthy but entry `verified` changed for real —
    /// the segment can split around it. `patches` lie within the prefix.
    Partial {
        anchor: usize,
        t: SegmentTransform,
        verified: usize,
        patches: Vec<usize>,
    },
    NotFound,
}

/// Finds a segment's anchor at or after `cursor` and verifies every entry
/// under the anchor's transform. `extra_window` widens the scan past earlier
/// missed segments, whose entries still sit in the run ahead of this
/// segment's. No cap on the patch count: recolor patches are byte-exact, so
/// even a segment recoloring every entry every frame (MEGA's twinkle field
/// does) replays correctly, and patch bytes cost far less than re-emitting
/// the span.
#[cfg(not(target_arch = "wasm32"))]
fn locate_and_verify(
    segment: &ReplaySegment,
    views: &[Option<(SnapshotShape, &Brush)>],
    cursor: usize,
    extra_window: usize,
    center: Point,
) -> LocateOutcome {
    let len = segment.entries.len();
    let Some(max_start) = views.len().checked_sub(len) else {
        return LocateOutcome::NotFound;
    };
    let scan_end = max_start.min(cursor.saturating_add(RESYNC_WINDOW + extra_window));
    if cursor > scan_end {
        return LocateOutcome::NotFound;
    }
    let anchor_entry = &segment.entries[0];
    for candidate in cursor..=scan_end {
        let Some((current, _)) = &views[candidate] else {
            continue;
        };
        if !anchor_compatible(current, &anchor_entry.shape) {
            continue;
        }
        let Some(t) = anchor_transform(current, &anchor_entry.shape, center) else {
            continue;
        };
        let probe = ANCHOR_PROBE_ENTRIES.min(len);
        let probe_ok = (0..probe).all(|k| {
            views[candidate + k].as_ref().is_some_and(|(shape, brush)| {
                !matches!(
                    match_entry(shape, brush, &segment.entries[k], center, t),
                    EntryMatch::Mismatch
                )
            })
        });
        if !probe_ok {
            continue;
        }
        let mut patches = Vec::new();
        let mut failed_at = None;
        for k in 0..len {
            let Some((shape, brush)) = &views[candidate + k] else {
                failed_at = Some(k);
                break;
            };
            match match_entry(shape, brush, &segment.entries[k], center, t) {
                EntryMatch::Exact => {}
                EntryMatch::Recolor => patches.push(k),
                EntryMatch::Mismatch => {
                    failed_at = Some(k);
                    break;
                }
            }
        }
        match failed_at {
            None => {
                return LocateOutcome::Match {
                    anchor: candidate,
                    t,
                    patches,
                }
            }
            Some(verified) if verified >= TRUSTED_PARTIAL_DEPTH => {
                return LocateOutcome::Partial {
                    anchor: candidate,
                    t,
                    verified,
                    patches,
                };
            }
            // A shallow failure looks like a false anchor; keep scanning.
            Some(_) => continue,
        }
    }
    LocateOutcome::NotFound
}

/// Builds the per-entry replay views, fanned out across the conversion
/// workers when the run is large: the view construction is a pure map over
/// ~17k entries, and on a watch-class core it is several milliseconds of the
/// frame all by itself.
#[cfg(not(target_arch = "wasm32"))]
fn build_replay_views<'r, 'e>(
    run: &'r [ShapeRunEntry<'e>],
) -> Vec<Option<(SnapshotShape, &'r Brush)>>
where
    'e: 'r,
{
    let mut views: Vec<Option<(SnapshotShape, &Brush)>> = Vec::new();
    crate::worker_pool::frame_pool().map_fill(run, &mut views, replay_entry_view);
    views
}

/// A segment's anchor and full-body verification result computed off the
/// render thread before the sequential replay walk consumes it.
#[cfg(not(target_arch = "wasm32"))]
struct PreVerdict {
    anchor: usize,
    t: SegmentTransform,
    patches: Vec<usize>,
    failed_at: Option<usize>,
}

/// Probe-only anchor scan: the first candidate whose leading entries match
/// under its implied transform. The expensive whole-body verification is
/// deferred so it can run in parallel across segments.
#[cfg(not(target_arch = "wasm32"))]
fn probe_anchor(
    segment: &ReplaySegment,
    views: &[Option<(SnapshotShape, &Brush)>],
    cursor: usize,
    center: Point,
) -> Option<(usize, SegmentTransform)> {
    let len = segment.entries.len();
    let max_start = views.len().checked_sub(len)?;
    let scan_end = max_start.min(cursor.saturating_add(RESYNC_WINDOW));
    if cursor > scan_end {
        return None;
    }
    let anchor_entry = &segment.entries[0];
    for candidate in cursor..=scan_end {
        let Some((current, _)) = &views[candidate] else {
            continue;
        };
        if !anchor_compatible(current, &anchor_entry.shape) {
            continue;
        }
        let Some(t) = anchor_transform(current, &anchor_entry.shape, center) else {
            continue;
        };
        let probe = ANCHOR_PROBE_ENTRIES.min(len);
        let probe_ok = (0..probe).all(|k| {
            views[candidate + k].as_ref().is_some_and(|(shape, brush)| {
                !matches!(
                    match_entry(shape, brush, &segment.entries[k], center, t),
                    EntryMatch::Mismatch
                )
            })
        });
        if probe_ok {
            return Some((candidate, t));
        }
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn verify_segment_body(
    segment: &ReplaySegment,
    views: &[Option<(SnapshotShape, &Brush)>],
    anchor: usize,
    t: SegmentTransform,
    center: Point,
) -> PreVerdict {
    let len = segment.entries.len();
    let mut patches = Vec::new();
    let mut failed_at = None;
    for k in 0..len {
        let Some((shape, brush)) = &views[anchor + k] else {
            failed_at = Some(k);
            break;
        };
        match match_entry(shape, brush, &segment.entries[k], center, t) {
            EntryMatch::Exact => {}
            EntryMatch::Recolor => patches.push(k),
            EntryMatch::Mismatch => {
                failed_at = Some(k);
                break;
            }
        }
    }
    PreVerdict {
        anchor,
        t,
        patches,
        failed_at,
    }
}

/// Locates and verifies every segment ahead of the sequential replay walk:
/// a cheap serial probe pass predicts each anchor (exact in steady state,
/// where segments sit where they sat last frame), then all body
/// verifications — the bulk of the per-frame replay cost, ~16k `match_entry`
/// calls on MEGA — run concurrently on the conversion workers. The walk
/// discards any verdict its true cursor disagrees with and falls back to the
/// sequential scan, so a wrong prediction costs time, never output.
#[cfg(not(target_arch = "wasm32"))]
fn pre_verify_segments(
    segments: &[ReplaySegment],
    views: &[Option<(SnapshotShape, &Brush)>],
    center: Point,
) -> Vec<Option<PreVerdict>> {
    let mut probes: Vec<Option<(usize, SegmentTransform)>> = Vec::with_capacity(segments.len());
    let mut cursor = 0usize;
    for segment in segments {
        let hit = probe_anchor(segment, views, cursor, center);
        if let Some((candidate, _)) = hit {
            cursor = candidate + segment.entries.len();
        }
        probes.push(hit);
    }

    let jobs: Vec<(usize, usize, SegmentTransform)> = probes
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.map(|(candidate, t)| (i, candidate, t)))
        .collect();
    let mut verdicts: Vec<Option<PreVerdict>> = Vec::new();
    verdicts.resize_with(segments.len(), || None);
    let mut job_results: Vec<Option<PreVerdict>> = Vec::new();
    job_results.resize_with(jobs.len(), || None);
    crate::worker_pool::frame_pool().map_into_min(
        &jobs,
        &mut job_results,
        2,
        |&(i, candidate, t)| Some(verify_segment_body(&segments[i], views, candidate, t, center)),
    );
    for (&(i, _, _), result) in jobs.iter().zip(job_results) {
        verdicts[i] = result;
    }
    verdicts
}

/// Turns a segment's pre-verdict into its final outcome, falling back to the
/// sequential scan whenever the prediction no longer applies.
#[cfg(not(target_arch = "wasm32"))]
fn resolve_segment_outcome(
    verdict: Option<PreVerdict>,
    segment: &ReplaySegment,
    views: &[Option<(SnapshotShape, &Brush)>],
    cursor: usize,
    extra_window: usize,
    center: Point,
) -> LocateOutcome {
    if let Some(v) = verdict {
        if v.anchor >= cursor {
            match v.failed_at {
                None => {
                    return LocateOutcome::Match {
                        anchor: v.anchor,
                        t: v.t,
                        patches: v.patches,
                    }
                }
                Some(verified) if verified >= TRUSTED_PARTIAL_DEPTH => {
                    return LocateOutcome::Partial {
                        anchor: v.anchor,
                        t: v.t,
                        verified,
                        patches: v.patches,
                    };
                }
                // A shallow failure marks a false anchor; resume the
                // sequential scan just past it, as the inline scan would.
                Some(_) => {
                    return locate_and_verify(segment, views, v.anchor + 1, extra_window, center)
                }
            }
        }
    }
    locate_and_verify(segment, views, cursor, extra_window, center)
}

/// Diagnostic post-mortem of a segment miss, for the capped miss log: where
/// the anchor scan or the verification actually gave up.
#[cfg(not(target_arch = "wasm32"))]
fn replay_miss_detail(
    segment: &ReplaySegment,
    views: &[Option<(SnapshotShape, &Brush)>],
    cursor: usize,
    center: Point,
) -> String {
    let len = segment.entries.len();
    let Some(max_start) = views.len().checked_sub(len) else {
        return format!("run shorter than segment ({} < {len})", views.len());
    };
    let scan_end = max_start.min(cursor.saturating_add(RESYNC_WINDOW));
    let anchor = &segment.entries[0];
    for candidate in cursor..=scan_end {
        let Some((current, _)) = &views[candidate] else {
            continue;
        };
        if !anchor_compatible(current, &anchor.shape) {
            continue;
        }
        let Some(t) = anchor_transform(current, &anchor.shape, center) else {
            continue;
        };
        let mut patches = 0usize;
        for k in 0..len {
            let Some((shape, brush)) = &views[candidate + k] else {
                return format!("candidate at {candidate}: entry {k} not replayable (t={t:?})");
            };
            match match_entry(shape, brush, &segment.entries[k], center, t) {
                EntryMatch::Exact => {}
                EntryMatch::Recolor => patches += 1,
                EntryMatch::Mismatch => {
                    return format!(
                        "candidate at {candidate}: entry {k} mismatched (t={t:?}, patches so far {patches}); current {shape:?} vs capture {:?}",
                        segment.entries[k].shape,
                    );
                }
            }
        }
        return format!(
            "candidate at {candidate}: diagnostic verify passed with {patches} patches of {len} — \
             the live scan must have failed differently (clip bounds?)"
        );
    }
    "no anchor candidate in window".to_string()
}

/// Queues the recolor patches of a matched prefix and updates the segment
/// snapshot so the next frame diffs against what the slot now holds.
#[cfg(not(target_arch = "wasm32"))]
fn commit_segment_patches(
    state: &mut ShapeReplayState,
    segment: &mut ReplaySegment,
    views: &[Option<(SnapshotShape, &Brush)>],
    anchor: usize,
    patches: &[usize],
    slot: u32,
    layer: &GraphicsLayer,
) {
    state.stat_patches += patches.len() as u64;
    for &k in patches {
        let (_, brush) = views[anchor + k].as_ref().expect("verified entry");
        segment.entries[k].brush = (*brush).clone();
        state.pending_patches.push(PendingBrushPatch {
            slot,
            shape_index: segment.slot_offset + k as u32,
            brush: cranpose_render_common::style_shared::apply_layer_to_brush(
                (*brush).clone(),
                layer,
            ),
        });
    }
}

/// Frame 2+ of a replay cycle: verify each captured segment against the
/// incoming run and replace it with one retained op; everything between
/// segments emits normally.
///
/// A segment whose prefix verifies but whose entry `k` genuinely changed
/// does not die: it splits around the changed entry, both halves keeping
/// their ranges of the already-captured slot. The changed entry alone goes
/// back to the normal pipeline. This is what makes the table converge on
/// scenes with stepped movers — content that sits still for a few frames
/// and then jumps — instead of cycling through full rebuilds that re-merge
/// the troublemakers every time.
#[cfg(not(target_arch = "wasm32"))]
fn replay_run(
    state: &mut ShapeReplayState,
    local_scene: &mut CompositorScene,
    run: &mut Vec<ShapeRunEntry<'_>>,
    context: &LocalPrimitiveContext<'_>,
    motion: bool,
) -> bool {
    let views = build_replay_views(run);
    let center = state.center;
    let center_final = Point::new(
        center.x + context.layer_bounds.x,
        center.y + context.layer_bounds.y,
    );
    let capture_clip = state.capture_clip;
    let root_scale = state.root_scale;
    let mut cursor = 0usize;
    // Entries of unreplayed-but-alive segments ahead of the cursor: widens
    // later anchors' scan windows, and counts as coverage the table still
    // expects to recover.
    let mut skipped_entries = 0usize;
    let mut retained_entries = 0usize;
    let mut carried_entries = 0usize;
    let old_segments = std::mem::take(&mut state.segments);
    let mut verdicts = pre_verify_segments(&old_segments, &views, center);
    for (segment_index, mut segment) in old_segments.into_iter().enumerate() {
        let len = segment.entries.len();
        let Some(slot) = segment.slot else {
            // Capture failed (slot pool exhausted); the span stays dynamic.
            skipped_entries += len;
            carried_entries += len;
            state.segments.push(segment);
            continue;
        };
        if local_scene.retained_draws.len() >= MAX_RETAINED_OPS {
            skipped_entries += len;
            carried_entries += len;
            state.segments.push(segment);
            continue;
        }
        match resolve_segment_outcome(
            verdicts[segment_index].take(),
            &segment,
            &views,
            cursor,
            skipped_entries,
            center,
        ) {
            LocateOutcome::Match { anchor, t, patches } => {
                let bounds_now = t.apply_to_bounds(center_final, segment.bounds);
                // The retained batch's baked clip moves with its shapes, so
                // it only stands in for the real, screen-fixed clip while
                // the whole segment stays inside it.
                if capture_clip.is_some_and(|clip| !rect_contains(clip, bounds_now)) {
                    skipped_entries += len;
                    state.drop_slot_ref(slot);
                    continue;
                }
                emit_entries_range(local_scene, &run[cursor..anchor], context, motion);
                commit_segment_patches(
                    state,
                    &mut segment,
                    &views,
                    anchor,
                    &patches,
                    slot,
                    context.local_layer,
                );
                local_scene.push_retained_draw(crate::scene::RetainedDraw {
                    slot,
                    transform: t.to_similarity(center_final, root_scale),
                    bounds: bounds_now,
                    first_shape: segment.slot_offset,
                    shape_count: len as u32,
                });
                retained_entries += len;
                cursor = anchor + len;
                skipped_entries = 0;
                state.segments.push(segment);
            }
            LocateOutcome::Partial {
                anchor,
                t,
                verified,
                patches,
            } => {
                state.stat_splits += 1;
                let b_len = len - verified - 1;
                let a_keep = verified >= MIN_SEGMENT_ENTRIES;
                let b_keep = b_len >= MIN_SEGMENT_ENTRIES;
                let parent_offset = segment.slot_offset;
                let mut b_entries = segment.entries.split_off(verified + 1);
                segment.entries.pop();
                match (a_keep, b_keep) {
                    (true, true) => {
                        // Both halves stay claimants of the shared slot.
                        *state.slot_refs.entry(slot).or_insert(1) += 1;
                    }
                    (false, false) => state.drop_slot_ref(slot),
                    _ => {}
                }
                if a_keep {
                    segment.bounds = entries_bounds(&segment.entries)
                        .translate(context.layer_bounds.x, context.layer_bounds.y);
                    let bounds_now = t.apply_to_bounds(center_final, segment.bounds);
                    if capture_clip.is_some_and(|clip| !rect_contains(clip, bounds_now)) {
                        state.drop_slot_ref(slot);
                        skipped_entries += verified;
                    } else {
                        emit_entries_range(local_scene, &run[cursor..anchor], context, motion);
                        commit_segment_patches(
                            state,
                            &mut segment,
                            &views,
                            anchor,
                            &patches,
                            slot,
                            context.local_layer,
                        );
                        local_scene.push_retained_draw(crate::scene::RetainedDraw {
                            slot,
                            transform: t.to_similarity(center_final, root_scale),
                            bounds: bounds_now,
                            first_shape: segment.slot_offset,
                            shape_count: verified as u32,
                        });
                        retained_entries += verified;
                        cursor = anchor + verified;
                        skipped_entries = 0;
                        state.segments.push(segment);
                    }
                } else {
                    skipped_entries += verified;
                }
                // The changed entry itself goes dynamic for good.
                skipped_entries += 1;
                if b_keep {
                    let bounds_b = entries_bounds(&b_entries)
                        .translate(context.layer_bounds.x, context.layer_bounds.y);
                    skipped_entries += b_entries.len();
                    carried_entries += b_entries.len();
                    state.segments.push(ReplaySegment {
                        entries: std::mem::take(&mut b_entries),
                        bounds: bounds_b,
                        slot: Some(slot),
                        // Offsets are capture-relative, so the sibling
                        // starts right past the excised entry.
                        slot_offset: parent_offset + verified as u32 + 1,
                    });
                } else {
                    skipped_entries += b_len;
                }
            }
            LocateOutcome::NotFound => {
                state.stat_deaths += 1;
                static MISS_LOGS: std::sync::atomic::AtomicU32 =
                    std::sync::atomic::AtomicU32::new(0);
                if MISS_LOGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 12 {
                    log::warn!(
                        "[similarity-replay] segment died len={} cursor={} views={} {}",
                        len,
                        cursor,
                        views.len(),
                        replay_miss_detail(&segment, &views, cursor, center),
                    );
                }
                skipped_entries += len;
                state.drop_slot_ref(slot);
            }
        }
    }
    emit_entries_range(local_scene, &run[cursor..], context, motion);
    let covered = retained_entries + carried_entries;
    if (covered as f32) < crate::shape_replay::MIN_COVERAGE_FRACTION * run.len() as f32
        && state.frame.wrapping_sub(state.last_rebuild_frame)
            >= crate::shape_replay::REBUILD_COOLDOWN_FRAMES
    {
        state.rebuild_pending = true;
        state.last_rebuild_frame = state.frame;
    }
    static FIRST_REPLAY: std::sync::Once = std::sync::Once::new();
    if retained_entries > 0 {
        FIRST_REPLAY.call_once(|| {
            log::warn!(
                "[similarity-replay] first replayed frame: {} retained ops cover {} of {} entries",
                local_scene.retained_draws.len(),
                retained_entries,
                run.len(),
            );
        });
    }
    if state.frame.is_multiple_of(300) {
        log::warn!(
            "[similarity-replay] frame {}: {} ops retain {}/{} entries ({} segments); lifetime deaths {} splits {} patches {}",
            state.frame,
            local_scene.retained_draws.len(),
            retained_entries,
            run.len(),
            state.segments.len(),
            state.stat_deaths,
            state.stat_splits,
            state.stat_patches,
        );
    }
    run.clear();
    true
}

/// Feeds one [`DrawRunNode`]'s primitives through the shape run, spilling any
/// non-shape draw through the ordinary emit path at its exact z position.
fn collect_draw_run<'a>(
    local_scene: &mut CompositorScene,
    run: &'a DrawRunNode,
    shape_run: &mut Vec<ShapeRunEntry<'a>>,
    context: &LocalPrimitiveContext<'_>,
) {
    #[cfg(not(target_arch = "wasm32"))]
    if let (Some(frame), Some(command)) = (run.replay.as_deref(), run.command) {
        if try_command_feed(local_scene, run, frame, command, shape_run, context) {
            return;
        }
    }
    for primitive in run.primitives.iter() {
        if let Some(entry) = ShapeRunEntry::new(primitive, None) {
            shape_run.push(entry);
            continue;
        }
        flush_shape_run(local_scene, shape_run, context);
        push_loose_draw_primitive(local_scene, primitive, context);
    }
}

/// Draws a run from its verified replay frame: retained spans as
/// [`crate::scene::RetainedDraw`]s referencing identity-keyed slots, dynamic
/// spans through the ordinary emit path, in exact span order. `true` means
/// the run was fully emitted here. `false` leaves the run to the ordinary
/// path — the feed is opt-in, unsupported contexts fall back wholesale, and
/// a fallen-back frame costs exactly one ordinarily-drawn frame.
#[cfg(not(target_arch = "wasm32"))]
fn try_command_feed<'a>(
    local_scene: &mut CompositorScene,
    run: &'a DrawRunNode,
    frame: &cranpose_ui_graphics::CommandReplayFrame,
    command: cranpose_render_common::graph::DrawCommandId,
    shape_run: &mut Vec<ShapeRunEntry<'a>>,
    context: &LocalPrimitiveContext<'_>,
) -> bool {
    use crate::shape_replay::{command_feed_enabled, PendingBrushPatch, PendingFeedCapture};
    if !command_feed_enabled() {
        return false;
    }
    let supported = SHAPE_REPLAY.with(|state| state.borrow().supported);
    if !supported
        || context.draw_snap_anchor.is_some()
        || !layer_supports_replay(context.local_layer)
    {
        return false;
    }
    // The feed owns retention for scenes it serves; a live flat-detector
    // table would be a second retention system fighting over slots.
    SHAPE_REPLAY.with(|state| {
        let mut state = state.borrow_mut();
        if state.phase != ReplayPhase::Idle {
            state.retire_all();
        }
    });
    let motion = context.motion_context_animated || context.content_offset_translation;
    let fingerprint = context_fingerprint(
        context.layer_bounds,
        context.visual_clip,
        context.local_layer.alpha,
        motion,
    );
    // Entries accumulated from earlier nodes flush first: span emission
    // below must land after them to keep z order exact.
    flush_shape_run(local_scene, shape_run, context);
    let center_final = Point::new(
        frame.center.x + context.layer_bounds.x,
        frame.center.y + context.layer_bounds.y,
    );
    let (root_scale, frame_now) =
        SHAPE_REPLAY.with(|state| (state.borrow().root_scale, state.borrow().frame));
    for span in &frame.spans {
        match span {
            cranpose_ui_graphics::FrameSpan::Dynamic { range } => {
                emit_feed_range(
                    local_scene,
                    &run.primitives[range.0 as usize..range.1 as usize],
                    context,
                    motion,
                );
            }
            cranpose_ui_graphics::FrameSpan::Retained {
                slot,
                capture: true,
                range,
                ..
            } => {
                // The capture frame: emit ordinarily, and when every record
                // produced exactly one uncut shape, ask the renderer to
                // retain the shape range under this span's identity.
                let shape_start = local_scene.shapes.len();
                let primitives = &run.primitives[range.0 as usize..range.1 as usize];
                let mut clean = !primitives.is_empty();
                for primitive in primitives {
                    let before = local_scene.shapes.len();
                    let mut emitted_rect = None;
                    if let Some(entry) = ShapeRunEntry::new(primitive, None) {
                        if let Some(params) = emit_shape_run_entry(
                            &entry,
                            context.layer_bounds,
                            context.local_layer,
                            context.visual_clip,
                            motion,
                        ) {
                            emitted_rect = Some(params.rect);
                            push_shape_params(local_scene, params);
                        }
                    } else {
                        push_loose_draw_primitive(local_scene, primitive, context);
                    }
                    let one_shape = local_scene.shapes.len() == before + 1;
                    let clip_ok = match (context.visual_clip, emitted_rect) {
                        (Some(clip), Some(rect)) => rect_contains(clip, rect),
                        (None, Some(_)) => true,
                        (_, None) => false,
                    };
                    if !(one_shape && clip_ok) {
                        clean = false;
                    }
                }
                if clean {
                    SHAPE_REPLAY.with(|state| {
                        state
                            .borrow_mut()
                            .pending_feed_captures
                            .push(PendingFeedCapture {
                                key: (command, *slot),
                                shape_start,
                                shape_count: primitives.len(),
                                fingerprint,
                                capture_clip: context.visual_clip,
                            });
                    });
                }
            }
            cranpose_ui_graphics::FrameSpan::Retained {
                slot,
                capture: false,
                slot_offset,
                range,
                transform,
                recolors,
                bounds,
            } => {
                let len = (range.1 - range.0) as usize;
                let bounds_now = Rect {
                    x: bounds.x + context.layer_bounds.x,
                    y: bounds.y + context.layer_bounds.y,
                    width: bounds.width,
                    height: bounds.height,
                };
                let emitted = local_scene.retained_draws.len() < MAX_RETAINED_OPS
                    && SHAPE_REPLAY.with(|state| {
                        let mut state = state.borrow_mut();
                        let Some(feed_slot) = state.feed_slots.get_mut(&(command, *slot))
                        else {
                            return None;
                        };
                        if feed_slot.fingerprint != fingerprint {
                            return None;
                        }
                        if feed_slot
                            .capture_clip
                            .is_some_and(|clip| !rect_contains(clip, bounds_now))
                        {
                            return None;
                        }
                        feed_slot.last_referenced = frame_now;
                        let gpu_slot = feed_slot.gpu_slot;
                        for (offset, color) in recolors {
                            state.pending_patches.push(PendingBrushPatch {
                                slot: gpu_slot,
                                shape_index: *slot_offset + offset,
                                brush: Brush::Solid(*color),
                            });
                        }
                        state.stat_patches += recolors.len() as u64;
                        Some(gpu_slot)
                    })
                    .map(|gpu_slot| {
                        local_scene.push_retained_draw(crate::scene::RetainedDraw {
                            slot: gpu_slot,
                            transform: SegmentTransform {
                                scale: transform.scale,
                                angle: transform.angle,
                            }
                            .to_similarity(center_final, root_scale),
                            bounds: bounds_now,
                            first_shape: *slot_offset,
                            shape_count: len as u32,
                        });
                    })
                    .is_some();
                if !emitted {
                    emit_feed_range(
                        local_scene,
                        &run.primitives[range.0 as usize..range.1 as usize],
                        context,
                        motion,
                    );
                }
            }
        }
    }
    true
}

/// The ordinary path for one span of a fed run: shape entries convert in
/// place, non-shape primitives spill loose, exactly as the unfed run loop
/// does. Kept away from [`flush_shape_run`] so span emission can never
/// re-enter the flat detector.
#[cfg(not(target_arch = "wasm32"))]
fn emit_feed_range(
    local_scene: &mut CompositorScene,
    primitives: &[DrawPrimitive],
    context: &LocalPrimitiveContext<'_>,
    motion: bool,
) {
    for primitive in primitives {
        if let Some(entry) = ShapeRunEntry::new(primitive, None) {
            if let Some(params) = emit_shape_run_entry(
                &entry,
                context.layer_bounds,
                context.local_layer,
                context.visual_clip,
                motion,
            ) {
                push_shape_params(local_scene, params);
            }
        } else {
            push_loose_draw_primitive(local_scene, primitive, context);
        }
    }
}

/// The `PrimitiveNode::Draw` arm of [`push_local_primitive`] for a primitive
/// with no per-draw clip — the shape a [`DrawRunNode`] carries.
fn push_loose_draw_primitive(
    local_scene: &mut CompositorScene,
    primitive: &DrawPrimitive,
    context: &LocalPrimitiveContext<'_>,
) {
    let counts_before = scene_counts(local_scene);
    push_draw_primitive(
        primitive,
        context.layer_bounds,
        context.local_layer,
        context.visual_clip,
        local_scene,
        None,
        context.motion_context_animated || context.content_offset_translation,
    );
    assign_snap_anchor_since(local_scene, counts_before, context.draw_snap_anchor);
}

fn push_shape_params(scene: &mut CompositorScene, params: ShapeDrawParams) {
    scene.push_shape_with_stroke_and_arc(
        params.rect,
        params.local_rect,
        params.quad,
        params.brush,
        params.shape,
        params.stroke,
        params.arc,
        params.clip,
        params.blend_mode,
        params.motion_context_animated,
    );
}

fn push_local_primitive(
    local_scene: &mut CompositorScene,
    text_layout: &mut impl TextLayoutResolver,
    primitive: &PrimitiveEntry,
    context: &LocalPrimitiveContext<'_>,
) {
    match &primitive.node {
        PrimitiveNode::Draw(draw) => {
            let counts_before = scene_counts(local_scene);
            let clip = resolve_primitive_clip(
                draw.clip,
                context.layer_bounds,
                context.local_layer,
                context.visual_clip,
                PrimitiveClipSpace::Local,
            );
            if draw.clip.is_some() && clip.is_none() {
                return;
            }
            push_draw_primitive(
                &draw.primitive,
                context.layer_bounds,
                context.local_layer,
                clip,
                local_scene,
                None,
                context.motion_context_animated || context.content_offset_translation,
            );
            assign_snap_anchor_since(local_scene, counts_before, context.draw_snap_anchor);
        }
        PrimitiveNode::Text(text) => {
            let counts_before = scene_counts(local_scene);
            let text_rect = text
                .rect
                .translate(context.layer_bounds.x, context.layer_bounds.y);
            let mut text_clip = resolve_primitive_clip(
                text.clip,
                context.layer_bounds,
                context.local_layer,
                context.visual_clip,
                PrimitiveClipSpace::Local,
            );
            if text.clip.is_some() && text_clip.is_none() {
                let Some(visual_clip) = context.visual_clip else {
                    return;
                };
                if !rect_should_collect_for_text_prewarm(text_rect, visual_clip) {
                    return;
                }
                text_clip = Some(visual_clip);
            }
            push_text_style_draws(
                local_scene,
                text_layout,
                text.node_id,
                context.layer_bounds,
                text_rect,
                context.local_layer,
                &text.text,
                &text.text_style,
                text.font_size,
                text.layout_options,
                text_clip,
            );
            mark_motion_stable_effect_layers_since(
                local_scene,
                counts_before,
                context.content_offset_translation,
            );
            mark_translated_text_since(local_scene, counts_before, context.translated_text_motion);
            assign_snap_anchor_since(local_scene, counts_before, context.text_snap_anchor);
        }
    }
}

pub(crate) fn translate_quad(quad: [[f32; 2]; 4], delta: Point) -> [[f32; 2]; 4] {
    quad.map(|[x, y]| [x + delta.x, y + delta.y])
}

#[allow(clippy::too_many_arguments)]
fn collect_layer_contents_into<'a>(
    layer: &'a LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    inherited_clip: Option<Rect>,
    layer_offset: Point,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    translation_context: TranslationRenderContext,
    local_scene: &mut CompositorScene,
    child_layers: &mut Vec<ChildLayerComposite<'a>>,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
) {
    let local_layer = local_content_layer_for(&layer.graphics_layer);
    let layer_bounds = layer.local_bounds.translate(layer_offset.x, layer_offset.y);
    let layer_clip = layer
        .clip_rect()
        .map(|clip| clip.translate(layer_offset.x, layer_offset.y));
    let mut visual_clip = resolve_clip(inherited_clip, layer_clip);

    if layer_clip.is_some() && inherited_clip.is_some() && visual_clip.is_none() {
        let Some(parent_clip) = inherited_clip else {
            return;
        };
        if !clipped_layer_should_collect_for_text_prewarm(layer, layer_bounds, parent_clip) {
            return;
        }
        visual_clip = Some(parent_clip);
    }

    let effective_translated_content_context =
        translation_context.inherited_content_translation || layer.translated_content_context;
    let direct_translated_content_axes = translation_context
        .translated_content_axes
        .union(translated_content_axes_for_layer(layer));
    let allow_rigid_snap = effective_translated_content_context || !layer.motion_context_animated;
    let boundary_snap_anchor = if !translation_context.inherited_content_translation
        && layer.translated_content_context
        && allow_rigid_snap
    {
        rigid_snap_anchor(
            layer_bounds.translate(
                layer.translated_content_offset.x,
                layer.translated_content_offset.y,
            ),
            &local_layer,
        )
    } else {
        None
    };
    let translated_snap_anchor = inherited_translated_snap_anchor.or(boundary_snap_anchor);
    let layer_snap_anchor = translated_snap_anchor.or_else(|| {
        if allow_rigid_snap && layer_needs_rigid_snap(layer, effective_translated_content_context) {
            rigid_snap_anchor(layer_bounds, &local_layer)
        } else {
            None
        }
    });
    let translated_local_picture = layer.translated_content_context
        && layer.motion_context_animated
        && layer_clip.is_none()
        && !translation_context.inherited_content_translation
        && !translation_context.surface_capture_active
        && !translation_context.local_picture_capture_active;
    let stabilize_translated_capture_x =
        layer.translated_content_offset.x.abs() > NORMALIZED_SCENE_AFFINE_TOLERANCE;
    let stabilize_translated_capture_y =
        layer.translated_content_offset.y.abs() > NORMALIZED_SCENE_AFFINE_TOLERANCE;
    let translated_text_motion =
        effective_translated_content_context && !translation_context.local_picture_capture_active;
    let mut translated_local_picture_state =
        translated_local_picture.then(|| TranslatedLocalPictureState {
            counts: scene_emission_counts(local_scene),
            z_start: local_scene.next_z,
            stabilize_x: stabilize_translated_capture_x,
            stabilize_y: stabilize_translated_capture_y,
        });
    let local_primitive_context = LocalPrimitiveContext {
        layer_bounds,
        local_layer: &local_layer,
        visual_clip,
        motion_context_animated: layer.motion_context_animated,
        content_offset_translation: effective_translated_content_context,
        translated_text_motion,
        draw_snap_anchor: layer_snap_anchor,
        text_snap_anchor: layer_snap_anchor,
    };
    let child_translation_context = TranslationRenderContext {
        inherited_content_translation: effective_translated_content_context,
        translated_content_axes: direct_translated_content_axes,
        surface_capture_active: translation_context.surface_capture_active,
        local_picture_capture_active: translation_context.local_picture_capture_active,
    };
    let mut deferred_draws: Vec<&RenderNode> = Vec::new();
    // Consecutive plain shape draws accumulate here and emit as one batch
    // instead of one bookkept scene push at a time. Anything else (text,
    // images, child layers) flushes the run first so z order is exactly what
    // the per-primitive path would have produced. Capacity is an upper bound
    // on any run this layer can produce, so a watch-scale run never pays the
    // doubling-realloc ladder mid-collection.
    let shape_run_bound: usize = layer
        .children
        .iter()
        .map(|child| match child {
            RenderNode::DrawRun(run) => run.primitives.len(),
            RenderNode::Primitive(_) => 1,
            _ => 0,
        })
        .sum();
    let mut shape_run: Vec<ShapeRunEntry<'_>> = Vec::with_capacity(shape_run_bound);

    for child in &layer.children {
        match child {
            RenderNode::Primitive(primitive) => match primitive.phase {
                PrimitivePhase::BeforeChildren => {
                    if let PrimitiveNode::Draw(draw) = &primitive.node {
                        if let Some(entry) = ShapeRunEntry::new(&draw.primitive, draw.clip) {
                            shape_run.push(entry);
                            continue;
                        }
                    }
                    flush_shape_run(local_scene, &mut shape_run, &local_primitive_context);
                    push_local_primitive(
                        local_scene,
                        text_layout,
                        primitive,
                        &local_primitive_context,
                    );
                }
                PrimitivePhase::AfterChildren => deferred_draws.push(child),
            },
            RenderNode::DrawRun(run) => match run.phase {
                PrimitivePhase::BeforeChildren => {
                    collect_draw_run(local_scene, run, &mut shape_run, &local_primitive_context);
                }
                PrimitivePhase::AfterChildren => deferred_draws.push(child),
            },
            RenderNode::Layer(child_layer) => {
                flush_shape_run(local_scene, &mut shape_run, &local_primitive_context);
                let child_requirements = layer_surface_requirements_cached(
                    child_layer.as_ref(),
                    layer_surface_requirements_cache,
                );
                if !child_requirements.has_isolating_requirement() {
                    if let Some(translation) = child_requirements.direct_translation {
                        let child_offset = Point::new(
                            layer_offset.x + translation.x,
                            layer_offset.y + translation.y,
                        );
                        let child_bounds = child_layer
                            .local_bounds
                            .translate(child_offset.x, child_offset.y);
                        let child_translated_snap_anchor = translated_snap_anchor.or_else(|| {
                            if effective_translated_content_context {
                                let child_local_layer =
                                    local_content_layer_for(&child_layer.graphics_layer);
                                rigid_snap_anchor(child_bounds, &child_local_layer)
                            } else {
                                None
                            }
                        });
                        let child_shadow_clip = resolve_clip(
                            visual_clip,
                            child_layer
                                .shadow_clip
                                .map(|clip| clip.translate(child_offset.x, child_offset.y)),
                        );
                        push_layer_shadow(
                            local_scene,
                            &child_layer.graphics_layer,
                            child_bounds,
                            child_bounds,
                            child_shadow_clip,
                        );
                        collect_layer_contents_into(
                            child_layer.as_ref(),
                            text_layout,
                            visual_clip,
                            child_offset,
                            child_translated_snap_anchor,
                            child_translation_context,
                            local_scene,
                            child_layers,
                            layer_surface_rect_cache,
                            layer_surface_requirements_cache,
                        );
                        continue;
                    }
                }
                flush_translated_local_picture(
                    local_scene,
                    &mut translated_local_picture_state,
                    visual_clip,
                    layer_snap_anchor,
                );
                let mut shadow_scene = CompositorScene::new();
                let child_logical_rect = estimate_layer_surface_rect_cached_with_text_layout(
                    child_layer.as_ref(),
                    text_layout,
                    layer_surface_rect_cache,
                    layer_surface_requirements_cache,
                );
                let child_bounds = quad_bounds(
                    child_layer
                        .transform_to_parent
                        .map_rect(child_layer.local_bounds),
                );
                let child_bounds = child_bounds.translate(layer_offset.x, layer_offset.y);
                let child_shadow_clip = resolve_clip(
                    visual_clip,
                    child_layer.shadow_clip.map(|clip| {
                        quad_bounds(child_layer.transform_to_parent.map_rect(clip))
                            .translate(layer_offset.x, layer_offset.y)
                    }),
                );
                push_layer_shadow(
                    &mut shadow_scene,
                    &child_layer.graphics_layer,
                    child_layer.local_bounds,
                    child_bounds,
                    child_shadow_clip,
                );
                let child_snap_anchor = translated_snap_anchor.or_else(|| {
                    let child_surface_needs_snap = surface_composite_needs_rigid_snap(
                        child_requirements,
                        effective_translated_content_context,
                        translation_context.surface_capture_active,
                    );
                    if effective_translated_content_context || child_surface_needs_snap {
                        let child_local_layer =
                            local_content_layer_for(&child_layer.graphics_layer);
                        rigid_snap_anchor(child_bounds, &child_local_layer)
                    } else {
                        None
                    }
                });
                let child_to_parent =
                    child_layer
                        .transform_to_parent
                        .then(ProjectiveTransform::translation(
                            layer_offset.x,
                            layer_offset.y,
                        ));
                let surface_clip = visual_clip.and_then(|clip| {
                    child_to_parent
                        .inverse()
                        .map(|parent_to_child| parent_to_child.bounds_for_rect(clip))
                });
                if cranpose_core::env_flag!("CRANPOSE_BACKDROP_DIAG")
                    && child_layer.backdrop().is_some()
                {
                    eprintln!(
                        "[backdrop-diag] child node={:?} local_bounds={:?} logical_rect={:?} offset={:?} transform={:?}",
                        child_layer.node_id,
                        child_layer.local_bounds,
                        child_logical_rect,
                        layer_offset,
                        child_layer.transform_to_parent,
                    );
                }
                child_layers.push(ChildLayerComposite {
                    z_index: local_scene.next_z,
                    layer: child_layer.as_ref(),
                    logical_rect: child_logical_rect,
                    dest_quad: translate_quad(
                        child_layer.transform_to_parent.map_rect(child_logical_rect),
                        layer_offset,
                    ),
                    snap_anchor: child_snap_anchor,
                    backdrop_rect: quad_bounds(translate_quad(
                        child_layer
                            .transform_to_parent
                            .map_rect(child_layer.local_bounds),
                        layer_offset,
                    )),
                    visual_clip,
                    surface_clip,
                    shadow_draws: std::mem::take(&mut shadow_scene.shadow_draws),
                    needs_nested_underlay: layer_contains_descendant_backdrop(child_layer.as_ref()),
                });
                local_scene.next_z += 1;
                if translated_local_picture_state.is_some() {
                    translated_local_picture_state = Some(TranslatedLocalPictureState {
                        counts: scene_emission_counts(local_scene),
                        z_start: local_scene.next_z,
                        stabilize_x: stabilize_translated_capture_x,
                        stabilize_y: stabilize_translated_capture_y,
                    });
                }
            }
        }
    }

    for child in deferred_draws {
        match child {
            RenderNode::Primitive(primitive) => {
                if let PrimitiveNode::Draw(draw) = &primitive.node {
                    if let Some(entry) = ShapeRunEntry::new(&draw.primitive, draw.clip) {
                        shape_run.push(entry);
                        continue;
                    }
                }
                flush_shape_run(local_scene, &mut shape_run, &local_primitive_context);
                push_local_primitive(
                    local_scene,
                    text_layout,
                    primitive,
                    &local_primitive_context,
                );
            }
            RenderNode::DrawRun(run) => {
                collect_draw_run(local_scene, run, &mut shape_run, &local_primitive_context);
            }
            // Only primitive and run nodes are ever deferred.
            RenderNode::Layer(_) => {}
        }
    }
    flush_shape_run(local_scene, &mut shape_run, &local_primitive_context);
    flush_translated_local_picture(
        local_scene,
        &mut translated_local_picture_state,
        visual_clip,
        layer_snap_anchor,
    );
}

#[cfg(test)]
pub(crate) fn collect_layer_contents<'a>(
    layer: &'a LayerNode,
    inherited_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
) -> CollectedLayer<'a> {
    let mut text_layout = UiTextLayoutResolver;
    collect_layer_contents_with_translation_context_and_text_layout(
        layer,
        &mut text_layout,
        inherited_clip,
        inherited_translated_snap_anchor,
        TranslationRenderContext::default(),
        layer_surface_rect_cache,
        layer_surface_requirements_cache,
    )
}

#[cfg(test)]
pub(crate) fn collect_layer_contents_with_translation_context<'a>(
    layer: &'a LayerNode,
    inherited_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    translation_context: TranslationRenderContext,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
) -> CollectedLayer<'a> {
    let mut text_layout = UiTextLayoutResolver;
    collect_layer_contents_with_translation_context_and_text_layout(
        layer,
        &mut text_layout,
        inherited_clip,
        inherited_translated_snap_anchor,
        translation_context,
        layer_surface_rect_cache,
        layer_surface_requirements_cache,
    )
}

pub(crate) fn collect_layer_contents_with_translation_context_and_text_layout<'a>(
    layer: &'a LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    inherited_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    translation_context: TranslationRenderContext,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
) -> CollectedLayer<'a> {
    collect_layer_contents_with_capacity(
        layer,
        text_layout,
        inherited_clip,
        inherited_translated_snap_anchor,
        translation_context,
        layer_surface_rect_cache,
        layer_surface_requirements_cache,
        SceneCapacityHint::default(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_layer_contents_with_capacity<'a>(
    layer: &'a LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    inherited_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    translation_context: TranslationRenderContext,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
    capacity: SceneCapacityHint,
) -> CollectedLayer<'a> {
    collect_layer_contents_reusing(
        layer,
        text_layout,
        inherited_clip,
        inherited_translated_snap_anchor,
        translation_context,
        layer_surface_rect_cache,
        layer_surface_requirements_cache,
        CompositorScene::with_capacity(capacity),
    )
}

/// Like [`collect_layer_contents_with_capacity`], but fills a scene recycled
/// from a previous frame. A fully animated scene re-collects every primitive
/// each frame, and its draw-op vector is megabytes — large enough that a
/// fresh allocation goes straight to mmap and back every frame. Reusing the
/// buffers keeps the steady-state frame allocation-free.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_layer_contents_reusing<'a>(
    layer: &'a LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    inherited_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    translation_context: TranslationRenderContext,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
    scene: CompositorScene,
) -> CollectedLayer<'a> {
    let mut local_scene = scene;
    local_scene.clear();
    let mut child_layers = Vec::new();
    collect_layer_contents_with_translation_context_into(
        layer,
        text_layout,
        inherited_clip,
        inherited_translated_snap_anchor,
        translation_context,
        &mut local_scene,
        &mut child_layers,
        layer_surface_rect_cache,
        layer_surface_requirements_cache,
    );

    CollectedLayer {
        scene: local_scene,
        child_layers,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_layer_contents_with_translation_context_into<'a>(
    layer: &'a LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    inherited_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    translation_context: TranslationRenderContext,
    local_scene: &mut CompositorScene,
    child_layers: &mut Vec<ChildLayerComposite<'a>>,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
) {
    local_scene.clear();
    child_layers.clear();
    collect_layer_contents_into(
        layer,
        text_layout,
        inherited_clip,
        Point::default(),
        inherited_translated_snap_anchor,
        translation_context,
        local_scene,
        child_layers,
        layer_surface_rect_cache,
        layer_surface_requirements_cache,
    );
}

#[cfg(test)]
pub(crate) fn estimate_layer_surface_rect(layer: &LayerNode) -> Rect {
    let mut layer_surface_rect_cache = HashMap::new();
    let mut layer_surface_requirements_cache = HashMap::new();
    estimate_layer_surface_rect_cached(
        layer,
        &mut layer_surface_rect_cache,
        &mut layer_surface_requirements_cache,
    )
}

pub(crate) fn estimate_layer_surface_rect_cached(
    layer: &LayerNode,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
) -> Rect {
    let mut text_layout = UiTextLayoutResolver;
    estimate_layer_surface_rect_cached_with_text_layout(
        layer,
        &mut text_layout,
        layer_surface_rect_cache,
        layer_surface_requirements_cache,
    )
}

pub(crate) fn estimate_layer_surface_rect_cached_with_text_layout(
    layer: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
) -> Rect {
    let cache_key = layer_cache_key(layer);
    if let Some(cached_rect) = layer_surface_rect_cache.get(&cache_key) {
        return *cached_rect;
    }

    let collected = collect_layer_contents_with_translation_context_and_text_layout(
        layer,
        text_layout,
        None,
        None,
        TranslationRenderContext::default(),
        layer_surface_rect_cache,
        layer_surface_requirements_cache,
    );
    let surface_requirements =
        layer_surface_requirements_cached(layer, layer_surface_requirements_cache);
    let translated_content_context =
        layer.translated_content_context || surface_requirements.contains_translated_content;
    let effective_requirements =
        effective_surface_requirements(translated_content_context, false, surface_requirements);
    let translated_content_axes = translated_content_axes_for_layer(layer)
        .union(surface_requirements.translated_content_axes);
    let bounds = motion_stable_capture_bounds(
        layer,
        &collected.scene,
        &collected.child_layers,
        effective_requirements,
        translated_content_axes,
        None,
    );
    let rect = resolved_layer_surface_rect(layer, bounds);
    layer_surface_rect_cache.insert(cache_key, rect);
    rect
}

pub(crate) fn resolved_layer_surface_rect(layer: &LayerNode, bounds: Option<Rect>) -> Rect {
    let rect = bounds.unwrap_or(layer.local_bounds);
    if layer.effect().is_some() || layer.backdrop().is_some() {
        union_rect(Some(rect), layer.local_bounds).unwrap_or(rect)
    } else {
        rect
    }
}

pub(crate) trait TranslateBy {
    fn translate_by(&mut self, delta: Point);
}

impl TranslateBy for Rect {
    fn translate_by(&mut self, delta: Point) {
        self.x += delta.x;
        self.y += delta.y;
    }
}

impl TranslateBy for DrawShape {
    fn translate_by(&mut self, delta: Point) {
        self.rect.translate_by(delta);
        self.local_rect.translate_by(delta);
        for point in &mut self.quad {
            point[0] += delta.x;
            point[1] += delta.y;
        }
        if let Some(clip) = self.clip.as_mut() {
            clip.translate_by(delta);
        }
        if let Some(arc) = self.arc.as_mut() {
            // The arc center lives in the same space as `local_rect`; leaving
            // it behind would slide the band out of its own bounding box.
            arc.center.x += delta.x;
            arc.center.y += delta.y;
        }
    }
}

impl TranslateBy for ImageDraw {
    fn translate_by(&mut self, delta: Point) {
        self.rect.translate_by(delta);
        self.local_rect.translate_by(delta);
        for point in &mut self.quad {
            point[0] += delta.x;
            point[1] += delta.y;
        }
        if let Some(clip) = self.clip.as_mut() {
            clip.translate_by(delta);
        }
    }
}

impl TranslateBy for TextDraw {
    fn translate_by(&mut self, delta: Point) {
        self.rect.translate_by(delta);
        if let Some(clip) = self.clip.as_mut() {
            clip.translate_by(delta);
        }
    }
}

impl TranslateBy for ShadowDraw {
    fn translate_by(&mut self, delta: Point) {
        for (shape, _) in &mut self.shapes {
            shape.translate_by(delta);
        }
        for text in &mut self.texts {
            text.translate_by(delta);
        }
        if let Some(clip) = self.clip.as_mut() {
            clip.translate_by(delta);
        }
    }
}

impl TranslateBy for EffectLayer {
    fn translate_by(&mut self, delta: Point) {
        self.rect.translate_by(delta);
        if let Some(clip) = self.clip.as_mut() {
            clip.translate_by(delta);
        }
    }
}

impl TranslateBy for BackdropLayer {
    fn translate_by(&mut self, delta: Point) {
        self.rect.translate_by(delta);
        if let Some(clip) = self.clip.as_mut() {
            clip.translate_by(delta);
        }
    }
}

impl<T: TranslateBy> TranslateBy for Vec<T> {
    fn translate_by(&mut self, delta: Point) {
        for item in self {
            item.translate_by(delta);
        }
    }
}

impl TranslateBy for CompositorScene {
    fn translate_by(&mut self, delta: Point) {
        self.shapes.translate_by(delta);
        self.images.translate_by(delta);
        self.texts.translate_by(delta);
        self.shadow_draws.translate_by(delta);
        self.effect_layers.translate_by(delta);
        self.backdrop_layers.translate_by(delta);
    }
}

pub(crate) struct SceneWindowSource<'a> {
    pub(crate) shapes: &'a [DrawShape],
    pub(crate) images: &'a [ImageDraw],
    pub(crate) texts: &'a [TextDraw],
    pub(crate) shadow_draws: &'a [ShadowDraw],
    pub(crate) draw_ops: &'a [DrawOp],
    pub(crate) effect_layers: &'a [EffectLayer],
    pub(crate) backdrop_layers: &'a [BackdropLayer],
}

pub(crate) fn effect_layer_in_range(layer: &EffectLayer, z_start: usize, z_end: usize) -> bool {
    layer.z_start >= z_start && layer.z_start < z_end && layer.z_end <= z_end
}

pub(crate) fn build_scene_window(
    source: SceneWindowSource<'_>,
    z_start: usize,
    z_end: usize,
    window_rect: Rect,
) -> CompositorScene {
    let mut scene = CompositorScene::new();
    let mut shape_map = vec![None; source.shapes.len()];
    for (source_index, shape) in source.shapes.iter().enumerate() {
        if shape.z_index >= z_start && shape.z_index < z_end {
            shape_map[source_index] = Some(scene.shapes.len());
            scene.shapes.push(shape.clone());
        }
    }
    let mut image_map = vec![None; source.images.len()];
    for (source_index, image) in source.images.iter().enumerate() {
        if image.z_index >= z_start && image.z_index < z_end {
            image_map[source_index] = Some(scene.images.len());
            scene.images.push(image.clone());
        }
    }
    let mut text_map = vec![None; source.texts.len()];
    for (source_index, text) in source.texts.iter().enumerate() {
        if text.z_index >= z_start && text.z_index < z_end {
            text_map[source_index] = Some(scene.texts.len());
            scene.texts.push(text.clone());
        }
    }
    let mut shadow_map = vec![None; source.shadow_draws.len()];
    for (source_index, shadow) in source.shadow_draws.iter().enumerate() {
        if shadow.z_index >= z_start && shadow.z_index < z_end {
            shadow_map[source_index] = Some(scene.shadow_draws.len());
            scene.shadow_draws.push(shadow.clone());
        }
    }
    for op in source.draw_ops {
        if op.z_index < z_start || op.z_index >= z_end {
            continue;
        }
        let kind = match op.kind {
            DrawOpKind::Shape(index) => shape_map
                .get(index)
                .copied()
                .flatten()
                .map(DrawOpKind::Shape),
            DrawOpKind::Image(index) => image_map
                .get(index)
                .copied()
                .flatten()
                .map(DrawOpKind::Image),
            DrawOpKind::Text(index) => text_map.get(index).copied().flatten().map(DrawOpKind::Text),
            DrawOpKind::Shadow(index) => shadow_map
                .get(index)
                .copied()
                .flatten()
                .map(DrawOpKind::Shadow),
            // Retained batches never appear inside windowed ranges: their
            // quads are baked in absolute device space, and a window scene is
            // translated into its own origin. The range chunker refuses to
            // cache across them (`draw_op_is_motion_sensitive` is true and
            // `scene_range_can_cache_as_transparent_surface` is false), so an
            // op reaching here would already be a bug upstream; dropping it
            // keeps the window's coordinates sane.
            DrawOpKind::Retained(_) => {
                debug_assert!(false, "retained draw op inside a windowed scene range");
                None
            }
        };
        if let Some(kind) = kind {
            scene.draw_ops.push(DrawOp {
                z_index: op.z_index,
                kind,
            });
        }
    }
    scene.effect_layers = source
        .effect_layers
        .iter()
        .filter(|layer| effect_layer_in_range(layer, z_start, z_end))
        .cloned()
        .collect();
    scene.backdrop_layers = source
        .backdrop_layers
        .iter()
        .filter(|layer| layer.z_index >= z_start && layer.z_index < z_end)
        .cloned()
        .collect();
    scene.translate_by(Point {
        x: -window_rect.x,
        y: -window_rect.y,
    });
    scene
}

pub(crate) fn filtered_effect_layer_index(
    effect_layers: &[EffectLayer],
    effect_layer_index: usize,
    z_start: usize,
    z_end: usize,
) -> Option<usize> {
    let mut filtered_index = 0usize;
    for (index, layer) in effect_layers.iter().enumerate() {
        if !effect_layer_in_range(layer, z_start, z_end) {
            continue;
        }
        if index == effect_layer_index {
            return Some(filtered_index);
        }
        filtered_index += 1;
    }
    None
}
