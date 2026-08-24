use cranpose_core::{collections::map::HashMap, NodeId};
use cranpose_render_common::{
    geometry::{expand_blurred_rect, union_rect},
    graph::{
        quad_bounds, CachePolicy, DrawRunNode, LayerNode, PrimitiveEntry, PrimitiveNode,
        PrimitivePhase, ProjectiveTransform, RenderNode,
    },
    layer_composition::{effective_layer_isolation, local_content_layer_for, LayerIsolation},
    primitive_emit::{
        arc_shape_params, rect_shape_params, resolve_clip, resolve_primitive_clip,
        round_rect_shape_params, PrimitiveClipSpace, ShapeDrawParams,
    },
};
use cranpose_ui_graphics::{Brush, DrawPrimitive, GraphicsLayer, Point, Rect, RenderEffect};

#[cfg(test)]
use crate::pipeline::UiTextLayoutResolver;
use crate::{
    effect_renderer::CompositeSampleMode,
    pipeline::{
        emitted_scene_bounds, push_draw_primitive, push_layer_shadow, push_text_style_draws,
        scene_emission_counts, SceneEmissionCounts, TextLayoutResolver,
    },
    scene::{
        BackdropLayer, CompositorScene, DrawOp, DrawOpKind, DrawShape, EffectLayer, ImageDraw,
        SceneCapacityHint, ShadowDraw, SnapAnchor, TextDraw,
    },
    surface_executor::{
        backend::LayerSurfaceRoundedClip, layer_source_uses_external_backdrop_underlay,
    },
    surface_plan::{
        composite_sample_mode_for_requirements, effective_surface_requirements, layer_cache_key,
        layer_contains_descendant_backdrop, layer_needs_rigid_snap,
        layer_surface_requirements_cached, layer_surface_scale, translated_content_axes_for_layer,
        LayerSurfaceRequirements, TranslatedContentAxes, TranslationRenderContext,
    },
    surface_requirements::{SurfaceRequirement, SurfaceRequirementSet},
};

const NORMALIZED_SCENE_AFFINE_TOLERANCE: f32 = 1e-4;
const MOTION_STABLE_CAPTURE_MIN_LEADING_GUARD: f32 = 64.0;
const MOTION_STABLE_CAPTURE_MAX_LEADING_GUARD: f32 = 2048.0;
const MOTION_STABLE_CAPTURE_LEADING_VIEWPORTS: f32 = 3.0;
const MOTION_STABLE_CAPTURE_CLIPPED_LEADING_VIEWPORTS: f32 = 0.25;
const MOTION_STABLE_CAPTURE_CLIPPED_MAX_LEADING_GUARD: f32 = 256.0;
const TRANSLATED_LOCAL_CAPTURE_STABLE_GUARD: f32 = 64.0;
const MOTION_STABLE_CAPTURE_CROSS_AXIS_LEADING_GUARD: f32 = 96.0;
const CLIPPED_TEXT_PREWARM_VIEWPORT_MULTIPLIER: f32 = 2.0;

/// A fully owned lowering of an isolating child layer: the POD composite
/// fields, a snapshot of every scalar the render path used to read off
/// `&LayerNode`, and the child's own collected content (`source`). All
/// snapshot values are captured at collection time so rendering never has to
/// reach back into the retained graph.
pub(crate) struct ChildLayerComposite {
    pub(crate) z_index: usize,
    pub(crate) logical_rect: Rect,
    pub(crate) dest_quad: [[f32; 2]; 4],
    pub(crate) snap_anchor: Option<SnapAnchor>,
    /// Transform pivot in the parent scene's logical coordinate space.
    pub(crate) composite_snap_origin: Option<Point>,
    pub(crate) backdrop_rect: Rect,
    pub(crate) visual_clip: Option<Rect>,
    pub(crate) surface_clip: Option<Rect>,
    pub(crate) shadow_draws: Vec<ShadowDraw>,
    pub(crate) needs_nested_underlay: bool,
    // --- snapshot of the layer node (captured at collection) ---
    pub(crate) node_id: Option<NodeId>,
    pub(crate) backdrop: Option<RenderEffect>,
    pub(crate) has_effect: bool,
    pub(crate) effect_contains_runtime_shader: bool,
    pub(crate) target_content_hash: u64,
    pub(crate) effect_hash: u64,
    /// Present exactly when the render body's source-cache-key decision reads
    /// it (a motion-stable source); the hash walks the subtree, so it is not
    /// computed for layers that can never consume it.
    pub(crate) motion_source_content_hash: Option<u64>,
    /// Same value as `needs_nested_underlay` (both snapshot
    /// `layer_contains_descendant_backdrop`); kept separate because the two
    /// fields serve different contracts (compositing vs. cache admission).
    pub(crate) contains_descendant_backdrop: bool,
    pub(crate) cache_policy: CachePolicy,
    pub(crate) surface_requirements: LayerSurfaceRequirements,
    pub(crate) rounded_clip: Option<LayerSurfaceRoundedClip>,
    pub(crate) isolation: Option<LayerIsolation>,
    pub(crate) translated_content_context: bool,
    /// `translated_content_axes_for_layer` of the child itself.
    pub(crate) own_translated_content_axes: TranslatedContentAxes,
    pub(crate) clip_rect: Option<Rect>,
    pub(crate) local_bounds: Rect,
    /// `layer_surface_scale` of the child (uniform transform scale).
    pub(crate) surface_scale: f32,
    /// The child's own collected content, in the child's local space. The
    /// parent's post-build shift must never translate it.
    pub(crate) source: LoweredChildSource,
}

/// The owned, recursive lowering of a child layer's content: its collected
/// scene plus the lowered isolating children found inside it.
#[derive(Default)]
pub(crate) struct LoweredChildSource {
    pub(crate) scene: CompositorScene,
    pub(crate) children: Vec<ChildLayerComposite>,
}

#[derive(Clone)]
pub(crate) struct ResolvedChildSurfaceComposite {
    pub(crate) logical_rect: Rect,
    pub(crate) dest_quad: [[f32; 2]; 4],
    pub(crate) snap_anchor: Option<SnapAnchor>,
    /// Transform pivot in the parent scene's logical coordinate space.
    pub(crate) composite_snap_origin: Option<Point>,
    pub(crate) backdrop_rect: Rect,
    pub(crate) surface_clip: Option<Rect>,
    pub(crate) shadow_draws: Vec<ShadowDraw>,
}

pub(crate) struct CollectedLayer {
    pub(crate) scene: CompositorScene,
    pub(crate) child_layers: Vec<ChildLayerComposite>,
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
    child_layers: &[ChildLayerComposite],
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
    child_layers: &[ChildLayerComposite],
    requirements: SurfaceRequirementSet,
    translated_content_axes: TranslatedContentAxes,
    capture_clip_override: Option<Rect>,
) -> Option<Rect> {
    motion_stable_capture_bounds_from_parts(
        layer.clip_rect(),
        layer.backdrop().is_some(),
        translated_content_axes_for_layer(layer),
        scene,
        child_layers,
        requirements,
        translated_content_axes,
        capture_clip_override,
    )
}

/// [`motion_stable_capture_bounds`] over collection-time snapshots of the
/// layer scalars it reads, for callers that no longer hold a `&LayerNode`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn motion_stable_capture_bounds_from_parts(
    layer_clip_rect: Option<Rect>,
    layer_has_backdrop: bool,
    own_translated_content_axes: TranslatedContentAxes,
    scene: &CompositorScene,
    child_layers: &[ChildLayerComposite],
    requirements: SurfaceRequirementSet,
    translated_content_axes: TranslatedContentAxes,
    capture_clip_override: Option<Rect>,
) -> Option<Rect> {
    let capture_clip = combined_capture_clip(layer_clip_rect, capture_clip_override);
    let visible_bounds = collected_layer_bounds(scene, child_layers, true)
        .and_then(|bounds| visible_draw_rect(bounds, capture_clip));
    if !requirements.contains(SurfaceRequirement::MotionStableCapture) || layer_has_backdrop {
        return visible_bounds;
    }

    let full_bounds = collected_layer_bounds(scene, child_layers, false);
    match (visible_bounds, full_bounds) {
        (Some(visible_bounds), Some(full_bounds))
            if hidden_content_precedes_visible_bounds(visible_bounds, full_bounds) =>
        {
            let translated_content_axes =
                translated_content_axes.union(own_translated_content_axes);
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

fn axis_aligned_composite_snap_origin(layer: &LayerNode, layer_offset: Point) -> Option<Point> {
    let graphics_layer = &layer.graphics_layer;
    if graphics_layer.rotation_x.abs() > NORMALIZED_SCENE_AFFINE_TOLERANCE
        || graphics_layer.rotation_y.abs() > NORMALIZED_SCENE_AFFINE_TOLERANCE
        || graphics_layer.rotation_z.abs() > NORMALIZED_SCENE_AFFINE_TOLERANCE
    {
        return None;
    }

    let local_bounds = layer.local_bounds;
    let local_origin = Point::new(
        local_bounds.x + local_bounds.width * graphics_layer.transform_origin.pivot_fraction_x,
        local_bounds.y + local_bounds.height * graphics_layer.transform_origin.pivot_fraction_y,
    );
    let mapped = layer.transform_to_parent.map_point(local_origin);
    Some(Point::new(
        mapped.x + layer_offset.x,
        mapped.y + layer_offset.y,
    ))
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
    backdrop_layers: usize,
}

fn scene_counts(scene: &CompositorScene) -> SceneCounts {
    SceneCounts {
        shapes: scene.shapes.len(),
        images: scene.images.len(),
        texts: scene.texts.len(),
        shadow_draws: scene.shadow_draws.len(),
        effect_layers: scene.effect_layers.len(),
        backdrop_layers: scene.backdrop_layers.len(),
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
    for layer in &mut scene.backdrop_layers[counts.backdrop_layers..] {
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
    child: &ChildLayerComposite,
) -> ResolvedChildSurfaceComposite {
    ResolvedChildSurfaceComposite {
        logical_rect: child.logical_rect,
        dest_quad: child.dest_quad,
        snap_anchor: child.snap_anchor,
        composite_snap_origin: child.composite_snap_origin,
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
        crate::stage_executor::stage_executor().map_fill(
            crate::stage_executor::Stage::Producer,
            run,
            &mut scratch,
            |entry| emit_shape_run_entry(entry, layer_bounds, layer, visual_clip, motion),
        );
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
    context_fingerprint, layer_supports_replay, rect_contains, SegmentTransform, MAX_RETAINED_OPS,
    SHAPE_REPLAY,
};

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
    use crate::{
        scene::{ColorPatch, PendingFeedCapture},
        shape_replay::command_feed_enabled,
    };
    // Guard order and content are the happy path's exact current checks —
    // when they all pass, nothing below this block costs anything new.
    let feed_ready = command_feed_enabled()
        && SHAPE_REPLAY.with(|state| state.borrow().supported)
        && context.draw_snap_anchor.is_none()
        && layer_supports_replay(context.local_layer);
    if !feed_ready {
        // Fail closed: a frame that bypassed materialization has spans
        // `run.primitives` is missing, so falling back to the caller's
        // ordinary loop would silently omit them. Only a frame with no
        // bypassed spans may return false here.
        return emit_unserved_frame_rematerialized(
            local_scene,
            run,
            frame,
            command,
            shape_run,
            context,
        );
    }
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
    let mut stat_frame_patches = 0u64;
    let (mut stat_retained, mut stat_fallback, mut stat_remat, mut stat_captures) =
        (0usize, 0usize, 0usize, 0usize);
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
                    stat_captures += 1;
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
                                frame: frame_now,
                            });
                    });
                }
            }
            cranpose_ui_graphics::FrameSpan::Retained {
                slot,
                capture: false,
                slot_offset,
                range,
                tape_range,
                transform,
                recolors,
                bounds,
            } => {
                let len = (tape_range.1 - tape_range.0) as usize;
                let bounds_now = Rect {
                    x: bounds.x + context.layer_bounds.x,
                    y: bounds.y + context.layer_bounds.y,
                    width: bounds.width,
                    height: bounds.height,
                };
                let emitted = local_scene.retained_draws.len() < MAX_RETAINED_OPS
                    && SHAPE_REPLAY
                        .with(|state| {
                            let mut state = state.borrow_mut();
                            let feed_slot = state.feed_slots.get_mut(&(command, *slot))?;
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
                            // Compact-record recolors are always solid: a
                            // flat 16-byte color write, no Brush at all.
                            for (offset, color) in recolors {
                                state.pending_color_patches.push(ColorPatch {
                                    slot: gpu_slot,
                                    shape_index: *slot_offset + offset,
                                    color: [color.r(), color.g(), color.b(), color.a()],
                                });
                            }
                            state.stat_patches += recolors.len() as u64;
                            stat_frame_patches += recolors.len() as u64;
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
                if emitted {
                    stat_retained += 1;
                } else {
                    stat_fallback += 1;
                    if range.1 > range.0 {
                        emit_feed_range(
                            local_scene,
                            &run.primitives[range.0 as usize..range.1 as usize],
                            context,
                            motion,
                        );
                    } else if let Some(primitives) = frame.fallback.as_ref().and_then(|recording| {
                        recording.materialize_range(tape_range.0 as usize, tape_range.1 as usize)
                    }) {
                        // Bypassed span whose retained buffer went away this
                        // frame: rebuild its primitives from the recording
                        // the frame itself owns — never from the ambient
                        // registry, whose contents may have moved on.
                        stat_remat += 1;
                        emit_feed_range(local_scene, &primitives, context, motion);
                    } else {
                        note_remat_miss(command, *slot);
                    }
                }
            }
        }
    }
    if cranpose_core::env_flag!("CRANPOSE_COMMAND_REPLAY_DIAG") {
        log::warn!(
            "[command-feed] frame {}: {} spans -> {} retained, {} fallback ({} remat), \
             {} captures queued, {} recolor patches; scene {} shapes {} retained ops",
            frame_now,
            frame.spans.len(),
            stat_retained,
            stat_fallback,
            stat_remat,
            stat_captures,
            stat_frame_patches,
            local_scene.shapes.len(),
            local_scene.retained_draws.len(),
        );
    }
    true
}

/// Fail-closed emission for a fed run the feed cannot serve (feed disabled,
/// unsupported collection window, snap-anchored context, unsupported
/// layer). A frame with NO bypassed spans returns `false`: `run.primitives`
/// is complete and the caller's ordinary loop draws it exactly as before —
/// the historical fallback, kept bit-identical. A frame that DID bypass
/// materialization returns `true` after emitting every span in order
/// itself: spans with primitives ordinarily, bypassed spans rebuilt from
/// the recording the frame itself owns (`frame.fallback`), with the
/// defensive terminal ([`note_remat_miss`]) on any span that cannot be
/// rebuilt.
#[cfg(not(target_arch = "wasm32"))]
fn emit_unserved_frame_rematerialized<'a>(
    local_scene: &mut CompositorScene,
    run: &'a DrawRunNode,
    frame: &cranpose_ui_graphics::CommandReplayFrame,
    command: cranpose_render_common::graph::DrawCommandId,
    shape_run: &mut Vec<ShapeRunEntry<'a>>,
    context: &LocalPrimitiveContext<'_>,
) -> bool {
    let has_bypassed_span = frame.spans.iter().any(|span| {
        matches!(
            span,
            cranpose_ui_graphics::FrameSpan::Retained {
                capture: false,
                range,
                ..
            } if range.1 <= range.0
        )
    });
    if !has_bypassed_span {
        return false;
    }
    {
        static WALKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = WALKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // First 8 walks verbatim, then every 1024th: a feed that fail-closes
        // in steady state must stay visible in logcat forever (the counter
        // in the line is the rate), not fall silent after startup — the
        // difference between "startup reconcile" and "the bypass never
        // engages" is exactly what a device log has to be able to answer.
        if n < 8 || n.is_multiple_of(1024) {
            log::warn!(
                "[command-feed] feed cannot serve a frame of {:?} with bypassed spans; \
                 rematerializing the whole run (fail-closed walk {})",
                command,
                n + 1,
            );
        }
    }
    // Entries accumulated from earlier nodes flush first, exactly as the
    // fed path does: span emission must land after them to keep z order.
    flush_shape_run(local_scene, shape_run, context);
    let motion = context.motion_context_animated || context.content_offset_translation;
    let counts_before = scene_counts(local_scene);
    for span in &frame.spans {
        let (range, bypassed_slot) = match span {
            cranpose_ui_graphics::FrameSpan::Dynamic { range } => (range, None),
            cranpose_ui_graphics::FrameSpan::Retained {
                slot,
                capture,
                range,
                tape_range,
                ..
            } => (range, (!capture).then_some((*slot, *tape_range))),
        };
        if range.1 > range.0 {
            emit_feed_range(
                local_scene,
                &run.primitives[range.0 as usize..range.1 as usize],
                context,
                motion,
            );
            continue;
        }
        // An empty dynamic or capture span carries nothing to draw; an
        // empty retained span is a bypass and must rebuild.
        let Some((slot, tape_range)) = bypassed_slot else {
            continue;
        };
        if let Some(primitives) = frame.fallback.as_ref().and_then(|recording| {
            recording.materialize_range(tape_range.0 as usize, tape_range.1 as usize)
        }) {
            emit_feed_range(local_scene, &primitives, context, motion);
        } else {
            note_remat_miss(command, slot);
        }
    }
    // One of the guards that routed us here may be a snap-anchored context:
    // the ordinary loop would have anchored these draws, so anchor
    // everything the walk emitted (idempotent over loose draws that already
    // anchored themselves).
    assign_snap_anchor_since(local_scene, counts_before, context.draw_snap_anchor);
    true
}

/// The defensive terminal for a bypassed span that could neither draw
/// retained nor rebuild: structurally unreachable for any frame the
/// builder produced, because every such frame owns a pinned handle to the
/// exact recording its spans address (`frame.fallback`) and
/// `materialize_range` on it cannot fail for the ranges the same recording
/// produced. Reaching this means a hand-built frame without its fallback
/// or a corrupt tape range — so keep the omission loud, counted, and
/// bounded. Revoking the confirmation makes the very next graph build
/// materialize the span again — the earliest self-heal point reachable
/// from here, as scene collection has no frame-invalidation hook to
/// request an early rebuild; on animating scenes that bound is the next
/// frame.
#[cfg(not(target_arch = "wasm32"))]
fn note_remat_miss(command: cranpose_render_common::graph::DrawCommandId, slot: u32) {
    cranpose_render_common::scene_builder::revoke_retained_slot(command, slot);
    let misses = SHAPE_REPLAY.with(|state| {
        let mut state = state.borrow_mut();
        state.stat_remat_miss += 1;
        state.stat_remat_miss
    });
    if misses <= 8 || misses.is_multiple_of(256) {
        log::warn!(
            "[command-feed] bypassed span for slot {} of {:?} could not be \
             rematerialized (lifetime misses {}); confirmation revoked, the \
             next build redraws it",
            slot,
            command,
            misses,
        );
    }
}

/// The ordinary path for one span of a fed run: shape entries convert in
/// place, non-shape primitives spill loose, exactly as the unfed run loop
/// does.
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
/// The request context `render_layer_surface` will receive for every
/// isolating child collected under `layer` — the flat child list is rendered
/// by the surface layer's uncached body, which derives one request context
/// from its own effective translation values and hands it to every child
/// (`render_layer_source_uncached`'s recursion). Collecting a child's
/// `source` with anything else would diverge from what the old render-time
/// re-collect produced.
pub(crate) fn derived_child_surface_context(
    layer: &LayerNode,
    translation_context: TranslationRenderContext,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
) -> TranslationRenderContext {
    let requirements = layer_surface_requirements_cached(layer, layer_surface_requirements_cache);
    TranslationRenderContext {
        inherited_content_translation: translation_context.inherited_content_translation
            || layer.translated_content_context
            || requirements.contains_translated_content,
        translated_content_axes: translation_context
            .translated_content_axes
            .union(translated_content_axes_for_layer(layer))
            .union(requirements.translated_content_axes),
        surface_capture_active: translation_context.surface_capture_active,
        local_picture_capture_active: translation_context.local_picture_capture_active,
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_layer_contents_into(
    layer: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    inherited_clip: Option<Rect>,
    layer_offset: Point,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    translation_context: TranslationRenderContext,
    child_surface_ctx: TranslationRenderContext,
    local_scene: &mut CompositorScene,
    child_layers: &mut Vec<ChildLayerComposite>,
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
                            // The flat child list still belongs to the same
                            // surface layer, so its render request context is
                            // unchanged.
                            child_surface_ctx,
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
                // Collect the child's content ONCE, at producer time, with the
                // exact translation context the render path used to re-collect
                // it with: the request context every consumer passes
                // (`child_surface_ctx`), post-transformed the way
                // `render_layer_surface` does for a nested capture
                // (`layer_surface_translation_context` with
                // `activates_nested_capture` true).
                let child_effective_translated_content_context = child_surface_ctx
                    .inherited_content_translation
                    || child_layer.translated_content_context
                    || child_requirements.contains_translated_content;
                let child_source_ctx = {
                    let child_effective_requirements = effective_surface_requirements(
                        child_effective_translated_content_context,
                        child_surface_ctx.surface_capture_active,
                        child_requirements,
                    );
                    TranslationRenderContext {
                        surface_capture_active: child_surface_ctx.surface_capture_active
                            || child_effective_requirements
                                .contains(SurfaceRequirement::MotionStableCapture),
                        ..child_surface_ctx
                    }
                };
                // Mirrors the effective requirements the render body
                // recomputes from the post-transform context; the motion hash
                // snapshot must exist exactly when its source-cache-key
                // decision reads it.
                let child_motion_stable_source = effective_surface_requirements(
                    child_effective_translated_content_context,
                    child_source_ctx.surface_capture_active,
                    child_requirements,
                )
                .contains(SurfaceRequirement::MotionStableCapture);
                let mut source_scene = CompositorScene::new();
                let mut source_children = Vec::new();
                collect_layer_contents_into(
                    child_layer.as_ref(),
                    text_layout,
                    None,
                    Point::default(),
                    None,
                    child_source_ctx,
                    derived_child_surface_context(
                        child_layer.as_ref(),
                        child_source_ctx,
                        layer_surface_requirements_cache,
                    ),
                    &mut source_scene,
                    &mut source_children,
                    layer_surface_rect_cache,
                    layer_surface_requirements_cache,
                );
                // The per-frame rect memo may short-circuit the bounds math.
                // On a miss the rect must match what the estimate path (a
                // default-context collect) computes: when the source context
                // IS the default context the source is that same collect, so
                // derive the rect from it directly; otherwise fall back to the
                // estimate so the memoized value stays identical.
                let rect_cache_key = layer_cache_key(child_layer.as_ref());
                let child_logical_rect =
                    if let Some(cached_rect) = layer_surface_rect_cache.get(&rect_cache_key) {
                        *cached_rect
                    } else if child_source_ctx == TranslationRenderContext::default() {
                        let estimate_translated_content_context = child_layer
                            .translated_content_context
                            || child_requirements.contains_translated_content;
                        let estimate_effective_requirements = effective_surface_requirements(
                            estimate_translated_content_context,
                            false,
                            child_requirements,
                        );
                        let estimate_axes = translated_content_axes_for_layer(child_layer.as_ref())
                            .union(child_requirements.translated_content_axes);
                        let bounds = motion_stable_capture_bounds(
                            child_layer.as_ref(),
                            &source_scene,
                            &source_children,
                            estimate_effective_requirements,
                            estimate_axes,
                            None,
                        );
                        let rect = resolved_layer_surface_rect(child_layer.as_ref(), bounds);
                        layer_surface_rect_cache.insert(rect_cache_key, rect);
                        rect
                    } else {
                        estimate_layer_surface_rect_cached_with_text_layout(
                            child_layer.as_ref(),
                            text_layout,
                            layer_surface_rect_cache,
                            layer_surface_requirements_cache,
                        )
                    };
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
                let child_composite_snap_origin = if child_requirements
                    .surface_requirements
                    .contains(SurfaceRequirement::NonTranslationTransform)
                {
                    axis_aligned_composite_snap_origin(child_layer, layer_offset)
                } else {
                    None
                };
                let child_snap_anchor = if child_composite_snap_origin.is_some() {
                    None
                } else {
                    translated_snap_anchor.or_else(|| {
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
                    })
                };
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
                let child_contains_descendant_backdrop =
                    layer_contains_descendant_backdrop(child_layer.as_ref());
                let child_needs_nested_underlay = child_contains_descendant_backdrop
                    && layer_source_uses_external_backdrop_underlay(
                        &source_scene,
                        &source_children,
                        true,
                    );
                child_layers.push(ChildLayerComposite {
                    z_index: local_scene.next_z,
                    logical_rect: child_logical_rect,
                    dest_quad: translate_quad(
                        child_layer.transform_to_parent.map_rect(child_logical_rect),
                        layer_offset,
                    ),
                    snap_anchor: child_snap_anchor,
                    composite_snap_origin: child_composite_snap_origin,
                    backdrop_rect: quad_bounds(translate_quad(
                        child_layer
                            .transform_to_parent
                            .map_rect(child_layer.local_bounds),
                        layer_offset,
                    )),
                    visual_clip,
                    surface_clip,
                    shadow_draws: std::mem::take(&mut shadow_scene.shadow_draws),
                    needs_nested_underlay: child_needs_nested_underlay,
                    node_id: child_layer.node_id,
                    backdrop: child_layer.backdrop().cloned(),
                    has_effect: child_layer.effect().is_some(),
                    effect_contains_runtime_shader: child_layer
                        .effect()
                        .is_some_and(|effect| effect.contains_runtime_shader()),
                    target_content_hash: child_layer.target_content_hash(),
                    effect_hash: child_layer.effect_hash(),
                    motion_source_content_hash: child_motion_stable_source
                        .then(|| child_layer.motion_source_content_hash()),
                    contains_descendant_backdrop: child_contains_descendant_backdrop,
                    cache_policy: child_layer.cache_policy,
                    surface_requirements: child_requirements,
                    rounded_clip: LayerSurfaceRoundedClip::from_layer(child_layer.as_ref()),
                    isolation: effective_layer_isolation(&child_layer.graphics_layer),
                    translated_content_context: child_layer.translated_content_context,
                    own_translated_content_axes: translated_content_axes_for_layer(
                        child_layer.as_ref(),
                    ),
                    clip_rect: child_layer.clip_rect(),
                    local_bounds: child_layer.local_bounds,
                    surface_scale: layer_surface_scale(child_layer.as_ref()),
                    source: LoweredChildSource {
                        scene: source_scene,
                        children: source_children,
                    },
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
pub(crate) fn collect_layer_contents(
    layer: &LayerNode,
    inherited_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
) -> CollectedLayer {
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
pub(crate) fn collect_layer_contents_with_translation_context(
    layer: &LayerNode,
    inherited_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    translation_context: TranslationRenderContext,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
) -> CollectedLayer {
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

pub(crate) fn collect_layer_contents_with_translation_context_and_text_layout(
    layer: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    inherited_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    translation_context: TranslationRenderContext,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
) -> CollectedLayer {
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
pub(crate) fn collect_layer_contents_with_capacity(
    layer: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    inherited_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    translation_context: TranslationRenderContext,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
    capacity: SceneCapacityHint,
) -> CollectedLayer {
    let child_surface_ctx =
        derived_child_surface_context(layer, translation_context, layer_surface_requirements_cache);
    collect_layer_contents_reusing(
        layer,
        text_layout,
        inherited_clip,
        inherited_translated_snap_anchor,
        translation_context,
        child_surface_ctx,
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
/// Like [`collect_layer_contents_with_capacity`], but the caller supplies the
/// request context its children will be rendered with (`child_surface_ctx`).
/// The direct-root path hands its children to `render_layer_surface` with the
/// root request context as-is, not the surface-derived one, so it must pass
/// `translation_context` here unchanged.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_layer_contents_reusing(
    layer: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    inherited_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    translation_context: TranslationRenderContext,
    child_surface_ctx: TranslationRenderContext,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
    scene: CompositorScene,
) -> CollectedLayer {
    let mut local_scene = scene;
    local_scene.clear();
    let mut child_layers = Vec::new();
    collect_layer_contents_with_translation_context_into(
        layer,
        text_layout,
        inherited_clip,
        inherited_translated_snap_anchor,
        translation_context,
        child_surface_ctx,
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

/// Builds the snapshot + collected source a root-level layer needs to run the
/// shared snapshot-consuming render body. The collect happens with the same
/// post-transform translation context the old in-body re-collect used.
/// Producer-side since step 6b: the caller supplies the text layout resolver
/// and the frontend's lowering memos instead of a render backend.
pub(crate) fn lower_layer_node(
    layer: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    surface_requirements: LayerSurfaceRequirements,
    logical_rect: Rect,
    translation_context: TranslationRenderContext,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
) -> (ChildLayerComposite, LoweredChildSource) {
    let CollectedLayer {
        scene,
        child_layers,
    } = collect_layer_contents_with_translation_context_and_text_layout(
        layer,
        text_layout,
        None,
        None,
        translation_context,
        layer_surface_rect_cache,
        layer_surface_requirements_cache,
    );
    let contains_descendant_backdrop = layer_contains_descendant_backdrop(layer);
    // The body decides the source cache key with the effective requirements
    // it recomputes from the post-transform context; the motion hash snapshot
    // must exist exactly when that decision reads it.
    let effective_translated_content_context = translation_context.inherited_content_translation
        || layer.translated_content_context
        || surface_requirements.contains_translated_content;
    let motion_stable_source = effective_surface_requirements(
        effective_translated_content_context,
        translation_context.surface_capture_active,
        surface_requirements,
    )
    .contains(SurfaceRequirement::MotionStableCapture);
    let lowered = ChildLayerComposite {
        // Parent-space composite fields are meaningless for a root-level
        // surface; the body never reads them.
        z_index: 0,
        logical_rect,
        dest_quad: [[0.0; 2]; 4],
        snap_anchor: None,
        composite_snap_origin: None,
        backdrop_rect: layer.local_bounds,
        visual_clip: None,
        surface_clip: None,
        shadow_draws: Vec::new(),
        needs_nested_underlay: contains_descendant_backdrop,
        node_id: layer.node_id,
        backdrop: layer.backdrop().cloned(),
        has_effect: layer.effect().is_some(),
        effect_contains_runtime_shader: layer
            .effect()
            .is_some_and(|effect| effect.contains_runtime_shader()),
        target_content_hash: layer.target_content_hash(),
        effect_hash: layer.effect_hash(),
        motion_source_content_hash: motion_stable_source
            .then(|| layer.motion_source_content_hash()),
        contains_descendant_backdrop,
        cache_policy: layer.cache_policy,
        surface_requirements,
        rounded_clip: LayerSurfaceRoundedClip::from_layer(layer),
        isolation: effective_layer_isolation(&layer.graphics_layer),
        translated_content_context: layer.translated_content_context,
        own_translated_content_axes: translated_content_axes_for_layer(layer),
        clip_rect: layer.clip_rect(),
        local_bounds: layer.local_bounds,
        surface_scale: layer_surface_scale(layer),
        source: LoweredChildSource::default(),
    };
    (
        lowered,
        LoweredChildSource {
            scene,
            children: child_layers,
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_layer_contents_with_translation_context_into(
    layer: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    inherited_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<SnapAnchor>,
    translation_context: TranslationRenderContext,
    child_surface_ctx: TranslationRenderContext,
    local_scene: &mut CompositorScene,
    child_layers: &mut Vec<ChildLayerComposite>,
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
        child_surface_ctx,
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

#[cfg(test)]
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
    resolved_layer_surface_rect_from_parts(
        layer.local_bounds,
        layer.effect().is_some(),
        layer.backdrop().is_some(),
        bounds,
    )
}

/// [`resolved_layer_surface_rect`] over collection-time snapshots of the
/// layer scalars it reads.
pub(crate) fn resolved_layer_surface_rect_from_parts(
    local_bounds: Rect,
    has_effect: bool,
    has_backdrop: bool,
    bounds: Option<Rect>,
) -> Rect {
    let rect = bounds.unwrap_or(local_bounds);
    if has_effect || has_backdrop {
        union_rect(Some(rect), local_bounds).unwrap_or(rect)
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

impl TranslateBy for SnapAnchor {
    fn translate_by(&mut self, delta: Point) {
        self.origin.x += delta.x;
        self.origin.y += delta.y;
    }
}

impl TranslateBy for Point {
    fn translate_by(&mut self, delta: Point) {
        self.x += delta.x;
        self.y += delta.y;
    }
}

impl TranslateBy for ChildLayerComposite {
    /// Shifts the parent-space composite fields by the parent's surface
    /// origin. `logical_rect`, `surface_clip` and the whole `source` tree are
    /// expressed in the child's own space and must stay put — the child's
    /// render applies its own shift when it builds its surface.
    fn translate_by(&mut self, delta: Point) {
        for point in &mut self.dest_quad {
            point[0] += delta.x;
            point[1] += delta.y;
        }
        if let Some(anchor) = self.snap_anchor.as_mut() {
            anchor.translate_by(delta);
        }
        if let Some(origin) = self.composite_snap_origin.as_mut() {
            origin.translate_by(delta);
        }
        self.backdrop_rect.translate_by(delta);
        if let Some(clip) = self.visual_clip.as_mut() {
            clip.translate_by(delta);
        }
        self.shadow_draws.translate_by(delta);
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
        if let Some(anchor) = self.snap_anchor.as_mut() {
            anchor.translate_by(delta);
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
        if let Some(anchor) = self.snap_anchor.as_mut() {
            anchor.translate_by(delta);
        }
    }
}

impl TranslateBy for TextDraw {
    fn translate_by(&mut self, delta: Point) {
        self.rect.translate_by(delta);
        if let Some(clip) = self.clip.as_mut() {
            clip.translate_by(delta);
        }
        if let Some(anchor) = self.snap_anchor.as_mut() {
            anchor.translate_by(delta);
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
        if let Some(anchor) = self.snap_anchor.as_mut() {
            anchor.translate_by(delta);
        }
    }
}

impl TranslateBy for BackdropLayer {
    fn translate_by(&mut self, delta: Point) {
        self.rect.translate_by(delta);
        if let Some(clip) = self.clip.as_mut() {
            clip.translate_by(delta);
        }
        if let Some(anchor) = self.snap_anchor.as_mut() {
            anchor.translate_by(delta);
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
    /// The brush table the shapes' gradient handles index — the owning
    /// scene's `brushes`.
    pub(crate) brushes: &'a [Brush],
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
    // Windowed shapes keep their gradient handles, so the window carries the
    // whole (gradients-only, tiny) source table — indices stay valid without
    // remapping.
    scene.brushes.extend_from_slice(source.brushes);
    let mut shape_map = vec![None; source.shapes.len()];
    for (source_index, shape) in source.shapes.iter().enumerate() {
        if shape.z_index >= z_start && shape.z_index < z_end {
            shape_map[source_index] = Some(scene.shapes.len());
            scene.shapes.push(*shape);
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
