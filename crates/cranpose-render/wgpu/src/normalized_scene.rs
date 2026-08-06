use crate::effect_renderer::CompositeSampleMode;
use crate::pipeline::{
    emitted_scene_bounds, push_draw_primitive, push_layer_shadow, push_text_style_draws,
    scene_emission_counts, SceneEmissionCounts, TextLayoutResolver, UiTextLayoutResolver,
};
use crate::scene::{
    BackdropLayer, CompositorScene, DrawOp, DrawOpKind, DrawShape, EffectLayer, ImageDraw,
    ShadowDraw, SnapAnchor, TextDraw,
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
use cranpose_render_common::graph::{
    quad_bounds, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform,
    RenderNode,
};
use cranpose_render_common::layer_composition::local_content_layer_for;
use cranpose_render_common::primitive_emit::{
    resolve_clip, resolve_primitive_clip, PrimitiveClipSpace,
};
use cranpose_ui_graphics::{GraphicsLayer, Point, Rect};

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
        RenderNode::Primitive(_) => false,
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
                draw.primitive.clone(),
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
    let mut deferred_primitives = Vec::new();

    for child in &layer.children {
        match child {
            RenderNode::Primitive(primitive) => match primitive.phase {
                PrimitivePhase::BeforeChildren => {
                    push_local_primitive(
                        local_scene,
                        text_layout,
                        primitive,
                        &local_primitive_context,
                    );
                }
                PrimitivePhase::AfterChildren => deferred_primitives.push(primitive),
            },
            RenderNode::Layer(child_layer) => {
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
                if std::env::var_os("CRANPOSE_BACKDROP_DIAG").is_some()
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
                    shadow_draws: shadow_scene.shadow_draws,
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

    for primitive in deferred_primitives {
        push_local_primitive(
            local_scene,
            text_layout,
            primitive,
            &local_primitive_context,
        );
    }
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
    let mut local_scene = CompositorScene::new();
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
