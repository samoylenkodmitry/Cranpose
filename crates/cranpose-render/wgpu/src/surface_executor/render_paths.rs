use super::backend::{LayerSurface, LayerSurfaceTexture, SurfaceExecutionBackend};
use super::geometry::{
    axis_aligned_quad_rect, canonicalized_anchored_scaled_quad, clamp_effect_surface_scale,
    content_effect_pixel_rect, device_pixel_exact_surface_rect,
    fit_capture_rect_to_scale_budget_for_axes, offscreen_byte_size,
    quantize_motion_stable_target_scale, scaled_quad, snap_delta_for_anchor,
    snap_dest_quad_to_stable_point, snap_motion_stable_dest_quad, surface_pixel_rect,
    surface_target_size, target_quad, visible_layer_rect,
};
use crate::effect_renderer::{
    CompositeBatchItem, CompositeSampleMode, ProjectiveSurfaceComposite, RoundedCompositeMask,
    ShaderCompositeBatchItem,
};
use crate::layer_events::{collect_effect_ranges, collect_layer_events, LayerEventKind};
use crate::layer_surface_cache::{
    MAX_LAYER_SURFACE_CACHE_BYTES, MAX_SCENE_RANGE_CACHE_ENTRY_BYTES,
};
use crate::normalized_scene::{
    build_scene_window, collected_layer_bounds, filtered_effect_layer_index,
    motion_stable_capture_bounds_from_parts, resolved_child_surface_composite,
    resolved_layer_surface_rect_from_parts, translate_quad, visible_draw_rect, ChildLayerComposite,
    CollectedLayer, LoweredChildSource, SceneWindowSource, TranslateBy,
};
use crate::offscreen::OffscreenTarget;
use crate::render::{has_backdrop_layer_in_range, scissor_rect_for_rect};
use crate::scene::{
    BackdropLayer, CompositorScene, DrawOp, DrawOpKind, DrawShape, EffectLayer, ImageDraw,
    SceneBrush, ShadowDraw, SnapAnchor, TextDraw,
};
use crate::surface_plan::{
    composite_sample_mode_for_effect_layer, composite_sample_mode_for_requirements,
    effect_layer_minimum_scale, effect_layer_target_scale, effective_surface_requirements,
    layer_surface_target_scale, LayerSurfaceRenderOptions, LayerSurfaceRequest,
    TranslatedContentAxes, TranslationRenderContext,
};
use crate::surface_requirements::{SurfaceRequirement, SurfaceRequirementSet};
use cranpose_core::collections::map::HashMap;
use cranpose_core::NodeId;
use cranpose_render_common::geometry::union_rect;
use cranpose_render_common::graph::{quad_bounds, CachePolicy, ProjectiveTransform};
use cranpose_render_common::raster_cache::{LayerRasterCacheKey, ScaleBucket};
use cranpose_ui::text::LinkKey;
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, FxHasher, Point, Rect, RenderEffect, RenderHash, RuntimeShader,
};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};

fn layer_render_diag_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_LAYER_RENDER_DIAG").is_some())
}

fn direct_scene_range_cache_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_DISABLE_DIRECT_SCENE_RANGE_CACHE").is_none())
}

fn direct_scene_range_cache_enable_all() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_ENABLE_DIRECT_SCENE_RANGE_CACHE").is_some())
}

fn direct_scene_range_coalesce_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED
        .get_or_init(|| std::env::var("CRANPOSE_DIRECT_SCENE_RANGE_COALESCE").as_deref() != Ok("0"))
}

/// Merges consecutive direct-rendered cache chunks into one flush range.
///
/// Chunk boundaries exist only so a chunk *could* become a scene-range cache
/// entry. A chunk that ends up rendering directly — its content hash missed
/// and admission refused it, or it never got a cache key — gains nothing from
/// the boundary, and on a tile-based mobile GPU every extra render pass is a
/// full tile store and reload of the target. An animated scene is the worst
/// case: every chunk's key changes every frame, so every chunk misses, and a
/// small-screen game frame was being cut into ~18 passes of 64 draw ops each.
/// Absorbing consecutive direct chunks and flushing once — before a chunk
/// that composites from the cache, or at the end of the walk — renders the
/// same ops in the same order in a single pass.
#[derive(Default)]
struct DirectChunkRunCoalescer {
    run_start: Option<usize>,
}

impl DirectChunkRunCoalescer {
    fn absorb(&mut self, chunk_start: usize) {
        self.run_start.get_or_insert(chunk_start);
    }

    fn flush_at(&mut self, boundary: usize) -> Option<(usize, usize)> {
        match self.run_start {
            Some(start) if start < boundary => {
                self.run_start = None;
                Some((start, boundary))
            }
            _ => None,
        }
    }
}

fn direct_scene_range_cache_enabled_for_entry_bytes(byte_size: u64) -> bool {
    direct_scene_range_cache_enabled_for_policy(
        direct_scene_range_cache_enable_all(),
        !direct_scene_range_cache_enabled(),
        byte_size,
    )
}

fn direct_scene_range_cache_enabled_for_policy(
    enable_all: bool,
    disable_all: bool,
    byte_size: u64,
) -> bool {
    !disable_all && (enable_all || byte_size <= DEFAULT_DIRECT_SCENE_RANGE_CACHE_BYTES)
}

fn direct_scene_range_hash_diag_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_DIRECT_SCENE_RANGE_HASH_DIAG").is_some())
}

fn direct_scene_range_hash_detail_z() -> Option<usize> {
    static DETAIL_Z: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    *DETAIL_Z.get_or_init(|| {
        std::env::var("CRANPOSE_DIRECT_SCENE_RANGE_HASH_DETAIL_Z")
            .ok()
            .and_then(|value| value.parse().ok())
    })
}

const MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS: usize = 2;
const MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS: usize = 64;
const MIN_SINGLE_DRAW_DIRECT_SCENE_RANGE_CACHE_BYTES: u64 = 256 * 1024;
const MAX_MOTION_SENSITIVE_DIRECT_SCENE_CACHE_DRAW_BYTES: u64 = 1_048_576;
const DEFAULT_DIRECT_SCENE_RANGE_CACHE_BYTES: u64 = 1024 * 1024;
const MAX_DIRECT_SCENE_RANGE_CACHE_BYTES: u64 = MAX_SCENE_RANGE_CACHE_ENTRY_BYTES;
const RENDER_STRING_HASH_CACHE_CAPACITY: usize = 2048;

thread_local! {
    static RENDER_STRING_HASH_CACHE: RefCell<RenderStringHashCache> =
        RefCell::new(RenderStringHashCache::default());
}

#[derive(Default)]
struct RenderStringHashCache {
    entries: HashMap<usize, RenderStringHashEntry>,
}

struct RenderStringHashEntry {
    text: std::sync::Weak<cranpose_ui::text::RenderString>,
    hash: u64,
}

impl RenderStringHashCache {
    fn get_or_insert(&mut self, text: &std::sync::Arc<cranpose_ui::text::RenderString>) -> u64 {
        let key = std::sync::Arc::as_ptr(text) as usize;
        if let Some(entry) = self.entries.get(&key) {
            if entry.text.strong_count() > 0 && entry.text.as_ptr() == std::sync::Arc::as_ptr(text)
            {
                return entry.hash;
            }
        }

        let hash = compute_render_string_hash(text);
        if self.entries.len() >= RENDER_STRING_HASH_CACHE_CAPACITY {
            self.entries
                .retain(|_, entry| entry.text.strong_count() > 0);
            if self.entries.len() >= RENDER_STRING_HASH_CACHE_CAPACITY {
                self.entries.clear();
            }
        }
        self.entries.insert(
            key,
            RenderStringHashEntry {
                text: std::sync::Arc::downgrade(text),
                hash,
            },
        );
        hash
    }
}

fn anchored_composite_dest_quad(
    dest_quad: [[f32; 2]; 4],
    snap_anchor: Option<SnapAnchor>,
    stable_origin: Option<Point>,
    root_scale: f32,
    sample_mode: CompositeSampleMode,
) -> [[f32; 2]; 4] {
    let scaled = if let Some(anchor) = snap_anchor {
        canonicalized_anchored_scaled_quad(dest_quad, anchor, root_scale)
    } else {
        scaled_quad(dest_quad, root_scale)
    };

    if let Some(origin) = stable_origin {
        let stable_point = [origin.x * root_scale, origin.y * root_scale];
        snap_dest_quad_to_stable_point(scaled, stable_point)
    } else {
        snap_motion_stable_dest_quad(scaled, sample_mode)
    }
}

fn composite_dest_viewport(
    dest_rect: Rect,
    source_width: u32,
    source_height: u32,
    sample_mode: CompositeSampleMode,
) -> (f32, f32, f32, f32) {
    if sample_mode != CompositeSampleMode::Box4 {
        return (dest_rect.x, dest_rect.y, dest_rect.width, dest_rect.height);
    }

    let source_width = source_width as f32;
    let source_height = source_height as f32;
    let width = if source_width.is_finite() && (source_width - dest_rect.width).abs() <= 1.0 {
        source_width
    } else {
        dest_rect.width
    };
    let height = if source_height.is_finite() && (source_height - dest_rect.height).abs() <= 1.0 {
        source_height
    } else {
        dest_rect.height
    };

    (dest_rect.x.round(), dest_rect.y.round(), width, height)
}

fn layer_surface_dest_quad(
    child_logical_rect: Rect,
    child_dest_quad: [[f32; 2]; 4],
    surface_logical_rect: Rect,
) -> [[f32; 2]; 4] {
    ProjectiveTransform::from_rect_to_quad(child_logical_rect, child_dest_quad)
        .map_rect(surface_logical_rect)
}

fn quad_bounds_rect(quad: [[f32; 2]; 4]) -> Option<Rect> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for [x, y] in quad {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    let width = max_x - min_x;
    let height = max_y - min_y;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    Some(Rect {
        x: min_x,
        y: min_y,
        width,
        height,
    })
}

fn child_composite_visible(
    dest_quad: [[f32; 2]; 4],
    visual_clip: Option<Rect>,
    root_scale: f32,
    width: u32,
    height: u32,
) -> bool {
    let Some(bounds) = axis_aligned_quad_rect(dest_quad).or_else(|| quad_bounds_rect(dest_quad))
    else {
        return false;
    };
    visible_layer_rect(bounds, visual_clip, root_scale, width, height).is_some()
}

fn rect_contains_rect(outer: Rect, inner: Rect) -> bool {
    const EPSILON: f32 = 0.001;
    inner.x + EPSILON >= outer.x
        && inner.y + EPSILON >= outer.y
        && inner.x + inner.width <= outer.x + outer.width + EPSILON
        && inner.y + inner.height <= outer.y + outer.height + EPSILON
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

fn clip_contains_rect(clip: Option<Rect>, rect: Rect) -> bool {
    clip.is_none_or(|clip| rect_contains_rect(clip, rect))
}

fn scene_brush_is_opaque(brush: SceneBrush, brushes: &[Brush]) -> bool {
    match brush {
        SceneBrush::Solid(color) => brush_is_opaque(&Brush::Solid(color)),
        SceneBrush::Gradient(index) => brush_is_opaque(&brushes[index as usize]),
    }
}

fn brush_is_opaque(brush: &Brush) -> bool {
    const OPAQUE_ALPHA: f32 = 0.999;
    match brush {
        Brush::Solid(color) => color.a() >= OPAQUE_ALPHA,
        Brush::LinearGradient { colors, .. }
        | Brush::RadialGradient { colors, .. }
        | Brush::SweepGradient { colors, .. } => {
            !colors.is_empty() && colors.iter().all(|color| color.a() >= OPAQUE_ALPHA)
        }
    }
}

/// Whether a filled rect with these corner radii paints every pixel of `rect`.
///
/// A rounded fill misses only its four corner squares, so a band that clears
/// them on one axis is covered whatever it does on the other: a row's frosted
/// button sits mid-height, far from the card's rounded corners, and the card
/// behind it is the whole of the button's backdrop input. Reading that as "not
/// covered" costs the row its raster cache on every frame it scrolls.
fn rounded_fill_covers_rect(
    bounds: Rect,
    shape: Option<cranpose_ui_graphics::RoundedCornerShape>,
    rect: Rect,
) -> bool {
    if !rect_contains_rect(bounds, rect) {
        return false;
    }
    let Some(shape) = shape else {
        return true;
    };
    let radii = shape.radii();
    let radius = radii
        .top_left
        .max(radii.top_right)
        .max(radii.bottom_right)
        .max(radii.bottom_left);
    if !(radius > 0.0) {
        return true;
    }
    let clears_corners_vertically =
        rect.y >= bounds.y + radius && rect.y + rect.height <= bounds.y + bounds.height - radius;
    let clears_corners_horizontally =
        rect.x >= bounds.x + radius && rect.x + rect.width <= bounds.x + bounds.width - radius;
    clears_corners_vertically || clears_corners_horizontally
}

fn shape_opaque_covers_rect(shape: &DrawShape, brushes: &[Brush], rect: Rect) -> bool {
    shape.blend_mode == BlendMode::SrcOver
        && shape.stroke.is_none()
        && shape.arc.is_none()
        && scene_brush_is_opaque(shape.brush, brushes)
        && clip_contains_rect(shape.clip, rect)
        && axis_aligned_quad_rect(shape.quad)
            .is_some_and(|bounds| rounded_fill_covers_rect(bounds, shape.shape, rect))
}

fn image_opaque_covers_rect(image: &ImageDraw, rect: Rect) -> bool {
    image.blend_mode == BlendMode::SrcOver
        && image.alpha >= 0.999
        && image.color_filter.is_none()
        && image.image.is_opaque()
        && clip_contains_rect(image.clip, rect)
        && axis_aligned_quad_rect(image.quad).is_some_and(|bounds| rect_contains_rect(bounds, rect))
}

fn draw_can_reduce_alpha(
    shapes: &[DrawShape],
    images: &[ImageDraw],
    shadow_draws: &[ShadowDraw],
    op: DrawOp,
    rect: Rect,
) -> bool {
    match op.kind {
        DrawOpKind::Shape(index) => shapes.get(index).is_some_and(|shape| {
            shape.blend_mode != BlendMode::SrcOver
                && axis_aligned_quad_rect(shape.quad)
                    .is_some_and(|bounds| rects_intersect(bounds, rect))
                && clip_contains_rect(shape.clip, rect)
        }),
        DrawOpKind::Image(index) => images.get(index).is_some_and(|image| {
            image.blend_mode != BlendMode::SrcOver
                && axis_aligned_quad_rect(image.quad)
                    .is_some_and(|bounds| rects_intersect(bounds, rect))
                && clip_contains_rect(image.clip, rect)
        }),
        DrawOpKind::Text(_) => false,
        DrawOpKind::Shadow(index) => shadow_draws.get(index).is_some_and(|shadow| {
            shadow.shapes.iter().any(|(shape, blend_mode)| {
                *blend_mode != BlendMode::SrcOver && rects_intersect(shape.rect, rect)
            })
        }),
        // Replayed batches are SrcOver-only by construction.
        DrawOpKind::Retained(_) => false,
    }
}

fn prior_layer_event_intersects_rect(
    effect_layers: &[EffectLayer],
    backdrop_layers: &[BackdropLayer],
    z_end: usize,
    rect: Rect,
) -> bool {
    effect_layers
        .iter()
        .any(|layer| layer.z_start < z_end && rects_intersect(layer.rect, rect))
        || backdrop_layers
            .iter()
            .any(|layer| layer.z_index < z_end && rects_intersect(layer.rect, rect))
}

#[allow(clippy::too_many_arguments)]
fn scene_range_has_opaque_cover_before(
    shapes: &[DrawShape],
    brushes: &[Brush],
    images: &[ImageDraw],
    shadow_draws: &[ShadowDraw],
    draw_ops: &[DrawOp],
    effect_layers: &[EffectLayer],
    backdrop_layers: &[BackdropLayer],
    z_end: usize,
    rect: Rect,
) -> bool {
    if prior_layer_event_intersects_rect(effect_layers, backdrop_layers, z_end, rect) {
        return false;
    }

    for op in scene_range_draw_ops(draw_ops, 0, z_end)
        .iter()
        .rev()
        .copied()
    {
        match op.kind {
            DrawOpKind::Shape(index) => {
                if shapes
                    .get(index)
                    .is_some_and(|shape| shape_opaque_covers_rect(shape, brushes, rect))
                {
                    return true;
                }
            }
            // A replayed batch is thousands of blended shapes; treating any
            // of it as opaque cover would need per-shape analysis it exists
            // to skip.
            DrawOpKind::Retained(_) => {}
            DrawOpKind::Image(index) => {
                if images
                    .get(index)
                    .is_some_and(|image| image_opaque_covers_rect(image, rect))
                {
                    return true;
                }
            }
            DrawOpKind::Text(_) | DrawOpKind::Shadow(_) => {}
        }
        if draw_can_reduce_alpha(shapes, images, shadow_draws, op, rect) {
            return false;
        }
    }

    false
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn backdrop_underlay_is_covered_by_local_content(
    shapes: &[DrawShape],
    brushes: &[Brush],
    images: &[ImageDraw],
    shadow_draws: &[ShadowDraw],
    draw_ops: &[DrawOp],
    effect_layers: &[EffectLayer],
    backdrop_layers: &[BackdropLayer],
    layer: &BackdropLayer,
) -> bool {
    let rect = layer
        .clip
        .and_then(|clip| layer.rect.intersect(clip))
        .unwrap_or(layer.rect);
    let covered = rect.width > 0.0
        && rect.height > 0.0
        && scene_range_has_opaque_cover_before(
            shapes,
            brushes,
            images,
            shadow_draws,
            draw_ops,
            effect_layers,
            backdrop_layers,
            layer.z_index,
            rect,
        );
    if !covered && layer_render_diag_enabled() {
        let prior_event =
            prior_layer_event_intersects_rect(effect_layers, backdrop_layers, layer.z_index, rect);
        let ops_below = scene_range_draw_ops(draw_ops, 0, layer.z_index).len();
        let shapes_below = shapes.iter().filter(|s| s.z_index < layer.z_index).count();
        log::warn!(
            "[layer-render-diag] backdrop node={:?} rect=({:.1},{:.1},{:.1},{:.1}) prior_event={prior_event} ops_below={ops_below} shapes_below={shapes_below}",
            layer.node_id,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
        );
        for shape in shapes.iter().filter(|s| s.z_index < layer.z_index).take(4) {
            log::warn!(
                "[layer-render-diag]   shape z={} bounds={:?} rounded={} stroke={} arc={} blend={:?} opaque={} clip_ok={}",
                shape.z_index,
                axis_aligned_quad_rect(shape.quad),
                shape.shape.is_some(),
                shape.stroke.is_some(),
                shape.arc.is_some(),
                shape.blend_mode,
                scene_brush_is_opaque(shape.brush, brushes),
                clip_contains_rect(shape.clip, rect),
            );
        }
    }
    covered
}

/// The one colour the underlay under `rect` holds, when it holds only one.
///
/// A child whose glass reads a flat colour reads the same colour wherever the
/// child sits, so its raster stays good while it moves. That needs the whole
/// rect covered by a single opaque solid fill with nothing over it: any other
/// draw that reaches into the rect, any layer event below it, and any sibling
/// already composited into the target all end it. When nothing at all reaches
/// the rect the answer is whatever the parent's own underlay holds.
#[allow(clippy::too_many_arguments)]
pub(crate) fn uniform_underlay_color_before(
    scene: &CompositorScene,
    prior_child_contributions: &[BackdropPrefixChildContribution],
    z_end: usize,
    rect: Rect,
    root_scale: f32,
    inherited: Option<u32>,
) -> Option<u32> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    if prior_layer_event_intersects_rect(&scene.effect_layers, &scene.backdrop_layers, z_end, rect)
    {
        return None;
    }
    let rect_pixels = surface_pixel_rect(rect, root_scale);
    let sibling_over_rect = prior_child_contributions.iter().any(|child| {
        child.z_index < z_end && dest_quad_intersects_rect(child.dest_quad, rect_pixels)
    });
    if sibling_over_rect {
        return None;
    }

    for op in scene_range_draw_ops(&scene.draw_ops, 0, z_end)
        .iter()
        .rev()
        .copied()
    {
        let touches =
            draw_op_visible_bounds(scene, op).is_none_or(|bounds| rects_intersect(bounds, rect));
        if !touches {
            continue;
        }
        let DrawOpKind::Shape(index) = op.kind else {
            return None;
        };
        let shape = scene.shapes.get(index)?;
        if !shape_opaque_covers_rect(shape, &scene.brushes, rect) {
            return None;
        }
        let SceneBrush::Solid(color) = shape.brush else {
            return None;
        };
        return Some(solid_color_bits(color));
    }

    inherited
}

fn solid_color_bits(color: Color) -> u32 {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
    (channel(color.r()) << 24)
        | (channel(color.g()) << 16)
        | (channel(color.b()) << 8)
        | channel(color.a())
}

pub(crate) fn layer_source_uses_external_backdrop_underlay(
    local_scene: &CompositorScene,
    child_layers: &[ChildLayerComposite],
    has_backdrop_underlay: bool,
) -> bool {
    if !has_backdrop_underlay {
        return false;
    }

    let scene_backdrop_reads_outside = local_scene.backdrop_layers.iter().any(|backdrop_layer| {
        !backdrop_underlay_is_covered_by_local_content(
            &local_scene.shapes,
            &local_scene.brushes,
            &local_scene.images,
            &local_scene.shadow_draws,
            &local_scene.draw_ops,
            &local_scene.effect_layers,
            &local_scene.backdrop_layers,
            backdrop_layer,
        )
    });
    if scene_backdrop_reads_outside {
        return true;
    }

    child_layers.iter().any(|child| {
        if child.contains_descendant_backdrop {
            return true;
        }

        let Some(effect) = child.backdrop.as_ref() else {
            return false;
        };

        let backdrop_layer = BackdropLayer {
            node_id: child.node_id,
            rect: child.backdrop_rect,
            clip: child.visual_clip,
            snap_anchor: child.snap_anchor,
            effect: effect.clone(),
            z_index: child.z_index,
        };

        !backdrop_underlay_is_covered_by_local_content(
            &local_scene.shapes,
            &local_scene.brushes,
            &local_scene.images,
            &local_scene.shadow_draws,
            &local_scene.draw_ops,
            &local_scene.effect_layers,
            &local_scene.backdrop_layers,
            &backdrop_layer,
        )
    })
}

fn combined_capture_clip(layer_clip: Option<Rect>, capture_clip: Option<Rect>) -> Option<Rect> {
    match (layer_clip, capture_clip) {
        (Some(layer_clip), Some(capture_clip)) => layer_clip.intersect(capture_clip),
        (Some(clip), None) | (None, Some(clip)) => Some(clip),
        (None, None) => None,
    }
}

fn source_pixel_region_batch_item<'a>(
    source: &'a OffscreenTarget,
    source_pixel_rect: Rect,
    dest_size: (u32, u32),
) -> CompositeBatchItem<'a> {
    CompositeBatchItem {
        source,
        alpha: 1.0,
        scissor: None,
        rounded_mask: None,
        blend_mode: BlendMode::SrcOver,
        dest_viewport: Some((0.0, 0.0, dest_size.0 as f32, dest_size.1 as f32)),
        source_viewport: Some((
            source_pixel_rect.x,
            source_pixel_rect.y,
            source_pixel_rect.width,
            source_pixel_rect.height,
        )),
        sample_mode: CompositeSampleMode::Linear,
    }
}

fn scissored_batch_item<'a>(
    mut item: CompositeBatchItem<'a>,
    scissor: Option<(u32, u32, u32, u32)>,
) -> CompositeBatchItem<'a> {
    if scissor.is_some() {
        item.scissor = scissor;
    }
    item
}

fn source_region_batch_item<'a>(
    source: &'a OffscreenTarget,
    source_rect: Rect,
    dest_size: (u32, u32),
    root_scale: f32,
) -> CompositeBatchItem<'a> {
    source_pixel_region_batch_item(
        source,
        surface_pixel_rect(source_rect, root_scale),
        dest_size,
    )
}

fn copy_projective_backdrop_inputs_to_view<B: SurfaceExecutionBackend>(
    backend: &mut B,
    backdrop_underlay: Option<&OffscreenTarget>,
    target: &OffscreenTarget,
    source_rect: Rect,
    dest_view: &wgpu::TextureView,
    dest_size: (u32, u32),
    root_scale: f32,
) -> Result<(), String> {
    if let Some(underlay) = backdrop_underlay {
        let composites = [
            source_region_batch_item(underlay, source_rect, dest_size, root_scale),
            source_region_batch_item(target, source_rect, dest_size, root_scale),
        ];
        backend.composite_surface_batch_to_view(
            dest_view,
            dest_size,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &composites,
        );
    } else {
        let composites = [source_region_batch_item(
            target,
            source_rect,
            dest_size,
            root_scale,
        )];
        backend.composite_surface_batch_to_view(
            dest_view,
            dest_size,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &composites,
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BackdropSnapshotCopyPlan {
    source_origin: (u32, u32),
    size: (u32, u32),
    effect_pixel_rect: [f32; 4],
    dest_viewport: (f32, f32, f32, f32),
}

fn axis_aligned_backdrop_snapshot_copy_plan(
    capture_rect: Rect,
    effect_rect: Rect,
    root_scale: f32,
    source_size: (u32, u32),
    max_texture_dim: u32,
) -> Option<BackdropSnapshotCopyPlan> {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }

    let capture_pixel_rect = surface_pixel_rect(capture_rect, root_scale);
    let effect_pixel_rect = surface_pixel_rect(effect_rect, root_scale);
    let left = capture_pixel_rect.x.floor();
    let top = capture_pixel_rect.y.floor();
    let right = (capture_pixel_rect.x + capture_pixel_rect.width).ceil();
    let bottom = (capture_pixel_rect.y + capture_pixel_rect.height).ceil();
    if left < 0.0 || top < 0.0 || right <= left || bottom <= top {
        return None;
    }

    let origin = (left as u32, top as u32);
    let size = ((right - left) as u32, (bottom - top) as u32);
    if size.0 == 0 || size.1 == 0 || size.0 > max_texture_dim || size.1 > max_texture_dim {
        return None;
    }
    if origin.0.checked_add(size.0)? > source_size.0
        || origin.1.checked_add(size.1)? > source_size.1
    {
        return None;
    }

    Some(BackdropSnapshotCopyPlan {
        source_origin: origin,
        size,
        effect_pixel_rect: [
            effect_pixel_rect.x - left,
            effect_pixel_rect.y - top,
            effect_pixel_rect.width,
            effect_pixel_rect.height,
        ],
        dest_viewport: (left, top, right - left, bottom - top),
    })
}

/// Env-gated backdrop composite diagnostics (`CRANPOSE_BACKDROP_DIAG=1`):
/// prints capture/scissor/padding decisions per backdrop layer to stderr —
/// works in release binaries without a logger.
fn backdrop_diag_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_BACKDROP_DIAG").is_some())
}

fn snapped_backdrop_geometry(layer: &BackdropLayer, root_scale: f32) -> (Rect, Option<Rect>) {
    let Some(anchor) = layer.snap_anchor else {
        return (layer.rect, layer.clip);
    };
    let delta = snap_delta_for_anchor(anchor, root_scale);
    (layer.rect.translate(delta.x, delta.y), layer.clip)
}

fn backdrop_capture_rect(
    effect_rect: Rect,
    clip: Option<Rect>,
    effect: &RenderEffect,
    root_scale: f32,
    target_size: (u32, u32),
) -> Rect {
    // The capture must cover both the sampling reach (input padding) and
    // everywhere the effect writes (output padding) — the shader needs real
    // backdrop texels under every pixel it produces.
    let padding = effect.input_padding().max(effect.output_padding());
    if !padding.is_finite() || padding <= 0.0 || !root_scale.is_finite() || root_scale <= 0.0 {
        return effect_rect;
    }

    let expanded = Rect {
        x: effect_rect.x - padding,
        y: effect_rect.y - padding,
        width: effect_rect.width + padding * 2.0,
        height: effect_rect.height + padding * 2.0,
    };
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: target_size.0 as f32 / root_scale,
        height: target_size.1 as f32 / root_scale,
    };
    let clipped = expanded.intersect(viewport).unwrap_or(effect_rect);
    clip.and_then(|clip| clipped.intersect(clip))
        .unwrap_or(clipped)
}

/// Where a backdrop effect may WRITE: the tight rect widened by the effect's
/// declared output padding (SDF coverage past node bounds — rim glow, wobble,
/// glued neighbor shapes), still clipped to the viewport and the layer clip.
/// The composite scissor derives from this instead of the tight rect.
fn backdrop_output_rect(
    effect_rect: Rect,
    clip: Option<Rect>,
    effect: &RenderEffect,
    root_scale: f32,
    target_size: (u32, u32),
) -> Rect {
    let padding = effect.output_padding();
    if !padding.is_finite() || padding <= 0.0 || !root_scale.is_finite() || root_scale <= 0.0 {
        return effect_rect;
    }

    let expanded = Rect {
        x: effect_rect.x - padding,
        y: effect_rect.y - padding,
        width: effect_rect.width + padding * 2.0,
        height: effect_rect.height + padding * 2.0,
    };
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: target_size.0 as f32 / root_scale,
        height: target_size.1 as f32 / root_scale,
    };
    let clipped = expanded.intersect(viewport).unwrap_or(effect_rect);
    clip.and_then(|clip| clipped.intersect(clip))
        .unwrap_or(clipped)
}

fn visible_backdrop_capture_rect(
    effect_rect: Rect,
    clip: Option<Rect>,
    effect: &RenderEffect,
    root_scale: f32,
    target_size: (u32, u32),
) -> Option<Rect> {
    let visible_rect =
        visible_layer_rect(effect_rect, clip, root_scale, target_size.0, target_size.1)?;
    Some(backdrop_capture_rect(
        visible_rect,
        clip,
        effect,
        root_scale,
        target_size,
    ))
}

#[cfg(test)]
fn axis_aligned_backdrop_copy_region(
    visible_rect: Rect,
    root_scale: f32,
    source_size: (u32, u32),
    target_size: (u32, u32),
) -> Option<((u32, u32), (u32, u32))> {
    const PIXEL_EPSILON: f32 = 0.01;
    let plan = axis_aligned_backdrop_snapshot_copy_plan(
        visible_rect,
        visible_rect,
        root_scale,
        source_size,
        target_size.0.max(target_size.1),
    )?;
    if plan.size != target_size
        || plan.effect_pixel_rect[0].abs() > PIXEL_EPSILON
        || plan.effect_pixel_rect[1].abs() > PIXEL_EPSILON
        || (plan.effect_pixel_rect[2] - target_size.0 as f32).abs() > PIXEL_EPSILON
        || (plan.effect_pixel_rect[3] - target_size.1 as f32).abs() > PIXEL_EPSILON
    {
        return None;
    }
    Some((plan.source_origin, plan.size))
}

fn flush_pending_clear<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target_view: &wgpu::TextureView,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
) {
    if matches!(*next_load_op, wgpu::LoadOp::Clear(_)) {
        backend.clear_target_view_with_load_op(target_view, *next_load_op);
        *next_load_op = wgpu::LoadOp::Load;
    }
}

/// The scale a child layer's own surface renders at, and therefore the scale
/// every texture handed to that surface has to be built at.
///
/// A magnifying layer is composited by mapping its surface through its own
/// transform, so the texture carries that magnification's worth of detail
/// instead of resampling a 1x raster upward — which means the child's scale is
/// **not** the parent's whenever a layer scales. Both the surface and its
/// backdrop underlay resolve it here so they cannot disagree.
fn child_surface_target_scale(
    child: &ChildLayerComposite,
    root_scale: f32,
    translation_context: TranslationRenderContext,
) -> f32 {
    layer_surface_target_scale(
        translation_context.inherited_content_translation || child.translated_content_context,
        translation_context.surface_capture_active,
        child.surface_requirements,
        root_scale,
        child.surface_scale,
    )
}

/// Projects the content behind a child layer into an underlay that layer's own
/// nested backdrops can sample.
///
/// Two different scales meet here, and conflating them silently rescales every
/// backdrop inside the child:
///
/// * `parent_scale` maps the parent surface's logical coordinates to its pixels,
///   and is what `child_dest_quad` — the region of the parent this content is
///   read from — is expressed against.
/// * `child_scale` is the scale the child's own surface renders at, which
///   [`layer_surface_target_scale`] quantizes *up* for a magnifying layer (a
///   1.03x press lift becomes 1.25x). Everything inside that surface, the
///   nested backdrop capture included, addresses this texture at `child_scale`,
///   so that is the resolution it has to be built at.
///
/// They are equal for every layer that does not magnify, which is why sharing
/// one scale looked correct until a glass surface grew a press transform: the
/// backdrop then showed the world at `1 / child_scale * parent_scale` of its
/// true size, anchored at the surface origin.
/// Every rect the child's own render will read back out of the underlay,
/// in the child's local coordinates: each backdrop it holds, grown by the
/// reach of its effect, and the whole quad of any child that carries a
/// backdrop deeper down, since that child builds its own underlay out of
/// this one.
fn underlay_sample_rect(source: &LoweredChildSource) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for layer in &source.scene.backdrop_layers {
        let rect = padded_backdrop_rect(layer.rect, &layer.effect);
        let rect = layer
            .clip
            .and_then(|clip| rect.intersect(clip))
            .unwrap_or(rect);
        bounds = union_rect(bounds, rect);
    }
    for layer in &source.scene.effect_layers {
        let rect = layer
            .clip
            .and_then(|clip| layer.rect.intersect(clip))
            .unwrap_or(layer.rect);
        bounds = union_rect(bounds, rect);
    }
    for child in &source.children {
        if let Some(effect) = child.backdrop.as_ref() {
            let rect = padded_backdrop_rect(child.backdrop_rect, effect);
            let rect = child
                .visual_clip
                .and_then(|clip| rect.intersect(clip))
                .unwrap_or(rect);
            bounds = union_rect(bounds, rect);
        }
        if child.contains_descendant_backdrop {
            let rect = quad_bounds(child.dest_quad);
            let rect = child
                .visual_clip
                .and_then(|clip| rect.intersect(clip))
                .unwrap_or(rect);
            bounds = union_rect(bounds, rect);
        }
    }
    bounds
}

fn padded_backdrop_rect(rect: Rect, effect: &RenderEffect) -> Rect {
    let padding = effect.input_padding().max(effect.output_padding());
    if !padding.is_finite() || padding <= 0.0 {
        return rect;
    }
    Rect {
        x: rect.x - padding,
        y: rect.y - padding,
        width: rect.width + padding * 2.0,
        height: rect.height + padding * 2.0,
    }
}

fn underlay_fill_scissor(
    sample_rect: Option<Rect>,
    child_logical_rect: Rect,
    child_scale: f32,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let rect = sample_rect?.translate(-child_logical_rect.x, -child_logical_rect.y);
    // Snapping moves a backdrop by up to a device pixel after this union is
    // taken, so the fill keeps a margin that far out.
    let margin = if child_scale.is_finite() && child_scale > 0.0 {
        2.0 / child_scale
    } else {
        2.0
    };
    let rect = Rect {
        x: rect.x - margin,
        y: rect.y - margin,
        width: rect.width + margin * 2.0,
        height: rect.height + margin * 2.0,
    };
    crate::render::scissor_rect_for_rect(rect, child_scale, width, height)
}

#[allow(clippy::too_many_arguments)]
fn create_projected_child_underlay<B: SurfaceExecutionBackend>(
    backend: &mut B,
    parent_target: &OffscreenTarget,
    parent_underlay: Option<&OffscreenTarget>,
    child_logical_rect: Rect,
    child_dest_quad: [[f32; 2]; 4],
    parent_scale: f32,
    child_scale: f32,
    sample_rect: Option<Rect>,
) -> OffscreenTarget {
    let (width, height) =
        surface_target_size(child_logical_rect, child_scale, backend.max_texture_dim());
    let underlay = backend.acquire_frame_surface(width, height);
    let fill_scissor =
        underlay_fill_scissor(sample_rect, child_logical_rect, child_scale, width, height);
    let child_source_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    if let Some(source_pixel_rect) =
        axis_aligned_quad_rect(scaled_quad(child_dest_quad, parent_scale))
    {
        if let Some(ancestor_underlay) = parent_underlay {
            let composites = [
                scissored_batch_item(
                    source_pixel_region_batch_item(
                        ancestor_underlay,
                        source_pixel_rect,
                        (width, height),
                    ),
                    fill_scissor,
                ),
                scissored_batch_item(
                    source_pixel_region_batch_item(
                        parent_target,
                        source_pixel_rect,
                        (width, height),
                    ),
                    fill_scissor,
                ),
            ];
            backend.composite_surface_batch_to_view(
                &underlay.view,
                (width, height),
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                &composites,
            );
        } else {
            let composites = [scissored_batch_item(
                source_pixel_region_batch_item(parent_target, source_pixel_rect, (width, height)),
                fill_scissor,
            )];
            backend.composite_surface_batch_to_view(
                &underlay.view,
                (width, height),
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                &composites,
            );
        }
        return underlay;
    }
    let transform = ProjectiveTransform::from_rect_to_quad(
        child_source_rect,
        scaled_quad(child_dest_quad, parent_scale),
    );
    let dest_quad = target_quad(width, height);
    let parent_composite = ProjectiveSurfaceComposite {
        source: parent_target,
        source_size: (parent_target.width as f32, parent_target.height as f32),
        inverse_matrix: transform.matrix(),
        dest_bounds: dest_quad,
        alpha: 1.0,
        load_op: if parent_underlay.is_some() {
            wgpu::LoadOp::Load
        } else {
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
        },
        scissor: fill_scissor,
        blend_mode: BlendMode::SrcOver,
        sample_mode: CompositeSampleMode::Linear,
    };

    if let Some(ancestor_underlay) = parent_underlay {
        let ancestor_composite = ProjectiveSurfaceComposite {
            source: ancestor_underlay,
            source_size: (
                ancestor_underlay.width as f32,
                ancestor_underlay.height as f32,
            ),
            inverse_matrix: transform.matrix(),
            dest_bounds: dest_quad,
            alpha: 1.0,
            load_op: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            scissor: fill_scissor,
            blend_mode: BlendMode::SrcOver,
            sample_mode: CompositeSampleMode::Linear,
        };
        let composites = [ancestor_composite, parent_composite];
        backend.composite_projective_surfaces_to_view(&underlay.view, (width, height), &composites);
    } else {
        let composites = [parent_composite];
        backend.composite_projective_surfaces_to_view(&underlay.view, (width, height), &composites);
    }

    underlay
}

fn translate_scissor_to_underlay(
    scissor: Option<(u32, u32, u32, u32)>,
    offset_x: f32,
    offset_y: f32,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let (x, y, w, h) = scissor?;
    let left = (x as f32 - offset_x).max(0.0).floor();
    let top = (y as f32 - offset_y).max(0.0).floor();
    let right = ((x + w) as f32 - offset_x).min(width as f32).ceil();
    let bottom = ((y + h) as f32 - offset_y).min(height as f32).ceil();
    if right <= left || bottom <= top {
        return None;
    }
    Some((
        left as u32,
        top as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    ))
}

fn pending_layer_underlay_batch_item<'a>(
    pending: &'a PendingLayerComposite,
    underlay_origin: Rect,
    root_scale: f32,
    width: u32,
    height: u32,
) -> Option<CompositeBatchItem<'a>> {
    let source = pending.surface.target.target();
    let dest_rect = axis_aligned_quad_rect(pending.dest_quad)?;
    let offset_x = underlay_origin.x * root_scale;
    let offset_y = underlay_origin.y * root_scale;
    let underlay_dest_rect = Rect {
        x: dest_rect.x - offset_x,
        y: dest_rect.y - offset_y,
        width: dest_rect.width,
        height: dest_rect.height,
    };
    let underlay_viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    if !rects_intersect(underlay_dest_rect, underlay_viewport) {
        return None;
    }
    Some(CompositeBatchItem {
        source,
        alpha: pending.surface.composite_alpha,
        scissor: translate_scissor_to_underlay(pending.scissor, offset_x, offset_y, width, height),
        rounded_mask: None,
        blend_mode: pending.surface.blend_mode,
        dest_viewport: Some(composite_dest_viewport(
            underlay_dest_rect,
            source.width,
            source.height,
            pending.surface.sample_mode,
        )),
        source_viewport: None,
        sample_mode: pending.surface.sample_mode,
    })
}

#[allow(clippy::too_many_arguments)]
fn render_scene_range_to_target<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target: &OffscreenTarget,
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    width: u32,
    height: u32,
    root_scale: f32,
    initial_load_op: wgpu::LoadOp<wgpu::Color>,
) -> Result<(), String> {
    if z_start >= z_end {
        if matches!(initial_load_op, wgpu::LoadOp::Clear(_)) {
            backend.clear_target_view_with_load_op(&target.view, initial_load_op);
        }
        return Ok(());
    }

    if range_contains_layer_events(&scene.effect_layers, &scene.backdrop_layers, z_start, z_end) {
        backend.render_range_with_layer_events_to_target(
            target,
            &scene.shapes,
            &scene.brushes,
            &scene.images,
            &scene.texts,
            &scene.shadow_draws,
            &scene.retained_draws,
            &scene.draw_ops,
            &scene.effect_layers,
            &scene.backdrop_layers,
            &scene_backdrop_input_hashes(scene, &[], (width, height), root_scale),
            z_start,
            z_end,
            None,
            width,
            height,
            root_scale,
            None,
            initial_load_op,
        )
    } else {
        backend.render_non_effect_segment(
            &target.view,
            &scene.shapes,
            &scene.brushes,
            &scene.images,
            &scene.texts,
            &scene.shadow_draws,
            &scene.retained_draws,
            &scene.draw_ops,
            z_start,
            z_end,
            &[],
            width,
            height,
            root_scale,
            initial_load_op,
        )
    }
}

fn scene_range_has_content_or_events(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
) -> bool {
    !scene_range_draw_ops(&scene.draw_ops, z_start, z_end).is_empty()
        || range_contains_layer_events(&scene.effect_layers, &scene.backdrop_layers, z_start, z_end)
}

fn scene_range_has_draw_ops(scene: &CompositorScene, z_start: usize, z_end: usize) -> bool {
    scene_range_draw_op_count(scene, z_start, z_end) > 0
}

/// The draw ops that fall inside `z_start..z_end`, found by binary search.
///
/// Every push assigns the scene's monotonically increasing `next_z`, so
/// `draw_ops` is always sorted by `z_index` and a z range is a contiguous
/// slice. The range queries here used to filter the whole list instead; an
/// animated game frame carries ~17k draw ops and asks for ranges several
/// times per frame, which made those linear scans a measurable slice of the
/// frame budget on a mobile core.
fn scene_range_draw_ops(draw_ops: &[DrawOp], z_start: usize, z_end: usize) -> &[DrawOp] {
    debug_assert!(draw_ops.windows(2).all(|w| w[0].z_index <= w[1].z_index));
    let start = draw_ops.partition_point(|op| op.z_index < z_start);
    let len = draw_ops[start..].partition_point(|op| op.z_index < z_end);
    &draw_ops[start..start + len]
}

fn scene_range_draw_op_count(scene: &CompositorScene, z_start: usize, z_end: usize) -> usize {
    scene_range_draw_ops(&scene.draw_ops, z_start, z_end).len()
}

fn scene_range_can_cache_as_transparent_surface(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
) -> bool {
    // Only the ops inside the range decide the verdict; every op outside it
    // passed trivially. The binary-searched slice visits exactly the deciding
    // ops — the full-array scan it replaces cost O(total ops) per chunk, so a
    // walk cutting an animated frame into ~19 chunks re-touched every op ~19
    // times just to skip it.
    scene_range_draw_ops(&scene.draw_ops, z_start, z_end)
        .iter()
        .all(|op| match op.kind {
            DrawOpKind::Shape(index) => scene
                .shapes
                .get(index)
                .is_some_and(|shape| shape.blend_mode == BlendMode::SrcOver),
            DrawOpKind::Image(index) => scene
                .images
                .get(index)
                .is_some_and(|image| image.blend_mode == BlendMode::SrcOver),
            DrawOpKind::Text(_) => true,
            DrawOpKind::Shadow(_) => false,
            // A replayed batch transforms every frame; a texture of it would
            // be stale on arrival.
            DrawOpKind::Retained(_) => false,
        })
}

fn scene_range_meets_direct_cache_floor(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    target_size: (u32, u32),
) -> bool {
    let draw_op_count = scene_range_draw_op_count(scene, z_start, z_end);
    if draw_op_count >= MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS {
        return true;
    }
    if draw_op_count != 1 {
        return false;
    }

    offscreen_byte_size(target_size.0, target_size.1)
        >= MIN_SINGLE_DRAW_DIRECT_SCENE_RANGE_CACHE_BYTES
}

/// The visible bounds of a scene range snapped to whole device pixels — the
/// logical rect a cache entry for the range would cover, `None` when nothing
/// in the range draws. The direct-scene walk derives this once per chunk and
/// threads it through the fits gate and the cache path, which used to union
/// the same draw-op bounds independently.
fn direct_scene_range_snapped_bounds(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    root_scale: f32,
) -> Option<Rect> {
    scene_range_visible_bounds(scene, z_start, z_end)
        .and_then(|bounds| snap_scene_range_bounds_to_pixels(bounds, root_scale))
}

/// Whether a chunk is small enough that the range cache would accept it.
///
/// This is the same size gate [`cached_direct_scene_range_surface`] applies
/// before it computes a key, hoisted so the caller can tell — cheaply, and
/// without touching the cache — whether cutting the range here could ever
/// produce a stored entry. A chunk that fails it is going to be rendered
/// directly whatever happens, so there is no reason to give it its own pass.
/// `snapped_bounds` is the chunk's [`direct_scene_range_snapped_bounds`],
/// computed by the caller and shared with the cache path.
fn direct_scene_range_chunk_fits_cache_entry(
    max_texture_dim: u32,
    snapped_bounds: Option<Rect>,
    root_scale: f32,
) -> bool {
    let Some(logical_rect) = snapped_bounds else {
        return false;
    };
    let (target_width, target_height) =
        surface_target_size(logical_rect, root_scale, max_texture_dim);
    direct_scene_range_cache_enabled_for_entry_bytes(offscreen_byte_size(
        target_width,
        target_height,
    ))
}

fn direct_scene_range_cache_chunk_end(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    root_scale: f32,
) -> usize {
    let mut draw_count = 0usize;
    for draw_op in scene_range_draw_ops(&scene.draw_ops, z_start, z_end) {
        if draw_op_splits_direct_scene_range_cache(scene, *draw_op, root_scale) {
            if draw_op.z_index <= z_start {
                return draw_op.z_index.saturating_add(1).min(z_end);
            }
            return draw_op.z_index.min(z_end);
        }

        draw_count = draw_count.saturating_add(1);
        if draw_count >= MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS {
            return draw_op.z_index.saturating_add(1).min(z_end);
        }
    }
    z_end
}

fn draw_op_splits_direct_scene_range_cache(
    scene: &CompositorScene,
    draw_op: DrawOp,
    root_scale: f32,
) -> bool {
    draw_op_is_motion_sensitive(scene, draw_op)
        && draw_op_visible_bytes(scene, draw_op, root_scale)
            .is_some_and(|bytes| bytes > MAX_MOTION_SENSITIVE_DIRECT_SCENE_CACHE_DRAW_BYTES)
}

fn draw_op_is_motion_sensitive(scene: &CompositorScene, draw_op: DrawOp) -> bool {
    match draw_op.kind {
        DrawOpKind::Shape(index) => scene
            .shapes
            .get(index)
            .is_some_and(|shape| shape.motion_context_animated),
        DrawOpKind::Image(index) => scene
            .images
            .get(index)
            .is_some_and(|image| image.motion_context_animated),
        DrawOpKind::Text(_) | DrawOpKind::Shadow(_) => false,
        DrawOpKind::Retained(_) => true,
    }
}

fn draw_op_visible_bytes(scene: &CompositorScene, draw_op: DrawOp, root_scale: f32) -> Option<u64> {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }
    let rect = surface_pixel_rect(draw_op_visible_bounds(scene, draw_op)?, root_scale);
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let width = rect.width.ceil().clamp(1.0, u32::MAX as f32) as u32;
    let height = rect.height.ceil().clamp(1.0, u32::MAX as f32) as u32;
    Some(offscreen_byte_size(width, height))
}

fn draw_op_visible_bounds(scene: &CompositorScene, draw_op: DrawOp) -> Option<Rect> {
    match draw_op.kind {
        DrawOpKind::Shape(index) => scene
            .shapes
            .get(index)
            .and_then(|shape| visible_draw_rect(shape.rect, shape.clip)),
        DrawOpKind::Image(index) => scene
            .images
            .get(index)
            .and_then(|image| visible_draw_rect(image.rect, image.clip)),
        DrawOpKind::Text(index) => scene
            .texts
            .get(index)
            .and_then(|text| visible_draw_rect(text.rect, text.clip)),
        DrawOpKind::Shadow(_) => None,
        DrawOpKind::Retained(index) => scene
            .retained_draws
            .get(index)
            .and_then(|retained| visible_draw_rect(retained.bounds, None)),
    }
}

fn scene_range_visible_bounds(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
) -> Option<Rect> {
    let mut bounds = None;
    for op in scene_range_draw_ops(&scene.draw_ops, z_start, z_end) {
        let rect = draw_op_visible_bounds(scene, *op);
        if let Some(rect) = rect {
            bounds = union_rect(bounds, rect);
        }
    }
    bounds
}

fn snap_scene_range_bounds_to_pixels(bounds: Rect, root_scale: f32) -> Option<Rect> {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }
    let pixel_rect = surface_pixel_rect(bounds, root_scale);
    let left = pixel_rect.x.floor();
    let top = pixel_rect.y.floor();
    let right = (pixel_rect.x + pixel_rect.width).ceil();
    let bottom = (pixel_rect.y + pixel_rect.height).ceil();
    if right <= left || bottom <= top {
        return None;
    }
    Some(Rect {
        x: left / root_scale,
        y: top / root_scale,
        width: (right - left) / root_scale,
        height: (bottom - top) / root_scale,
    })
}

fn flush_underlay_composite_batch<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target_view: &wgpu::TextureView,
    viewport: (u32, u32),
    pending_items: &mut Vec<CompositeBatchItem<'_>>,
    batch_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
) {
    if pending_items.is_empty() {
        return;
    }
    let load_op = batch_load_op.take().unwrap_or(*next_load_op);
    backend.composite_surface_batch_to_view(target_view, viewport, load_op, pending_items);
    pending_items.clear();
    *next_load_op = wgpu::LoadOp::Load;
}

#[allow(clippy::too_many_arguments)]
fn create_direct_root_child_underlay<B: SurfaceExecutionBackend>(
    backend: &mut B,
    local_scene: &CompositorScene,
    child_logical_rect: Rect,
    child_dest_quad: [[f32; 2]; 4],
    child_z_index: usize,
    pending_composites: &[PendingLayerComposite],
    root_scale: f32,
) -> Result<OffscreenTarget, String> {
    let dest_rect = axis_aligned_quad_rect(child_dest_quad)
        .ok_or_else(|| "direct root child underlay requires an axis-aligned child".to_string())?;
    if (dest_rect.width - child_logical_rect.width).abs() > 0.001
        || (dest_rect.height - child_logical_rect.height).abs() > 0.001
    {
        return Err("direct root child underlay requires a translated child".to_string());
    }

    let (width, height) =
        surface_target_size(child_logical_rect, root_scale, backend.max_texture_dim());
    let underlay = backend.acquire_frame_surface(width, height);
    let window_scene = build_scene_window(
        SceneWindowSource {
            shapes: &local_scene.shapes,
            brushes: &local_scene.brushes,
            images: &local_scene.images,
            texts: &local_scene.texts,
            shadow_draws: &local_scene.shadow_draws,
            draw_ops: &local_scene.draw_ops,
            effect_layers: &local_scene.effect_layers,
            backdrop_layers: &local_scene.backdrop_layers,
        },
        0,
        child_z_index,
        dest_rect,
    );

    let render_result = (|| -> Result<(), String> {
        let mut cursor_z = 0usize;
        let mut next_load_op = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let mut pending_items = Vec::new();
        let mut pending_batch_load_op = None;
        for pending in pending_composites
            .iter()
            .filter(|pending| pending.z_index < child_z_index)
        {
            if scene_range_has_content_or_events(&window_scene, cursor_z, pending.z_index) {
                flush_underlay_composite_batch(
                    backend,
                    &underlay.view,
                    (width, height),
                    &mut pending_items,
                    &mut pending_batch_load_op,
                    &mut next_load_op,
                );
                render_scene_range_to_target(
                    backend,
                    &underlay,
                    &window_scene,
                    cursor_z,
                    pending.z_index,
                    width,
                    height,
                    root_scale,
                    next_load_op,
                )?;
                next_load_op = wgpu::LoadOp::Load;
            }
            if let Some(item) =
                pending_layer_underlay_batch_item(pending, dest_rect, root_scale, width, height)
            {
                if pending_items.is_empty() {
                    pending_batch_load_op = Some(next_load_op);
                }
                pending_items.push(item);
                next_load_op = wgpu::LoadOp::Load;
            }
            cursor_z = cursor_z.max(pending.z_index.saturating_add(1));
        }
        if scene_range_has_content_or_events(&window_scene, cursor_z, child_z_index) {
            flush_underlay_composite_batch(
                backend,
                &underlay.view,
                (width, height),
                &mut pending_items,
                &mut pending_batch_load_op,
                &mut next_load_op,
            );
            render_scene_range_to_target(
                backend,
                &underlay,
                &window_scene,
                cursor_z,
                child_z_index,
                width,
                height,
                root_scale,
                next_load_op,
            )
        } else if !pending_items.is_empty() {
            flush_underlay_composite_batch(
                backend,
                &underlay.view,
                (width, height),
                &mut pending_items,
                &mut pending_batch_load_op,
                &mut next_load_op,
            );
            Ok(())
        } else {
            render_scene_range_to_target(
                backend,
                &underlay,
                &window_scene,
                cursor_z,
                child_z_index,
                width,
                height,
                root_scale,
                next_load_op,
            )
        }
    })();
    if let Err(error) = render_result {
        backend.release_frame_surface(underlay);
        return Err(error);
    }
    Ok(underlay)
}

pub(crate) fn root_direct_scene_events_are_supported(
    scene: &CompositorScene,
    root_target_reads: bool,
) -> bool {
    if scene.backdrop_layers.is_empty() {
        return true;
    }
    if !root_target_reads {
        return false;
    }
    !scene
        .effect_layers
        .iter()
        .any(|layer| has_backdrop_layer_in_range(&scene.backdrop_layers, layer.z_start, layer.z_end))
}

#[allow(clippy::too_many_arguments)]
fn render_effect_layer_to_view<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target_view: &wgpu::TextureView,
    shapes: &[DrawShape],
    brushes: &[Brush],
    images: &[ImageDraw],
    texts: &[TextDraw],
    shadow_draws: &[ShadowDraw],
    draw_ops: &[DrawOp],
    effect_layers: &[EffectLayer],
    backdrop_layers: &[BackdropLayer],
    effect_layer_index: usize,
    width: u32,
    height: u32,
    root_scale: f32,
) -> Result<(), String> {
    let layer = effect_layers
        .get(effect_layer_index)
        .cloned()
        .ok_or_else(|| "effect layer index out of bounds".to_string())?;
    let Some(visible_rect) = visible_layer_rect(layer.rect, layer.clip, root_scale, width, height)
    else {
        return Ok(());
    };
    let Some(scissor) = scissor_rect_for_rect(visible_rect, root_scale, width, height) else {
        return Ok(());
    };
    let sample_mode = composite_sample_mode_for_effect_layer(&layer);
    let stable_local_capture = sample_mode == CompositeSampleMode::Box4
        && (layer.effect.is_none()
            || layer
                .requirements
                .contains(SurfaceRequirement::TextMaterialMask))
        && !has_backdrop_layer_in_range(backdrop_layers, layer.z_start, layer.z_end)
        && layer
            .requirements
            .contains(SurfaceRequirement::MotionStableCapture);
    let capture_rect = if stable_local_capture {
        layer.rect
    } else {
        visible_rect
    };
    let effect_root_scale = clamp_effect_surface_scale(
        capture_rect,
        effect_layer_minimum_scale(&layer, root_scale),
        effect_layer_target_scale(&layer, root_scale),
        backend.max_texture_dim(),
    );
    let effect_root_scale = quantize_motion_stable_target_scale(effect_root_scale, sample_mode);
    let (effect_width, effect_height) =
        surface_target_size(capture_rect, effect_root_scale, backend.max_texture_dim());
    let window_scene = build_scene_window(
        SceneWindowSource {
            shapes,
            brushes,
            images,
            texts,
            shadow_draws,
            draw_ops,
            effect_layers,
            backdrop_layers,
        },
        layer.z_start,
        layer.z_end,
        capture_rect,
    );
    if has_backdrop_layer_in_range(&window_scene.backdrop_layers, layer.z_start, layer.z_end) {
        return Err(
            "root direct effect path does not support root-local backdrop sampling".to_string(),
        );
    }
    let Some(window_effect_index) = filtered_effect_layer_index(
        effect_layers,
        effect_layer_index,
        layer.z_start,
        layer.z_end,
    ) else {
        return Err("effect layer window index is missing".to_string());
    };

    let source = backend.acquire_frame_surface(effect_width, effect_height);
    let render_result = backend.render_range_with_layer_events_to_target(
        &source,
        &window_scene.shapes,
        &window_scene.brushes,
        &window_scene.images,
        &window_scene.texts,
        &window_scene.shadow_draws,
        &window_scene.retained_draws,
        &window_scene.draw_ops,
        &window_scene.effect_layers,
        &window_scene.backdrop_layers,
        &[],
        layer.z_start,
        layer.z_end,
        Some(window_effect_index),
        effect_width,
        effect_height,
        effect_root_scale,
        None,
        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
    );
    if let Err(error) = render_result {
        backend.release_frame_surface(source);
        return Err(error);
    }

    let dest_quad = anchored_composite_dest_quad(
        crate::rect_to_quad(capture_rect),
        layer.snap_anchor,
        None,
        root_scale,
        sample_mode,
    );
    let composite_result = if layer.effect.is_none() {
        composite_surface_to_view(
            backend,
            &source,
            target_view,
            (width, height),
            dest_quad,
            layer.composite_alpha,
            wgpu::LoadOp::Load,
            Some(scissor),
            layer.blend_mode,
            sample_mode,
        )
    } else if let Some(effect) = &layer.effect {
        if backend.is_render_effect_supported(effect) {
            if let Some(dest_rect) = axis_aligned_quad_rect(dest_quad) {
                let dest_viewport = Some(composite_dest_viewport(
                    dest_rect,
                    effect_width,
                    effect_height,
                    sample_mode,
                ));
                if let RenderEffect::Shader { shader } = effect {
                    backend.apply_shader_and_composite_to_view(
                        &source,
                        shader,
                        content_effect_pixel_rect(
                            Some(layer.rect),
                            capture_rect,
                            effect_width,
                            effect_height,
                        ),
                        target_view,
                        layer.composite_alpha,
                        wgpu::LoadOp::Load,
                        Some(scissor),
                        layer.blend_mode,
                        dest_viewport,
                        sample_mode,
                    );
                    Ok(())
                } else {
                    backend.apply_effect_and_composite_to_view(
                        &source,
                        effect,
                        content_effect_pixel_rect(
                            Some(layer.rect),
                            capture_rect,
                            effect_width,
                            effect_height,
                        ),
                        target_view,
                        layer.composite_alpha,
                        wgpu::LoadOp::Load,
                        Some(scissor),
                        layer.blend_mode,
                        dest_viewport,
                        sample_mode,
                    )
                }
            } else {
                let source_rect = Rect {
                    x: 0.0,
                    y: 0.0,
                    width: effect_width as f32,
                    height: effect_height as f32,
                };
                let inverse = ProjectiveTransform::from_rect_to_quad(source_rect, dest_quad)
                    .inverse()
                    .ok_or_else(|| "effect layer transform is not invertible".to_string())?;
                if let RenderEffect::Shader { shader } = effect {
                    backend.apply_shader_and_composite_to_view_projective(
                        &source,
                        shader,
                        content_effect_pixel_rect(
                            Some(layer.rect),
                            capture_rect,
                            effect_width,
                            effect_height,
                        ),
                        target_view,
                        (width, height),
                        (source_rect.width, source_rect.height),
                        inverse.matrix(),
                        dest_quad,
                        layer.composite_alpha,
                        wgpu::LoadOp::Load,
                        Some(scissor),
                        layer.blend_mode,
                        sample_mode,
                    );
                    Ok(())
                } else {
                    backend.apply_effect_and_composite_to_view_projective(
                        &source,
                        effect,
                        content_effect_pixel_rect(
                            Some(layer.rect),
                            capture_rect,
                            effect_width,
                            effect_height,
                        ),
                        target_view,
                        (width, height),
                        (source_rect.width, source_rect.height),
                        inverse.matrix(),
                        dest_quad,
                        layer.composite_alpha,
                        wgpu::LoadOp::Load,
                        Some(scissor),
                        layer.blend_mode,
                        sample_mode,
                    )
                }
            }
        } else {
            backend.warn_unsupported_effect_once();
            composite_surface_to_view(
                backend,
                &source,
                target_view,
                (width, height),
                dest_quad,
                layer.composite_alpha,
                wgpu::LoadOp::Load,
                Some(scissor),
                layer.blend_mode,
                sample_mode,
            )
        }
    } else {
        Err("effect layer destination requested without render effect".to_string())
    };
    backend.release_frame_surface(source);
    composite_result
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn render_range_with_layer_events_to_view<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target_view: &wgpu::TextureView,
    root_target: Option<&OffscreenTarget>,
    backdrop_input_hashes: &[u64],
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    excluded_effect_layer: Option<usize>,
    width: u32,
    height: u32,
    root_scale: f32,
    initial_load_op: wgpu::LoadOp<wgpu::Color>,
) -> Result<(), String> {
    if z_start >= z_end {
        if matches!(initial_load_op, wgpu::LoadOp::Clear(_)) {
            backend.clear_target_view_with_load_op(target_view, initial_load_op);
        }
        return Ok(());
    }

    let mut effect_z_ranges = Vec::new();
    collect_effect_ranges(
        &scene.effect_layers,
        z_start,
        z_end,
        excluded_effect_layer,
        &mut effect_z_ranges,
    );
    let mut events = Vec::new();
    collect_layer_events(
        &scene.effect_layers,
        &scene.backdrop_layers,
        z_start,
        z_end,
        excluded_effect_layer,
        &mut events,
    );

    let mut next_load_op = initial_load_op;
    let mut cursor_z = z_start;
    for event in &events {
        if event.z_index > cursor_z {
            backend.render_non_effect_segment(
                target_view,
                &scene.shapes,
                &scene.brushes,
                &scene.images,
                &scene.texts,
                &scene.shadow_draws,
                &scene.retained_draws,
                &scene.draw_ops,
                cursor_z,
                event.z_index,
                &effect_z_ranges,
                width,
                height,
                root_scale,
                next_load_op,
            )?;
            next_load_op = wgpu::LoadOp::Load;
            cursor_z = event.z_index;
        } else if event.z_index < cursor_z {
            continue;
        }

        if matches!(next_load_op, wgpu::LoadOp::Clear(_)) {
            backend.clear_target_view_with_load_op(target_view, next_load_op);
            next_load_op = wgpu::LoadOp::Load;
        }

        match event.kind {
            LayerEventKind::Backdrop(index) => {
                let Some(root_target) = root_target else {
                    return Err(
                        "root direct path does not support root-local backdrop sampling".to_string(),
                    );
                };
                apply_backdrop_layer_to_target(
                    backend,
                    root_target,
                    &scene.backdrop_layers[index],
                    None,
                    width,
                    height,
                    root_scale,
                    backdrop_input_hashes.get(index).copied(),
                )?;
            }
            LayerEventKind::Effect(index) => {
                let layer = &scene.effect_layers[index];
                if layer.z_start < cursor_z {
                    continue;
                }
                render_effect_layer_to_view(
                    backend,
                    target_view,
                    &scene.shapes,
                    &scene.brushes,
                    &scene.images,
                    &scene.texts,
                    &scene.shadow_draws,
                    &scene.draw_ops,
                    &scene.effect_layers,
                    &scene.backdrop_layers,
                    index,
                    width,
                    height,
                    root_scale,
                )?;
                cursor_z = cursor_z.max(layer.z_end);
            }
        }
    }

    if cursor_z < z_end {
        backend.render_non_effect_segment(
            target_view,
            &scene.shapes,
            &scene.brushes,
            &scene.images,
            &scene.texts,
            &scene.shadow_draws,
            &scene.retained_draws,
            &scene.draw_ops,
            cursor_z,
            z_end,
            &effect_z_ranges,
            width,
            height,
            root_scale,
            next_load_op,
        )?;
    } else if matches!(next_load_op, wgpu::LoadOp::Clear(_)) {
        backend.clear_target_view_with_load_op(target_view, next_load_op);
    }

    Ok(())
}

// result_large_err: the Err arm deliberately carries the same
// CompositorScene the Ok arm does, so the caller recycles the scene
// buffers on BOTH arms; the Ok (hot) arm is equally sized, and boxing
// would only add an allocation to the error path.
#[allow(clippy::too_many_arguments, clippy::result_large_err)]
pub(crate) fn render_root_direct<B: SurfaceExecutionBackend>(
    backend: &mut B,
    surface_view: &wgpu::TextureView,
    root_target: Option<&OffscreenTarget>,
    collected: CollectedLayer,
    width: u32,
    height: u32,
    root_scale: f32,
    initial_load_op: wgpu::LoadOp<wgpu::Color>,
) -> Result<CompositorScene, (String, CompositorScene)> {
    let CollectedLayer {
        scene: local_scene,
        child_layers,
    } = collected;
    let surface_view = root_target.map_or(surface_view, |target| &target.view);
    let result = (|| -> Result<(), String> {
        let mut cursor_z = 0usize;
        let mut next_load_op = initial_load_op;
        let mut pending_composites = Vec::new();
        let mut pending_composite_load_op = None;
        let mut pending_shader_composites = Vec::new();
        let mut pending_shader_load_op = None;
        let mut prior_child_contributions = Vec::new();
        for mut child in child_layers {
            if cursor_z < child.z_index {
                let range_has_events = range_contains_layer_events(
                    &local_scene.effect_layers,
                    &local_scene.backdrop_layers,
                    cursor_z,
                    child.z_index,
                );
                if range_has_events {
                    render_non_effect_range_with_pending_composites(
                        backend,
                        surface_view,
                        &local_scene,
                        cursor_z,
                        cursor_z,
                        width,
                        height,
                        root_scale,
                        &mut pending_composites,
                        &mut pending_composite_load_op,
                        &mut pending_shader_composites,
                        &mut pending_shader_load_op,
                        &mut next_load_op,
                    )?;
                    let backdrop_input_hashes = scene_backdrop_input_hashes(
                        &local_scene,
                        &prior_child_contributions,
                        (width, height),
                        root_scale,
                    );
                    render_range_with_layer_events_to_view(
                        backend,
                        surface_view,
                        root_target,
                        &backdrop_input_hashes,
                        &local_scene,
                        cursor_z,
                        child.z_index,
                        None,
                        width,
                        height,
                        root_scale,
                        next_load_op,
                    )?;
                } else {
                    render_direct_scene_range_with_pending_composites(
                        backend,
                        surface_view,
                        &local_scene,
                        cursor_z,
                        child.z_index,
                        width,
                        height,
                        root_scale,
                        &mut pending_composites,
                        &mut pending_composite_load_op,
                        &mut pending_shader_composites,
                        &mut pending_shader_load_op,
                        &mut next_load_op,
                    )?;
                }
                next_load_op = wgpu::LoadOp::Load;
            }

            let resolved_child = resolved_child_surface_composite(&child);
            let child_dest_quad = if let Some(anchor) = resolved_child.snap_anchor {
                translate_quad(
                    resolved_child.dest_quad,
                    snap_delta_for_anchor(anchor, root_scale),
                )
            } else {
                resolved_child.dest_quad
            };
            if resolved_child.shadow_draws.is_empty()
                && !child_composite_visible(
                    child_dest_quad,
                    child.visual_clip,
                    root_scale,
                    width,
                    height,
                )
            {
                cursor_z = child.z_index.saturating_add(1);
                continue;
            }

            let child_backdrop_reads_target = child.backdrop.is_some() && root_target.is_some();
            if !resolved_child.shadow_draws.is_empty() && !child_backdrop_reads_target {
                render_non_effect_range_with_pending_composites(
                    backend,
                    surface_view,
                    &local_scene,
                    child.z_index,
                    child.z_index,
                    width,
                    height,
                    root_scale,
                    &mut pending_composites,
                    &mut pending_composite_load_op,
                    &mut pending_shader_composites,
                    &mut pending_shader_load_op,
                    &mut next_load_op,
                )?;
                flush_pending_clear(backend, surface_view, &mut next_load_op);
                for shadow in &resolved_child.shadow_draws {
                    backend.render_shadow_draw(surface_view, shadow, width, height, root_scale);
                }
            }

            if child.needs_nested_underlay {
                flush_pending_shader_layer_composites(
                    backend,
                    &mut pending_shader_composites,
                    surface_view,
                    (width, height),
                    &mut pending_shader_load_op,
                    &mut next_load_op,
                )?;
            }

            let child_underlay = if child.needs_nested_underlay {
                Some(create_direct_root_child_underlay(
                    backend,
                    &local_scene,
                    resolved_child.logical_rect,
                    resolved_child.dest_quad,
                    child.z_index,
                    &pending_composites,
                    root_scale,
                )?)
            } else {
                None
            };

            let child_surface_result = render_layer_surface(
                backend,
                &mut child,
                LayerSurfaceRequest {
                    root_scale,
                    backdrop_underlay: child_underlay.as_ref(),
                    backdrop_underlay_color: None,
                    allow_runtime_cache: true,
                    logical_rect_override: Some(resolved_child.logical_rect),
                    capture_clip_override: resolved_child.surface_clip,
                    activates_nested_capture: true,
                    translation_context: TranslationRenderContext::default(),
                },
            );
            if let Some(underlay) = child_underlay {
                backend.release_frame_surface(underlay);
            }
            let child_surface = child_surface_result?;
            if let Some(backdrop) = child_surface.backdrop.clone() {
                let Some(root_target) = root_target else {
                    backend.release_layer_surface_target(child_surface.target);
                    return Err(
                        "root direct path does not support backdrop child surfaces".to_string()
                    );
                };
                flush_pending_shader_layer_composites(
                    backend,
                    &mut pending_shader_composites,
                    surface_view,
                    (width, height),
                    &mut pending_shader_load_op,
                    &mut next_load_op,
                )?;
                flush_pending_layer_composites(
                    backend,
                    &mut pending_composites,
                    surface_view,
                    (width, height),
                    &mut pending_composite_load_op,
                    &mut next_load_op,
                );
                flush_pending_clear(backend, surface_view, &mut next_load_op);
                let backdrop_layer = BackdropLayer {
                    node_id: child.node_id,
                    rect: resolved_child.backdrop_rect,
                    clip: child.visual_clip,
                    snap_anchor: resolved_child.snap_anchor,
                    effect: backdrop,
                    z_index: child.z_index,
                };
                let child_backdrop_capture_rect = visible_backdrop_capture_rect(
                    resolved_child.backdrop_rect,
                    child.visual_clip,
                    &backdrop_layer.effect,
                    root_scale,
                    (width, height),
                );
                let backdrop_input_hash = backdrop_scene_prefix_hash(
                    &local_scene,
                    &prior_child_contributions,
                    child.z_index,
                    child_backdrop_capture_rect.unwrap_or(backdrop_layer.rect),
                    (width, height),
                    root_scale,
                );
                if let Some(prepared) = prepare_cached_backdrop_layer_composite(
                    backend,
                    root_target,
                    &backdrop_layer,
                    None,
                    width,
                    height,
                    root_scale,
                    Some(backdrop_input_hash),
                )? {
                    if pending_composites.is_empty() {
                        pending_composite_load_op = Some(next_load_op);
                    }
                    pending_composites.push(PendingLayerComposite {
                        z_index: child.z_index,
                        surface: prepared.surface,
                        dest_quad: prepared.dest_quad,
                        scissor: prepared.scissor,
                    });
                    next_load_op = wgpu::LoadOp::Load;
                } else {
                    apply_backdrop_layer_to_target(
                        backend,
                        root_target,
                        &backdrop_layer,
                        None,
                        width,
                        height,
                        root_scale,
                        Some(backdrop_input_hash),
                    )?;
                    if !pending_composites.is_empty() {
                        pending_composite_load_op = Some(wgpu::LoadOp::Load);
                    }
                }
            }
            if child_backdrop_reads_target && !resolved_child.shadow_draws.is_empty() {
                flush_pending_shader_layer_composites(
                    backend,
                    &mut pending_shader_composites,
                    surface_view,
                    (width, height),
                    &mut pending_shader_load_op,
                    &mut next_load_op,
                )?;
                flush_pending_layer_composites(
                    backend,
                    &mut pending_composites,
                    surface_view,
                    (width, height),
                    &mut pending_composite_load_op,
                    &mut next_load_op,
                );
                flush_pending_clear(backend, surface_view, &mut next_load_op);
                for shadow in &resolved_child.shadow_draws {
                    backend.render_shadow_draw(surface_view, shadow, width, height, root_scale);
                }
            }

            let dest_quad = layer_surface_dest_quad(
                resolved_child.logical_rect,
                resolved_child.dest_quad,
                child_surface.logical_rect,
            );
            let dest_quad = anchored_composite_dest_quad(
                dest_quad,
                resolved_child.snap_anchor,
                resolved_child.composite_snap_origin,
                root_scale,
                child_surface.sample_mode,
            );
            let scissor = child
                .visual_clip
                .and_then(|clip| scissor_rect_for_rect(clip, root_scale, width, height));
            let child_prefix_contribution =
                backdrop_prefix_child_contribution(&child, &child_surface, dest_quad, scissor);
            if child_surface.deferred_effect.is_none()
                && axis_aligned_quad_rect(dest_quad).is_some()
            {
                if pending_composites.is_empty() {
                    pending_composite_load_op = Some(next_load_op);
                }
                pending_composites.push(PendingLayerComposite {
                    z_index: child.z_index,
                    surface: child_surface,
                    dest_quad,
                    scissor,
                });
                next_load_op = wgpu::LoadOp::Load;
            } else {
                match direct_shader_layer_composite(
                    child_surface,
                    child.z_index,
                    dest_quad,
                    scissor,
                ) {
                    Ok(pending) => {
                        if pending_shader_composites.is_empty() {
                            pending_shader_load_op = Some(next_load_op);
                        }
                        pending_shader_composites.push(pending);
                        next_load_op = wgpu::LoadOp::Load;
                    }
                    Err(child_surface) => {
                        render_non_effect_range_with_pending_composites(
                            backend,
                            surface_view,
                            &local_scene,
                            child.z_index,
                            child.z_index,
                            width,
                            height,
                            root_scale,
                            &mut pending_composites,
                            &mut pending_composite_load_op,
                            &mut pending_shader_composites,
                            &mut pending_shader_load_op,
                            &mut next_load_op,
                        )?;
                        let composite_load_op = next_load_op;
                        composite_layer_surface_to_view(
                            backend,
                            &child_surface,
                            surface_view,
                            (width, height),
                            dest_quad,
                            composite_load_op,
                            scissor,
                        )?;
                        next_load_op = wgpu::LoadOp::Load;
                        backend.release_layer_surface_target(child_surface.target);
                    }
                }
            }
            prior_child_contributions.push(child_prefix_contribution);
            cursor_z = child.z_index.saturating_add(1);
        }

        if cursor_z < local_scene.next_z {
            let range_has_events = range_contains_layer_events(
                &local_scene.effect_layers,
                &local_scene.backdrop_layers,
                cursor_z,
                local_scene.next_z,
            );
            if range_has_events {
                render_non_effect_range_with_pending_composites(
                    backend,
                    surface_view,
                    &local_scene,
                    cursor_z,
                    cursor_z,
                    width,
                    height,
                    root_scale,
                    &mut pending_composites,
                    &mut pending_composite_load_op,
                    &mut pending_shader_composites,
                    &mut pending_shader_load_op,
                    &mut next_load_op,
                )?;
                let backdrop_input_hashes = scene_backdrop_input_hashes(
                    &local_scene,
                    &prior_child_contributions,
                    (width, height),
                    root_scale,
                );
                render_range_with_layer_events_to_view(
                    backend,
                    surface_view,
                    root_target,
                    &backdrop_input_hashes,
                    &local_scene,
                    cursor_z,
                    local_scene.next_z,
                    None,
                    width,
                    height,
                    root_scale,
                    next_load_op,
                )?;
            } else {
                render_direct_scene_range_with_pending_composites(
                    backend,
                    surface_view,
                    &local_scene,
                    cursor_z,
                    local_scene.next_z,
                    width,
                    height,
                    root_scale,
                    &mut pending_composites,
                    &mut pending_composite_load_op,
                    &mut pending_shader_composites,
                    &mut pending_shader_load_op,
                    &mut next_load_op,
                )?;
                render_non_effect_range_with_pending_composites(
                    backend,
                    surface_view,
                    &local_scene,
                    local_scene.next_z,
                    local_scene.next_z,
                    width,
                    height,
                    root_scale,
                    &mut pending_composites,
                    &mut pending_composite_load_op,
                    &mut pending_shader_composites,
                    &mut pending_shader_load_op,
                    &mut next_load_op,
                )?;
            }
        } else if matches!(next_load_op, wgpu::LoadOp::Clear(_)) {
            backend.clear_target_view_with_load_op(surface_view, next_load_op);
        } else {
            render_non_effect_range_with_pending_composites(
                backend,
                surface_view,
                &local_scene,
                local_scene.next_z,
                local_scene.next_z,
                width,
                height,
                root_scale,
                &mut pending_composites,
                &mut pending_composite_load_op,
                &mut pending_shader_composites,
                &mut pending_shader_load_op,
                &mut next_load_op,
            )?;
        }

        Ok(())
    })();
    // Hand the scene back in BOTH arms so the caller can recycle its
    // buffers even when the draw errs; for a heavy animated frame they are
    // megabytes of Vec that would otherwise round-trip through mmap on
    // every frame.
    match result {
        Ok(()) => Ok(local_scene),
        Err(error) => Err((error, local_scene)),
    }
}

/// Renders the surface for a producer-lowered child layer. Every value the
/// old `&LayerNode` path read off the node comes from the collection-time
/// snapshot; the child's content is consumed from `child.source`.
pub(crate) fn render_layer_surface<B: SurfaceExecutionBackend>(
    backend: &mut B,
    child: &mut ChildLayerComposite,
    request: LayerSurfaceRequest<'_>,
) -> Result<LayerSurface, String> {
    let LayerSurfaceRequest {
        root_scale,
        backdrop_underlay,
        backdrop_underlay_color,
        allow_runtime_cache,
        logical_rect_override,
        capture_clip_override,
        activates_nested_capture,
        translation_context,
    } = request;
    // Taken up front: on a cache hit the collected source is dropped unused,
    // matching the old lazy path that never collected it.
    let source = std::mem::take(&mut child.source);
    let surface_requirements = child.surface_requirements;
    let direct_translated_content_context =
        translation_context.inherited_content_translation || child.translated_content_context;
    let effective_translated_content_context =
        direct_translated_content_context || surface_requirements.contains_translated_content;
    let effective_requirements = effective_surface_requirements(
        effective_translated_content_context,
        translation_context.surface_capture_active,
        surface_requirements,
    );
    let composite_sample_mode = composite_sample_mode_for_requirements(
        effective_translated_content_context,
        translation_context.surface_capture_active,
        surface_requirements,
    );
    let target_scale = child_surface_target_scale(child, root_scale, translation_context);
    let translation_context = layer_surface_translation_context(
        translation_context,
        activates_nested_capture
            && effective_requirements.contains(SurfaceRequirement::MotionStableCapture),
    );
    let cache_candidate = child_layer_raster_cache_candidate(
        child,
        target_scale,
        backdrop_underlay.is_some(),
        backdrop_underlay_color,
        allow_runtime_cache,
        logical_rect_override,
        backend.max_texture_dim(),
    );
    if let Some((cache_key, logical_rect)) = cache_candidate {
        if let Some((target, logical_rect)) = backend.cached_layer_surface(&cache_key) {
            let (composite_alpha, blend_mode) = child
                .isolation
                .as_ref()
                .map(|isolation| (isolation.composite_alpha, isolation.blend_mode))
                .unwrap_or((1.0, BlendMode::SrcOver));
            return Ok(LayerSurface {
                target: LayerSurfaceTexture::Cached(target),
                logical_rect,
                composite_alpha,
                blend_mode,
                rounded_clip: child.rounded_clip,
                backdrop: child.backdrop.clone(),
                deferred_effect: None,
                effect_content_rect: None,
                sample_mode: composite_sample_mode,
            });
        }
        let (width, height) = cache_key.pixel_size();
        backend.record_layer_cache_miss(width, height);
        return render_layer_surface_uncached(
            backend,
            child,
            source,
            LayerSurfaceRenderOptions {
                target_scale,
                backdrop_underlay,
                backdrop_underlay_color,
                allow_runtime_cache,
                cache_candidate: Some((cache_key, logical_rect)),
                logical_rect_override,
                capture_clip_override,
                composite_sample_mode,
                translation_context,
            },
        );
    }

    render_layer_surface_uncached(
        backend,
        child,
        source,
        LayerSurfaceRenderOptions {
            target_scale,
            backdrop_underlay,
            backdrop_underlay_color,
            allow_runtime_cache,
            cache_candidate: None,
            logical_rect_override,
            capture_clip_override,
            composite_sample_mode,
            translation_context,
        },
    )
}

/// Snapshot-consuming twin of the backend's `layer_raster_cache_candidate`:
/// the same admission rules and key, decided from the values captured at
/// collection time. `child.logical_rect` stands in for the memoized estimate
/// the node-based path re-reads — it is that memoized value.
#[allow(clippy::too_many_arguments)]
fn child_layer_raster_cache_candidate(
    child: &ChildLayerComposite,
    root_scale: f32,
    has_backdrop_underlay: bool,
    backdrop_underlay_color: Option<u32>,
    allow_runtime_cache: bool,
    logical_rect_override: Option<Rect>,
    max_texture_dim: u32,
) -> Option<(LayerRasterCacheKey, Rect)> {
    let surface_requirements = child.surface_requirements;
    let runtime_cache_is_safe = allow_runtime_cache
        && surface_requirements
            .surface_requirements
            .has_isolating_requirement()
        && !surface_requirements.contains_runtime_shader;
    let cache_is_allowed = child.cache_policy == CachePolicy::Auto
        || (allow_runtime_cache && surface_requirements.has_renderer_forced_surface())
        || runtime_cache_is_safe;
    if !cache_is_allowed {
        return None;
    }
    let external_backdrop_input = has_backdrop_underlay && child.contains_descendant_backdrop;
    if external_backdrop_input && backdrop_underlay_color.is_none() {
        return None;
    }
    // A RuntimeShader produces different output every frame from uniforms no
    // content hash carries, so nothing can invalidate a texture it was
    // rastered into. Caching the shader's own layer would only fill the LRU
    // with stale entries; caching an ANCESTOR of one freezes the effect
    // outright, because the ancestor stays a hit forever and replays whatever
    // frame it first rastered. The whole subtree is therefore uncacheable.
    if surface_requirements.contains_runtime_shader {
        return None;
    }

    let logical_rect = logical_rect_override.unwrap_or(child.logical_rect);
    let pixel_size = surface_target_size(logical_rect, root_scale, max_texture_dim);
    let content_hash = if external_backdrop_input {
        let mut hasher = FxHasher::default();
        0xF1A7_C010u64.hash(&mut hasher);
        child.target_content_hash.hash(&mut hasher);
        backdrop_underlay_color.hash(&mut hasher);
        hasher.finish()
    } else {
        child.target_content_hash
    };
    Some((
        LayerRasterCacheKey::new(
            child.node_id,
            content_hash,
            child.effect_hash,
            logical_rect,
            pixel_size,
            ScaleBucket::from_scale(root_scale),
        ),
        logical_rect,
    ))
}

pub(crate) fn layer_surface_translation_context(
    translation_context: TranslationRenderContext,
    surface_provides_motion_stable_capture: bool,
) -> TranslationRenderContext {
    TranslationRenderContext {
        inherited_content_translation: translation_context.inherited_content_translation,
        translated_content_axes: translation_context.translated_content_axes,
        surface_capture_active: translation_context.surface_capture_active
            || surface_provides_motion_stable_capture,
        local_picture_capture_active: translation_context.local_picture_capture_active,
    }
}

fn minimum_surface_scale_for_composite(
    target_scale: f32,
    sample_mode: CompositeSampleMode,
    effective_requirements: SurfaceRequirementSet,
) -> f32 {
    if sample_mode == CompositeSampleMode::Box4
        && !effective_requirements.contains(SurfaceRequirement::MotionStableCapture)
    {
        target_scale
    } else {
        target_scale.min(1.0)
    }
}

fn can_materialize_cached_effect(effect: &RenderEffect, backdrop: Option<&RenderEffect>) -> bool {
    backdrop.is_none() && !effect.contains_runtime_shader()
}

fn materialize_render_effect_to_target<B: SurfaceExecutionBackend>(
    backend: &mut B,
    source: &OffscreenTarget,
    effect: &RenderEffect,
    target: &OffscreenTarget,
    layer_pixel_rect: [f32; 4],
    sample_mode: CompositeSampleMode,
) -> Result<(), String> {
    let dest_viewport = Some((0.0, 0.0, target.width as f32, target.height as f32));
    let load_op = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
    if let RenderEffect::Shader { shader } = effect {
        backend.apply_shader_and_composite_to_view(
            source,
            shader,
            layer_pixel_rect,
            &target.view,
            1.0,
            load_op,
            None,
            BlendMode::SrcOver,
            dest_viewport,
            sample_mode,
        );
        return Ok(());
    }

    backend.apply_effect_and_composite_to_view(
        source,
        effect,
        layer_pixel_rect,
        &target.view,
        1.0,
        load_op,
        None,
        BlendMode::SrcOver,
        dest_viewport,
        sample_mode,
    )
}

#[allow(clippy::too_many_arguments)]
fn layer_source_cache_key(
    child: &ChildLayerComposite,
    effective_requirements: SurfaceRequirementSet,
    surface_rect: Rect,
    pixel_size: (u32, u32),
    target_scale: f32,
    has_backdrop_underlay: bool,
    allow_runtime_cache: bool,
) -> Option<LayerRasterCacheKey> {
    let runtime_shader_source = child.effect_contains_runtime_shader;
    let motion_stable_source =
        effective_requirements.contains(SurfaceRequirement::MotionStableCapture);
    let backdrop_local_source = child.backdrop.is_some();
    if !runtime_shader_source && !motion_stable_source && !backdrop_local_source {
        return None;
    }

    let cache_is_allowed = child.cache_policy == CachePolicy::Auto
        || allow_runtime_cache
        || child.surface_requirements.has_renderer_forced_surface()
        || motion_stable_source;
    if !cache_is_allowed || (has_backdrop_underlay && child.contains_descendant_backdrop) {
        return None;
    }

    let content_hash = if motion_stable_source {
        child
            .motion_source_content_hash
            .expect("motion-stable source snapshots its motion content hash at collection")
    } else {
        child.target_content_hash
    };

    Some(LayerRasterCacheKey::source_content(
        child.node_id,
        content_hash,
        surface_rect,
        pixel_size,
        ScaleBucket::from_scale(target_scale),
    ))
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BackdropPrefixChildContribution {
    z_index: usize,
    node_id: Option<NodeId>,
    content_hash: u64,
    effect_hash: u64,
    backdrop_hash: u64,
    deferred_effect_hash: u64,
    logical_rect: Rect,
    dest_quad: [[f32; 2]; 4],
    scissor: Option<(u32, u32, u32, u32)>,
    composite_alpha_bits: u32,
    blend_mode: BlendMode,
    sample_mode: CompositeSampleMode,
}

fn backdrop_prefix_child_contribution(
    child: &ChildLayerComposite,
    surface: &LayerSurface,
    dest_quad: [[f32; 2]; 4],
    scissor: Option<(u32, u32, u32, u32)>,
) -> BackdropPrefixChildContribution {
    BackdropPrefixChildContribution {
        z_index: child.z_index,
        node_id: child.node_id,
        content_hash: child.target_content_hash,
        effect_hash: child.effect_hash,
        backdrop_hash: surface
            .backdrop
            .as_ref()
            .map(retained_render_effect_hash)
            .unwrap_or(0),
        deferred_effect_hash: surface
            .deferred_effect
            .as_ref()
            .map(retained_render_effect_hash)
            .unwrap_or(0),
        logical_rect: surface.logical_rect,
        dest_quad,
        scissor,
        composite_alpha_bits: surface.composite_alpha.to_bits(),
        blend_mode: surface.blend_mode,
        sample_mode: surface.sample_mode,
    }
}

fn backdrop_effect_cache_key(
    layer: &BackdropLayer,
    input_content_hash: u64,
    visible_rect: Rect,
    pixel_size: (u32, u32),
    root_scale: f32,
) -> Option<LayerRasterCacheKey> {
    Some(LayerRasterCacheKey::backdrop_effect(
        layer.node_id,
        input_content_hash,
        retained_render_effect_hash(&layer.effect),
        visible_rect,
        pixel_size,
        ScaleBucket::from_scale(root_scale),
    ))
}

fn retained_render_effect_hash(effect: &RenderEffect) -> u64 {
    let mut hasher = FxHasher::default();
    hash_retained_render_effect(effect, &mut hasher);
    hasher.finish()
}

fn hash_retained_render_effect<H: Hasher>(effect: &RenderEffect, state: &mut H) {
    match effect {
        RenderEffect::Blur {
            radius_x,
            radius_y,
            edge_treatment,
        } => {
            0u8.hash(state);
            hash_f32_bits(*radius_x, state);
            hash_f32_bits(*radius_y, state);
            edge_treatment.hash(state);
        }
        RenderEffect::Offset { offset_x, offset_y } => {
            1u8.hash(state);
            hash_f32_bits(*offset_x, state);
            hash_f32_bits(*offset_y, state);
        }
        RenderEffect::Shader { shader } => {
            2u8.hash(state);
            shader.source_hash().hash(state);
            hash_f32_bits(shader.input_padding(), state);
            hash_f32_bits(shader.output_padding(), state);
            shader.uniforms().len().hash(state);
            for uniform in shader.uniforms() {
                hash_f32_bits(*uniform, state);
            }
        }
        RenderEffect::Chain { first, second } => {
            3u8.hash(state);
            hash_retained_render_effect(first, state);
            hash_retained_render_effect(second, state);
        }
    }
}

fn backdrop_scene_prefix_hash(
    scene: &CompositorScene,
    prior_child_contributions: &[BackdropPrefixChildContribution],
    z_end: usize,
    capture_rect: Rect,
    target_size: (u32, u32),
    root_scale: f32,
) -> u64 {
    let mut hasher = FxHasher::default();
    0xBCAD_0F0Du64.hash(&mut hasher);
    target_size.hash(&mut hasher);
    hash_f32_bits(root_scale, &mut hasher);
    z_end.hash(&mut hasher);

    for draw_op in scene_range_draw_ops(&scene.draw_ops, 0, z_end)
        .iter()
        .filter(|draw_op| {
            draw_op_visible_bounds(scene, **draw_op)
                .is_none_or(|bounds| rects_intersect(bounds, capture_rect))
        })
    {
        0u8.hash(&mut hasher);
        hash_draw_op(scene, draw_op, &mut hasher);
    }
    for effect_layer in scene
        .effect_layers
        .iter()
        .filter(|layer| layer.z_end <= z_end && rects_intersect(layer.rect, capture_rect))
    {
        1u8.hash(&mut hasher);
        hash_effect_layer(effect_layer, &mut hasher);
    }
    for backdrop_layer in scene.backdrop_layers.iter().filter(|layer| {
        layer.z_index < z_end
            && rects_intersect(
                backdrop_output_rect(
                    layer.rect,
                    layer.clip,
                    &layer.effect,
                    root_scale,
                    target_size,
                ),
                capture_rect,
            )
    }) {
        2u8.hash(&mut hasher);
        hash_backdrop_layer(backdrop_layer, &mut hasher);
    }
    for child in prior_child_contributions.iter().filter(|child| {
        if child.z_index >= z_end {
            return false;
        }
        let capture_pixels = surface_pixel_rect(capture_rect, root_scale);
        dest_quad_intersects_rect(child.dest_quad, capture_pixels)
            && child.scissor.is_none_or(|(x, y, width, height)| {
                rects_intersect(
                    Rect {
                        x: x as f32,
                        y: y as f32,
                        width: width as f32,
                        height: height as f32,
                    },
                    capture_pixels,
                )
            })
    }) {
        3u8.hash(&mut hasher);
        hash_backdrop_prefix_child(child, &mut hasher);
    }

    hasher.finish()
}

/// One input hash per backdrop the scene carries, in the order the render
/// path indexes them. A backdrop that lives in a scene rather than on a child
/// composite reaches the blur cache through these; without one the blur runs
/// again on every frame whose scene under it never moved.
fn scene_backdrop_input_hashes(
    scene: &CompositorScene,
    prior_child_contributions: &[BackdropPrefixChildContribution],
    target_size: (u32, u32),
    root_scale: f32,
) -> Vec<u64> {
    if scene.backdrop_layers.is_empty() {
        return Vec::new();
    }
    scene
        .backdrop_layers
        .iter()
        .map(|layer| {
            let (layer_rect, layer_clip) = snapped_backdrop_geometry(layer, root_scale);
            let capture_rect = visible_backdrop_capture_rect(
                layer_rect,
                layer_clip,
                &layer.effect,
                root_scale,
                target_size,
            )
            .unwrap_or(layer.rect);
            backdrop_scene_prefix_hash(
                scene,
                prior_child_contributions,
                layer.z_index,
                capture_rect,
                target_size,
                root_scale,
            )
        })
        .collect()
}

fn scene_range_content_hash(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    target_size: (u32, u32),
    root_scale: f32,
) -> u64 {
    let mut hasher = FxHasher::default();
    0xD1EC_7A6Eu64.hash(&mut hasher);
    target_size.hash(&mut hasher);
    hash_f32_bits(root_scale, &mut hasher);
    z_start.hash(&mut hasher);
    z_end.hash(&mut hasher);

    for draw_op in scene_range_draw_ops(&scene.draw_ops, z_start, z_end) {
        0u8.hash(&mut hasher);
        hash_draw_op(scene, draw_op, &mut hasher);
    }
    for effect_layer in scene
        .effect_layers
        .iter()
        .filter(|layer| z_start < layer.z_end && layer.z_start < z_end)
    {
        1u8.hash(&mut hasher);
        hash_effect_layer(effect_layer, &mut hasher);
    }
    for backdrop_layer in scene
        .backdrop_layers
        .iter()
        .filter(|layer| layer.z_index >= z_start && layer.z_index < z_end)
    {
        2u8.hash(&mut hasher);
        hash_backdrop_layer(backdrop_layer, &mut hasher);
    }

    hasher.finish()
}

fn draw_op_content_hash(scene: &CompositorScene, draw_op: &DrawOp) -> u64 {
    let mut hasher = FxHasher::default();
    hash_draw_op(scene, draw_op, &mut hasher);
    hasher.finish()
}

fn log_direct_scene_draw_op_detail(scene: &CompositorScene, draw_op: &DrawOp) {
    match draw_op.kind {
        DrawOpKind::Shape(index) => {
            if let Some(shape) = scene.shapes.get(index) {
                log::warn!(
                    "[wgpu-render-stage:direct-scene-cache-op] z={} kind=shape rect=({:.1},{:.1},{:.1},{:.1}) local=({:.1},{:.1},{:.1},{:.1}) quad=({:.1},{:.1})({:.1},{:.1})({:.1},{:.1})({:.1},{:.1}) clip={} blend={:?} motion={} brush_hash={:016x}",
                    draw_op.z_index,
                    shape.rect.x,
                    shape.rect.y,
                    shape.rect.width,
                    shape.rect.height,
                    shape.local_rect.x,
                    shape.local_rect.y,
                    shape.local_rect.width,
                    shape.local_rect.height,
                    shape.quad[0][0],
                    shape.quad[0][1],
                    shape.quad[1][0],
                    shape.quad[1][1],
                    shape.quad[2][0],
                    shape.quad[2][1],
                    shape.quad[3][0],
                    shape.quad[3][1],
                    shape.clip.is_some(),
                    shape.blend_mode,
                    shape.motion_context_animated,
                    shape.brush.render_hash(&scene.brushes),
                );
            }
        }
        DrawOpKind::Image(index) => {
            if let Some(image) = scene.images.get(index) {
                log::warn!(
                    "[wgpu-render-stage:direct-scene-cache-op] z={} kind=image rect=({:.1},{:.1},{:.1},{:.1}) local=({:.1},{:.1},{:.1},{:.1}) clip={} blend={:?} alpha={:.3} motion={}",
                    draw_op.z_index,
                    image.rect.x,
                    image.rect.y,
                    image.rect.width,
                    image.rect.height,
                    image.local_rect.x,
                    image.local_rect.y,
                    image.local_rect.width,
                    image.local_rect.height,
                    image.clip.is_some(),
                    image.blend_mode,
                    image.alpha,
                    image.motion_context_animated,
                );
            }
        }
        DrawOpKind::Text(index) => {
            if let Some(text) = scene.texts.get(index) {
                log::warn!(
                    "[wgpu-render-stage:direct-scene-cache-op] z={} kind=text node={:?} rect=({:.1},{:.1},{:.1},{:.1}) clip={} chars={}",
                    draw_op.z_index,
                    text.node_id,
                    text.rect.x,
                    text.rect.y,
                    text.rect.width,
                    text.rect.height,
                    text.clip.is_some(),
                    text.text.text.len(),
                );
            }
        }
        DrawOpKind::Shadow(index) => {
            if let Some(shadow) = scene.shadow_draws.get(index) {
                log::warn!(
                    "[wgpu-render-stage:direct-scene-cache-op] z={} kind=shadow shapes={} texts={} blur={:.1} clip={}",
                    draw_op.z_index,
                    shadow.shapes.len(),
                    shadow.texts.len(),
                    shadow.blur_radius,
                    shadow.clip.is_some(),
                );
            }
        }
        DrawOpKind::Retained(index) => {
            if let Some(retained) = scene.retained_draws.get(index) {
                log::warn!(
                    "[wgpu-render-stage:direct-scene-cache-op] z={} kind=retained slot={} shapes={} bounds=({:.1},{:.1},{:.1},{:.1})",
                    draw_op.z_index,
                    retained.slot,
                    retained.shape_count,
                    retained.bounds.x,
                    retained.bounds.y,
                    retained.bounds.width,
                    retained.bounds.height,
                );
            }
        }
    }
}

fn log_direct_scene_range_hash_diag(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    viewport_rect: Rect,
    target_size: (u32, u32),
    root_scale: f32,
    content_hash: u64,
) {
    if !direct_scene_range_hash_diag_enabled() {
        return;
    }

    let mut entries = String::new();
    for draw_op in scene_range_draw_ops(&scene.draw_ops, z_start, z_end) {
        if !entries.is_empty() {
            entries.push(' ');
        }
        let kind = match draw_op.kind {
            DrawOpKind::Shape(_) => "S",
            DrawOpKind::Image(_) => "I",
            DrawOpKind::Text(_) => "T",
            DrawOpKind::Shadow(_) => "H",
            DrawOpKind::Retained(_) => "R",
        };
        entries.push_str(&format!(
            "{}{}:{:016x}",
            kind,
            draw_op.z_index,
            draw_op_content_hash(scene, draw_op)
        ));
        if Some(draw_op.z_index) == direct_scene_range_hash_detail_z() {
            log_direct_scene_draw_op_detail(scene, draw_op);
        }
    }

    log::warn!(
        "[wgpu-render-stage:direct-scene-cache-hash] z_start={z_start} z_end={z_end} content={content_hash:016x} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{} scale={:.3} ops={entries}",
        viewport_rect.x,
        viewport_rect.y,
        viewport_rect.width,
        viewport_rect.height,
        target_size.0,
        target_size.1,
        root_scale,
    );
}

fn direct_scene_range_cache_key(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    viewport_rect: Rect,
    target_size: (u32, u32),
    root_scale: f32,
) -> Option<LayerRasterCacheKey> {
    if !scene_range_has_draw_ops(scene, z_start, z_end)
        || !scene_range_meets_direct_cache_floor(scene, z_start, z_end, target_size)
        || range_contains_layer_events(&scene.effect_layers, &scene.backdrop_layers, z_start, z_end)
        || !scene_range_can_cache_as_transparent_surface(scene, z_start, z_end)
        || offscreen_byte_size(target_size.0, target_size.1) > MAX_DIRECT_SCENE_RANGE_CACHE_BYTES
    {
        return None;
    }

    let content_hash = scene_range_content_hash(scene, z_start, z_end, target_size, root_scale);
    log_direct_scene_range_hash_diag(
        scene,
        z_start,
        z_end,
        viewport_rect,
        target_size,
        root_scale,
        content_hash,
    );

    Some(LayerRasterCacheKey::scene_range(
        content_hash,
        viewport_rect,
        target_size,
        ScaleBucket::from_scale(root_scale),
    ))
}

fn direct_scene_range_cache_skip_reason(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    target_size: (u32, u32),
    _root_scale: f32,
) -> &'static str {
    let draw_op_count = scene_range_draw_op_count(scene, z_start, z_end);
    if draw_op_count == 0 {
        return "no-draw-ops";
    }
    if !scene_range_meets_direct_cache_floor(scene, z_start, z_end, target_size) {
        if draw_op_count == 1 {
            return "single-draw-too-small";
        }
        return "too-few-draw-ops";
    }
    if range_contains_layer_events(&scene.effect_layers, &scene.backdrop_layers, z_start, z_end) {
        return "layer-events";
    }
    if !scene_range_can_cache_as_transparent_surface(scene, z_start, z_end) {
        return "non-transparent-safe";
    }
    if offscreen_byte_size(target_size.0, target_size.1) > MAX_DIRECT_SCENE_RANGE_CACHE_BYTES {
        return "too-large";
    }
    "unknown"
}

fn hash_draw_op<H: Hasher>(scene: &CompositorScene, draw_op: &DrawOp, state: &mut H) {
    draw_op.z_index.hash(state);
    draw_op.kind.hash(state);
    match draw_op.kind {
        DrawOpKind::Shape(index) => {
            if let Some(shape) = scene.shapes.get(index) {
                hash_draw_shape(shape, &scene.brushes, state);
            }
        }
        DrawOpKind::Image(index) => {
            if let Some(image) = scene.images.get(index) {
                hash_image_draw(image, state);
            }
        }
        DrawOpKind::Text(index) => {
            if let Some(text) = scene.texts.get(index) {
                hash_text_draw(text, state);
            }
        }
        DrawOpKind::Shadow(index) => {
            if let Some(shadow) = scene.shadow_draws.get(index) {
                hash_shadow_draw(shadow, state);
            }
        }
        DrawOpKind::Retained(index) => {
            if let Some(retained) = scene.retained_draws.get(index) {
                retained.slot.hash(state);
                retained.first_shape.hash(state);
                retained.shape_count.hash(state);
                retained.transform.center[0].to_bits().hash(state);
                retained.transform.center[1].to_bits().hash(state);
                retained.transform.rot[0].to_bits().hash(state);
                retained.transform.rot[1].to_bits().hash(state);
                retained.transform.scale.to_bits().hash(state);
                hash_rect(retained.bounds, state);
            }
        }
    }
}

fn hash_draw_shape<H: Hasher>(shape: &DrawShape, brushes: &[Brush], state: &mut H) {
    hash_rect(shape.rect, state);
    hash_rect(shape.local_rect, state);
    hash_quad(shape.quad, state);
    hash_snap_anchor(shape.snap_anchor, state);
    shape.brush.render_hash(brushes).hash(state);
    shape
        .shape
        .map(|shape| shape.radii().render_hash())
        .hash(state);
    shape.z_index.hash(state);
    hash_optional_rect(shape.clip, state);
    shape.blend_mode.hash(state);
    shape.motion_context_animated.hash(state);
}

fn hash_image_draw<H: Hasher>(image: &ImageDraw, state: &mut H) {
    hash_rect(image.rect, state);
    hash_rect(image.local_rect, state);
    hash_quad(image.quad, state);
    hash_snap_anchor(image.snap_anchor, state);
    image.image.render_hash().hash(state);
    hash_f32_bits(image.alpha, state);
    image
        .color_filter
        .map(|filter| filter.render_hash())
        .hash(state);
    image.sampling.hash(state);
    image.z_index.hash(state);
    hash_optional_rect(image.clip, state);
    image.blend_mode.hash(state);
    hash_optional_rect(image.src_rect, state);
    image.motion_context_animated.hash(state);
}

fn hash_text_draw<H: Hasher>(text: &TextDraw, state: &mut H) {
    text.node_id.hash(state);
    hash_rect(text.rect, state);
    hash_snap_anchor(text.snap_anchor, state);
    text.translated_content_context.hash(state);
    render_string_scene_hash(&text.text).hash(state);
    text.color.render_hash().hash(state);
    text.text_style.render_hash().hash(state);
    hash_f32_bits(text.font_size, state);
    hash_f32_bits(text.scale, state);
    text.layout_options.hash(state);
    text.z_index.hash(state);
    hash_optional_rect(text.clip, state);
}

fn render_string_scene_hash(text: &std::sync::Arc<cranpose_ui::text::RenderString>) -> u64 {
    RENDER_STRING_HASH_CACHE.with(|cache| cache.borrow_mut().get_or_insert(text))
}

fn compute_render_string_hash(text: &cranpose_ui::text::RenderString) -> u64 {
    let mut hasher = FxHasher::default();
    hash_render_string_contents(text, &mut hasher);
    hasher.finish()
}

fn hash_render_string_contents<H: Hasher>(text: &cranpose_ui::text::RenderString, state: &mut H) {
    text.text.hash(state);
    text.span_styles.len().hash(state);
    for style in &text.span_styles {
        style.range.start.hash(state);
        style.range.end.hash(state);
        style.item.render_hash().hash(state);
    }
    text.paragraph_styles.len().hash(state);
    for style in &text.paragraph_styles {
        style.range.start.hash(state);
        style.range.end.hash(state);
        style.item.render_hash().hash(state);
    }
    text.string_annotations.len().hash(state);
    for annotation in &text.string_annotations {
        annotation.range.start.hash(state);
        annotation.range.end.hash(state);
        annotation.item.tag.hash(state);
        annotation.item.annotation.hash(state);
    }
    text.links.len().hash(state);
    for link in &text.links {
        link.range.start.hash(state);
        link.range.end.hash(state);
        match &link.item {
            LinkKey::Url(url) => {
                0u8.hash(state);
                url.hash(state);
            }
            LinkKey::Clickable(tag) => {
                1u8.hash(state);
                tag.hash(state);
            }
        }
    }
}

fn hash_shadow_draw<H: Hasher>(shadow: &ShadowDraw, state: &mut H) {
    shadow.shapes.len().hash(state);
    for (shape, blend_mode) in &shadow.shapes {
        hash_draw_shape(shape, &shadow.brushes, state);
        blend_mode.hash(state);
    }
    shadow.texts.len().hash(state);
    for text in &shadow.texts {
        hash_text_draw(text, state);
    }
    hash_f32_bits(shadow.blur_radius, state);
    hash_optional_rect(shadow.clip, state);
    shadow.z_index.hash(state);
}

fn hash_effect_layer<H: Hasher>(layer: &EffectLayer, state: &mut H) {
    hash_rect(layer.rect, state);
    hash_optional_rect(layer.clip, state);
    hash_snap_anchor(layer.snap_anchor, state);
    layer
        .effect
        .as_ref()
        .map(retained_render_effect_hash)
        .hash(state);
    layer.blend_mode.hash(state);
    hash_f32_bits(layer.composite_alpha, state);
    layer.z_start.hash(state);
    layer.z_end.hash(state);
    layer.requirements.hash(state);
}

fn hash_backdrop_layer<H: Hasher>(layer: &BackdropLayer, state: &mut H) {
    layer.node_id.hash(state);
    hash_rect(layer.rect, state);
    hash_optional_rect(layer.clip, state);
    hash_snap_anchor(layer.snap_anchor, state);
    retained_render_effect_hash(&layer.effect).hash(state);
    layer.z_index.hash(state);
}

fn hash_backdrop_prefix_child<H: Hasher>(child: &BackdropPrefixChildContribution, state: &mut H) {
    child.z_index.hash(state);
    child.node_id.hash(state);
    child.content_hash.hash(state);
    child.effect_hash.hash(state);
    child.backdrop_hash.hash(state);
    child.deferred_effect_hash.hash(state);
    hash_rect(child.logical_rect, state);
    hash_quad(child.dest_quad, state);
    child.scissor.hash(state);
    child.composite_alpha_bits.hash(state);
    child.blend_mode.hash(state);
    match child.sample_mode {
        CompositeSampleMode::Linear => 0u8.hash(state),
        CompositeSampleMode::Box4 => 1u8.hash(state),
        CompositeSampleMode::Nearest => 2u8.hash(state),
    }
}

fn hash_f32_bits<H: Hasher>(value: f32, state: &mut H) {
    value.to_bits().hash(state);
}

fn hash_rect<H: Hasher>(rect: Rect, state: &mut H) {
    hash_f32_bits(rect.x, state);
    hash_f32_bits(rect.y, state);
    hash_f32_bits(rect.width, state);
    hash_f32_bits(rect.height, state);
}

fn hash_optional_rect<H: Hasher>(rect: Option<Rect>, state: &mut H) {
    match rect {
        Some(rect) => {
            true.hash(state);
            hash_rect(rect, state);
        }
        None => false.hash(state),
    }
}

fn hash_quad<H: Hasher>(quad: [[f32; 2]; 4], state: &mut H) {
    for point in quad {
        hash_f32_bits(point[0], state);
        hash_f32_bits(point[1], state);
    }
}

fn hash_snap_anchor<H: Hasher>(anchor: Option<SnapAnchor>, state: &mut H) {
    match anchor {
        Some(anchor) => {
            true.hash(state);
            hash_f32_bits(anchor.origin.x, state);
            hash_f32_bits(anchor.origin.y, state);
            hash_f32_bits(anchor.device_pixel_step, state);
        }
        None => false.hash(state),
    }
}

struct PendingLayerComposite {
    z_index: usize,
    surface: LayerSurface,
    dest_quad: [[f32; 2]; 4],
    scissor: Option<(u32, u32, u32, u32)>,
}

struct PreparedBackdropComposite {
    surface: LayerSurface,
    dest_quad: [[f32; 2]; 4],
    scissor: Option<(u32, u32, u32, u32)>,
}

struct PendingShaderLayerComposite {
    z_index: usize,
    surface: LayerSurface,
    shader: RuntimeShader,
    dest_quad: [[f32; 2]; 4],
    scissor: Option<(u32, u32, u32, u32)>,
    dest_viewport: (f32, f32, f32, f32),
}

#[allow(clippy::result_large_err)]
fn direct_shader_layer_composite(
    surface: LayerSurface,
    z_index: usize,
    dest_quad: [[f32; 2]; 4],
    scissor: Option<(u32, u32, u32, u32)>,
) -> Result<PendingShaderLayerComposite, LayerSurface> {
    let Some(RenderEffect::Shader { shader }) = surface.deferred_effect.as_ref() else {
        return Err(surface);
    };
    let shader = shader.clone();
    if surface.composite_alpha != 1.0 || surface.blend_mode != BlendMode::SrcOver {
        return Err(surface);
    }
    let Some(dest_rect) = axis_aligned_quad_rect(dest_quad) else {
        return Err(surface);
    };
    let source = surface.target.target();
    let dest_viewport =
        composite_dest_viewport(dest_rect, source.width, source.height, surface.sample_mode);
    if dest_viewport.2 <= 0.0 || dest_viewport.3 <= 0.0 {
        return Err(surface);
    }
    Ok(PendingShaderLayerComposite {
        z_index,
        surface,
        shader,
        dest_quad,
        scissor,
        dest_viewport,
    })
}

fn pending_shader_layer_composite_batch_items(
    pending: &[PendingShaderLayerComposite],
) -> Vec<(usize, ShaderCompositeBatchItem<'_>)> {
    pending
        .iter()
        .map(|pending| {
            let source = pending.surface.target.target();
            (
                pending.z_index,
                ShaderCompositeBatchItem {
                    source,
                    shader: &pending.shader,
                    layer_pixel_rect: content_effect_pixel_rect(
                        pending.surface.effect_content_rect,
                        pending.surface.logical_rect,
                        source.width,
                        source.height,
                    ),
                    scissor: pending.scissor,
                    dest_viewport: pending.dest_viewport,
                },
            )
        })
        .collect()
}

fn shader_layer_composite_batch_items(
    pending: &[PendingShaderLayerComposite],
) -> Vec<ShaderCompositeBatchItem<'_>> {
    pending_shader_layer_composite_batch_items(pending)
        .into_iter()
        .map(|(_, item)| item)
        .collect()
}

fn flush_pending_shader_layer_composites<B: SurfaceExecutionBackend>(
    backend: &mut B,
    pending: &mut Vec<PendingShaderLayerComposite>,
    target_view: &wgpu::TextureView,
    viewport: (u32, u32),
    pending_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
) -> Result<(), String> {
    if pending.is_empty() {
        return Ok(());
    }

    let mut load_op = pending_load_op.take().unwrap_or(*next_load_op);
    let batch_items = shader_layer_composite_batch_items(pending);
    if backend.shader_composite_batch_to_view(target_view, viewport, load_op, &batch_items) {
        for pending in pending.drain(..) {
            backend.release_layer_surface_target(pending.surface.target);
        }
        *next_load_op = wgpu::LoadOp::Load;
        return Ok(());
    }

    for pending in pending.drain(..) {
        composite_layer_surface_to_view(
            backend,
            &pending.surface,
            target_view,
            viewport,
            pending.dest_quad,
            load_op,
            pending.scissor,
        )?;
        load_op = wgpu::LoadOp::Load;
        backend.release_layer_surface_target(pending.surface.target);
    }
    *next_load_op = wgpu::LoadOp::Load;
    Ok(())
}

fn release_pending_shader_layer_composites<B: SurfaceExecutionBackend>(
    backend: &mut B,
    pending: &mut Vec<PendingShaderLayerComposite>,
) {
    for pending in pending.drain(..) {
        backend.release_layer_surface_target(pending.surface.target);
    }
}

fn layer_surface_composite_batch_item(
    pending: &PendingLayerComposite,
) -> Option<CompositeBatchItem<'_>> {
    if pending.surface.deferred_effect.is_some() {
        return None;
    }

    let source = pending.surface.target.target();
    let dest_rect = axis_aligned_quad_rect(pending.dest_quad)?;
    let rounded_mask = layer_surface_rounded_mask(&pending.surface, dest_rect);
    Some(CompositeBatchItem {
        source,
        alpha: pending.surface.composite_alpha,
        scissor: pending.scissor,
        rounded_mask,
        blend_mode: pending.surface.blend_mode,
        dest_viewport: Some(composite_dest_viewport(
            dest_rect,
            source.width,
            source.height,
            pending.surface.sample_mode,
        )),
        source_viewport: None,
        sample_mode: pending.surface.sample_mode,
    })
}

fn pending_layer_composite_batch_items(
    pending: &[PendingLayerComposite],
) -> Vec<(usize, CompositeBatchItem<'_>)> {
    pending
        .iter()
        .map(|pending| {
            (
                pending.z_index,
                layer_surface_composite_batch_item(pending)
                    .expect("pending layer composite must be batchable"),
            )
        })
        .collect()
}

fn layer_surface_rounded_mask(
    surface: &LayerSurface,
    dest_rect: Rect,
) -> Option<RoundedCompositeMask> {
    surface
        .rounded_clip
        .map(|clip| clip.composite_mask(surface.logical_rect, dest_rect))
}

fn release_pending_layer_composites<B: SurfaceExecutionBackend>(
    backend: &mut B,
    pending: &mut Vec<PendingLayerComposite>,
) {
    for pending in pending.drain(..) {
        backend.release_layer_surface_target(pending.surface.target);
    }
}

fn pending_layer_composites_intersect_rect(pending: &[PendingLayerComposite], rect: Rect) -> bool {
    pending
        .iter()
        .any(|pending| dest_quad_intersects_rect(pending.dest_quad, rect))
}

fn dest_quad_intersects_rect(dest_quad: [[f32; 2]; 4], rect: Rect) -> bool {
    axis_aligned_quad_rect(dest_quad)
        .or_else(|| quad_bounds_rect(dest_quad))
        .is_none_or(|bounds| rects_intersect(bounds, rect))
}

fn flush_pending_layer_composites<B: SurfaceExecutionBackend>(
    backend: &mut B,
    pending: &mut Vec<PendingLayerComposite>,
    target_view: &wgpu::TextureView,
    viewport: (u32, u32),
    pending_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
) {
    if pending.is_empty() {
        return;
    }

    {
        let load_op = pending_load_op.take().unwrap_or(*next_load_op);
        let batch_items: Vec<_> = pending_layer_composite_batch_items(pending)
            .into_iter()
            .map(|(_, item)| item)
            .collect();
        if !batch_items.is_empty() {
            backend.composite_surface_batch_to_view(target_view, viewport, load_op, &batch_items);
            *next_load_op = wgpu::LoadOp::Load;
        }
    }

    release_pending_layer_composites(backend, pending);
}

fn take_ordered_pending_composite_load_op(
    pending_composites: &[PendingLayerComposite],
    pending_composite_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    pending_shader_composites: &[PendingShaderLayerComposite],
    pending_shader_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::LoadOp<wgpu::Color> {
    let composite_start_z = pending_composites
        .iter()
        .map(|pending| pending.z_index)
        .min();
    let shader_start_z = pending_shader_composites
        .iter()
        .map(|pending| pending.z_index)
        .min();
    let composite_load_op = pending_composite_load_op.take();
    let shader_load_op = pending_shader_load_op.take();
    match (composite_start_z, shader_start_z) {
        (Some(composite_z), Some(shader_z)) if composite_z <= shader_z => {
            composite_load_op.or(shader_load_op).unwrap_or(next_load_op)
        }
        (Some(_), Some(_)) => shader_load_op.or(composite_load_op).unwrap_or(next_load_op),
        (Some(_), None) => composite_load_op.unwrap_or(next_load_op),
        (None, Some(_)) => shader_load_op.unwrap_or(next_load_op),
        (None, None) => next_load_op,
    }
}

#[allow(clippy::too_many_arguments)]
fn render_non_effect_range_with_pending_composites<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target_view: &wgpu::TextureView,
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    width: u32,
    height: u32,
    root_scale: f32,
    pending_composites: &mut Vec<PendingLayerComposite>,
    pending_composite_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    pending_shader_composites: &mut Vec<PendingShaderLayerComposite>,
    pending_shader_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
) -> Result<(), String> {
    if pending_composites.is_empty() && pending_shader_composites.is_empty() {
        backend.render_non_effect_segment(
            target_view,
            &scene.shapes,
            &scene.brushes,
            &scene.images,
            &scene.texts,
            &scene.shadow_draws,
            &scene.retained_draws,
            &scene.draw_ops,
            z_start,
            z_end,
            &[],
            width,
            height,
            root_scale,
            *next_load_op,
        )?;
        *next_load_op = wgpu::LoadOp::Load;
        return Ok(());
    }

    let load_op = take_ordered_pending_composite_load_op(
        pending_composites,
        pending_composite_load_op,
        pending_shader_composites,
        pending_shader_load_op,
        *next_load_op,
    );
    let composite_items = pending_layer_composite_batch_items(pending_composites);
    let shader_composite_items =
        pending_shader_layer_composite_batch_items(pending_shader_composites);
    backend.render_non_effect_segment_with_composites(
        target_view,
        &scene.shapes,
        &scene.brushes,
        &scene.images,
        &scene.texts,
        &scene.shadow_draws,
        &scene.retained_draws,
        &scene.draw_ops,
        z_start,
        z_end,
        &[],
        &composite_items,
        &shader_composite_items,
        width,
        height,
        root_scale,
        load_op,
    )?;
    release_pending_layer_composites(backend, pending_composites);
    release_pending_shader_layer_composites(backend, pending_shader_composites);
    *next_load_op = wgpu::LoadOp::Load;
    Ok(())
}

fn range_contains_layer_events(
    effect_layers: &[EffectLayer],
    backdrop_layers: &[BackdropLayer],
    z_start: usize,
    z_end: usize,
) -> bool {
    effect_layers
        .iter()
        .any(|layer| z_start < layer.z_end && layer.z_start < z_end)
        || backdrop_layers
            .iter()
            .any(|layer| layer.z_index >= z_start && layer.z_index < z_end)
}

#[allow(clippy::too_many_arguments)]
fn cached_direct_scene_range_surface<B: SurfaceExecutionBackend>(
    backend: &mut B,
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    root_scale: f32,
    snapped_bounds: Option<Rect>,
) -> Result<Option<LayerSurface>, String> {
    // `snapped_bounds` is the caller's [`direct_scene_range_snapped_bounds`]
    // for exactly this range — the same value this function used to derive
    // itself, handed over so the union over the range's draw ops happens
    // once per chunk instead of once per gate.
    let Some(logical_rect) = snapped_bounds else {
        if layer_render_diag_enabled() {
            log::warn!(
                "[wgpu-render-stage:direct-scene-cache] skip reason=no-visible-bounds z_start={z_start} z_end={z_end}",
            );
        }
        return Ok(None);
    };
    let (target_width, target_height) =
        surface_target_size(logical_rect, root_scale, backend.max_texture_dim());
    let target_bytes = offscreen_byte_size(target_width, target_height);
    if !direct_scene_range_cache_enabled_for_entry_bytes(target_bytes) {
        if layer_render_diag_enabled() {
            let draw_ops = scene_range_draw_op_count(scene, z_start, z_end);
            log::warn!(
                "[wgpu-render-stage:direct-scene-cache] skip reason=default-entry-budget z_start={z_start} z_end={z_end} draw_ops={draw_ops} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{} bytes={target_bytes}",
                logical_rect.x,
                logical_rect.y,
                logical_rect.width,
                logical_rect.height,
                target_width,
                target_height,
            );
        }
        return Ok(None);
    }
    let Some(cache_key) = direct_scene_range_cache_key(
        scene,
        z_start,
        z_end,
        logical_rect,
        (target_width, target_height),
        root_scale,
    ) else {
        if layer_render_diag_enabled() {
            let reason = direct_scene_range_cache_skip_reason(
                scene,
                z_start,
                z_end,
                (target_width, target_height),
                root_scale,
            );
            let draw_ops = scene_range_draw_op_count(scene, z_start, z_end);
            let bytes = offscreen_byte_size(target_width, target_height);
            log::warn!(
                "[wgpu-render-stage:direct-scene-cache] skip reason={reason} z_start={z_start} z_end={z_end} draw_ops={draw_ops} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{} bytes={bytes}",
                logical_rect.x,
                logical_rect.y,
                logical_rect.width,
                logical_rect.height,
                target_width,
                target_height,
            );
        }
        return Ok(None);
    };

    let target = if let Some((cached_target, _)) = backend.cached_layer_surface(&cache_key) {
        if layer_render_diag_enabled() {
            let draw_ops = scene_range_draw_op_count(scene, z_start, z_end);
            log::warn!(
                "[wgpu-render-stage:direct-scene-cache] hit z_start={z_start} z_end={z_end} draw_ops={draw_ops} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{}",
                logical_rect.x,
                logical_rect.y,
                logical_rect.width,
                logical_rect.height,
                target_width,
                target_height,
            );
        }
        LayerSurfaceTexture::Cached(cached_target)
    } else {
        if !backend.admit_layer_surface_cache_miss(&cache_key) {
            if layer_render_diag_enabled() {
                let draw_ops = scene_range_draw_op_count(scene, z_start, z_end);
                log::warn!(
                    "[wgpu-render-stage:direct-scene-cache] observe z_start={z_start} z_end={z_end} draw_ops={draw_ops} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{}",
                    logical_rect.x,
                    logical_rect.y,
                    logical_rect.width,
                    logical_rect.height,
                    target_width,
                    target_height,
                );
            }
            return Ok(None);
        }
        if layer_render_diag_enabled() {
            let draw_ops = scene_range_draw_op_count(scene, z_start, z_end);
            log::warn!(
                "[wgpu-render-stage:direct-scene-cache] miss z_start={z_start} z_end={z_end} draw_ops={draw_ops} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{}",
                logical_rect.x,
                logical_rect.y,
                logical_rect.width,
                logical_rect.height,
                target_width,
                target_height,
            );
        }
        backend.record_layer_cache_miss(target_width, target_height);
        let target = backend.acquire_retained_surface(target_width, target_height);
        let window_scene = build_scene_window(
            SceneWindowSource {
                shapes: &scene.shapes,
                brushes: &scene.brushes,
                images: &scene.images,
                texts: &scene.texts,
                shadow_draws: &scene.shadow_draws,
                draw_ops: &scene.draw_ops,
                effect_layers: &scene.effect_layers,
                backdrop_layers: &scene.backdrop_layers,
            },
            z_start,
            z_end,
            logical_rect,
        );
        let render_result = backend.render_non_effect_segment(
            &target.view,
            &window_scene.shapes,
            &window_scene.brushes,
            &window_scene.images,
            &window_scene.texts,
            &window_scene.shadow_draws,
            &window_scene.retained_draws,
            &window_scene.draw_ops,
            z_start,
            z_end,
            &[],
            target_width,
            target_height,
            root_scale,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        if let Err(error) = render_result {
            backend.release_layer_surface_target(LayerSurfaceTexture::Owned(target));
            return Err(error);
        }
        LayerSurfaceTexture::Cached(backend.insert_cached_layer_surface(
            cache_key,
            target,
            logical_rect,
        ))
    };

    Ok(Some(LayerSurface {
        target,
        logical_rect,
        composite_alpha: 1.0,
        blend_mode: BlendMode::SrcOver,
        rounded_clip: None,
        backdrop: None,
        deferred_effect: None,
        effect_content_rect: None,
        sample_mode: CompositeSampleMode::Linear,
    }))
}

/// Queues a composite for the range when the cache can serve it. The sole
/// caller — the direct-scene walk — checks [`direct_scene_range_cache_enabled`]
/// before calling and hands over the range's precomputed snapped bounds.
#[allow(clippy::too_many_arguments)]
fn queue_cached_direct_scene_range<B: SurfaceExecutionBackend>(
    backend: &mut B,
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    root_scale: f32,
    snapped_bounds: Option<Rect>,
    pending_composites: &mut Vec<PendingLayerComposite>,
    pending_composite_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
) -> Result<bool, String> {
    let Some(surface) = cached_direct_scene_range_surface(
        backend,
        scene,
        z_start,
        z_end,
        root_scale,
        snapped_bounds,
    )?
    else {
        return Ok(false);
    };

    if pending_composites.is_empty() {
        *pending_composite_load_op = Some(*next_load_op);
    }
    let logical_rect = surface.logical_rect;
    let dest_quad = anchored_composite_dest_quad(
        crate::rect_to_quad(logical_rect),
        None,
        None,
        root_scale,
        surface.sample_mode,
    );
    pending_composites.push(PendingLayerComposite {
        z_index: z_start,
        surface,
        dest_quad,
        scissor: None,
    });
    *next_load_op = wgpu::LoadOp::Load;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn render_direct_scene_range_with_pending_composites<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target_view: &wgpu::TextureView,
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    width: u32,
    height: u32,
    root_scale: f32,
    pending_composites: &mut Vec<PendingLayerComposite>,
    pending_composite_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    pending_shader_composites: &mut Vec<PendingShaderLayerComposite>,
    pending_shader_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
) -> Result<(), String> {
    let cache_enabled = direct_scene_range_cache_enabled();
    let mut cursor_z = z_start;
    let mut direct_run = DirectChunkRunCoalescer::default();
    while cursor_z < z_end {
        let mut chunk_end = direct_scene_range_cache_chunk_end(scene, cursor_z, z_end, root_scale);
        if chunk_end <= cursor_z {
            return Err("direct scene cache chunk did not advance".to_string());
        }
        // The chunk's snapped visible bounds, derived once and read by both
        // the fits gate and the cache path below (each used to union the
        // same draw-op bounds itself). Only valid for the un-widened chunk:
        // after a widen the chunk cannot cache and the value is never read.
        // With the cache disabled no gate ever reads bounds, so none are
        // derived.
        let chunk_bounds = if cache_enabled {
            direct_scene_range_snapped_bounds(scene, cursor_z, chunk_end, root_scale)
        } else {
            None
        };
        // A chunk boundary only exists to keep a *cache entry* small enough to
        // store. When the chunk is too big to be admitted anyway, the split
        // buys nothing and costs a whole render pass — and on a tile-based
        // mobile GPU a render pass is a tile store and reload of the entire
        // target. An animated scene is the worst case: every chunk covers the
        // whole surface, so every one is refused, and a game frame of ~640
        // draw ops was being cut into ten passes that all wrote the same
        // pixels. Rendering the remainder in one pass measured 18% less CPU
        // on a Pixel 9 Pro.
        let mut chunk_can_cache = cache_enabled;
        if chunk_end < z_end
            && !(cache_enabled
                && direct_scene_range_chunk_fits_cache_entry(
                    backend.max_texture_dim(),
                    chunk_bounds,
                    root_scale,
                ))
        {
            chunk_end = z_end;
            // The widened range can only fail the same admission check: its
            // visible bounds contain the refused chunk's, and the byte budget
            // is monotone in bounds. Asking the cache anyway would union the
            // visible bounds of every remaining draw op — ~17k rects in an
            // animated game frame — just to be told no.
            chunk_can_cache = false;
        }
        if chunk_can_cache
            && queue_cached_direct_scene_range(
                backend,
                scene,
                cursor_z,
                chunk_end,
                root_scale,
                chunk_bounds,
                pending_composites,
                pending_composite_load_op,
                next_load_op,
            )?
        {
            // The chunk composites from the cache. Any direct run absorbed so
            // far sits below it in z; flushing now keeps painter's order — the
            // just-queued composite rides along in the flush, interleaved
            // after the run's lower-z draw ops.
            if let Some((run_start, run_end)) = direct_run.flush_at(cursor_z) {
                render_non_effect_range_with_pending_composites(
                    backend,
                    target_view,
                    scene,
                    run_start,
                    run_end,
                    width,
                    height,
                    root_scale,
                    pending_composites,
                    pending_composite_load_op,
                    pending_shader_composites,
                    pending_shader_load_op,
                    next_load_op,
                )?;
            }
            cursor_z = chunk_end;
            continue;
        }
        direct_run.absorb(cursor_z);
        cursor_z = chunk_end;
        if !direct_scene_range_coalesce_enabled() {
            if let Some((run_start, run_end)) = direct_run.flush_at(cursor_z) {
                render_non_effect_range_with_pending_composites(
                    backend,
                    target_view,
                    scene,
                    run_start,
                    run_end,
                    width,
                    height,
                    root_scale,
                    pending_composites,
                    pending_composite_load_op,
                    pending_shader_composites,
                    pending_shader_load_op,
                    next_load_op,
                )?;
            }
        }
    }
    if let Some((run_start, run_end)) = direct_run.flush_at(z_end) {
        render_non_effect_range_with_pending_composites(
            backend,
            target_view,
            scene,
            run_start,
            run_end,
            width,
            height,
            root_scale,
            pending_composites,
            pending_composite_load_op,
            pending_shader_composites,
            pending_shader_load_op,
            next_load_op,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_layer_source_uncached<B: SurfaceExecutionBackend>(
    backend: &mut B,
    local_scene: &CompositorScene,
    child_layers: Vec<ChildLayerComposite>,
    target_scale: f32,
    backdrop_underlay: Option<&OffscreenTarget>,
    underlay_color: Option<u32>,
    width: u32,
    height: u32,
    effective_translated_content_context: bool,
    effective_translated_content_axes: TranslatedContentAxes,
    translation_context: TranslationRenderContext,
) -> Result<OffscreenTarget, String> {
    let target = backend.acquire_retained_surface(width, height);
    let mut cursor_z = 0usize;
    let mut next_load_op = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
    let mut pending_composites = Vec::new();
    let mut pending_composite_load_op = None;
    let mut pending_shader_composites = Vec::new();
    let mut pending_shader_load_op = None;
    let mut prior_child_contributions = Vec::new();
    for mut child in child_layers {
        if cursor_z < child.z_index {
            flush_pending_shader_layer_composites(
                backend,
                &mut pending_shader_composites,
                &target.view,
                (width, height),
                &mut pending_shader_load_op,
                &mut next_load_op,
            )?;
            if !pending_composites.is_empty()
                && !range_contains_layer_events(
                    &local_scene.effect_layers,
                    &local_scene.backdrop_layers,
                    cursor_z,
                    child.z_index,
                )
            {
                let load_op = pending_composite_load_op.take().unwrap_or(next_load_op);
                let composite_items = pending_layer_composite_batch_items(&pending_composites);
                backend.render_non_effect_segment_with_composites(
                    &target.view,
                    &local_scene.shapes,
                    &local_scene.brushes,
                    &local_scene.images,
                    &local_scene.texts,
                    &local_scene.shadow_draws,
                    &local_scene.retained_draws,
                    &local_scene.draw_ops,
                    cursor_z,
                    child.z_index,
                    &[],
                    &composite_items,
                    &[],
                    width,
                    height,
                    target_scale,
                    load_op,
                )?;
                release_pending_layer_composites(backend, &mut pending_composites);
            } else {
                flush_pending_layer_composites(
                    backend,
                    &mut pending_composites,
                    &target.view,
                    (width, height),
                    &mut pending_composite_load_op,
                    &mut next_load_op,
                );
                let local_backdrop_hashes = scene_backdrop_input_hashes(
                    local_scene,
                    &prior_child_contributions,
                    (width, height),
                    target_scale,
                );
                backend.render_range_with_layer_events_to_target(
                    &target,
                    &local_scene.shapes,
                    &local_scene.brushes,
                    &local_scene.images,
                    &local_scene.texts,
                    &local_scene.shadow_draws,
                    &local_scene.retained_draws,
                    &local_scene.draw_ops,
                    &local_scene.effect_layers,
                    &local_scene.backdrop_layers,
                    &local_backdrop_hashes,
                    cursor_z,
                    child.z_index,
                    None,
                    width,
                    height,
                    target_scale,
                    backdrop_underlay,
                    next_load_op,
                )?;
            }
            next_load_op = wgpu::LoadOp::Load;
        }

        let resolved_child = resolved_child_surface_composite(&child);
        let child_dest_quad = if let Some(anchor) = resolved_child.snap_anchor {
            translate_quad(
                resolved_child.dest_quad,
                snap_delta_for_anchor(anchor, target_scale),
            )
        } else {
            resolved_child.dest_quad
        };
        if resolved_child.shadow_draws.is_empty()
            && !child_composite_visible(
                child_dest_quad,
                child.visual_clip,
                target_scale,
                width,
                height,
            )
        {
            cursor_z = child.z_index.saturating_add(1);
            continue;
        }
        if child.needs_nested_underlay {
            flush_pending_shader_layer_composites(
                backend,
                &mut pending_shader_composites,
                &target.view,
                (width, height),
                &mut pending_shader_load_op,
                &mut next_load_op,
            )?;
            flush_pending_layer_composites(
                backend,
                &mut pending_composites,
                &target.view,
                (width, height),
                &mut pending_composite_load_op,
                &mut next_load_op,
            );
            flush_pending_clear(backend, &target.view, &mut next_load_op);
        }
        let child_translation_context = TranslationRenderContext {
            inherited_content_translation: effective_translated_content_context,
            translated_content_axes: effective_translated_content_axes,
            surface_capture_active: translation_context.surface_capture_active,
            local_picture_capture_active: translation_context.local_picture_capture_active,
        };
        let child_underlay_color = child.needs_nested_underlay.then(|| {
            uniform_underlay_color_before(
                local_scene,
                &prior_child_contributions,
                child.z_index,
                quad_bounds(child_dest_quad),
                target_scale,
                underlay_color,
            )
        });
        let child_underlay = child.needs_nested_underlay.then(|| {
            let sample_rect = underlay_sample_rect(&child.source);
            create_projected_child_underlay(
                backend,
                &target,
                backdrop_underlay,
                resolved_child.logical_rect,
                child_dest_quad,
                target_scale,
                child_surface_target_scale(&child, target_scale, child_translation_context),
                sample_rect,
            )
        });
        let child_surface = render_layer_surface(
            backend,
            &mut child,
            LayerSurfaceRequest {
                root_scale: target_scale,
                backdrop_underlay: child_underlay.as_ref(),
                backdrop_underlay_color: child_underlay_color.flatten(),
                allow_runtime_cache: true,
                logical_rect_override: Some(resolved_child.logical_rect),
                capture_clip_override: resolved_child.surface_clip,
                activates_nested_capture: true,
                translation_context: child_translation_context,
            },
        )?;

        let child_backdrop_capture_rect = child_surface.backdrop.as_ref().and_then(|backdrop| {
            visible_backdrop_capture_rect(
                resolved_child.backdrop_rect,
                child.visual_clip,
                backdrop,
                target_scale,
                (width, height),
            )
        });

        // Pending composite dest quads are in physical pixels; the capture rect is
        // logical, so scale it before the overlap test.
        if child_backdrop_capture_rect.is_some_and(|rect| {
            pending_layer_composites_intersect_rect(
                &pending_composites,
                surface_pixel_rect(rect, target_scale),
            )
        }) || !resolved_child.shadow_draws.is_empty()
        {
            flush_pending_shader_layer_composites(
                backend,
                &mut pending_shader_composites,
                &target.view,
                (width, height),
                &mut pending_shader_load_op,
                &mut next_load_op,
            )?;
            flush_pending_layer_composites(
                backend,
                &mut pending_composites,
                &target.view,
                (width, height),
                &mut pending_composite_load_op,
                &mut next_load_op,
            );
        }

        if let Some(backdrop) = &child_surface.backdrop {
            flush_pending_shader_layer_composites(
                backend,
                &mut pending_shader_composites,
                &target.view,
                (width, height),
                &mut pending_shader_load_op,
                &mut next_load_op,
            )?;
            flush_pending_clear(backend, &target.view, &mut next_load_op);
            let backdrop_layer = BackdropLayer {
                node_id: child.node_id,
                rect: resolved_child.backdrop_rect,
                clip: child.visual_clip,
                snap_anchor: resolved_child.snap_anchor,
                effect: backdrop.clone(),
                z_index: child.z_index,
            };
            let backdrop_input_hash = backdrop_scene_prefix_hash(
                local_scene,
                &prior_child_contributions,
                child.z_index,
                child_backdrop_capture_rect.unwrap_or(backdrop_layer.rect),
                (width, height),
                target_scale,
            );
            let effective_backdrop_underlay = if backdrop_underlay.is_some()
                && backdrop_underlay_is_covered_by_local_content(
                    &local_scene.shapes,
                    &local_scene.brushes,
                    &local_scene.images,
                    &local_scene.shadow_draws,
                    &local_scene.draw_ops,
                    &local_scene.effect_layers,
                    &local_scene.backdrop_layers,
                    &backdrop_layer,
                ) {
                None
            } else {
                backdrop_underlay
            };
            if let Some(prepared) = prepare_cached_backdrop_layer_composite(
                backend,
                &target,
                &backdrop_layer,
                effective_backdrop_underlay,
                width,
                height,
                target_scale,
                Some(backdrop_input_hash),
            )? {
                if pending_composites.is_empty() {
                    pending_composite_load_op = Some(next_load_op);
                }
                pending_composites.push(PendingLayerComposite {
                    z_index: child.z_index,
                    surface: prepared.surface,
                    dest_quad: prepared.dest_quad,
                    scissor: prepared.scissor,
                });
                next_load_op = wgpu::LoadOp::Load;
            } else {
                apply_backdrop_layer_to_target(
                    backend,
                    &target,
                    &backdrop_layer,
                    effective_backdrop_underlay,
                    width,
                    height,
                    target_scale,
                    Some(backdrop_input_hash),
                )?;
                if !pending_composites.is_empty() {
                    pending_composite_load_op = Some(wgpu::LoadOp::Load);
                }
            }
        }

        if !resolved_child.shadow_draws.is_empty() {
            flush_pending_shader_layer_composites(
                backend,
                &mut pending_shader_composites,
                &target.view,
                (width, height),
                &mut pending_shader_load_op,
                &mut next_load_op,
            )?;
            flush_pending_clear(backend, &target.view, &mut next_load_op);
        }
        for shadow in &resolved_child.shadow_draws {
            backend.render_shadow_draw(&target.view, shadow, width, height, target_scale);
        }

        let dest_quad = layer_surface_dest_quad(
            resolved_child.logical_rect,
            resolved_child.dest_quad,
            child_surface.logical_rect,
        );
        let dest_quad = anchored_composite_dest_quad(
            dest_quad,
            resolved_child.snap_anchor,
            resolved_child.composite_snap_origin,
            target_scale,
            child_surface.sample_mode,
        );
        let scissor = child
            .visual_clip
            .and_then(|clip| scissor_rect_for_rect(clip, target_scale, width, height));
        let child_prefix_contribution =
            backdrop_prefix_child_contribution(&child, &child_surface, dest_quad, scissor);
        if child_surface.deferred_effect.is_none() && axis_aligned_quad_rect(dest_quad).is_some() {
            flush_pending_shader_layer_composites(
                backend,
                &mut pending_shader_composites,
                &target.view,
                (width, height),
                &mut pending_shader_load_op,
                &mut next_load_op,
            )?;
            if pending_composites.is_empty() {
                pending_composite_load_op = Some(next_load_op);
            }
            pending_composites.push(PendingLayerComposite {
                z_index: child.z_index,
                surface: child_surface,
                dest_quad,
                scissor,
            });
            next_load_op = wgpu::LoadOp::Load;
        } else {
            flush_pending_layer_composites(
                backend,
                &mut pending_composites,
                &target.view,
                (width, height),
                &mut pending_composite_load_op,
                &mut next_load_op,
            );
            match direct_shader_layer_composite(child_surface, child.z_index, dest_quad, scissor) {
                Ok(pending) => {
                    if pending_shader_composites.is_empty() {
                        pending_shader_load_op = Some(next_load_op);
                    }
                    pending_shader_composites.push(pending);
                    next_load_op = wgpu::LoadOp::Load;
                }
                Err(child_surface) => {
                    flush_pending_shader_layer_composites(
                        backend,
                        &mut pending_shader_composites,
                        &target.view,
                        (width, height),
                        &mut pending_shader_load_op,
                        &mut next_load_op,
                    )?;
                    let composite_load_op = next_load_op;
                    composite_layer_surface_to_view(
                        backend,
                        &child_surface,
                        &target.view,
                        (width, height),
                        dest_quad,
                        composite_load_op,
                        scissor,
                    )?;
                    next_load_op = wgpu::LoadOp::Load;
                    backend.release_layer_surface_target(child_surface.target);
                }
            }
        }
        if let Some(underlay) = child_underlay {
            backend.release_frame_surface(underlay);
        }
        prior_child_contributions.push(child_prefix_contribution);
        cursor_z = child.z_index.saturating_add(1);
    }

    if cursor_z < local_scene.next_z {
        flush_pending_shader_layer_composites(
            backend,
            &mut pending_shader_composites,
            &target.view,
            (width, height),
            &mut pending_shader_load_op,
            &mut next_load_op,
        )?;
        if !pending_composites.is_empty()
            && !range_contains_layer_events(
                &local_scene.effect_layers,
                &local_scene.backdrop_layers,
                cursor_z,
                local_scene.next_z,
            )
        {
            let load_op = pending_composite_load_op.take().unwrap_or(next_load_op);
            let composite_items = pending_layer_composite_batch_items(&pending_composites);
            backend.render_non_effect_segment_with_composites(
                &target.view,
                &local_scene.shapes,
                &local_scene.brushes,
                &local_scene.images,
                &local_scene.texts,
                &local_scene.shadow_draws,
                &local_scene.retained_draws,
                &local_scene.draw_ops,
                cursor_z,
                local_scene.next_z,
                &[],
                &composite_items,
                &[],
                width,
                height,
                target_scale,
                load_op,
            )?;
            release_pending_layer_composites(backend, &mut pending_composites);
        } else {
            flush_pending_layer_composites(
                backend,
                &mut pending_composites,
                &target.view,
                (width, height),
                &mut pending_composite_load_op,
                &mut next_load_op,
            );
            let local_backdrop_hashes = scene_backdrop_input_hashes(
                local_scene,
                &prior_child_contributions,
                (width, height),
                target_scale,
            );
            backend.render_range_with_layer_events_to_target(
                &target,
                &local_scene.shapes,
                &local_scene.brushes,
                &local_scene.images,
                &local_scene.texts,
                &local_scene.shadow_draws,
                &local_scene.retained_draws,
                &local_scene.draw_ops,
                &local_scene.effect_layers,
                &local_scene.backdrop_layers,
                &local_backdrop_hashes,
                cursor_z,
                local_scene.next_z,
                None,
                width,
                height,
                target_scale,
                backdrop_underlay,
                next_load_op,
            )?;
        }
    } else if matches!(next_load_op, wgpu::LoadOp::Clear(_)) {
        backend.clear_target_view_with_load_op(&target.view, next_load_op);
    } else {
        flush_pending_shader_layer_composites(
            backend,
            &mut pending_shader_composites,
            &target.view,
            (width, height),
            &mut pending_shader_load_op,
            &mut next_load_op,
        )?;
        flush_pending_layer_composites(
            backend,
            &mut pending_composites,
            &target.view,
            (width, height),
            &mut pending_composite_load_op,
            &mut next_load_op,
        );
    }

    Ok(target)
}

fn render_layer_surface_uncached<B: SurfaceExecutionBackend>(
    backend: &mut B,
    child: &ChildLayerComposite,
    source: LoweredChildSource,
    options: LayerSurfaceRenderOptions<'_>,
) -> Result<LayerSurface, String> {
    let LayerSurfaceRenderOptions {
        target_scale,
        backdrop_underlay,
        backdrop_underlay_color,
        allow_runtime_cache,
        mut cache_candidate,
        logical_rect_override,
        capture_clip_override,
        composite_sample_mode,
        translation_context,
    } = options;
    let isolation = child.isolation.clone();
    let LoweredChildSource {
        scene: mut local_scene,
        children: child_layers,
    } = source;
    let result = (|| -> Result<LayerSurface, String> {
        let surface_requirements = child.surface_requirements;
        let effective_translated_content_context = translation_context
            .inherited_content_translation
            || child.translated_content_context
            || surface_requirements.contains_translated_content;
        let effective_translated_content_axes = translation_context
            .translated_content_axes
            .union(child.own_translated_content_axes)
            .union(surface_requirements.translated_content_axes);
        let effective_requirements = effective_surface_requirements(
            effective_translated_content_context,
            translation_context.surface_capture_active,
            surface_requirements,
        );
        let capture_clip = combined_capture_clip(child.clip_rect, capture_clip_override);
        let estimated_surface_rect = cache_candidate
            .as_ref()
            .map(|(_, logical_rect)| *logical_rect)
            .or(logical_rect_override)
            .unwrap_or_else(|| {
                let bounds = motion_stable_capture_bounds_from_parts(
                    child.clip_rect,
                    child.backdrop.is_some(),
                    child.own_translated_content_axes,
                    &local_scene,
                    &child_layers,
                    effective_requirements,
                    effective_translated_content_axes,
                    capture_clip,
                );
                resolved_layer_surface_rect_from_parts(
                    child.local_bounds,
                    child.has_effect,
                    child.backdrop.is_some(),
                    bounds,
                )
            });
        let mut surface_rect =
            if effective_requirements.contains(SurfaceRequirement::MotionStableCapture) {
                let bounds = motion_stable_capture_bounds_from_parts(
                    child.clip_rect,
                    child.backdrop.is_some(),
                    child.own_translated_content_axes,
                    &local_scene,
                    &child_layers,
                    effective_requirements,
                    effective_translated_content_axes,
                    capture_clip,
                );
                resolved_layer_surface_rect_from_parts(
                    child.local_bounds,
                    child.has_effect,
                    child.backdrop.is_some(),
                    bounds,
                )
            } else {
                estimated_surface_rect
            };
        if cache_candidate
            .as_ref()
            .is_some_and(|(_, logical_rect)| *logical_rect != surface_rect)
        {
            cache_candidate = None;
        }
        let max_dim = backend.max_texture_dim() as f32;
        if effective_requirements.contains(SurfaceRequirement::MotionStableCapture) {
            if let Some(visible_bounds) = collected_layer_bounds(&local_scene, &child_layers, true)
                .and_then(|bounds| visible_draw_rect(bounds, capture_clip))
            {
                let required_rect = resolved_layer_surface_rect_from_parts(
                    child.local_bounds,
                    child.has_effect,
                    child.backdrop.is_some(),
                    Some(visible_bounds),
                );
                let desired_scale =
                    quantize_motion_stable_target_scale(target_scale, composite_sample_mode);
                surface_rect = fit_capture_rect_to_scale_budget_for_axes(
                    surface_rect,
                    required_rect,
                    desired_scale,
                    backend.max_texture_dim(),
                    effective_translated_content_axes,
                );
            }
        }
        let target_scale = target_scale
            .min(max_dim / surface_rect.width.max(1.0))
            .min(max_dim / surface_rect.height.max(1.0));
        let minimum_surface_scale = minimum_surface_scale_for_composite(
            target_scale,
            composite_sample_mode,
            effective_requirements,
        );
        let target_scale = clamp_effect_surface_scale(
            surface_rect,
            minimum_surface_scale,
            target_scale,
            backend.max_texture_dim(),
        );
        let target_scale = quantize_motion_stable_target_scale(target_scale, composite_sample_mode);
        let (width, height) =
            surface_target_size(surface_rect, target_scale, backend.max_texture_dim());
        // The composite maps the whole texture onto the destination quad, so
        // the rect the quad is built from has to be the area the texture
        // actually covers. Without this the ceil padding is stretched across
        // the quad and the content lands up to half a device pixel toward its
        // origin — invisible while the scale is constant, per-frame jitter
        // once the scale animates.
        let surface_rect =
            device_pixel_exact_surface_rect(surface_rect, target_scale, width, height);
        let shift = cranpose_ui_graphics::Point {
            x: -surface_rect.x,
            y: -surface_rect.y,
        };
        local_scene.translate_by(shift);

        let mut child_layers = child_layers;
        for nested_child in &mut child_layers {
            nested_child.translate_by(shift);
        }
        backend.record_isolated_layer_render(
            width,
            height,
            child.node_id,
            surface_rect,
            effective_requirements,
        );
        let source_uses_external_backdrop = layer_source_uses_external_backdrop_underlay(
            &local_scene,
            &child_layers,
            backdrop_underlay.is_some(),
        );
        let source_cache_key = layer_source_cache_key(
            child,
            effective_requirements,
            surface_rect,
            (width, height),
            target_scale,
            source_uses_external_backdrop,
            allow_runtime_cache,
        );
        if layer_render_diag_enabled() {
            log::warn!(
                "[layer-render-diag] node={:?} size={}x{} scale={:.3} rect=({:.1},{:.1},{:.1},{:.1}) requirements={:?} cache_candidate={} backdrop_underlay={} external_backdrop_input={}",
                child.node_id,
                width,
                height,
                target_scale,
                surface_rect.x,
                surface_rect.y,
                surface_rect.width,
                surface_rect.height,
                effective_requirements,
                source_cache_key.is_some(),
                backdrop_underlay.is_some(),
                source_uses_external_backdrop,
            );
        }
        let mut target = if let Some(cache_key) = source_cache_key {
            if let Some((cached_target, _)) = backend.cached_layer_surface(&cache_key) {
                LayerSurfaceTexture::Cached(cached_target)
            } else {
                backend.record_layer_cache_miss(width, height);
                let rendered = render_layer_source_uncached(
                    backend,
                    &local_scene,
                    child_layers,
                    target_scale,
                    backdrop_underlay,
                    backdrop_underlay_color,
                    width,
                    height,
                    effective_translated_content_context,
                    effective_translated_content_axes,
                    translation_context,
                )?;
                if offscreen_byte_size(rendered.width, rendered.height)
                    <= MAX_LAYER_SURFACE_CACHE_BYTES
                {
                    let cached_target =
                        backend.insert_cached_layer_surface(cache_key, rendered, surface_rect);
                    LayerSurfaceTexture::Cached(cached_target)
                } else {
                    LayerSurfaceTexture::Owned(rendered)
                }
            }
        } else {
            LayerSurfaceTexture::Owned(render_layer_source_uncached(
                backend,
                &local_scene,
                child_layers,
                target_scale,
                backdrop_underlay,
                backdrop_underlay_color,
                width,
                height,
                effective_translated_content_context,
                effective_translated_content_axes,
                translation_context,
            )?)
        };
        let mut deferred_effect = None;
        if let Some(effect) = isolation.as_ref().and_then(|params| params.effect.as_ref()) {
            if backend.is_render_effect_supported(effect) {
                deferred_effect = Some(effect.clone());
            } else {
                backend.warn_unsupported_effect_once();
            }
        }

        let composite_alpha = isolation
            .as_ref()
            .map(|params| params.composite_alpha)
            .unwrap_or(1.0);
        let blend_mode = isolation
            .as_ref()
            .map(|params| params.blend_mode)
            .unwrap_or(BlendMode::SrcOver);
        let backdrop = child.backdrop.clone();
        let rounded_clip = child.rounded_clip;

        if let Some(effect) = deferred_effect.as_ref() {
            if can_materialize_cached_effect(effect, backdrop.as_ref())
                && offscreen_byte_size(width, height) <= MAX_LAYER_SURFACE_CACHE_BYTES
            {
                if let Some((cache_key, logical_rect)) = cache_candidate.take() {
                    let effect_target = backend.acquire_retained_surface(width, height);
                    materialize_render_effect_to_target(
                        backend,
                        target.target(),
                        effect,
                        &effect_target,
                        content_effect_pixel_rect(
                            Some(child.local_bounds),
                            logical_rect,
                            width,
                            height,
                        ),
                        composite_sample_mode,
                    )?;
                    backend.release_layer_surface_target(target);
                    let cached_target =
                        backend.insert_cached_layer_surface(cache_key, effect_target, logical_rect);
                    return Ok(LayerSurface {
                        target: LayerSurfaceTexture::Cached(cached_target),
                        logical_rect,
                        composite_alpha,
                        blend_mode,
                        rounded_clip,
                        backdrop,
                        deferred_effect: None,
                        effect_content_rect: None,
                        sample_mode: composite_sample_mode,
                    });
                }
            }
        }

        if rounded_clip.is_some() {
            if let Some(effect) = deferred_effect.take() {
                let effect_target = backend.acquire_frame_surface(width, height);
                let materialized = materialize_render_effect_to_target(
                    backend,
                    target.target(),
                    &effect,
                    &effect_target,
                    content_effect_pixel_rect(
                        Some(child.local_bounds),
                        surface_rect,
                        width,
                        height,
                    ),
                    composite_sample_mode,
                );
                if let Err(error) = materialized {
                    backend.release_frame_surface(effect_target);
                    return Err(error);
                }
                backend.release_layer_surface_target(target);
                target = LayerSurfaceTexture::Owned(effect_target);
            }
        }

        if deferred_effect.is_none() {
            if let Some((cache_key, logical_rect)) = cache_candidate {
                if let LayerSurfaceTexture::Owned(owned_target) = target {
                    if offscreen_byte_size(owned_target.width, owned_target.height)
                        <= MAX_LAYER_SURFACE_CACHE_BYTES
                    {
                        let cached_target = backend.insert_cached_layer_surface(
                            cache_key,
                            owned_target,
                            logical_rect,
                        );
                        return Ok(LayerSurface {
                            target: LayerSurfaceTexture::Cached(cached_target),
                            logical_rect,
                            composite_alpha,
                            blend_mode,
                            rounded_clip,
                            backdrop,
                            deferred_effect: None,
                            effect_content_rect: None,
                            sample_mode: composite_sample_mode,
                        });
                    }
                    target = LayerSurfaceTexture::Owned(owned_target);
                }
            }
        }

        Ok(LayerSurface {
            target,
            logical_rect: surface_rect,
            composite_alpha,
            blend_mode,
            rounded_clip,
            backdrop,
            deferred_effect,
            effect_content_rect: Some(child.local_bounds),
            sample_mode: composite_sample_mode,
        })
    })();
    drop(local_scene);
    result
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_effect_layer_to_target<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target: &OffscreenTarget,
    shapes: &[DrawShape],
    brushes: &[Brush],
    images: &[ImageDraw],
    texts: &[TextDraw],
    shadow_draws: &[ShadowDraw],
    draw_ops: &[DrawOp],
    effect_layers: &[EffectLayer],
    backdrop_layers: &[BackdropLayer],
    effect_layer_index: usize,
    backdrop_underlay: Option<&OffscreenTarget>,
    width: u32,
    height: u32,
    root_scale: f32,
) -> Result<(), String> {
    let layer = effect_layers
        .get(effect_layer_index)
        .cloned()
        .ok_or_else(|| "effect layer index out of bounds".to_string())?;
    let Some(visible_rect) = visible_layer_rect(layer.rect, layer.clip, root_scale, width, height)
    else {
        return Ok(());
    };
    let Some(scissor) = scissor_rect_for_rect(visible_rect, root_scale, width, height) else {
        return Ok(());
    };
    let sample_mode = composite_sample_mode_for_effect_layer(&layer);
    let stable_local_capture = sample_mode == CompositeSampleMode::Box4
        && (layer.effect.is_none()
            || layer
                .requirements
                .contains(SurfaceRequirement::TextMaterialMask))
        && !has_backdrop_layer_in_range(backdrop_layers, layer.z_start, layer.z_end)
        && backdrop_underlay.is_none()
        && layer
            .requirements
            .contains(SurfaceRequirement::MotionStableCapture);
    let capture_rect = if stable_local_capture {
        layer.rect
    } else {
        visible_rect
    };
    let effect_root_scale = clamp_effect_surface_scale(
        capture_rect,
        effect_layer_minimum_scale(&layer, root_scale),
        effect_layer_target_scale(&layer, root_scale),
        backend.max_texture_dim(),
    );
    let effect_root_scale = quantize_motion_stable_target_scale(effect_root_scale, sample_mode);
    let (effect_width, effect_height) =
        surface_target_size(capture_rect, effect_root_scale, backend.max_texture_dim());
    let window_scene = build_scene_window(
        SceneWindowSource {
            shapes,
            brushes,
            images,
            texts,
            shadow_draws,
            draw_ops,
            effect_layers,
            backdrop_layers,
        },
        layer.z_start,
        layer.z_end,
        capture_rect,
    );
    let has_nested_backdrop =
        has_backdrop_layer_in_range(&window_scene.backdrop_layers, layer.z_start, layer.z_end);
    let Some(window_effect_index) = filtered_effect_layer_index(
        effect_layers,
        effect_layer_index,
        layer.z_start,
        layer.z_end,
    ) else {
        return Err("effect layer window index is missing".to_string());
    };

    let source = backend.acquire_frame_surface(effect_width, effect_height);
    let layer_underlay = if has_nested_backdrop {
        let underlay = backend.acquire_frame_surface(effect_width, effect_height);
        copy_projective_backdrop_inputs_to_view(
            backend,
            backdrop_underlay,
            target,
            visible_rect,
            &underlay.view,
            (effect_width, effect_height),
            effect_root_scale,
        )?;
        Some(underlay)
    } else {
        None
    };

    let render_result = backend.render_range_with_layer_events_to_target(
        &source,
        &window_scene.shapes,
        &window_scene.brushes,
        &window_scene.images,
        &window_scene.texts,
        &window_scene.shadow_draws,
        &window_scene.retained_draws,
        &window_scene.draw_ops,
        &window_scene.effect_layers,
        &window_scene.backdrop_layers,
        &[],
        layer.z_start,
        layer.z_end,
        Some(window_effect_index),
        effect_width,
        effect_height,
        effect_root_scale,
        layer_underlay.as_ref(),
        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
    );

    if let Some(underlay) = layer_underlay {
        backend.release_frame_surface(underlay);
    }

    render_result?;

    let dest_quad = anchored_composite_dest_quad(
        crate::rect_to_quad(capture_rect),
        layer.snap_anchor,
        None,
        root_scale,
        sample_mode,
    );
    if layer.effect.is_none() {
        let composite_result = composite_surface_to_view(
            backend,
            &source,
            &target.view,
            (width, height),
            dest_quad,
            layer.composite_alpha,
            wgpu::LoadOp::Load,
            Some(scissor),
            layer.blend_mode,
            sample_mode,
        );
        backend.release_frame_surface(source);
        return composite_result;
    }

    let Some(effect) = &layer.effect else {
        backend.release_frame_surface(source);
        return Err("effect layer destination requested without render effect".to_string());
    };
    if backend.is_render_effect_supported(effect) {
        if let Some(dest_rect) = axis_aligned_quad_rect(dest_quad) {
            let dest_viewport = Some(composite_dest_viewport(
                dest_rect,
                effect_width,
                effect_height,
                sample_mode,
            ));
            if let RenderEffect::Shader { shader } = effect {
                backend.apply_shader_and_composite_to_view(
                    &source,
                    shader,
                    content_effect_pixel_rect(
                        Some(layer.rect),
                        capture_rect,
                        effect_width,
                        effect_height,
                    ),
                    &target.view,
                    layer.composite_alpha,
                    wgpu::LoadOp::Load,
                    Some(scissor),
                    layer.blend_mode,
                    dest_viewport,
                    sample_mode,
                );
            } else {
                backend.apply_effect_and_composite_to_view(
                    &source,
                    effect,
                    content_effect_pixel_rect(
                        Some(layer.rect),
                        capture_rect,
                        effect_width,
                        effect_height,
                    ),
                    &target.view,
                    layer.composite_alpha,
                    wgpu::LoadOp::Load,
                    Some(scissor),
                    layer.blend_mode,
                    dest_viewport,
                    sample_mode,
                )?;
            }
        } else {
            let source_rect = Rect {
                x: 0.0,
                y: 0.0,
                width: effect_width as f32,
                height: effect_height as f32,
            };
            let inverse = ProjectiveTransform::from_rect_to_quad(source_rect, dest_quad)
                .inverse()
                .ok_or_else(|| "effect layer transform is not invertible".to_string())?;
            backend.apply_effect_and_composite_to_view_projective(
                &source,
                effect,
                content_effect_pixel_rect(
                    Some(layer.rect),
                    capture_rect,
                    effect_width,
                    effect_height,
                ),
                &target.view,
                (width, height),
                (source_rect.width, source_rect.height),
                inverse.matrix(),
                dest_quad,
                layer.composite_alpha,
                wgpu::LoadOp::Load,
                Some(scissor),
                layer.blend_mode,
                sample_mode,
            )?;
        }
    } else {
        backend.warn_unsupported_effect_once();
        composite_surface_to_view(
            backend,
            &source,
            &target.view,
            (width, height),
            dest_quad,
            layer.composite_alpha,
            wgpu::LoadOp::Load,
            Some(scissor),
            layer.blend_mode,
            sample_mode,
        )?;
    }

    backend.release_frame_surface(source);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn prepare_cached_backdrop_layer_composite<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target: &OffscreenTarget,
    layer: &BackdropLayer,
    backdrop_underlay: Option<&OffscreenTarget>,
    width: u32,
    height: u32,
    root_scale: f32,
    input_content_hash: Option<u64>,
) -> Result<Option<PreparedBackdropComposite>, String> {
    let diag = backdrop_diag_enabled();
    let (layer_rect, layer_clip) = snapped_backdrop_geometry(layer, root_scale);
    let Some(visible_rect) = visible_layer_rect(layer_rect, layer_clip, root_scale, width, height)
    else {
        if diag {
            eprintln!(
                "[backdrop-diag] prepare SKIP: no visible rect for {:?}",
                layer_rect
            );
        }
        return Ok(None);
    };
    let Some(scissor) = scissor_rect_for_rect(
        backdrop_output_rect(
            visible_rect,
            layer_clip,
            &layer.effect,
            root_scale,
            (width, height),
        ),
        root_scale,
        width,
        height,
    ) else {
        if diag {
            eprintln!("[backdrop-diag] prepare SKIP: empty scissor for {visible_rect:?}");
        }
        return Ok(None);
    };
    let capture_rect = backdrop_capture_rect(
        visible_rect,
        layer_clip,
        &layer.effect,
        root_scale,
        (width, height),
    );
    if diag {
        eprintln!(
            "[backdrop-diag] prepare node={:?} visible={visible_rect:?} capture={capture_rect:?} scissor={scissor:?} in_pad={} out_pad={}",
            layer.node_id,
            layer.effect.input_padding(),
            layer.effect.output_padding(),
        );
    }
    let backdrop_scale = clamp_effect_surface_scale(
        capture_rect,
        root_scale,
        root_scale,
        backend.max_texture_dim(),
    );
    // The effect geometry anchors to the FULL node rect: a viewport or
    // scroll clip shrinks the visible/capture rects, and a shader
    // handed the clipped rect rescales its whole dp mapping (the
    // window-bottom bar's lens drew its capsule ~9dp high). Clipping
    // belongs to the scissor, never to the effect rect.
    //
    // The copy plan is the single source of truth for the snapshot size: it
    // spans floor(origin)..ceil(origin+extent) in texels, which runs one
    // texel past ceil(extent) whenever the capture origin is fractional.
    // Sizing the surface independently rejects the 1:1 copy every frame and
    // the projective fallback hands the effect a backdrop with fractional
    // alpha — the glass body then composites washed-out (menu-expand gray).
    let copy_plan = if (backdrop_scale - root_scale).abs() <= 0.01 {
        axis_aligned_backdrop_snapshot_copy_plan(
            capture_rect,
            layer_rect,
            root_scale,
            (target.width, target.height),
            backend.max_texture_dim(),
        )
    } else {
        None
    };
    let (backdrop_width, backdrop_height) = copy_plan.map(|plan| plan.size).unwrap_or_else(|| {
        surface_target_size(capture_rect, backdrop_scale, backend.max_texture_dim())
    });
    let Some(cache_key) = input_content_hash.and_then(|hash| {
        (backdrop_underlay.is_none()
            && backend.is_render_effect_supported(&layer.effect)
            && offscreen_byte_size(backdrop_width, backdrop_height)
                <= MAX_LAYER_SURFACE_CACHE_BYTES)
            .then(|| {
                backdrop_effect_cache_key(
                    layer,
                    hash,
                    capture_rect,
                    (backdrop_width, backdrop_height),
                    root_scale,
                )
            })
            .flatten()
    }) else {
        return Ok(None);
    };

    let target_texture = if let Some((cached_target, _)) = backend.cached_layer_surface(&cache_key)
    {
        LayerSurfaceTexture::Cached(cached_target)
    } else {
        backend.record_layer_cache_miss(backdrop_width, backdrop_height);
        let snapshot = backend.acquire_frame_surface(backdrop_width, backdrop_height);
        let copied_snapshot = copy_plan.is_some_and(|plan| {
            backend.copy_texture_region_to_target(target, plan.source_origin, &snapshot, plan.size)
        });
        if !copied_snapshot {
            copy_projective_backdrop_inputs_to_view(
                backend,
                None,
                target,
                capture_rect,
                &snapshot.view,
                (backdrop_width, backdrop_height),
                backdrop_scale,
            )?;
        }

        let effect_target = backend.acquire_retained_surface(backdrop_width, backdrop_height);
        if diag {
            eprintln!(
                "[backdrop-diag] prepare MISS copied={} plan={:?} backdrop_size=({},{}) scale={}",
                copied_snapshot,
                copy_plan.map(|plan| (plan.source_origin, plan.size, plan.effect_pixel_rect)),
                backdrop_width,
                backdrop_height,
                backdrop_scale,
            );
        }
        materialize_render_effect_to_target(
            backend,
            &snapshot,
            &layer.effect,
            &effect_target,
            copy_plan
                .map(|plan| plan.effect_pixel_rect)
                .unwrap_or_else(|| {
                    let capture = surface_pixel_rect(capture_rect, backdrop_scale);
                    let effect = surface_pixel_rect(layer_rect, backdrop_scale);
                    [
                        effect.x - capture.x,
                        effect.y - capture.y,
                        effect.width,
                        effect.height,
                    ]
                }),
            CompositeSampleMode::Linear,
        )?;
        backend.release_frame_surface(snapshot);
        LayerSurfaceTexture::Cached(backend.insert_cached_layer_surface(
            cache_key,
            effect_target,
            capture_rect,
        ))
    };

    Ok(Some(PreparedBackdropComposite {
        surface: LayerSurface {
            target: target_texture,
            logical_rect: capture_rect,
            composite_alpha: 1.0,
            blend_mode: BlendMode::SrcOver,
            rounded_clip: None,
            backdrop: None,
            deferred_effect: None,
            effect_content_rect: None,
            sample_mode: CompositeSampleMode::Linear,
        },
        dest_quad: scaled_quad(crate::rect_to_quad(capture_rect), root_scale),
        scissor: Some(scissor),
    }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_backdrop_layer_to_target<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target: &OffscreenTarget,
    layer: &BackdropLayer,
    backdrop_underlay: Option<&OffscreenTarget>,
    width: u32,
    height: u32,
    root_scale: f32,
    input_content_hash: Option<u64>,
) -> Result<(), String> {
    let (layer_rect, layer_clip) = snapped_backdrop_geometry(layer, root_scale);
    if backdrop_diag_enabled() {
        eprintln!(
            "[backdrop-diag] apply rect={:?} clip={:?} in_pad={} out_pad={} hash={:?}",
            layer_rect,
            layer_clip,
            layer.effect.input_padding(),
            layer.effect.output_padding(),
            input_content_hash.is_some()
        );
    }
    if let Some(prepared) = prepare_cached_backdrop_layer_composite(
        backend,
        target,
        layer,
        backdrop_underlay,
        width,
        height,
        root_scale,
        input_content_hash,
    )? {
        if backdrop_diag_enabled() {
            eprintln!(
                "[backdrop-diag] cached path dest_quad={:?} scissor={:?}",
                prepared.dest_quad, prepared.scissor
            );
        }
        composite_layer_surface_to_view(
            backend,
            &prepared.surface,
            &target.view,
            (width, height),
            prepared.dest_quad,
            wgpu::LoadOp::Load,
            prepared.scissor,
        )?;
        backend.release_layer_surface_target(prepared.surface.target);
        return Ok(());
    }

    let Some(visible_rect) = visible_layer_rect(layer_rect, layer_clip, root_scale, width, height)
    else {
        if backdrop_diag_enabled() {
            eprintln!("[backdrop-diag] SKIP: no visible rect");
        }
        return Ok(());
    };
    let Some(scissor) = scissor_rect_for_rect(
        backdrop_output_rect(
            visible_rect,
            layer_clip,
            &layer.effect,
            root_scale,
            (width, height),
        ),
        root_scale,
        width,
        height,
    ) else {
        if backdrop_diag_enabled() {
            eprintln!("[backdrop-diag] SKIP: empty scissor");
        }
        return Ok(());
    };
    if backdrop_diag_enabled() {
        eprintln!(
            "[backdrop-diag] uncached visible={visible_rect:?} scissor={scissor:?} capture={:?}",
            backdrop_capture_rect(
                visible_rect,
                layer_clip,
                &layer.effect,
                root_scale,
                (width, height),
            )
        );
    }
    let capture_rect = backdrop_capture_rect(
        visible_rect,
        layer_clip,
        &layer.effect,
        root_scale,
        (width, height),
    );
    let backdrop_scale = clamp_effect_surface_scale(
        capture_rect,
        root_scale,
        root_scale,
        backend.max_texture_dim(),
    );
    let snapshot_copy_plan =
        if backdrop_underlay.is_none() && (backdrop_scale - root_scale).abs() <= 0.01 {
            axis_aligned_backdrop_snapshot_copy_plan(
                capture_rect,
                layer_rect,
                root_scale,
                (target.width, target.height),
                backend.max_texture_dim(),
            )
        } else {
            None
        };
    let (backdrop_width, backdrop_height) =
        snapshot_copy_plan.map(|plan| plan.size).unwrap_or_else(|| {
            surface_target_size(capture_rect, backdrop_scale, backend.max_texture_dim())
        });
    let effect_pixel_rect = snapshot_copy_plan
        .map(|plan| plan.effect_pixel_rect)
        .unwrap_or_else(|| {
            let capture = surface_pixel_rect(capture_rect, backdrop_scale);
            let effect = surface_pixel_rect(layer_rect, backdrop_scale);
            [
                effect.x - capture.x,
                effect.y - capture.y,
                effect.width,
                effect.height,
            ]
        });
    let dest_viewport = Some(
        snapshot_copy_plan
            .map(|plan| plan.dest_viewport)
            .unwrap_or((
                capture_rect.x * root_scale,
                capture_rect.y * root_scale,
                capture_rect.width * root_scale,
                capture_rect.height * root_scale,
            )),
    );

    let snapshot = backend.acquire_frame_surface(backdrop_width, backdrop_height);
    let copied_snapshot = snapshot_copy_plan.is_some_and(|plan| {
        backend.copy_texture_region_to_target(target, plan.source_origin, &snapshot, plan.size)
    });
    if !copied_snapshot {
        copy_projective_backdrop_inputs_to_view(
            backend,
            backdrop_underlay,
            target,
            capture_rect,
            &snapshot.view,
            (backdrop_width, backdrop_height),
            backdrop_scale,
        )?;
    }

    if backend.is_render_effect_supported(&layer.effect) {
        if let RenderEffect::Shader { shader } = &layer.effect {
            backend.apply_shader_and_composite_to_view(
                &snapshot,
                shader,
                effect_pixel_rect,
                &target.view,
                1.0,
                wgpu::LoadOp::Load,
                Some(scissor),
                BlendMode::SrcOver,
                dest_viewport,
                CompositeSampleMode::Linear,
            );
        } else {
            backend.apply_effect_and_composite_to_view(
                &snapshot,
                &layer.effect,
                effect_pixel_rect,
                &target.view,
                1.0,
                wgpu::LoadOp::Load,
                Some(scissor),
                BlendMode::SrcOver,
                dest_viewport,
                CompositeSampleMode::Linear,
            )?;
        }
    } else {
        backend.warn_unsupported_effect_once();
        backend.composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
            &snapshot,
            &target.view,
            1.0,
            wgpu::LoadOp::Load,
            Some(scissor),
            None,
            BlendMode::SrcOver,
            dest_viewport,
            CompositeSampleMode::Linear,
        );
    }

    backend.release_frame_surface(snapshot);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_surface_to_view<B: SurfaceExecutionBackend>(
    backend: &mut B,
    source: &OffscreenTarget,
    dest_view: &wgpu::TextureView,
    viewport: (u32, u32),
    dest_quad: [[f32; 2]; 4],
    alpha: f32,
    load_op: wgpu::LoadOp<wgpu::Color>,
    scissor: Option<(u32, u32, u32, u32)>,
    blend_mode: BlendMode,
    sample_mode: CompositeSampleMode,
) -> Result<(), String> {
    if let Some(dest_rect) = axis_aligned_quad_rect(dest_quad) {
        backend.composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
            source,
            dest_view,
            alpha,
            load_op,
            scissor,
            None,
            blend_mode,
            Some(composite_dest_viewport(
                dest_rect,
                source.width,
                source.height,
                sample_mode,
            )),
            sample_mode,
        );
        return Ok(());
    }

    let source_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: source.width as f32,
        height: source.height as f32,
    };
    let inverse = ProjectiveTransform::from_rect_to_quad(source_rect, dest_quad)
        .inverse()
        .ok_or_else(|| {
            format!(
                "child layer transform is not invertible: source={}x{}, destination={dest_quad:?}",
                source.width, source.height
            )
        })?;
    backend.composite_to_view_projective(
        source,
        dest_view,
        viewport,
        (source_rect.width, source_rect.height),
        inverse.matrix(),
        dest_quad,
        alpha,
        load_op,
        scissor,
        blend_mode,
        sample_mode,
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn composite_layer_surface_to_view<B: SurfaceExecutionBackend>(
    backend: &mut B,
    surface: &LayerSurface,
    dest_view: &wgpu::TextureView,
    viewport: (u32, u32),
    dest_quad: [[f32; 2]; 4],
    load_op: wgpu::LoadOp<wgpu::Color>,
    scissor: Option<(u32, u32, u32, u32)>,
) -> Result<(), String> {
    let source = surface.target.target();
    if let Some(effect) = &surface.deferred_effect {
        if let RenderEffect::Shader { shader } = effect {
            if let Some(dest_rect) = axis_aligned_quad_rect(dest_quad) {
                backend.apply_shader_and_composite_to_view(
                    source,
                    shader,
                    content_effect_pixel_rect(
                        surface.effect_content_rect,
                        surface.logical_rect,
                        source.width,
                        source.height,
                    ),
                    dest_view,
                    surface.composite_alpha,
                    load_op,
                    scissor,
                    surface.blend_mode,
                    Some(composite_dest_viewport(
                        dest_rect,
                        source.width,
                        source.height,
                        surface.sample_mode,
                    )),
                    surface.sample_mode,
                );
                return Ok(());
            }

            let source_rect = Rect {
                x: 0.0,
                y: 0.0,
                width: source.width as f32,
                height: source.height as f32,
            };
            let inverse = ProjectiveTransform::from_rect_to_quad(source_rect, dest_quad)
                .inverse()
                .ok_or_else(|| "shader child layer transform is not invertible".to_string())?;
            backend.apply_shader_and_composite_to_view_projective(
                source,
                shader,
                content_effect_pixel_rect(
                    surface.effect_content_rect,
                    surface.logical_rect,
                    source.width,
                    source.height,
                ),
                dest_view,
                viewport,
                (source_rect.width, source_rect.height),
                inverse.matrix(),
                dest_quad,
                surface.composite_alpha,
                load_op,
                scissor,
                surface.blend_mode,
                surface.sample_mode,
            );
            return Ok(());
        }

        if let Some(dest_rect) = axis_aligned_quad_rect(dest_quad) {
            backend.apply_effect_and_composite_to_view(
                source,
                effect,
                content_effect_pixel_rect(
                    surface.effect_content_rect,
                    surface.logical_rect,
                    source.width,
                    source.height,
                ),
                dest_view,
                surface.composite_alpha,
                load_op,
                scissor,
                surface.blend_mode,
                Some(composite_dest_viewport(
                    dest_rect,
                    source.width,
                    source.height,
                    surface.sample_mode,
                )),
                surface.sample_mode,
            )?;
            return Ok(());
        }

        let source_rect = Rect {
            x: 0.0,
            y: 0.0,
            width: source.width as f32,
            height: source.height as f32,
        };
        let inverse = ProjectiveTransform::from_rect_to_quad(source_rect, dest_quad)
            .inverse()
            .ok_or_else(|| "effect child layer transform is not invertible".to_string())?;
        backend.apply_effect_and_composite_to_view_projective(
            source,
            effect,
            content_effect_pixel_rect(
                surface.effect_content_rect,
                surface.logical_rect,
                source.width,
                source.height,
            ),
            dest_view,
            viewport,
            (source_rect.width, source_rect.height),
            inverse.matrix(),
            dest_quad,
            surface.composite_alpha,
            load_op,
            scissor,
            surface.blend_mode,
            surface.sample_mode,
        )?;
        return Ok(());
    }

    if let Some(dest_rect) = axis_aligned_quad_rect(dest_quad) {
        backend.composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
            source,
            dest_view,
            surface.composite_alpha,
            load_op,
            scissor,
            layer_surface_rounded_mask(surface, dest_rect),
            surface.blend_mode,
            Some(composite_dest_viewport(
                dest_rect,
                source.width,
                source.height,
                surface.sample_mode,
            )),
            surface.sample_mode,
        );
        return Ok(());
    }

    composite_surface_to_view(
        backend,
        source,
        dest_view,
        viewport,
        dest_quad,
        surface.composite_alpha,
        load_op,
        scissor,
        surface.blend_mode,
        surface.sample_mode,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        anchored_composite_dest_quad, axis_aligned_backdrop_copy_region,
        axis_aligned_backdrop_snapshot_copy_plan, backdrop_effect_cache_key,
        backdrop_scene_prefix_hash, backdrop_underlay_is_covered_by_local_content,
        child_composite_visible, composite_dest_viewport, dest_quad_intersects_rect,
        direct_scene_range_cache_chunk_end, direct_scene_range_cache_enabled_for_policy,
        direct_scene_range_cache_key, direct_scene_range_chunk_fits_cache_entry,
        direct_scene_range_snapped_bounds, layer_source_cache_key,
        layer_source_uses_external_backdrop_underlay, layer_surface_dest_quad,
        layer_surface_translation_context, minimum_surface_scale_for_composite, quad_bounds_rect,
        rects_intersect, render_string_scene_hash, retained_render_effect_hash,
        rounded_fill_covers_rect, scene_backdrop_input_hashes, snapped_backdrop_geometry,
        surface_target_size, underlay_fill_scissor, underlay_sample_rect,
        visible_backdrop_capture_rect, BackdropPrefixChildContribution, DirectChunkRunCoalescer,
        SceneBrush, DEFAULT_DIRECT_SCENE_RANGE_CACHE_BYTES, MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
        MAX_MOTION_SENSITIVE_DIRECT_SCENE_CACHE_DRAW_BYTES, MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
    };
    use crate::effect_renderer::CompositeSampleMode;
    use crate::normalized_scene::TranslateBy;
    use crate::scene::{
        BackdropLayer, CompositorScene, DrawOp, DrawOpKind, DrawShape, ImageDraw, SnapAnchor,
    };
    use crate::surface_plan::layer_surface_requirements_cached;
    use crate::surface_plan::TranslationRenderContext;
    use crate::surface_requirements::{SurfaceRequirement, SurfaceRequirementSet};
    use cranpose_core::collections::map::HashMap;
    use cranpose_core::NodeId;
    use cranpose_render_common::graph::{
        DrawPrimitiveNode, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
        ProjectiveTransform, RenderNode, TextPrimitiveNode,
    };
    use cranpose_ui::text::{AnnotatedString, TextStyle};
    use cranpose_ui::TextLayoutOptions;
    use cranpose_ui_graphics::Point;
    use cranpose_ui_graphics::Rect;
    use cranpose_ui_graphics::{
        BlendMode, Brush, Color, GraphicsLayer, ImageBitmap, ImageSampling, RenderEffect,
        RuntimeShader,
    };
    use std::sync::Arc;

    #[test]
    fn a_rounded_card_covers_what_sits_clear_of_its_corners() {
        let card = Rect {
            x: 0.0,
            y: 0.0,
            width: 344.0,
            height: 110.0,
        };
        let shape = Some(cranpose_ui_graphics::RoundedCornerShape::uniform(20.0));
        let button = Rect {
            x: 284.0,
            y: 33.0,
            width: 44.0,
            height: 44.0,
        };
        assert!(rounded_fill_covers_rect(card, shape, button));

        let corner = Rect {
            x: 2.0,
            y: 2.0,
            width: 30.0,
            height: 30.0,
        };
        assert!(!rounded_fill_covers_rect(card, shape, corner));

        let full_width_band = Rect {
            x: 0.0,
            y: 40.0,
            width: 344.0,
            height: 20.0,
        };
        assert!(rounded_fill_covers_rect(card, shape, full_width_band));

        let outside = Rect {
            x: 340.0,
            y: 40.0,
            width: 20.0,
            height: 20.0,
        };
        assert!(!rounded_fill_covers_rect(card, shape, outside));
        assert!(rounded_fill_covers_rect(card, None, corner));
    }

    #[test]
    fn direct_chunk_run_coalescer_merges_consecutive_direct_chunks() {
        let mut run = DirectChunkRunCoalescer::default();
        // Three consecutive direct chunks 0..64, 64..128, 128..192 absorb
        // into one range flushed at the walk's end.
        run.absorb(0);
        run.absorb(64);
        run.absorb(128);
        assert_eq!(run.flush_at(192), Some((0, 192)));
        // The flush drains the run: nothing left for a second boundary.
        assert_eq!(run.flush_at(192), None);
    }

    #[test]
    fn direct_chunk_run_coalescer_flushes_below_a_composited_chunk() {
        let mut run = DirectChunkRunCoalescer::default();
        run.absorb(0);
        // A cached chunk at z 64..128 composites: the run below it flushes
        // up to the composite's start.
        assert_eq!(run.flush_at(64), Some((0, 64)));
        // Direct chunks resume above the composite and flush independently.
        run.absorb(128);
        assert_eq!(run.flush_at(256), Some((128, 256)));
    }

    #[test]
    fn direct_chunk_run_coalescer_is_quiet_without_direct_chunks() {
        let mut run = DirectChunkRunCoalescer::default();
        // An all-cached walk never absorbs, so no boundary flushes anything.
        assert_eq!(run.flush_at(64), None);
        assert_eq!(run.flush_at(128), None);
        // A boundary that hasn't advanced past the run start must not emit a
        // zero-length render call — and must keep the run for a later flush.
        run.absorb(64);
        assert_eq!(run.flush_at(64), None);
        assert_eq!(run.flush_at(128), Some((64, 128)));
    }

    #[test]
    fn direct_scene_range_cache_policy_keeps_default_entries_small() {
        let small_entry = DEFAULT_DIRECT_SCENE_RANGE_CACHE_BYTES;
        let large_entry = DEFAULT_DIRECT_SCENE_RANGE_CACHE_BYTES + 1;

        assert!(direct_scene_range_cache_enabled_for_policy(
            false,
            false,
            small_entry
        ));
        assert!(!direct_scene_range_cache_enabled_for_policy(
            false,
            false,
            large_entry
        ));
        assert!(direct_scene_range_cache_enabled_for_policy(
            true,
            false,
            large_entry
        ));
        assert!(!direct_scene_range_cache_enabled_for_policy(
            true,
            true,
            small_entry
        ));
        assert!(!direct_scene_range_cache_enabled_for_policy(
            false,
            true,
            small_entry
        ));
    }

    fn default_cache_runtime_shader_layer() -> LayerNode {
        let primitive = PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                    rect: Rect {
                        x: 2.0,
                        y: 3.0,
                        width: 18.0,
                        height: 12.0,
                    },
                    brush: Brush::solid(Color::BLACK),
                    stroke: None,
                },
                clip: None,
            }),
        };
        let mut layer = crate::test_support::layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 32.0,
                height: 24.0,
            },
            ProjectiveTransform::identity(),
            GraphicsLayer {
                render_effect: Some(RenderEffect::runtime_shader(RuntimeShader::new(
                    "fn main() -> vec4<f32> { return vec4<f32>(1.0); }",
                ))),
                ..Default::default()
            },
            vec![RenderNode::Primitive(primitive)],
        );
        layer.node_id = Some(77);
        layer.recompute_raster_cache_hashes();
        layer
    }

    fn image_draw(rect: Rect, opaque: bool) -> ImageDraw {
        let alpha = if opaque { u8::MAX } else { 128 };
        let image =
            ImageBitmap::from_rgba8(2, 2, [255, 255, 255, alpha].repeat(4)).expect("valid image");
        ImageDraw {
            rect,
            local_rect: rect,
            quad: crate::rect_to_quad(rect),
            snap_anchor: None,
            image,
            alpha: 1.0,
            color_filter: None,
            sampling: ImageSampling::Nearest,
            z_index: 0,
            clip: None,
            blend_mode: BlendMode::SrcOver,
            src_rect: None,
            motion_context_animated: false,
        }
    }

    fn prefix_shape(z_index: usize, color: Color) -> DrawShape {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        };
        DrawShape {
            rect,
            local_rect: rect,
            quad: crate::rect_to_quad(rect),
            snap_anchor: None,
            brush: SceneBrush::Solid(color),
            shape: None,
            stroke: None,
            arc: None,
            z_index,
            clip: None,
            blend_mode: BlendMode::SrcOver,
            motion_context_animated: false,
        }
    }

    fn scene_with_prefix_shape(color: Color) -> CompositorScene {
        let mut scene = CompositorScene::new();
        scene.shapes.push(prefix_shape(0, color));
        scene.draw_ops.push(DrawOp {
            z_index: 0,
            kind: DrawOpKind::Shape(0),
        });
        scene.next_z = 3;
        scene
    }

    fn scene_with_cacheable_prefix_shapes(color: Color) -> CompositorScene {
        let mut scene = CompositorScene::new();
        for z_index in 0..MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS {
            let mut shape = prefix_shape(z_index, color);
            shape.rect.x = z_index as f32 * 4.0;
            shape.local_rect = shape.rect;
            shape.quad = crate::rect_to_quad(shape.rect);
            scene.shapes.push(shape);
            scene.draw_ops.push(DrawOp {
                z_index,
                kind: DrawOpKind::Shape(z_index),
            });
        }
        scene.next_z = MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS;
        scene
    }

    fn child_prefix_contribution(z_index: usize, x: f32) -> BackdropPrefixChildContribution {
        let rect = Rect {
            x,
            y: 0.0,
            width: 20.0,
            height: 20.0,
        };
        BackdropPrefixChildContribution {
            z_index,
            node_id: Some(900 + z_index),
            content_hash: 17,
            effect_hash: 19,
            backdrop_hash: 0,
            deferred_effect_hash: 0,
            logical_rect: rect,
            dest_quad: crate::rect_to_quad(rect),
            scissor: None,
            composite_alpha_bits: 1.0f32.to_bits(),
            blend_mode: BlendMode::SrcOver,
            sample_mode: CompositeSampleMode::Linear,
        }
    }

    fn test_backdrop_layer(rect: Rect) -> BackdropLayer {
        BackdropLayer {
            node_id: Some(77),
            rect,
            clip: None,
            snap_anchor: None,
            effect: RenderEffect::blur(4.0),
            z_index: 1,
        }
    }

    fn backdrop_child_layer(node_id: NodeId) -> LayerNode {
        let mut layer = crate::test_support::layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: 40.0,
            },
            ProjectiveTransform::identity(),
            GraphicsLayer {
                backdrop_effect: Some(RenderEffect::blur(4.0)),
                ..Default::default()
            },
            vec![],
        );
        layer.node_id = Some(node_id);
        layer.recompute_raster_cache_hashes();
        layer
    }

    /// Snapshots a `LayerNode` into the owned child struct the way the
    /// collection site does, with an empty source; tests that used to hand
    /// the borrowed node to the render path build their input through this.
    fn lower_test_layer(layer: &LayerNode) -> crate::normalized_scene::ChildLayerComposite {
        use crate::surface_plan::{
            layer_contains_descendant_backdrop, layer_surface_scale,
            translated_content_axes_for_layer,
        };
        use cranpose_render_common::layer_composition::effective_layer_isolation;

        let mut requirements_cache = HashMap::new();
        let surface_requirements =
            layer_surface_requirements_cached(layer, &mut requirements_cache);
        let contains_descendant_backdrop = layer_contains_descendant_backdrop(layer);
        crate::normalized_scene::ChildLayerComposite {
            z_index: 0,
            logical_rect: layer.local_bounds,
            dest_quad: crate::rect_to_quad(layer.local_bounds),
            snap_anchor: None,
            composite_snap_origin: None,
            backdrop_rect: layer.local_bounds,
            visual_clip: None,
            surface_clip: None,
            shadow_draws: Vec::new(),
            needs_nested_underlay: false,
            node_id: layer.node_id,
            backdrop: layer.backdrop().cloned(),
            has_effect: layer.effect().is_some(),
            effect_contains_runtime_shader: layer
                .effect()
                .is_some_and(|effect| effect.contains_runtime_shader()),
            target_content_hash: layer.target_content_hash(),
            effect_hash: layer.effect_hash(),
            motion_source_content_hash: Some(layer.motion_source_content_hash()),
            contains_descendant_backdrop,
            cache_policy: layer.cache_policy,
            surface_requirements,
            rounded_clip: crate::surface_executor::backend::LayerSurfaceRoundedClip::from_layer(
                layer,
            ),
            isolation: effective_layer_isolation(&layer.graphics_layer),
            translated_content_context: layer.translated_content_context,
            own_translated_content_axes: translated_content_axes_for_layer(layer),
            clip_rect: layer.clip_rect(),
            local_bounds: layer.local_bounds,
            surface_scale: layer_surface_scale(layer),
            source: crate::normalized_scene::LoweredChildSource::default(),
        }
    }

    fn child_layer_composite(
        layer: &LayerNode,
        z_index: usize,
        rect: Rect,
    ) -> crate::normalized_scene::ChildLayerComposite {
        crate::normalized_scene::ChildLayerComposite {
            z_index,
            logical_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: rect.width,
                height: rect.height,
            },
            dest_quad: crate::rect_to_quad(rect),
            snap_anchor: None,
            composite_snap_origin: None,
            backdrop_rect: rect,
            visual_clip: None,
            surface_clip: None,
            shadow_draws: Vec::new(),
            needs_nested_underlay: false,
            ..lower_test_layer(layer)
        }
    }

    #[test]
    fn the_underlay_fill_covers_only_what_the_glass_reads_back() {
        let row = Rect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 140.0,
        };
        let glass = Rect {
            x: 300.0,
            y: 48.0,
            width: 44.0,
            height: 44.0,
        };
        let layer = crate::test_support::layer_node(
            glass,
            ProjectiveTransform::identity(),
            cranpose_ui_graphics::GraphicsLayer::default(),
            Vec::new(),
        );
        let mut child = child_layer_composite(&layer, 4, glass);
        child.backdrop = Some(RenderEffect::blur(6.0));
        let source = crate::normalized_scene::LoweredChildSource {
            scene: CompositorScene::new(),
            children: vec![child],
        };

        let sample_rect = underlay_sample_rect(&source).expect("the glass reads back a rect");
        let full = surface_target_size(row, 3.0, 4096);
        let scissor = underlay_fill_scissor(Some(sample_rect), row, 3.0, full.0, full.1)
            .expect("the fill must be scissored to that rect");

        let filled = (scissor.2 as u64) * (scissor.3 as u64);
        let whole = (full.0 as u64) * (full.1 as u64);
        assert_eq!(whole, 504_000, "a 1200x420 row underlay");
        assert_eq!(
            filled, 29_584,
            "only the glass, the reach of its blur and a snap margin"
        );
        assert!(
            sample_rect.x <= glass.x && sample_rect.y <= glass.y,
            "the blur reach must widen the rect: sample={sample_rect:?} glass={glass:?}"
        );
    }

    #[test]
    fn a_deeper_backdrop_keeps_the_whole_child_quad_filled() {
        let row = Rect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 140.0,
        };
        let panel = Rect {
            x: 20.0,
            y: 10.0,
            width: 360.0,
            height: 120.0,
        };
        let layer = crate::test_support::layer_node(
            panel,
            ProjectiveTransform::identity(),
            cranpose_ui_graphics::GraphicsLayer::default(),
            Vec::new(),
        );
        let mut child = child_layer_composite(&layer, 4, panel);
        child.backdrop = None;
        child.contains_descendant_backdrop = true;
        let source = crate::normalized_scene::LoweredChildSource {
            scene: CompositorScene::new(),
            children: vec![child],
        };

        let sample_rect = underlay_sample_rect(&source).expect("the nested underlay reads it back");

        assert_eq!(
            sample_rect, panel,
            "a child that builds its own underlay out of this one reads its whole quad"
        );
        let full = surface_target_size(row, 3.0, 4096);
        assert!(
            underlay_fill_scissor(Some(sample_rect), row, 3.0, full.0, full.1).is_some(),
            "the fill still runs, just bounded"
        );
    }

    #[test]
    fn fractional_origin_backdrop_snapshot_spans_more_than_ceiled_extent() {
        // A capture rect whose scaled origin is fractional spans one texel
        // more than ceil(extent): floor(origin)..ceil(origin + extent). The
        // snapshot surface MUST be allocated from the copy plan's size — an
        // independently ceiled extent rejects the 1:1 copy every frame and
        // the projective fallback feeds the glass a broken backdrop (the
        // menu-expand panel composited washed-out gray, tint-blind).
        let capture = Rect {
            x: 53.0,
            y: 95.435,
            width: 410.0,
            height: 76.0,
        };
        let plan =
            axis_aligned_backdrop_snapshot_copy_plan(capture, capture, 2.0, (1800, 1600), 4096)
                .expect("axis-aligned capture must plan a 1:1 copy");
        let ceiled = surface_target_size(capture, 2.0, 4096);
        assert_eq!(plan.size.1, ceiled.1 + 1, "fractional origin adds a texel");
        assert_eq!(plan.size.0, ceiled.0, "integral axis stays equal");
    }

    #[test]
    fn retained_render_effect_hash_includes_runtime_shader_uniforms() {
        let source = "@group(0) @binding(0) var input_texture: texture_2d<f32>;";
        let mut first = RuntimeShader::new(source);
        first.set_float(0, 1.0);
        let mut second = RuntimeShader::new(source);
        second.set_float(0, 2.0);

        assert_ne!(
            retained_render_effect_hash(&RenderEffect::runtime_shader(first)),
            retained_render_effect_hash(&RenderEffect::runtime_shader(second)),
            "retained backdrop cache keys must distinguish runtime shader uniforms"
        );
    }

    #[test]
    fn render_string_scene_hash_is_content_based_and_retained() {
        let first = Arc::new(AnnotatedString::new("cached text".to_string()).render_string());
        let same_content =
            Arc::new(AnnotatedString::new("cached text".to_string()).render_string());
        let different =
            Arc::new(AnnotatedString::new("different text".to_string()).render_string());

        let first_hash = render_string_scene_hash(&first);

        assert_eq!(
            first_hash,
            render_string_scene_hash(&first),
            "retained render strings should reuse their cached render hash"
        );
        assert_eq!(
            first_hash,
            render_string_scene_hash(&same_content),
            "distinct allocations with equal text content should produce the same render hash"
        );
        assert_ne!(
            first_hash,
            render_string_scene_hash(&different),
            "text content changes must still invalidate direct scene range cache keys"
        );
    }

    #[test]
    fn backdrop_cache_key_uses_prior_scene_prefix_not_later_child_motion() {
        let scene = scene_with_prefix_shape(Color::BLACK);
        let layer = test_backdrop_layer(Rect {
            x: 10.0,
            y: 10.0,
            width: 60.0,
            height: 40.0,
        });
        let later_child_left = [child_prefix_contribution(2, 20.0)];
        let later_child_right = [child_prefix_contribution(2, 80.0)];

        let left_prefix =
            backdrop_scene_prefix_hash(&scene, &later_child_left, 1, layer.rect, (200, 120), 1.0);
        let right_prefix =
            backdrop_scene_prefix_hash(&scene, &later_child_right, 1, layer.rect, (200, 120), 1.0);
        let left_key = backdrop_effect_cache_key(&layer, left_prefix, layer.rect, (60, 40), 1.0);
        let right_key = backdrop_effect_cache_key(&layer, right_prefix, layer.rect, (60, 40), 1.0);

        assert_eq!(
            left_key, right_key,
            "a later moving child must not invalidate an earlier backdrop input"
        );
    }

    #[test]
    fn a_backdrop_in_a_scene_carries_an_input_hash_to_the_blur_cache() {
        let layer = test_backdrop_layer(Rect {
            x: 10.0,
            y: 10.0,
            width: 60.0,
            height: 40.0,
        });
        let mut black_scene = scene_with_prefix_shape(Color::BLACK);
        black_scene.backdrop_layers.push(layer.clone());
        let mut red_scene = scene_with_prefix_shape(Color::RED);
        red_scene.backdrop_layers.push(layer.clone());
        let mut black_again = scene_with_prefix_shape(Color::BLACK);
        black_again.backdrop_layers.push(layer.clone());

        let black = scene_backdrop_input_hashes(&black_scene, &[], (200, 120), 1.0);
        let red = scene_backdrop_input_hashes(&red_scene, &[], (200, 120), 1.0);
        let repeat = scene_backdrop_input_hashes(&black_again, &[], (200, 120), 1.0);

        assert_eq!(black.len(), 1, "one hash per backdrop the scene carries");
        assert_eq!(
            black, repeat,
            "an unchanged scene under the glass must reach the same cache entry"
        );
        assert_ne!(
            black, red,
            "a changed draw under the glass must miss the cache"
        );
        assert!(
            scene_backdrop_input_hashes(
                &scene_with_prefix_shape(Color::BLACK),
                &[],
                (200, 120),
                1.0
            )
            .is_empty(),
            "a scene with no backdrop asks for no hashes"
        );
    }

    #[test]
    fn backdrop_cache_key_changes_when_prior_content_changes() {
        let layer = test_backdrop_layer(Rect {
            x: 10.0,
            y: 10.0,
            width: 60.0,
            height: 40.0,
        });
        let black_scene = scene_with_prefix_shape(Color::BLACK);
        let red_scene = scene_with_prefix_shape(Color::RED);

        let black_prefix =
            backdrop_scene_prefix_hash(&black_scene, &[], 1, layer.rect, (200, 120), 1.0);
        let red_prefix =
            backdrop_scene_prefix_hash(&red_scene, &[], 1, layer.rect, (200, 120), 1.0);

        assert_ne!(
            backdrop_effect_cache_key(&layer, black_prefix, layer.rect, (60, 40), 1.0),
            backdrop_effect_cache_key(&layer, red_prefix, layer.rect, (60, 40), 1.0),
            "prior scene pixels must invalidate cached backdrop output"
        );
    }

    #[test]
    fn backdrop_cache_key_ignores_prior_content_outside_capture_rect() {
        let layer = test_backdrop_layer(Rect {
            x: 10.0,
            y: 10.0,
            width: 60.0,
            height: 40.0,
        });
        let mut black_scene = scene_with_prefix_shape(Color::BLACK);
        let mut red_scene = scene_with_prefix_shape(Color::RED);
        for scene in [&mut black_scene, &mut red_scene] {
            let rect = Rect {
                x: 120.0,
                y: 80.0,
                width: 40.0,
                height: 30.0,
            };
            scene.shapes[0].rect = rect;
            scene.shapes[0].local_rect = rect;
            scene.shapes[0].quad = crate::rect_to_quad(rect);
        }

        let black_prefix =
            backdrop_scene_prefix_hash(&black_scene, &[], 1, layer.rect, (200, 120), 1.0);
        let red_prefix =
            backdrop_scene_prefix_hash(&red_scene, &[], 1, layer.rect, (200, 120), 1.0);

        assert_eq!(
            backdrop_effect_cache_key(&layer, black_prefix, layer.rect, (60, 40), 1.0),
            backdrop_effect_cache_key(&layer, red_prefix, layer.rect, (60, 40), 1.0),
            "content outside the sampled backdrop capture must not invalidate cached glass"
        );
    }

    #[test]
    fn backdrop_cache_key_changes_when_prior_child_moves() {
        let scene = scene_with_prefix_shape(Color::BLACK);
        let layer = test_backdrop_layer(Rect {
            x: 10.0,
            y: 10.0,
            width: 60.0,
            height: 40.0,
        });
        let prior_child_left = [child_prefix_contribution(0, 20.0)];
        let prior_child_right = [child_prefix_contribution(0, 80.0)];

        let left_prefix =
            backdrop_scene_prefix_hash(&scene, &prior_child_left, 1, layer.rect, (200, 120), 1.0);
        let right_prefix =
            backdrop_scene_prefix_hash(&scene, &prior_child_right, 1, layer.rect, (200, 120), 1.0);

        assert_ne!(
            backdrop_effect_cache_key(&layer, left_prefix, layer.rect, (60, 40), 1.0),
            backdrop_effect_cache_key(&layer, right_prefix, layer.rect, (60, 40), 1.0),
            "prior child movement changes the pixels sampled by a later backdrop"
        );
    }

    #[test]
    fn backdrop_cache_key_ignores_prior_child_motion_outside_capture_rect() {
        let scene = scene_with_prefix_shape(Color::BLACK);
        let layer = test_backdrop_layer(Rect {
            x: 10.0,
            y: 10.0,
            width: 60.0,
            height: 40.0,
        });
        let prior_child_left = [child_prefix_contribution(0, 100.0)];
        let prior_child_right = [child_prefix_contribution(0, 140.0)];

        let left_prefix =
            backdrop_scene_prefix_hash(&scene, &prior_child_left, 1, layer.rect, (200, 120), 1.0);
        let right_prefix =
            backdrop_scene_prefix_hash(&scene, &prior_child_right, 1, layer.rect, (200, 120), 1.0);

        assert_eq!(
            backdrop_effect_cache_key(&layer, left_prefix, layer.rect, (60, 40), 1.0),
            backdrop_effect_cache_key(&layer, right_prefix, layer.rect, (60, 40), 1.0),
            "isolated content outside the sampled backdrop capture must not invalidate cached glass"
        );
    }

    #[test]
    fn opaque_image_cover_elides_backdrop_underlay() {
        let cover = image_draw(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 120.0,
            },
            true,
        );
        let backdrop = test_backdrop_layer(Rect {
            x: 20.0,
            y: 20.0,
            width: 80.0,
            height: 40.0,
        });
        let draw_ops = [DrawOp {
            z_index: 0,
            kind: DrawOpKind::Image(0),
        }];

        assert!(backdrop_underlay_is_covered_by_local_content(
            &[],
            &[],
            &[cover],
            &[],
            &draw_ops,
            &[],
            &[],
            &backdrop,
        ));
    }

    #[test]
    fn transparent_image_cover_keeps_backdrop_underlay() {
        let cover = image_draw(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 120.0,
            },
            false,
        );
        let backdrop = test_backdrop_layer(Rect {
            x: 20.0,
            y: 20.0,
            width: 80.0,
            height: 40.0,
        });
        let draw_ops = [DrawOp {
            z_index: 0,
            kind: DrawOpKind::Image(0),
        }];

        assert!(!backdrop_underlay_is_covered_by_local_content(
            &[],
            &[],
            &[cover],
            &[],
            &draw_ops,
            &[],
            &[],
            &backdrop,
        ));
    }

    #[test]
    fn direct_scene_range_cache_key_accepts_src_over_draw_ops() {
        let scene = scene_with_cacheable_prefix_shapes(Color::BLACK);
        let key = direct_scene_range_cache_key(
            &scene,
            0,
            MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 120.0,
            },
            (200, 120),
            1.0,
        );

        assert!(
            key.is_some(),
            "ordinary SrcOver root ranges should be retained"
        );
    }

    #[test]
    fn a_chunk_too_large_to_cache_is_not_worth_splitting_for() {
        // Every shape covers the full 1280x2856 surface, the way a game frame's
        // moving objects do once their bounds are unioned. Each 64-op chunk is
        // then far past the entry budget, so the cache would refuse it and the
        // split would buy a render pass for nothing.
        let mut scene = CompositorScene::new();
        for z_index in 0..(MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS * 2) {
            let mut shape = prefix_shape(z_index, Color::BLACK);
            shape.rect = Rect {
                x: 0.0,
                y: 0.0,
                width: 1280.0,
                height: 2856.0,
            };
            shape.local_rect = shape.rect;
            shape.quad = crate::rect_to_quad(shape.rect);
            let shape_index = scene.shapes.len();
            scene.shapes.push(shape);
            scene.draw_ops.push(DrawOp {
                z_index,
                kind: DrawOpKind::Shape(shape_index),
            });
        }
        scene.next_z = MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS * 2;

        assert!(
            !direct_scene_range_chunk_fits_cache_entry(
                8192,
                direct_scene_range_snapped_bounds(
                    &scene,
                    0,
                    MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
                    1.0,
                ),
                1.0,
            ),
            "a full-surface chunk is far past the per-entry byte budget"
        );
    }

    #[test]
    fn a_chunk_small_enough_to_cache_still_reports_that_it_fits() {
        let scene = scene_with_cacheable_prefix_shapes(Color::BLACK);
        assert!(
            direct_scene_range_chunk_fits_cache_entry(
                8192,
                direct_scene_range_snapped_bounds(
                    &scene,
                    0,
                    MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
                    1.0,
                ),
                1.0,
            ),
            "a small range is exactly what the range cache is for"
        );
    }

    #[test]
    fn direct_scene_range_cache_chunks_bound_large_ranges() {
        let mut scene = CompositorScene::new();
        for z_index in 0..(MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS * 2 + 3) {
            let mut shape = prefix_shape(z_index, Color::BLACK);
            shape.rect.x = z_index as f32 * 20.0;
            shape.local_rect = shape.rect;
            shape.quad = crate::rect_to_quad(shape.rect);
            let shape_index = scene.shapes.len();
            scene.shapes.push(shape);
            scene.draw_ops.push(DrawOp {
                z_index,
                kind: DrawOpKind::Shape(shape_index),
            });
        }
        scene.next_z = MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS * 2 + 3;

        assert_eq!(
            direct_scene_range_cache_chunk_end(&scene, 0, scene.next_z, 1.0),
            MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS
        );
        assert_eq!(
            direct_scene_range_cache_chunk_end(
                &scene,
                MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
                scene.next_z,
                1.0
            ),
            MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS * 2
        );
        assert_eq!(
            direct_scene_range_cache_chunk_end(
                &scene,
                MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS * 2,
                scene.next_z,
                1.0
            ),
            scene.next_z
        );
    }

    #[test]
    fn direct_scene_range_cache_chunks_isolate_large_motion_sensitive_draw_ops() {
        let mut scene = CompositorScene::new();
        let root_scale = 1.0;
        let large_side =
            ((MAX_MOTION_SENSITIVE_DIRECT_SCENE_CACHE_DRAW_BYTES / 4) as f32).sqrt() + 16.0;
        let motion_z = MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS;
        let suffix_start = motion_z + 1;
        let total = suffix_start + MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS;

        for z_index in 0..total {
            let mut shape = prefix_shape(z_index, Color::BLACK);
            if z_index == motion_z {
                shape.rect = Rect {
                    x: 0.0,
                    y: 0.0,
                    width: large_side,
                    height: large_side,
                };
                shape.motion_context_animated = true;
            } else {
                shape.rect.x = z_index as f32 * 12.0;
                shape.rect.y = large_side + z_index as f32 * 4.0;
            }
            shape.local_rect = shape.rect;
            shape.quad = crate::rect_to_quad(shape.rect);
            let shape_index = scene.shapes.len();
            scene.shapes.push(shape);
            scene.draw_ops.push(DrawOp {
                z_index,
                kind: DrawOpKind::Shape(shape_index),
            });
        }
        scene.next_z = total;

        assert_eq!(
            direct_scene_range_cache_chunk_end(&scene, 0, scene.next_z, root_scale),
            motion_z,
            "the static prefix should be cacheable before the large moving primitive"
        );
        assert_eq!(
            direct_scene_range_cache_chunk_end(&scene, motion_z, scene.next_z, root_scale),
            suffix_start,
            "the large moving primitive should render live by itself"
        );
        assert_eq!(
            direct_scene_range_cache_chunk_end(&scene, suffix_start, scene.next_z, root_scale),
            scene.next_z,
            "the static suffix should be cacheable after the large moving primitive"
        );

        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: large_side,
            height: large_side + 120.0,
        };
        assert!(direct_scene_range_cache_key(
            &scene,
            0,
            motion_z,
            viewport,
            (large_side as u32, large_side as u32),
            root_scale,
        )
        .is_some());
        assert!(direct_scene_range_cache_key(
            &scene,
            motion_z,
            suffix_start,
            viewport,
            (large_side as u32, large_side as u32),
            root_scale,
        )
        .is_some());
        assert!(direct_scene_range_cache_key(
            &scene,
            suffix_start,
            scene.next_z,
            viewport,
            (large_side as u32, large_side as u32),
            root_scale,
        )
        .is_some());
    }

    #[test]
    fn direct_scene_range_cache_key_accepts_stable_large_motion_sensitive_draw_op() {
        let mut first = CompositorScene::new();
        let mut second = CompositorScene::new();
        let root_scale = 1.0;
        let large_width = 960.0;
        for z_index in 0..5 {
            let mut first_shape = prefix_shape(z_index, Color::BLACK);
            if z_index == 0 {
                first_shape.rect = Rect {
                    x: 0.0,
                    y: 0.0,
                    width: large_width,
                    height: large_width,
                };
                first_shape.motion_context_animated = true;
            } else {
                first_shape.rect.x = z_index as f32 * 12.0;
                first_shape.rect.y = large_width + z_index as f32 * 4.0;
            }
            first_shape.local_rect = first_shape.rect;
            first_shape.quad = crate::rect_to_quad(first_shape.rect);

            let mut second_shape = first_shape;
            if z_index == 0 {
                second_shape.brush = SceneBrush::Solid(Color::RED);
            }

            let first_index = first.shapes.len();
            first.shapes.push(first_shape);
            first.draw_ops.push(DrawOp {
                z_index,
                kind: DrawOpKind::Shape(first_index),
            });

            let second_index = second.shapes.len();
            second.shapes.push(second_shape);
            second.draw_ops.push(DrawOp {
                z_index,
                kind: DrawOpKind::Shape(second_index),
            });
        }
        first.next_z = 5;
        second.next_z = 5;

        assert_eq!(
            direct_scene_range_cache_chunk_end(&first, 0, first.next_z, root_scale),
            1,
            "large motion-sensitive draws should be isolated so they do not invalidate stable siblings"
        );

        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: large_width,
            height: large_width + 120.0,
        };
        let first_motion_range_key = direct_scene_range_cache_key(
            &first,
            0,
            first.next_z,
            viewport,
            (large_width as u32, large_width as u32),
            root_scale,
        );
        let second_motion_range_key = direct_scene_range_cache_key(
            &second,
            0,
            second.next_z,
            viewport,
            (large_width as u32, large_width as u32),
            root_scale,
        );

        assert!(
            first_motion_range_key.is_some() && second_motion_range_key.is_some(),
            "large motion-sensitive ranges still get stable cache keys; moving geometry naturally changes those keys before admission"
        );
        assert_ne!(
            first_motion_range_key, second_motion_range_key,
            "content changes in a large motion-sensitive range must invalidate the retained range key"
        );
    }

    #[test]
    fn direct_scene_range_chunks_isolate_one_changing_draw_op() {
        let mut first = CompositorScene::new();
        let mut second = CompositorScene::new();
        let total = MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS * 3;
        let changed_z = MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS + 1;
        for z_index in 0..total {
            let mut first_shape = prefix_shape(z_index, Color::BLACK);
            first_shape.rect.x = z_index as f32 * 20.0;
            first_shape.local_rect = first_shape.rect;
            first_shape.quad = crate::rect_to_quad(first_shape.rect);
            let mut second_shape = first_shape;
            if z_index == changed_z {
                second_shape.brush = SceneBrush::Solid(Color::RED);
            }

            let first_index = first.shapes.len();
            first.shapes.push(first_shape);
            first.draw_ops.push(DrawOp {
                z_index,
                kind: DrawOpKind::Shape(first_index),
            });

            let second_index = second.shapes.len();
            second.shapes.push(second_shape);
            second.draw_ops.push(DrawOp {
                z_index,
                kind: DrawOpKind::Shape(second_index),
            });
        }
        first.next_z = total;
        second.next_z = total;

        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 120.0,
        };
        let first_prefix = direct_scene_range_cache_key(
            &first,
            0,
            MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
            viewport,
            (800, 120),
            1.0,
        );
        let second_prefix = direct_scene_range_cache_key(
            &second,
            0,
            MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
            viewport,
            (800, 120),
            1.0,
        );
        let first_middle = direct_scene_range_cache_key(
            &first,
            MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
            MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS * 2,
            viewport,
            (800, 120),
            1.0,
        );
        let second_middle = direct_scene_range_cache_key(
            &second,
            MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
            MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS * 2,
            viewport,
            (800, 120),
            1.0,
        );
        let first_suffix = direct_scene_range_cache_key(
            &first,
            MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS * 2,
            total,
            viewport,
            (800, 120),
            1.0,
        );
        let second_suffix = direct_scene_range_cache_key(
            &second,
            MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS * 2,
            total,
            viewport,
            (800, 120),
            1.0,
        );

        assert_eq!(
            first_prefix, second_prefix,
            "a changed middle draw op must not invalidate the preceding retained chunk"
        );
        assert_ne!(
            first_middle, second_middle,
            "the chunk containing changed draw content must invalidate"
        );
        assert_eq!(
            first_suffix, second_suffix,
            "a changed middle draw op must not invalidate the following retained chunk"
        );
    }

    #[test]
    fn direct_scene_range_cache_key_accepts_full_viewport_ranges_within_scene_budget() {
        let scene = scene_with_cacheable_prefix_shapes(Color::BLACK);
        let key = direct_scene_range_cache_key(
            &scene,
            0,
            MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
            Rect {
                x: 20.0,
                y: 28.0,
                width: 1160.0,
                height: 952.0,
            },
            (1160, 952),
            1.0,
        );

        assert!(
            key.is_some(),
            "static full-viewport direct ranges must fit the retained scene-range cache"
        );
    }

    #[test]
    fn direct_scene_range_cache_key_changes_when_content_changes() {
        let black_scene = scene_with_cacheable_prefix_shapes(Color::BLACK);
        let red_scene = scene_with_cacheable_prefix_shapes(Color::RED);
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 120.0,
        };

        let black_key = direct_scene_range_cache_key(
            &black_scene,
            0,
            MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
            viewport,
            (200, 120),
            1.0,
        );
        let red_key = direct_scene_range_cache_key(
            &red_scene,
            0,
            MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
            viewport,
            (200, 120),
            1.0,
        );

        assert_ne!(black_key, red_key);
    }

    #[test]
    fn direct_scene_range_cache_key_rejects_tiny_ranges() {
        let scene = scene_with_prefix_shape(Color::BLACK);
        let key = direct_scene_range_cache_key(
            &scene,
            0,
            1,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 120.0,
            },
            (200, 120),
            1.0,
        );

        assert_eq!(key, None);
    }

    #[test]
    fn direct_scene_range_cache_key_accepts_large_static_single_draw_range() {
        let mut scene = scene_with_prefix_shape(Color::BLACK);
        let rect = Rect {
            x: 20.0,
            y: 95.0,
            width: 872.0,
            height: 885.0,
        };
        scene.shapes[0].rect = rect;
        scene.shapes[0].local_rect = rect;
        scene.shapes[0].quad = crate::rect_to_quad(rect);

        let key = direct_scene_range_cache_key(&scene, 0, 1, rect, (872, 885), 1.0);

        assert!(
            key.is_some(),
            "large stable one-op surfaces should not be redrawn every frame"
        );
    }

    #[test]
    fn direct_scene_range_cache_key_accepts_large_motion_sensitive_single_draw_range() {
        let mut scene = scene_with_prefix_shape(Color::BLACK);
        let rect = Rect {
            x: 20.0,
            y: 95.0,
            width: 872.0,
            height: 885.0,
        };
        scene.shapes[0].rect = rect;
        scene.shapes[0].local_rect = rect;
        scene.shapes[0].quad = crate::rect_to_quad(rect);
        scene.shapes[0].motion_context_animated = true;

        let key = direct_scene_range_cache_key(&scene, 0, 1, rect, (872, 885), 1.0);

        assert!(
            key.is_some(),
            "large one-op motion-marked surfaces get a content-addressed key; changing geometry naturally changes that key before cache admission"
        );
    }

    #[test]
    fn direct_scene_range_cache_key_rejects_empty_ranges() {
        let scene = CompositorScene::new();
        let key = direct_scene_range_cache_key(
            &scene,
            0,
            1,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 120.0,
            },
            (200, 120),
            1.0,
        );

        assert_eq!(key, None);
    }

    #[test]
    fn direct_scene_range_cache_key_rejects_non_src_over_ranges() {
        let mut scene = scene_with_cacheable_prefix_shapes(Color::BLACK);
        scene.shapes[0].blend_mode = BlendMode::Multiply;
        let key = direct_scene_range_cache_key(
            &scene,
            0,
            MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 120.0,
            },
            (200, 120),
            1.0,
        );

        assert_eq!(key, None);
    }

    #[test]
    fn opaque_local_cover_allows_layer_source_cache_with_backdrop_underlay() {
        let cover = image_draw(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 120.0,
            },
            true,
        );
        let mut scene = CompositorScene::new();
        scene.images.push(cover);
        scene.draw_ops.push(DrawOp {
            z_index: 0,
            kind: DrawOpKind::Image(0),
        });
        let child = backdrop_child_layer(91);
        let children = [child_layer_composite(
            &child,
            1,
            Rect {
                x: 20.0,
                y: 20.0,
                width: 80.0,
                height: 40.0,
            },
        )];

        assert!(
            !layer_source_uses_external_backdrop_underlay(&scene, &children, true),
            "covered direct child backdrops do not sample the external underlay"
        );
    }

    #[test]
    fn transparent_local_cover_keeps_layer_source_external_backdrop_dependency() {
        let cover = image_draw(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 120.0,
            },
            false,
        );
        let mut scene = CompositorScene::new();
        scene.images.push(cover);
        scene.draw_ops.push(DrawOp {
            z_index: 0,
            kind: DrawOpKind::Image(0),
        });
        let child = backdrop_child_layer(92);
        let children = [child_layer_composite(
            &child,
            1,
            Rect {
                x: 20.0,
                y: 20.0,
                width: 80.0,
                height: 40.0,
            },
        )];

        assert!(
            layer_source_uses_external_backdrop_underlay(&scene, &children, true),
            "transparent local content must preserve the external underlay dependency"
        );
    }

    #[test]
    fn nested_descendant_backdrop_keeps_layer_source_external_backdrop_dependency() {
        let cover = image_draw(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 120.0,
            },
            true,
        );
        let mut scene = CompositorScene::new();
        scene.images.push(cover);
        scene.draw_ops.push(DrawOp {
            z_index: 0,
            kind: DrawOpKind::Image(0),
        });
        let grandchild = backdrop_child_layer(93);
        let mut child = crate::test_support::layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
            },
            ProjectiveTransform::identity(),
            GraphicsLayer::default(),
            vec![RenderNode::Layer(Box::new(grandchild))],
        );
        child.node_id = Some(94);
        child.recompute_raster_cache_hashes();
        let children = [child_layer_composite(
            &child,
            1,
            Rect {
                x: 20.0,
                y: 20.0,
                width: 100.0,
                height: 80.0,
            },
        )];

        assert!(
            layer_source_uses_external_backdrop_underlay(&scene, &children, true),
            "nested descendant backdrops stay conservative until their input is hashed precisely"
        );
    }

    #[test]
    fn dest_quad_intersection_detects_backdrop_overlap() {
        let quad = crate::rect_to_quad(Rect {
            x: 10.0,
            y: 10.0,
            width: 40.0,
            height: 30.0,
        });

        assert!(dest_quad_intersects_rect(
            quad,
            Rect {
                x: 30.0,
                y: 20.0,
                width: 40.0,
                height: 30.0,
            },
        ));
        assert!(!dest_quad_intersects_rect(
            quad,
            Rect {
                x: 80.0,
                y: 20.0,
                width: 40.0,
                height: 30.0,
            },
        ));
    }

    #[test]
    fn anchored_box4_composite_snaps_final_dest_quad() {
        let quad = [
            [0.0, -1953.0],
            [1119.0, -1953.0],
            [0.0, 808.0],
            [1119.0, 808.0],
        ];

        assert_eq!(
            anchored_composite_dest_quad(
                quad,
                Some(SnapAnchor::rigid(Point::new(0.0, 0.0))),
                None,
                1.25,
                CompositeSampleMode::Box4,
            ),
            [
                [0.0, -2441.0],
                [1398.75, -2441.0],
                [0.0, 1010.25],
                [1398.75, 1010.25],
            ]
        );
    }

    #[test]
    fn anchored_projective_composite_is_stable_across_device_pixel_steps() {
        let mut quad = [
            [185.134_52, 490.533_26],
            [236.936_63, 495.065_03],
            [179.556_53, 554.289_8],
            [231.358_64, 558.821_6],
        ];
        let mut anchor = SnapAnchor::rigid(Point::new(48.0, 127.600_006));
        let initial =
            anchored_composite_dest_quad(quad, Some(anchor), None, 1.25, CompositeSampleMode::Box4);

        for step in 1..=10 {
            for point in &mut quad {
                point[1] -= 0.8;
            }
            anchor.origin.y -= 0.8;
            let translated = anchored_composite_dest_quad(
                quad,
                Some(anchor),
                None,
                1.25,
                CompositeSampleMode::Box4,
            );

            for (initial_point, translated_point) in initial.iter().zip(translated) {
                assert_eq!(translated_point[0], initial_point[0]);
                assert_eq!(
                    translated_point[1] + step as f32,
                    initial_point[1],
                    "projective composite geometry drifted after {step} physical pixels"
                );
            }
        }
    }

    #[test]
    fn unanchored_box4_composite_snaps_final_dest_quad() {
        let quad = [[0.25, 10.5], [40.75, 10.5], [0.25, 20.25], [40.75, 20.25]];

        assert_eq!(
            anchored_composite_dest_quad(quad, None, None, 1.0, CompositeSampleMode::Box4),
            [[0.0, 11.0], [40.5, 11.0], [0.0, 20.75], [40.5, 20.75]],
            "unanchored pixel-stable surfaces should keep their composite phase snapped"
        );
    }

    #[test]
    fn scaled_linear_composite_snaps_around_its_transform_origin() {
        let quad = [[54.9, 416.5], [89.1, 416.5], [54.9, 450.7], [89.1, 450.7]];
        let snapped = anchored_composite_dest_quad(
            quad,
            None,
            Some(Point::new(72.0, 433.6)),
            1.0,
            CompositeSampleMode::Linear,
        );
        let center = [
            (snapped[0][0] + snapped[3][0]) * 0.5,
            (snapped[0][1] + snapped[3][1]) * 0.5,
        ];

        assert_eq!(center, [72.0, 434.0]);
        assert!((snapped[1][0] - snapped[0][0] - 34.2).abs() < 1e-4);
        assert!((snapped[2][1] - snapped[0][1] - 34.2).abs() < 1e-4);
    }

    #[test]
    fn parent_surface_translation_keeps_child_composite_coordinates_together() {
        let layer = crate::test_support::layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 36.0,
                height: 36.0,
            },
            ProjectiveTransform::identity(),
            GraphicsLayer::default(),
            vec![],
        );
        let mut child = child_layer_composite(
            &layer,
            0,
            Rect {
                x: 54.0,
                y: 416.0,
                width: 36.0,
                height: 36.0,
            },
        );
        child.snap_anchor = Some(SnapAnchor::rigid(Point::new(54.0, 416.0)));
        child.composite_snap_origin = Some(Point::new(72.0, 434.0));
        child.translate_by(Point::new(-12.0, 8.0));

        assert_eq!(child.dest_quad[0], [42.0, 424.0]);
        assert_eq!(
            child.snap_anchor.expect("snap anchor").origin,
            Point::new(42.0, 424.0)
        );
        assert_eq!(child.composite_snap_origin, Some(Point::new(60.0, 442.0)));
        assert_eq!(child.backdrop_rect.x, 42.0);
        assert_eq!(child.backdrop_rect.y, 424.0);
    }

    #[test]
    fn box4_composite_viewport_uses_integer_source_extent_for_one_to_one_resolve() {
        let viewport = composite_dest_viewport(
            Rect {
                x: 0.0,
                y: -2441.0,
                width: 1398.75,
                height: 3451.25,
            },
            1399,
            3452,
            CompositeSampleMode::Box4,
        );

        assert_eq!(viewport, (0.0, -2441.0, 1399.0, 3452.0));
    }

    #[test]
    fn box4_composite_viewport_keeps_supersampled_destination_extent() {
        let viewport = composite_dest_viewport(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 320.0,
                height: 180.0,
            },
            2880,
            1620,
            CompositeSampleMode::Box4,
        );

        assert_eq!(viewport, (0.0, 0.0, 320.0, 180.0));
    }

    #[test]
    fn linear_composite_viewport_keeps_fractional_geometry_extent() {
        let viewport = composite_dest_viewport(
            Rect {
                x: 0.25,
                y: 10.5,
                width: 40.75,
                height: 20.25,
            },
            41,
            21,
            CompositeSampleMode::Linear,
        );

        assert_eq!(viewport, (0.25, 10.5, 40.75, 20.25));
    }

    #[test]
    fn child_composite_visibility_rejects_offscreen_quad() {
        let quad = [[130.0, 10.0], [160.0, 10.0], [130.0, 40.0], [160.0, 40.0]];

        assert!(!child_composite_visible(quad, None, 1.0, 100, 100));
    }

    #[test]
    fn child_composite_visibility_rejects_clipped_away_quad() {
        let quad = [[10.0, 10.0], [60.0, 10.0], [10.0, 60.0], [60.0, 60.0]];
        let clip = Rect {
            x: 70.0,
            y: 70.0,
            width: 10.0,
            height: 10.0,
        };

        assert!(!child_composite_visible(quad, Some(clip), 1.0, 100, 100));
    }

    #[test]
    fn child_composite_visibility_accepts_transformed_intersecting_quad() {
        let quad = [[80.0, 80.0], [130.0, 110.0], [60.0, 140.0], [110.0, 170.0]];

        assert_eq!(
            quad_bounds_rect(quad),
            Some(Rect {
                x: 60.0,
                y: 80.0,
                width: 70.0,
                height: 90.0,
            })
        );
        assert!(child_composite_visible(quad, None, 1.0, 100, 100));
    }

    #[test]
    fn runtime_shader_layers_reuse_source_content_without_user_cache_policy() {
        let layer = default_cache_runtime_shader_layer();
        let lowered = lower_test_layer(&layer);
        assert!(
            layer_source_cache_key(
                &lowered,
                lowered.surface_requirements.surface_requirements,
                layer.local_bounds,
                (32, 24),
                1.0,
                false,
                true,
            )
            .is_some(),
            "runtime shader source content must be cacheable by the renderer even when the public layer cache policy is default"
        );
    }

    #[test]
    fn backdrop_layers_reuse_static_local_source_without_user_cache_policy() {
        let layer = backdrop_child_layer(98);
        let lowered = lower_test_layer(&layer);

        assert!(layer.backdrop().is_some());
        assert!(
            layer_source_cache_key(
                &lowered,
                lowered.surface_requirements.surface_requirements,
                layer.local_bounds,
                (80, 40),
                1.0,
                false,
                true,
            )
            .is_some(),
            "moving backdrop layers must retain static local source content by content hash"
        );
    }

    #[test]
    fn backdrop_layers_reuse_static_local_source_with_external_underlay() {
        let layer = backdrop_child_layer(99);
        let lowered = lower_test_layer(&layer);

        assert!(
            layer_source_cache_key(
                &lowered,
                lowered.surface_requirements.surface_requirements,
                layer.local_bounds,
                (80, 40),
                1.0,
                true,
                true,
            )
            .is_some(),
            "a layer's own backdrop is applied after its local source, so the source cache is independent from the external underlay"
        );
    }

    #[test]
    fn nested_backdrop_sources_do_not_cache_external_underlay_dependency() {
        let child = backdrop_child_layer(100);
        let mut layer = crate::test_support::layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 90.0,
            },
            ProjectiveTransform::identity(),
            GraphicsLayer {
                backdrop_effect: Some(RenderEffect::blur(4.0)),
                ..Default::default()
            },
            vec![RenderNode::Layer(Box::new(child))],
        );
        layer.node_id = Some(101);
        layer.recompute_raster_cache_hashes();
        let lowered = lower_test_layer(&layer);

        assert_eq!(
            layer_source_cache_key(
                &lowered,
                lowered.surface_requirements.surface_requirements,
                layer.local_bounds,
                (120, 90),
                1.0,
                true,
                true,
            ),
            None,
            "nested backdrop sources depend on the external underlay until the descendant input is isolated"
        );
    }

    #[test]
    fn motion_stable_translated_text_source_cache_ignores_scroll_offset() {
        let text = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id: 91,
                rect: Rect {
                    x: 4.0,
                    y: 8.0,
                    width: 160.0,
                    height: 24.0,
                },
                text: std::rc::Rc::new(AnnotatedString::from("cached translated text")),
                text_style: TextStyle::default(),
                font_size: 16.0,
                layout_options: TextLayoutOptions::default(),
                clip: None,
            })),
        });
        let mut base = crate::test_support::layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 220.0,
                height: 96.0,
            },
            ProjectiveTransform::identity(),
            GraphicsLayer::default(),
            vec![text],
        );
        base.node_id = Some(91);
        base.translated_content_context = true;
        base.motion_context_animated = true;
        base.translated_content_offset = Point::new(0.0, -40.0);
        base.recompute_raster_cache_hashes();

        let mut moved = base.clone();
        moved.translated_content_offset = Point::new(0.0, -80.0);
        moved.recompute_raster_cache_hashes();

        assert_ne!(base.target_content_hash(), moved.target_content_hash());
        assert_eq!(
            base.motion_source_content_hash(),
            moved.motion_source_content_hash()
        );

        let base_lowered = lower_test_layer(&base);
        assert!(base_lowered
            .surface_requirements
            .surface_requirements
            .contains(SurfaceRequirement::MotionStableCapture));

        let moved_lowered = lower_test_layer(&moved);
        let base_key = layer_source_cache_key(
            &base_lowered,
            base_lowered.surface_requirements.surface_requirements,
            base.local_bounds,
            (220, 96),
            1.0,
            false,
            true,
        );
        let moved_key = layer_source_cache_key(
            &moved_lowered,
            moved_lowered.surface_requirements.surface_requirements,
            moved.local_bounds,
            (220, 96),
            1.0,
            false,
            true,
        );

        assert_eq!(base_key, moved_key);
    }

    #[test]
    fn box4_composite_viewport_keeps_scaled_geometry_extent() {
        let viewport = composite_dest_viewport(
            Rect {
                x: 0.25,
                y: 10.5,
                width: 140.0,
                height: 80.0,
            },
            70,
            40,
            CompositeSampleMode::Box4,
        );

        assert_eq!(viewport, (0.0, 11.0, 140.0, 80.0));
    }

    #[test]
    fn box4_non_capture_surface_keeps_device_scale_as_minimum() {
        assert_eq!(
            minimum_surface_scale_for_composite(
                1.35,
                CompositeSampleMode::Box4,
                SurfaceRequirementSet::default().with(SurfaceRequirement::ExplicitOffscreen),
            ),
            1.35
        );
    }

    #[test]
    fn linear_surface_can_use_memory_budget_scale_floor() {
        assert_eq!(
            minimum_surface_scale_for_composite(
                1.35,
                CompositeSampleMode::Linear,
                SurfaceRequirementSet::default()
                    .with(SurfaceRequirement::ExplicitOffscreen)
                    .with(SurfaceRequirement::NonTranslationTransform),
            ),
            1.0
        );
    }

    #[test]
    fn motion_stable_capture_keeps_its_existing_budget_scale_floor() {
        assert_eq!(
            minimum_surface_scale_for_composite(
                8.0,
                CompositeSampleMode::Box4,
                SurfaceRequirementSet::default().with(SurfaceRequirement::MotionStableCapture),
            ),
            1.0
        );
    }

    #[test]
    fn layer_surface_dest_quad_maps_actual_trimmed_surface_rect() {
        let child_rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 200.0,
        };
        let child_quad = [
            [110.0, 220.0],
            [310.0, 220.0],
            [110.0, 620.0],
            [310.0, 620.0],
        ];
        let surface_rect = Rect {
            x: 10.0,
            y: 70.0,
            width: 100.0,
            height: 80.0,
        };

        assert_eq!(
            layer_surface_dest_quad(child_rect, child_quad, surface_rect),
            [
                [110.0, 320.0],
                [310.0, 320.0],
                [110.0, 480.0],
                [310.0, 480.0],
            ],
        );
    }

    #[test]
    fn layer_surface_context_marks_nested_motion_stable_capture_active() {
        let context = layer_surface_translation_context(
            TranslationRenderContext {
                inherited_content_translation: true,
                surface_capture_active: false,
                ..TranslationRenderContext::default()
            },
            true,
        );

        assert_eq!(
            context,
            TranslationRenderContext {
                inherited_content_translation: true,
                surface_capture_active: true,
                ..TranslationRenderContext::default()
            }
        );
    }

    #[test]
    fn layer_surface_context_keeps_generic_parent_surface_inactive() {
        let context = layer_surface_translation_context(
            TranslationRenderContext {
                inherited_content_translation: true,
                surface_capture_active: false,
                ..TranslationRenderContext::default()
            },
            false,
        );

        assert_eq!(
            context,
            TranslationRenderContext {
                inherited_content_translation: true,
                surface_capture_active: false,
                ..TranslationRenderContext::default()
            }
        );
    }

    #[test]
    fn integer_aligned_backdrop_copy_region_is_copyable() {
        let region = axis_aligned_backdrop_copy_region(
            Rect {
                x: 12.0,
                y: 20.0,
                width: 140.0,
                height: 100.0,
            },
            2.0,
            (800, 600),
            (280, 200),
        );

        assert_eq!(region, Some(((24, 40), (280, 200))));
    }

    #[test]
    fn fractional_backdrop_copy_region_requires_render_pass_fallback() {
        let region = axis_aligned_backdrop_copy_region(
            Rect {
                x: 12.25,
                y: 20.0,
                width: 140.0,
                height: 100.0,
            },
            2.0,
            (800, 600),
            (280, 200),
        );

        assert_eq!(region, None);
    }

    #[test]
    fn fractional_backdrop_snapshot_copy_plan_uses_enclosing_pixels() {
        let plan = axis_aligned_backdrop_snapshot_copy_plan(
            Rect {
                x: 12.25,
                y: 20.5,
                width: 140.0,
                height: 100.0,
            },
            Rect {
                x: 12.25,
                y: 20.5,
                width: 140.0,
                height: 100.0,
            },
            2.0,
            (800, 600),
            4096,
        )
        .expect(
            "fractional axis-aligned backdrop should be copyable through an enclosing snapshot",
        );

        assert_eq!(plan.source_origin, (24, 41));
        assert_eq!(plan.size, (281, 200));
        assert_eq!(plan.effect_pixel_rect, [0.5, 0.0, 280.0, 200.0]);
        assert_eq!(plan.dest_viewport, (24.0, 41.0, 281.0, 200.0));
    }

    #[test]
    fn backdrop_snapshot_effect_geometry_is_stable_across_device_pixel_steps() {
        let mut layer = test_backdrop_layer(Rect {
            x: 59.0,
            y: 416.932_77,
            width: 398.0,
            height: 54.0,
        });
        let fixed_clip = Rect {
            x: 40.0,
            y: 120.0,
            width: 500.0,
            height: 600.0,
        };
        layer.clip = Some(fixed_clip);
        layer.snap_anchor = Some(SnapAnchor::rigid(Point::new(0.0, 127.600_006)));
        let (effect, clip) = snapped_backdrop_geometry(&layer, 1.25);
        assert_eq!(clip, Some(fixed_clip));
        let capture = Rect {
            x: effect.x - 25.9,
            y: effect.y - 25.9,
            width: effect.width + 51.8,
            height: effect.height + 51.8,
        };
        let initial =
            axis_aligned_backdrop_snapshot_copy_plan(capture, effect, 1.25, (900, 1_600), 4_096)
                .expect("visible backdrop must have a copy plan");

        for step in 1..=10 {
            layer.rect.y -= 0.8;
            layer
                .snap_anchor
                .as_mut()
                .expect("test backdrop keeps its snap anchor")
                .origin
                .y -= 0.8;
            let (effect, clip) = snapped_backdrop_geometry(&layer, 1.25);
            assert_eq!(
                clip,
                Some(fixed_clip),
                "content snapping moved the fixed backdrop clip after {step} physical pixels"
            );
            let capture = Rect {
                x: effect.x - 25.9,
                y: effect.y - 25.9,
                width: effect.width + 51.8,
                height: effect.height + 51.8,
            };
            let translated = axis_aligned_backdrop_snapshot_copy_plan(
                capture,
                effect,
                1.25,
                (900, 1_600),
                4_096,
            )
            .expect("translated backdrop must keep a copy plan");

            assert_eq!(
                translated.effect_pixel_rect, initial.effect_pixel_rect,
                "shader-local geometry drifted after {step} physical pixels"
            );
            assert_eq!(translated.size, initial.size);
            assert_eq!(
                translated.source_origin.1 + step,
                initial.source_origin.1,
                "snapshot must translate by exactly one physical pixel per step"
            );
        }
    }

    #[test]
    fn padded_backdrop_snapshot_copy_plan_offsets_effect_rect_inside_capture() {
        let plan = axis_aligned_backdrop_snapshot_copy_plan(
            Rect {
                x: 2.0,
                y: 14.0,
                width: 164.0,
                height: 124.0,
            },
            Rect {
                x: 14.0,
                y: 26.0,
                width: 140.0,
                height: 100.0,
            },
            2.0,
            (800, 600),
            4096,
        )
        .expect("padded backdrop capture should be copyable");

        assert_eq!(plan.source_origin, (4, 28));
        assert_eq!(plan.size, (328, 248));
        assert_eq!(plan.effect_pixel_rect, [24.0, 24.0, 280.0, 200.0]);
        assert_eq!(plan.dest_viewport, (4.0, 28.0, 328.0, 248.0));
    }

    #[test]
    fn backdrop_pending_flush_region_includes_effect_input_padding() {
        let visible = Rect {
            x: 100.0,
            y: 80.0,
            width: 50.0,
            height: 40.0,
        };
        let pending_outside_visible_inside_capture = Rect {
            x: 82.0,
            y: 88.0,
            width: 10.0,
            height: 10.0,
        };
        let mut shader = RuntimeShader::new("// test");
        shader.set_input_padding(24.0);
        let effect = RenderEffect::runtime_shader(shader);

        let capture = visible_backdrop_capture_rect(visible, None, &effect, 1.0, (220, 180))
            .expect("visible backdrop capture");

        assert_eq!(
            capture,
            Rect {
                x: 76.0,
                y: 56.0,
                width: 98.0,
                height: 88.0,
            }
        );
        assert!(!rects_intersect(
            visible,
            pending_outside_visible_inside_capture
        ));
        assert!(rects_intersect(
            capture,
            pending_outside_visible_inside_capture
        ));
    }

    #[test]
    fn layer_surface_context_keeps_existing_capture_active() {
        let context = layer_surface_translation_context(
            TranslationRenderContext {
                inherited_content_translation: false,
                surface_capture_active: true,
                ..TranslationRenderContext::default()
            },
            false,
        );

        assert_eq!(
            context,
            TranslationRenderContext {
                inherited_content_translation: false,
                surface_capture_active: true,
                ..TranslationRenderContext::default()
            }
        );
    }

    #[test]
    fn layer_surface_context_keeps_root_viewport_uncaptured() {
        let context = layer_surface_translation_context(
            TranslationRenderContext {
                inherited_content_translation: false,
                surface_capture_active: false,
                ..TranslationRenderContext::default()
            },
            false,
        );

        assert_eq!(
            context,
            TranslationRenderContext {
                inherited_content_translation: false,
                surface_capture_active: false,
                ..TranslationRenderContext::default()
            }
        );
    }
}
