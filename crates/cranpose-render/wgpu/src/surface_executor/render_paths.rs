use std::{
    cell::RefCell,
    hash::{Hash, Hasher},
    ops::Range,
    rc::Rc,
};

use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_render_common::{
    geometry::union_rect,
    graph::{CachePolicy, ProjectiveTransform, quad_bounds},
    raster_cache::{LayerRasterCacheKey, ScaleBucket},
};
use cranpose_ui::text::LinkKey;
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, FxHasher, Point, Rect, RenderEffect, RenderHash, RuntimeShader,
};

use super::{
    backend::{LayerSurface, LayerSurfaceTexture, SurfaceExecutionBackend},
    geometry::{
        axis_aligned_quad_rect, canonicalized_anchored_scaled_quad, clamp_effect_surface_scale,
        content_effect_pixel_rect, device_pixel_exact_surface_rect,
        fit_capture_rect_to_scale_budget_for_axes, offscreen_byte_size,
        quantize_motion_stable_target_scale, scaled_quad, snap_delta_for_anchor,
        snap_dest_quad_to_stable_point, snap_motion_stable_dest_quad, surface_pixel_rect,
        surface_target_size, target_quad, visible_layer_rect,
    },
};
#[cfg(not(target_arch = "wasm32"))]
use crate::shape_replay::{any_pending_feed_captures, shape_index_pending_feed_capture};
use crate::{
    effect_renderer::{
        CompositeBatchItem, CompositeSampleMode, FusedCompositeItem, ProjectiveSurfaceComposite,
        RoundedCompositeMask, ShaderCompositeBatchItem,
    },
    layer_events::{LayerEventKind, collect_effect_ranges, collect_layer_events},
    layer_surface_cache::{MAX_LAYER_SURFACE_CACHE_BYTES, MAX_SCENE_RANGE_CACHE_ENTRY_BYTES},
    normalized_scene::{
        ChildLayerComposite, CollectedLayer, LoweredChildSource, ResolvedChildSurfaceComposite,
        SceneWindowSource, TranslateBy, build_scene_window, collected_layer_bounds,
        filtered_effect_layer_index, motion_stable_capture_bounds_from_parts,
        resolved_child_surface_composite, resolved_layer_surface_rect_from_parts,
        shadow_draw_is_blurred_drop, translate_quad, visible_draw_rect,
    },
    offscreen::OffscreenTarget,
    render::{has_backdrop_layer_in_range, is_in_effect_range, scissor_rect_for_rect},
    scene::{
        BackdropLayer, CompositorScene, DrawOp, DrawOpKind, DrawShape, EffectLayer, ImageDraw,
        SceneBrush, ShadowDraw, SnapAnchor, TextDraw,
    },
    surface_executor::CacheAdmission,
    surface_plan::{
        LayerSurfaceRenderOptions, LayerSurfaceRequest, TranslatedContentAxes,
        TranslationRenderContext, composite_sample_mode_for_effect_layer,
        composite_sample_mode_for_requirements, effect_layer_minimum_scale,
        effect_layer_target_scale, effective_surface_requirements, layer_surface_target_scale,
    },
    surface_requirements::{SurfaceRequirement, SurfaceRequirementSet},
};

/// The command feed and its capture machinery are native-only, so on wasm
/// no feed capture can ever be pending: both predicates are identically
/// false here rather than configured out at every call site.
#[cfg(target_arch = "wasm32")]
fn any_pending_feed_captures() -> bool {
    false
}

#[cfg(target_arch = "wasm32")]
fn shape_index_pending_feed_capture(_shape_index: usize) -> bool {
    false
}

fn layer_render_diag_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_LAYER_RENDER_DIAG").is_some())
}

fn record_layer_cache_miss<B: SurfaceExecutionBackend>(
    backend: &mut B,
    site: &str,
    key: &LayerRasterCacheKey,
    width: u32,
    height: u32,
) {
    if crate::layer_surface_cache::cache_diag_enabled() {
        log::warn!("[layer-cache-diag] miss site={site} key={key:?}");
    }
    backend.record_layer_cache_miss(key, width, height);
}

fn direct_scene_range_cache_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        crate::debug_toggles::debug_toggle("CRANPOSE_DISABLE_DIRECT_SCENE_RANGE_CACHE").is_none()
    })
}

/// Kill switch for prefix snapshots alone (`CRANPOSE_DISABLE_PREFIX_SNAPSHOT`,
/// mirrored as `debug.cranpose.no_prefix_snap` on Android), so a device A/B
/// can separate the snapshot mechanism from the flatten cache it shares a
/// budget with. The range-cache kill switch disables both.
fn prefix_snapshot_enabled() -> bool {
    crate::debug_toggles::debug_toggle("CRANPOSE_DISABLE_PREFIX_SNAPSHOT").is_none()
}

fn deferred_direct_run_enabled() -> bool {
    crate::debug_toggles::debug_toggle("CRANPOSE_NO_DEFERRED_RUN").as_deref() != Some("1")
}

fn shadow_composite_queue_enabled() -> bool {
    crate::debug_toggles::debug_toggle("CRANPOSE_NO_SHADOW_COMPOSITE_QUEUE").as_deref() != Some("1")
}

fn direct_scene_range_coalesce_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        crate::debug_toggles::debug_toggle("CRANPOSE_DIRECT_SCENE_RANGE_COALESCE").as_deref()
            != Some("0")
    })
}

#[derive(Default)]
struct DirectChunkRunCoalescer {
    run_start: Option<usize>,
}

impl DirectChunkRunCoalescer {
    fn absorb(&mut self, chunk_start: usize) {
        self.run_start.get_or_insert(chunk_start);
    }

    fn peek(&self, boundary: usize) -> Option<(usize, usize)> {
        self.run_start
            .filter(|start| *start < boundary)
            .map(|start| (start, boundary))
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
    direct_scene_range_cache_enabled_for_policy(!direct_scene_range_cache_enabled(), byte_size)
}

/// The flatten cache's flat 2 MB floor. Flatten entries composite content
/// through an intermediate texture, which cannot reproduce the direct
/// path's chained per-draw roundings — a bounded, tested inexactness the
/// small class trades for mid-scene coverage. Large stable regions are
/// served exactly instead, by the prefix snapshot path, so the floor is a
/// scope line between the inexact-but-general mechanism and the
/// exact-but-prefix-only one.
fn direct_scene_range_cache_enabled_for_policy(disable_all: bool, byte_size: u64) -> bool {
    !disable_all && byte_size <= DIRECT_SCENE_RANGE_CACHE_FLOOR_BYTES
}

fn direct_scene_range_hash_diag_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_DIRECT_SCENE_RANGE_HASH_DIAG").is_some())
}

fn direct_scene_range_hash_detail_z() -> Option<usize> {
    static DETAIL_Z: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    *DETAIL_Z.get_or_init(|| {
        crate::debug_toggles::debug_toggle("CRANPOSE_DIRECT_SCENE_RANGE_HASH_DETAIL_Z")
            .and_then(|value| value.parse().ok())
    })
}

const MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS: usize = 2;
const MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS: usize = 64;
const MIN_SINGLE_DRAW_DIRECT_SCENE_RANGE_CACHE_BYTES: u64 = 512 * 1024;
const MAX_MOTION_SENSITIVE_DIRECT_SCENE_CACHE_DRAW_BYTES: u64 = 2_097_152;
const DIRECT_SCENE_RANGE_CACHE_FLOOR_BYTES: u64 = 2 * 1024 * 1024;
const MAX_DIRECT_SCENE_RANGE_CACHE_BYTES: u64 = MAX_SCENE_RANGE_CACHE_ENTRY_BYTES;
const MIN_PREFIX_SNAPSHOT_DRAW_OPS: usize = 2;
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
        if let Some(entry) = self.entries.get(&key)
            && entry.text.strong_count() > 0
            && entry.text.as_ptr() == std::sync::Arc::as_ptr(text)
        {
            return entry.hash;
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
    if !matches!(
        sample_mode,
        CompositeSampleMode::Box4 | CompositeSampleMode::Nearest
    ) {
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

fn exact_translation_sample_mode(
    dest_rect: Rect,
    source_width: u32,
    source_height: u32,
    sample_mode: CompositeSampleMode,
) -> CompositeSampleMode {
    if dest_rect.x.fract() != 0.0
        || dest_rect.y.fract() != 0.0
        || (dest_rect.width - source_width as f32).abs() > 1.0
        || (dest_rect.height - source_height as f32).abs() > 1.0
    {
        return sample_mode;
    }
    CompositeSampleMode::Nearest
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
    if radius <= 0.0 || radius.is_nan() {
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn uniform_underlay_color_before(
    scene: &CompositorScene,
    prior_child_contributions: &[BackdropPrefixChildContribution],
    z_end: usize,
    rect: Rect,
    root_scale: f32,
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

    None
}

/// Identity of the pixels a nested backdrop reads from its parent's target
/// under `rect`: a uniform colour when the prefix is one opaque solid (so the
/// identity survives translation), otherwise the hash of every prefix writer
/// intersecting `rect` together with `rect` itself, because the same writers
/// put different pixels under a rect that moved, mixed with the parent's own
/// underlay identity. `None` when the parent reads an underlay whose identity
/// is unknown, which keeps a surface that bakes such pixels out of the cache.
#[allow(clippy::too_many_arguments)]
pub(crate) fn underlay_identity_before(
    scene: &CompositorScene,
    prior_child_contributions: &[BackdropPrefixChildContribution],
    z_end: usize,
    rect: Rect,
    target_size: (u32, u32),
    root_scale: f32,
    inherited: Option<u64>,
    has_inherited_underlay: bool,
) -> Option<u64> {
    if has_inherited_underlay && inherited.is_none() {
        return None;
    }
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let mut hasher = FxHasher::default();
    0x0BD5_1DEDu64.hash(&mut hasher);
    inherited.hash(&mut hasher);
    match uniform_underlay_color_before(scene, prior_child_contributions, z_end, rect, root_scale) {
        Some(color) => {
            0u8.hash(&mut hasher);
            color.hash(&mut hasher);
        }
        None => {
            1u8.hash(&mut hasher);
            hash_rect(rect, &mut hasher);
            backdrop_scene_prefix_hash(
                scene,
                prior_child_contributions,
                z_end,
                rect,
                target_size,
                root_scale,
            )
            .hash(&mut hasher);
        }
    }
    Some(hasher.finish())
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
    if capture_pixel_rect.x.floor() < 0.0
        || capture_pixel_rect.y.floor() < 0.0
        || capture_pixel_rect.width <= 0.0
        || capture_pixel_rect.height <= 0.0
    {
        return None;
    }

    // The window's size is a function of the capture's SIZE alone — the
    // ceiled span plus the one pixel a fractional phase can add — never of
    // its position. A floor/ceil span flips by one pixel as translation
    // crosses pixel boundaries, and that flip churns the cache key and
    // defeats pool recycling for content that did not change. A capture
    // spanning the whole axis takes the whole axis (its clipped position is
    // zero at every phase), and a window that would overhang the source
    // shifts inward instead of falling off the copy path, so the fast-path
    // choice is phase-independent too.
    let axis = |span: f32, position: f32, source: u32| -> Option<(u32, u32)> {
        let span = span.ceil() as u32;
        let size = if span >= source {
            source
        } else {
            span.saturating_add(1)
        };
        if size == 0 || size > source {
            return None;
        }
        let origin = (position.floor() as u32).min(source - size);
        Some((origin, size))
    };
    let (left, width) = axis(
        capture_pixel_rect.width,
        capture_pixel_rect.x,
        source_size.0,
    )?;
    let (top, height) = axis(
        capture_pixel_rect.height,
        capture_pixel_rect.y,
        source_size.1,
    )?;
    if width > max_texture_dim || height > max_texture_dim {
        return None;
    }

    Some(BackdropSnapshotCopyPlan {
        source_origin: (left, top),
        size: (width, height),
        effect_pixel_rect: [
            effect_pixel_rect.x - left as f32,
            effect_pixel_rect.y - top as f32,
            effect_pixel_rect.width,
            effect_pixel_rect.height,
        ],
        dest_viewport: (left as f32, top as f32, width as f32, height as f32),
    })
}

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

fn scene_range_draw_ops(draw_ops: &[DrawOp], z_start: usize, z_end: usize) -> &[DrawOp] {
    debug_assert!(draw_ops.windows(2).all(|w| w[0].z_index <= w[1].z_index));
    let start = draw_ops.partition_point(|op| op.z_index < z_start);
    let len = draw_ops[start..].partition_point(|op| op.z_index < z_end);
    &draw_ops[start..start + len]
}

fn scene_range_draw_op_count(scene: &CompositorScene, z_start: usize, z_end: usize) -> usize {
    scene_range_draw_ops(&scene.draw_ops, z_start, z_end).len()
}

fn draw_op_caches_as_transparent_surface(scene: &CompositorScene, op: DrawOp) -> bool {
    match op.kind {
        // The feed-capture exclusion: a capture queued this frame copies
        // those exact shape slots out of the ordinary conversion stream, and
        // a range composite that swallows the span retains wrong content
        // under a confirmed identity (command_feed_parity pins this).
        DrawOpKind::Shape(index) => {
            scene
                .shapes
                .get(index)
                .is_some_and(|shape| shape.blend_mode == BlendMode::SrcOver)
                && !shape_index_pending_feed_capture(index)
        }
        DrawOpKind::Image(index) => scene
            .images
            .get(index)
            .is_some_and(|image| image.blend_mode == BlendMode::SrcOver),
        DrawOpKind::Text(_) => true,
        DrawOpKind::Shadow(_) => false,
        // A replayed batch transforms every frame; a texture of it would
        // be stale on arrival.
        DrawOpKind::Retained(_) => false,
    }
}

fn scene_range_can_cache_as_transparent_surface(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
) -> bool {
    scene_range_draw_ops(&scene.draw_ops, z_start, z_end)
        .iter()
        .all(|op| draw_op_caches_as_transparent_surface(scene, *op))
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

fn direct_scene_range_snapped_bounds(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    root_scale: f32,
) -> Option<Rect> {
    scene_range_visible_bounds(scene, z_start, z_end)
        .and_then(|bounds| snap_scene_range_bounds_to_pixels(bounds, root_scale))
}

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

/// Every pixel a deferred range can write, shadows included: a blurred
/// shadow reaches beyond its shapes by the blur extent, so it is bounded by
/// its expanded, clipped rectangle rather than skipped like the chunk cache
/// skips it.
fn deferred_range_bounds(
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    excluded: &[Range<usize>],
) -> Option<Rect> {
    let mut bounds = None;
    for op in scene_range_draw_ops(&scene.draw_ops, z_start, z_end) {
        if is_in_effect_range(op.z_index, excluded) {
            continue;
        }
        let rect = match op.kind {
            DrawOpKind::Shadow(index) => scene.shadow_draws.get(index).and_then(|shadow| {
                crate::normalized_scene::shadow_draws_bounds(std::slice::from_ref(shadow))
            }),
            _ => draw_op_visible_bounds(scene, *op),
        };
        if let Some(rect) = rect {
            bounds = union_rect(bounds, rect);
        }
    }
    bounds
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
/// A child composited with alpha one, `SrcOver`, no group effect and no snap
/// can be rendered on top of the pixels beneath it: `underlay ∘ content`
/// composited over `underlay` is `content` composited over `underlay` at
/// every pixel, mask coverage included, so the surface may bake its underlay.
fn child_composites_plainly(child: &ChildLayerComposite) -> bool {
    child.isolation.as_ref().is_none_or(|isolation| {
        isolation.effect.is_none()
            && isolation.blend_mode == BlendMode::SrcOver
            && isolation.composite_alpha == 1.0
    }) && !child.has_effect
}

/// A snapped surface rendered at the target's own scale lands on whole
/// device pixels, and its quad then covers exactly its own texels: the
/// composite is a copy instead of a per-frame resample of every card, and the
/// underlay copy beneath it is the pixels the composite will replace.
fn texel_aligned_dest_quad(
    dest_quad: [[f32; 2]; 4],
    surface_size: (u32, u32),
    snap_anchor: Option<SnapAnchor>,
    surface_scale: f32,
    target_scale: f32,
) -> [[f32; 2]; 4] {
    let whole_pixel_snap = snap_anchor.is_some_and(|anchor| anchor.device_pixel_step == 1.0);
    if !whole_pixel_snap || (surface_scale - target_scale).abs() > 1e-6 {
        return dest_quad;
    }
    let Some(rect) = axis_aligned_quad_rect(dest_quad) else {
        return dest_quad;
    };
    let on_device_pixels = |value: f32| (value - value.round()).abs() <= 1e-3;
    let (width, height) = (surface_size.0 as f32, surface_size.1 as f32);
    if !on_device_pixels(rect.x)
        || !on_device_pixels(rect.y)
        || (rect.width - width).abs() >= 1.0
        || (rect.height - height).abs() >= 1.0
    {
        return dest_quad;
    }
    crate::rect_to_quad(Rect {
        x: rect.x.round(),
        y: rect.y.round(),
        width,
        height,
    })
}

/// The device pixel the child's surface origin lands on once its composite
/// is anchored and snapped exactly as the composite will be, or `None` when
/// the composite is not a whole-pixel translation of the surface.
fn baked_underlay_device_origin(
    child: &ChildLayerComposite,
    logical_rect: Rect,
    dest_quad: [[f32; 2]; 4],
    snap_anchor: Option<SnapAnchor>,
    composite_snap_origin: Option<Point>,
    scale: f32,
    translation_context: TranslationRenderContext,
) -> Option<(u32, u32)> {
    let translated_content_context = translation_context.inherited_content_translation
        || child.translated_content_context
        || child.surface_requirements.contains_translated_content;
    let sample_mode = composite_sample_mode_for_requirements(
        translated_content_context,
        translation_context.surface_capture_active,
        child.surface_requirements,
    );
    let device_quad = anchored_composite_dest_quad(
        layer_surface_dest_quad(logical_rect, dest_quad, logical_rect),
        snap_anchor,
        composite_snap_origin,
        scale,
        sample_mode,
    );
    let (width, height) = surface_target_size(logical_rect, scale, u32::MAX);
    let device_quad =
        texel_aligned_dest_quad(device_quad, (width, height), snap_anchor, scale, scale);
    let device_rect = axis_aligned_quad_rect(device_quad)?;
    let on_device_pixels = |value: f32| value >= 0.0 && (value - value.round()).abs() <= 1e-3;
    let whole_translation = on_device_pixels(device_rect.x)
        && on_device_pixels(device_rect.y)
        && (device_rect.width - width as f32).abs() <= 1e-3
        && (device_rect.height - height as f32).abs() <= 1e-3;
    whole_translation.then(|| (device_rect.x.round() as u32, device_rect.y.round() as u32))
}

struct ChildUnderlayPlacement {
    logical_rect: Rect,
    dest_quad: [[f32; 2]; 4],
    snapped_dest_quad: [[f32; 2]; 4],
    snap_anchor: Option<SnapAnchor>,
    composite_snap_origin: Option<Point>,
}

struct ChildUnderlay {
    target: OffscreenTarget,
    baked: bool,
}

/// The parent target a child's underlay is sampled from, with the composite
/// queues still waiting to land on it.
struct UnderlayCaptureSource<'a, 'q, 's> {
    target_view: &'a wgpu::TextureView,
    viewport: (u32, u32),
    dependency_rect: Rect,
    queues: &'a mut PendingQueues<'q, 's>,
}

/// The pixels beneath a child that carries nested backdrops. A whole-pixel
/// translated child gets a verbatim copy it bakes as its surface's base, and
/// the pending composites that overlap it, its own backdrop included, are
/// replayed into that copy instead of being flushed to the parent first: the
/// same blend on the same source and destination pixels, shifted by the
/// copy's whole-pixel origin, so the copy carries the bytes a flush would
/// have produced while the parent keeps batching. Any other placement
/// flushes the queues and reads a projected resample through them.
#[allow(clippy::too_many_arguments)]
fn sample_child_underlay<B: SurfaceExecutionBackend>(
    backend: &mut B,
    parent_target: &OffscreenTarget,
    parent_underlay: Option<&OffscreenTarget>,
    child: &ChildLayerComposite,
    placement: ChildUnderlayPlacement,
    underlay_identity: Option<u64>,
    parent_scale: f32,
    translation_context: TranslationRenderContext,
    source: UnderlayCaptureSource<'_, '_, '_>,
) -> Result<ChildUnderlay, String> {
    let child_scale = child_surface_target_scale(child, parent_scale, translation_context);
    let device_origin = (backend.underlay_bake_enabled()
        && parent_underlay.is_none()
        && underlay_identity.is_some()
        && child_composites_plainly(child)
        && (child_scale - parent_scale).abs() <= 1e-6)
        .then(|| {
            baked_underlay_device_origin(
                child,
                placement.logical_rect,
                placement.dest_quad,
                placement.snap_anchor,
                placement.composite_snap_origin,
                parent_scale,
                translation_context,
            )
        })
        .flatten();
    flush_deferred_run_for_dependency(
        backend,
        source.target_view,
        source.viewport,
        parent_scale,
        source.dependency_rect,
        child.z_index,
        source.queues,
    )?;
    let dependency_pixels = surface_pixel_rect(source.dependency_rect, parent_scale);
    let conflicts = pending_capture_conflicts(
        source.queues.composites,
        source.queues.composite_load_op,
        source.queues.shader_composites,
        source.queues.shader_load_op,
        dependency_pixels,
    );
    let replay =
        device_origin.is_some() && backend.underlay_replay_enabled() && !conflicts.clear_held;
    if backdrop_diag_enabled() {
        eprintln!(
            "[backdrop-diag] underlay node={:?} plain={} identity={} device_origin={:?} replay={replay} conflicts={}:{}",
            child.node_id,
            child_composites_plainly(child),
            underlay_identity.is_some(),
            device_origin,
            conflicts.composites,
            conflicts.shaders,
        );
    }
    let flush_queues = |backend: &mut B, queues: &mut PendingQueues<'_, '_>| {
        flush_pending_queues_for_backdrop_capture(
            backend,
            queues.composites,
            queues.composite_load_op,
            queues.shader_composites,
            queues.shader_load_op,
            source.target_view,
            source.viewport,
            queues.next_load_op,
            source.dependency_rect,
            parent_scale,
        )
    };
    if replay {
        flush_pending_clear(backend, source.target_view, source.queues.next_load_op);
    } else {
        flush_queues(backend, source.queues)?;
    }
    if let Some(origin) = device_origin {
        if let Some(target) = copy_child_underlay(
            backend,
            parent_target,
            origin,
            placement.logical_rect,
            parent_scale,
        ) {
            if replay && conflicts.any() {
                replay_pending_into_copy(
                    backend,
                    &target,
                    origin,
                    source.queues.composites,
                    source.queues.shader_composites,
                    dependency_pixels,
                )?;
            }
            return Ok(ChildUnderlay {
                target,
                baked: true,
            });
        }
        if replay {
            flush_queues(backend, source.queues)?;
        }
    }
    let target = create_projected_child_underlay(
        backend,
        parent_target,
        parent_underlay,
        placement.logical_rect,
        placement.snapped_dest_quad,
        parent_scale,
        child_scale,
        underlay_sample_rect(&child.source),
    );
    Ok(ChildUnderlay {
        target,
        baked: false,
    })
}

/// A pending write moved from parent pixels into the underlay copy's own
/// pixel space: the same destination shifted by the copy's origin, with its
/// scissor cut down to the copy. `None` when the scissor leaves nothing of
/// the write inside the copy.
struct ReplayedWrite {
    dest_quad: [[f32; 2]; 4],
    dest_rect: Option<Rect>,
    scissor: Option<(u32, u32, u32, u32)>,
}

fn replayed_write(
    dest_quad: [[f32; 2]; 4],
    scissor: Option<(u32, u32, u32, u32)>,
    origin: (u32, u32),
    underlay_size: (u32, u32),
) -> Option<ReplayedWrite> {
    let (origin_x, origin_y) = (origin.0 as f32, origin.1 as f32);
    let dest_quad = dest_quad.map(|[x, y]| [x - origin_x, y - origin_y]);
    let scissor = match scissor {
        None => None,
        Some((x, y, width, height)) => {
            let left = x.max(origin.0);
            let top = y.max(origin.1);
            let right = x
                .saturating_add(width)
                .min(origin.0.saturating_add(underlay_size.0));
            let bottom = y
                .saturating_add(height)
                .min(origin.1.saturating_add(underlay_size.1));
            if right <= left || bottom <= top {
                return None;
            }
            Some((left - origin.0, top - origin.1, right - left, bottom - top))
        }
    };
    Some(ReplayedWrite {
        dest_quad,
        dest_rect: axis_aligned_quad_rect(dest_quad),
        scissor,
    })
}

/// Draws the pending composites that touch `dependency_pixels` into a copy
/// taken out of the parent at `origin`, in the order they will later land on
/// the parent, without consuming them: the copy then holds what the parent
/// will show there, and the parent's own batch stays intact.
fn replay_pending_into_copy<B: SurfaceExecutionBackend>(
    backend: &mut B,
    underlay: &OffscreenTarget,
    origin: (u32, u32),
    pending_composites: &[PendingLayerComposite],
    pending_shader_composites: &[PendingShaderLayerComposite],
    dependency_pixels: Rect,
) -> Result<(), String> {
    let viewport = (underlay.width, underlay.height);
    let mut order: Vec<(usize, usize, bool, usize)> = pending_composites
        .iter()
        .enumerate()
        .filter(|(_, pending)| {
            pending_write_intersects_rect(pending.dest_quad, pending.scissor, dependency_pixels)
        })
        .map(|(index, pending)| (pending.z_index, pending.seq, false, index))
        .chain(
            pending_shader_composites
                .iter()
                .enumerate()
                .filter(|(_, pending)| {
                    pending_write_intersects_rect(
                        pending.dest_quad,
                        pending.scissor,
                        dependency_pixels,
                    )
                })
                .map(|(index, pending)| (pending.z_index, pending.seq, true, index)),
        )
        .collect();
    order.sort_by_key(|&(z_index, seq, _, _)| (z_index, seq));

    let mut items = Vec::with_capacity(order.len());
    let mut writes = Vec::with_capacity(order.len());
    for &(_, _, is_shader, index) in &order {
        let (dest_quad, scissor) = if is_shader {
            let pending = &pending_shader_composites[index];
            (pending.dest_quad, pending.scissor)
        } else {
            let pending = &pending_composites[index];
            (pending.dest_quad, pending.scissor)
        };
        let Some(write) = replayed_write(dest_quad, scissor, origin, viewport) else {
            continue;
        };
        let item = if is_shader {
            let pending = &pending_shader_composites[index];
            let source = pending.surface.target.target();
            let (origin_x, origin_y) = (origin.0 as f32, origin.1 as f32);
            let (x, y, width, height) = pending.dest_viewport;
            FusedCompositeItem::Shader(ShaderCompositeBatchItem {
                source,
                shader: &pending.shader,
                layer_pixel_rect: content_effect_pixel_rect(
                    pending.surface.effect_content_rect,
                    pending.surface.logical_rect,
                    source.width,
                    source.height,
                ),
                scissor: write.scissor,
                dest_viewport: (x - origin_x, y - origin_y, width, height),
            })
        } else {
            let pending = &pending_composites[index];
            let Some(mut item) = layer_surface_composite_batch_item(pending) else {
                return Err("a pending layer composite must be batchable".to_string());
            };
            let source = pending.surface.target.target();
            item.scissor = write.scissor;
            item.rounded_mask = write
                .dest_rect
                .and_then(|dest_rect| layer_surface_rounded_mask(&pending.surface, dest_rect));
            item.dest_viewport = write.dest_rect.map(|dest_rect| {
                composite_dest_viewport(
                    dest_rect,
                    source.width,
                    source.height,
                    pending.surface.sample_mode,
                )
            });
            FusedCompositeItem::Blit(item)
        };
        items.push(item);
        writes.push((is_shader, index, write));
    }
    if items.is_empty() {
        return Ok(());
    }
    if backend.fused_composite_batch_to_view(&underlay.view, viewport, wgpu::LoadOp::Load, &items) {
        return Ok(());
    }
    for (is_shader, index, write) in writes {
        let surface = if is_shader {
            &pending_shader_composites[index].surface
        } else {
            &pending_composites[index].surface
        };
        composite_layer_surface_to_view(
            backend,
            surface,
            &underlay.view,
            viewport,
            write.dest_quad,
            wgpu::LoadOp::Load,
            write.scissor,
        )?;
    }
    Ok(())
}

/// Copies the device pixels the child's surface will be composited onto out
/// of `source` into a fresh surface of the child's size, the exact pixels a
/// re-render of the prefix under the child would produce once everything
/// below it has been flushed into `source`. `None` when the copy would leave
/// the source, in which case the caller re-renders instead.
fn copy_child_underlay<B: SurfaceExecutionBackend>(
    backend: &mut B,
    source: &OffscreenTarget,
    origin: (u32, u32),
    logical_rect: Rect,
    scale: f32,
) -> Option<OffscreenTarget> {
    let (width, height) = surface_target_size(logical_rect, scale, backend.max_texture_dim());
    if origin.0.saturating_add(width) > source.width
        || origin.1.saturating_add(height) > source.height
    {
        return None;
    }
    let underlay = backend.acquire_frame_surface(width, height);
    if backend.copy_texture_region_to_target(source, origin, &underlay, (width, height)) {
        Some(underlay)
    } else {
        backend.release_frame_surface(underlay);
        None
    }
}

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
    !scene.effect_layers.iter().any(|layer| {
        has_backdrop_layer_in_range(&scene.backdrop_layers, layer.z_start, layer.z_end)
    })
}

#[allow(clippy::too_many_arguments)]
fn composite_captured_effect_layer<B: SurfaceExecutionBackend>(
    backend: &mut B,
    source: &OffscreenTarget,
    target_view: &wgpu::TextureView,
    layer: &EffectLayer,
    dest_quad: [[f32; 2]; 4],
    scissor: (u32, u32, u32, u32),
    capture_rect: Rect,
    effect_width: u32,
    effect_height: u32,
    width: u32,
    height: u32,
    sample_mode: CompositeSampleMode,
) -> Result<(), String> {
    let composite_plain = |backend: &mut B| {
        composite_surface_to_view(
            backend,
            source,
            target_view,
            (width, height),
            dest_quad,
            layer.composite_alpha,
            wgpu::LoadOp::Load,
            Some(scissor),
            layer.blend_mode,
            sample_mode,
        )
    };
    let Some(effect) = &layer.effect else {
        return composite_plain(backend);
    };
    if !backend.is_render_effect_supported(effect) {
        backend.warn_unsupported_effect_once();
        return composite_plain(backend);
    }

    let pixel_rect =
        content_effect_pixel_rect(Some(layer.rect), capture_rect, effect_width, effect_height);
    if let Some(dest_rect) = axis_aligned_quad_rect(dest_quad) {
        let dest_viewport = Some(composite_dest_viewport(
            dest_rect,
            effect_width,
            effect_height,
            sample_mode,
        ));
        if let RenderEffect::Shader { shader } = effect {
            backend.apply_shader_and_composite_to_view(
                source,
                shader,
                pixel_rect,
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
                source,
                effect,
                pixel_rect,
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
                source,
                shader,
                pixel_rect,
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
                source,
                effect,
                pixel_rect,
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
    let composite_result = composite_captured_effect_layer(
        backend,
        &source,
        target_view,
        &layer,
        dest_quad,
        scissor,
        capture_rect,
        effect_width,
        effect_height,
        width,
        height,
        sample_mode,
    );
    backend.release_frame_surface(source);
    composite_result
}

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
                        "root direct path does not support root-local backdrop sampling"
                            .to_string(),
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
        let mut composite_seq = 0usize;
        let mut prior_child_contributions = Vec::new();
        let mut deferred = DeferredDirectRun::new(&local_scene);
        for mut child in child_layers {
            if cursor_z < child.z_index {
                let range_has_events = range_contains_layer_events(
                    &local_scene.effect_layers,
                    &local_scene.backdrop_layers,
                    cursor_z,
                    child.z_index,
                );
                if range_has_events {
                    flush_deferred_run(
                        backend,
                        surface_view,
                        (width, height),
                        root_scale,
                        cursor_z,
                        &mut deferred,
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
                        &deferred.excluded,
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
                        root_target,
                        &local_scene,
                        cursor_z,
                        child.z_index,
                        width,
                        height,
                        root_scale,
                        &mut pending_composites,
                        &mut composite_seq,
                        &mut pending_composite_load_op,
                        &mut pending_shader_composites,
                        &mut pending_shader_load_op,
                        &mut next_load_op,
                        &mut deferred,
                        !deferred_direct_run_enabled(),
                    )?;
                    if !deferred_direct_run_enabled() {
                        next_load_op = wgpu::LoadOp::Load;
                    }
                }
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
                flush_deferred_run(
                    backend,
                    surface_view,
                    (width, height),
                    root_scale,
                    child.z_index,
                    &mut deferred,
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
                    &deferred.excluded,
                )?;
                flush_pending_clear(backend, surface_view, &mut next_load_op);
                for shadow in &resolved_child.shadow_draws {
                    backend.render_shadow_draw(surface_view, shadow, width, height, root_scale);
                }
            }

            let wants_underlay = child.needs_nested_underlay;
            let child_underlay_identity = wants_underlay
                .then(|| {
                    underlay_identity_before(
                        &local_scene,
                        &prior_child_contributions,
                        child.z_index,
                        axis_aligned_quad_rect(resolved_child.dest_quad)?,
                        (width, height),
                        root_scale,
                        None,
                        false,
                    )
                })
                .flatten();
            composite_root_child_backdrop(
                backend,
                root_target,
                surface_view,
                &local_scene,
                &prior_child_contributions,
                &child,
                &resolved_child,
                (width, height),
                root_scale,
                &mut PendingQueues {
                    composites: &mut pending_composites,
                    composite_load_op: &mut pending_composite_load_op,
                    shader_composites: &mut pending_shader_composites,
                    shader_load_op: &mut pending_shader_load_op,
                    next_load_op: &mut next_load_op,
                    composite_seq: &mut composite_seq,
                    deferred: &mut deferred,
                },
            )?;
            if child_is_bare_backdrop(&child) {
                let scissor = child
                    .visual_clip
                    .and_then(|clip| scissor_rect_for_rect(clip, root_scale, width, height));
                prior_child_contributions.push(bare_backdrop_child_contribution(
                    &child,
                    &resolved_child,
                    scissor,
                ));
                cursor_z = child.z_index.saturating_add(1);
                continue;
            }
            let mut bake_underlay = false;
            let child_underlay = if wants_underlay {
                Some(match root_target {
                    Some(root_target) => {
                        let underlay = sample_child_underlay(
                            backend,
                            root_target,
                            None,
                            &child,
                            ChildUnderlayPlacement {
                                logical_rect: resolved_child.logical_rect,
                                dest_quad: resolved_child.dest_quad,
                                snapped_dest_quad: child_dest_quad,
                                snap_anchor: resolved_child.snap_anchor,
                                composite_snap_origin: resolved_child.composite_snap_origin,
                            },
                            child_underlay_identity,
                            root_scale,
                            TranslationRenderContext::default(),
                            UnderlayCaptureSource {
                                target_view: surface_view,
                                viewport: (width, height),
                                dependency_rect: quad_bounds(child_dest_quad),
                                queues: &mut PendingQueues {
                                    composites: &mut pending_composites,
                                    composite_load_op: &mut pending_composite_load_op,
                                    shader_composites: &mut pending_shader_composites,
                                    shader_load_op: &mut pending_shader_load_op,
                                    next_load_op: &mut next_load_op,
                                    composite_seq: &mut composite_seq,
                                    deferred: &mut deferred,
                                },
                            },
                        )?;
                        bake_underlay = underlay.baked;
                        underlay.target
                    }
                    None => {
                        flush_pending_shader_layer_composites(
                            backend,
                            &mut pending_shader_composites,
                            surface_view,
                            (width, height),
                            &mut pending_shader_load_op,
                            &mut next_load_op,
                        )?;
                        create_direct_root_child_underlay(
                            backend,
                            &local_scene,
                            resolved_child.logical_rect,
                            resolved_child.dest_quad,
                            child.z_index,
                            &pending_composites,
                            root_scale,
                        )?
                    }
                })
            } else {
                None
            };

            let child_surface_result = render_layer_surface(
                backend,
                &mut child,
                LayerSurfaceRequest {
                    root_scale,
                    backdrop_underlay: child_underlay.as_ref(),
                    backdrop_underlay_identity: child_underlay_identity,
                    bake_underlay,
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
            if child_backdrop_reads_target && !resolved_child.shadow_draws.is_empty() {
                if backdrop_diag_enabled() {
                    eprintln!(
                        "[backdrop-diag] shadow-flush node={:?} shadows={} pending={} shader_pending={}",
                        child.node_id,
                        resolved_child.shadow_draws.len(),
                        pending_composites.len(),
                        pending_shader_composites.len()
                    );
                }
                flush_deferred_run(
                    backend,
                    surface_view,
                    (width, height),
                    root_scale,
                    child.z_index,
                    &mut deferred,
                    &mut pending_composites,
                    &mut pending_composite_load_op,
                    &mut pending_shader_composites,
                    &mut pending_shader_load_op,
                    &mut next_load_op,
                )?;
                flush_pending_composite_queues_fused(
                    backend,
                    &mut pending_composites,
                    &mut pending_composite_load_op,
                    &mut pending_shader_composites,
                    &mut pending_shader_load_op,
                    surface_view,
                    (width, height),
                    &mut next_load_op,
                )?;
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
            let dest_quad = texel_aligned_dest_quad(
                dest_quad,
                {
                    let target = child_surface.target.target();
                    (target.width, target.height)
                },
                resolved_child.snap_anchor,
                child_surface_target_scale(&child, root_scale, TranslationRenderContext::default()),
                root_scale,
            );
            let scissor = child
                .visual_clip
                .and_then(|clip| scissor_rect_for_rect(clip, root_scale, width, height));
            let child_prefix_contribution = backdrop_prefix_child_contribution(
                &child,
                &child_surface,
                dest_quad,
                scissor,
                child_underlay_identity,
            );
            if child_surface.deferred_effect.is_none()
                && axis_aligned_quad_rect(dest_quad).is_some()
            {
                if pending_composites.is_empty() {
                    pending_composite_load_op = Some(next_load_op);
                }
                pending_composites.push(PendingLayerComposite {
                    z_index: child.z_index,
                    seq: next_composite_seq(&mut composite_seq),
                    surface: child_surface,
                    dest_quad,
                    scissor,
                });
                next_load_op = wgpu::LoadOp::Load;
            } else {
                match direct_shader_layer_composite(
                    child_surface,
                    child.z_index,
                    next_composite_seq(&mut composite_seq),
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
                        flush_deferred_run(
                            backend,
                            surface_view,
                            (width, height),
                            root_scale,
                            child.z_index,
                            &mut deferred,
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
                            &deferred.excluded,
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

        finish_root_ranges(
            backend,
            surface_view,
            root_target,
            &local_scene,
            &prior_child_contributions,
            cursor_z,
            width,
            height,
            root_scale,
            &mut deferred,
            &mut pending_composites,
            &mut composite_seq,
            &mut pending_composite_load_op,
            &mut pending_shader_composites,
            &mut pending_shader_load_op,
            &mut next_load_op,
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(local_scene),
        Err(error) => Err((error, local_scene)),
    }
}

/// Draws whatever the root walk left after its last child: the trailing
/// range, the deferred run and every pending composite, as one fused pass
/// where the scene allows it.
#[allow(clippy::too_many_arguments)]
fn finish_root_ranges<B: SurfaceExecutionBackend>(
    backend: &mut B,
    surface_view: &wgpu::TextureView,
    root_target: Option<&OffscreenTarget>,
    local_scene: &CompositorScene,
    prior_child_contributions: &[BackdropPrefixChildContribution],
    cursor_z: usize,
    width: u32,
    height: u32,
    root_scale: f32,
    deferred: &mut DeferredDirectRun<'_>,
    pending_composites: &mut Vec<PendingLayerComposite>,
    composite_seq: &mut usize,
    pending_composite_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    pending_shader_composites: &mut Vec<PendingShaderLayerComposite>,
    pending_shader_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
) -> Result<(), String> {
    if cursor_z < local_scene.next_z {
        let range_has_events = range_contains_layer_events(
            &local_scene.effect_layers,
            &local_scene.backdrop_layers,
            cursor_z,
            local_scene.next_z,
        );
        if range_has_events {
            flush_deferred_run(
                backend,
                surface_view,
                (width, height),
                root_scale,
                cursor_z,
                deferred,
                pending_composites,
                pending_composite_load_op,
                pending_shader_composites,
                pending_shader_load_op,
                next_load_op,
            )?;
            render_non_effect_range_with_pending_composites(
                backend,
                surface_view,
                local_scene,
                cursor_z,
                cursor_z,
                width,
                height,
                root_scale,
                pending_composites,
                pending_composite_load_op,
                pending_shader_composites,
                pending_shader_load_op,
                next_load_op,
                &deferred.excluded,
            )?;
            let backdrop_input_hashes = scene_backdrop_input_hashes(
                local_scene,
                prior_child_contributions,
                (width, height),
                root_scale,
            );
            render_range_with_layer_events_to_view(
                backend,
                surface_view,
                root_target,
                &backdrop_input_hashes,
                local_scene,
                cursor_z,
                local_scene.next_z,
                None,
                width,
                height,
                root_scale,
                *next_load_op,
            )?;
        } else {
            render_direct_scene_range_with_pending_composites(
                backend,
                surface_view,
                root_target,
                local_scene,
                cursor_z,
                local_scene.next_z,
                width,
                height,
                root_scale,
                pending_composites,
                composite_seq,
                pending_composite_load_op,
                pending_shader_composites,
                pending_shader_load_op,
                next_load_op,
                deferred,
                true,
            )?;
            render_non_effect_range_with_pending_composites(
                backend,
                surface_view,
                local_scene,
                local_scene.next_z,
                local_scene.next_z,
                width,
                height,
                root_scale,
                pending_composites,
                pending_composite_load_op,
                pending_shader_composites,
                pending_shader_load_op,
                next_load_op,
                &deferred.excluded,
            )?;
        }
    } else {
        flush_deferred_run(
            backend,
            surface_view,
            (width, height),
            root_scale,
            local_scene.next_z,
            deferred,
            pending_composites,
            pending_composite_load_op,
            pending_shader_composites,
            pending_shader_load_op,
            next_load_op,
        )?;
        if matches!(*next_load_op, wgpu::LoadOp::Clear(_)) {
            backend.clear_target_view_with_load_op(surface_view, *next_load_op);
        } else {
            render_non_effect_range_with_pending_composites(
                backend,
                surface_view,
                local_scene,
                local_scene.next_z,
                local_scene.next_z,
                width,
                height,
                root_scale,
                pending_composites,
                pending_composite_load_op,
                pending_shader_composites,
                pending_shader_load_op,
                next_load_op,
                &deferred.excluded,
            )?;
        }
    }

    Ok(())
}

pub(crate) fn render_layer_surface<B: SurfaceExecutionBackend>(
    backend: &mut B,
    child: &mut ChildLayerComposite,
    request: LayerSurfaceRequest<'_>,
) -> Result<LayerSurface, String> {
    let LayerSurfaceRequest {
        root_scale,
        backdrop_underlay,
        backdrop_underlay_identity,
        bake_underlay,
        allow_runtime_cache,
        logical_rect_override,
        capture_clip_override,
        activates_nested_capture,
        translation_context,
    } = request;
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
    let supported_isolation_effect = child
        .isolation
        .as_ref()
        .and_then(|params| params.effect.as_ref())
        .filter(|effect| backend.is_render_effect_supported(effect));
    let cache_candidate = child_layer_raster_cache_candidate(
        child,
        target_scale,
        effective_requirements,
        supported_isolation_effect,
        backdrop_underlay.is_some(),
        backdrop_underlay_identity,
        allow_runtime_cache,
        logical_rect_override,
        backend.max_texture_dim(),
    );
    if let Some((cache_key, logical_rect)) = cache_candidate {
        if backdrop_diag_enabled() {
            eprintln!(
                "[backdrop-diag] probe node={:?} identity={:?} key={:?} bake={}",
                child.node_id, backdrop_underlay_identity, cache_key, bake_underlay
            );
        }
        if let Some((target, logical_rect)) = backend.cached_layer_surface(&cache_key) {
            if backdrop_diag_enabled() {
                eprintln!("[backdrop-diag] probe HIT node={:?}", child.node_id);
            }
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
        let cache_candidate = backend
            .admit_layer_surface_cache_miss(&cache_key, surface_cache_admission(child))
            .then(|| {
                record_layer_cache_miss(backend, "child-candidate", &cache_key, width, height);
                (cache_key, logical_rect)
            });
        return render_layer_surface_uncached(
            backend,
            child,
            source,
            LayerSurfaceRenderOptions {
                target_scale,
                backdrop_underlay,
                backdrop_underlay_identity,
                bake_underlay,
                allow_runtime_cache,
                cache_candidate,
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
            backdrop_underlay_identity,
            bake_underlay,
            allow_runtime_cache,
            cache_candidate: None,
            logical_rect_override,
            capture_clip_override,
            composite_sample_mode,
            translation_context,
        },
    )
}

/// A surface whose subtree carries a runtime shader can change every frame
/// through uniforms alone, so it earns a cache slot only once its key repeats;
/// every other surface caches on its first miss.
fn surface_cache_admission(child: &ChildLayerComposite) -> CacheAdmission {
    if child.surface_requirements.contains_runtime_shader {
        CacheAdmission::OnRepeat
    } else {
        CacheAdmission::Immediate
    }
}

#[allow(clippy::too_many_arguments)]
fn child_layer_raster_cache_candidate(
    child: &ChildLayerComposite,
    root_scale: f32,
    effective_requirements: SurfaceRequirementSet,
    supported_isolation_effect: Option<&RenderEffect>,
    has_backdrop_underlay: bool,
    backdrop_underlay_identity: Option<u64>,
    allow_runtime_cache: bool,
    logical_rect_override: Option<Rect>,
    max_texture_dim: u32,
) -> Option<(LayerRasterCacheKey, Rect)> {
    let surface_requirements = child.surface_requirements;
    if let Some(source) = layer_source_cache_entry(
        child,
        effective_requirements,
        has_backdrop_underlay,
        backdrop_underlay_identity,
        allow_runtime_cache,
    ) {
        match supported_isolation_effect {
            None => {
                let content_hash = source.collected_content_hash?;
                let logical_rect = logical_rect_override.unwrap_or(child.logical_rect);
                let pixel_size = surface_target_size(logical_rect, root_scale, max_texture_dim);
                if offscreen_byte_size(pixel_size.0, pixel_size.1) > MAX_LAYER_SURFACE_CACHE_BYTES {
                    return None;
                }
                return Some((
                    source.key(content_hash, logical_rect, pixel_size, root_scale),
                    logical_rect,
                ));
            }
            Some(effect) if can_materialize_cached_effect(effect, child.backdrop.as_ref()) => {}
            Some(_) => {
                return None;
            }
        }
    }

    let runtime_cache_is_safe = allow_runtime_cache
        && surface_requirements
            .surface_requirements
            .has_isolating_requirement();
    let cache_is_allowed = child.cache_policy == CachePolicy::Auto
        || (allow_runtime_cache && surface_requirements.has_renderer_forced_surface())
        || runtime_cache_is_safe;
    if !cache_is_allowed {
        return None;
    }
    let external_backdrop_input = has_backdrop_underlay && child.contains_descendant_backdrop;
    if external_backdrop_input && backdrop_underlay_identity.is_none() {
        return None;
    }

    let logical_rect = logical_rect_override.unwrap_or(child.logical_rect);
    let pixel_size = surface_target_size(logical_rect, root_scale, max_texture_dim);
    if offscreen_byte_size(pixel_size.0, pixel_size.1) > MAX_LAYER_SURFACE_CACHE_BYTES {
        return None;
    }
    let content_hash = if external_backdrop_input {
        content_hash_over_underlay(child.target_content_hash, backdrop_underlay_identity)
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

    if split_backdrop_effect(effect).1.is_none()
        && backend.materialize_effect_direct(source, effect, layer_pixel_rect, target)?
    {
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

struct LayerSourceCacheEntry {
    stable_id: Option<NodeId>,
    collected_content_hash: Option<u64>,
}

impl LayerSourceCacheEntry {
    fn key(
        &self,
        content_hash: u64,
        rect: Rect,
        pixel_size: (u32, u32),
        scale: f32,
    ) -> LayerRasterCacheKey {
        LayerRasterCacheKey::source_content(
            self.stable_id,
            content_hash,
            rect,
            pixel_size,
            ScaleBucket::from_scale(scale),
        )
    }
}

fn content_hash_over_underlay(content_hash: u64, underlay_identity: Option<u64>) -> u64 {
    let mut hasher = FxHasher::default();
    0xF1A7_C010u64.hash(&mut hasher);
    content_hash.hash(&mut hasher);
    underlay_identity.hash(&mut hasher);
    hasher.finish()
}

fn layer_source_cache_entry(
    child: &ChildLayerComposite,
    effective_requirements: SurfaceRequirementSet,
    has_backdrop_underlay: bool,
    backdrop_underlay_identity: Option<u64>,
    allow_runtime_cache: bool,
) -> Option<LayerSourceCacheEntry> {
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
    let reads_external_underlay = has_backdrop_underlay && child.contains_descendant_backdrop;
    if !cache_is_allowed || (reads_external_underlay && backdrop_underlay_identity.is_none()) {
        return None;
    }
    let content_hash = if reads_external_underlay {
        content_hash_over_underlay(child.target_content_hash, backdrop_underlay_identity)
    } else {
        child.target_content_hash
    };

    Some(LayerSourceCacheEntry {
        stable_id: child.node_id,
        // A motion-stable destination can reuse its geometry while it moves,
        // but its source pixels remain live input for backdrop effects. A
        // source cache keyed without the translated content hash freezes a
        // glass pane over scrolling content.
        collected_content_hash: Some(content_hash),
    })
}

#[allow(clippy::too_many_arguments)]
fn layer_source_cache_key(
    child: &ChildLayerComposite,
    effective_requirements: SurfaceRequirementSet,
    surface_rect: Rect,
    pixel_size: (u32, u32),
    target_scale: f32,
    has_backdrop_underlay: bool,
    backdrop_underlay_identity: Option<u64>,
    allow_runtime_cache: bool,
) -> Option<LayerRasterCacheKey> {
    let source = layer_source_cache_entry(
        child,
        effective_requirements,
        has_backdrop_underlay,
        backdrop_underlay_identity,
        allow_runtime_cache,
    )?;
    let content_hash = source
        .collected_content_hash
        .expect("cacheable layer sources retain their live content hash");
    Some(source.key(content_hash, surface_rect, pixel_size, target_scale))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BackdropPrefixChildContribution {
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
    underlay_identity: Option<u64>,
) -> BackdropPrefixChildContribution {
    let content_hash = match underlay_identity {
        Some(_) if child.contains_descendant_backdrop => {
            content_hash_over_underlay(child.target_content_hash, underlay_identity)
        }
        _ => child.target_content_hash,
    };
    if backdrop_diag_enabled() {
        eprintln!(
            "[backdrop-diag] contribution node={:?} z={} content_hash={} target_content_hash={} identity={:?} dest0={:?}",
            child.node_id,
            child.z_index,
            content_hash,
            child.target_content_hash,
            underlay_identity,
            dest_quad[0]
        );
    }
    BackdropPrefixChildContribution {
        z_index: child.z_index,
        node_id: child.node_id,
        content_hash,
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

fn child_is_bare_backdrop(child: &ChildLayerComposite) -> bool {
    child.backdrop.is_some() && child.source.is_empty()
}

/// The contribution of a backdrop child that draws nothing over its effect
/// output: only the effect output reaches later captures, so no surface is
/// rendered or composited for it.
fn bare_backdrop_child_contribution(
    child: &ChildLayerComposite,
    resolved_child: &ResolvedChildSurfaceComposite,
    scissor: Option<(u32, u32, u32, u32)>,
) -> BackdropPrefixChildContribution {
    BackdropPrefixChildContribution {
        z_index: child.z_index,
        node_id: child.node_id,
        content_hash: child.target_content_hash,
        effect_hash: child.effect_hash,
        backdrop_hash: child
            .backdrop
            .as_ref()
            .map(retained_render_effect_hash)
            .unwrap_or(0),
        deferred_effect_hash: 0,
        logical_rect: resolved_child.logical_rect,
        dest_quad: resolved_child.dest_quad,
        scissor,
        composite_alpha_bits: 1.0f32.to_bits(),
        blend_mode: BlendMode::SrcOver,
        sample_mode: CompositeSampleMode::Linear,
    }
}

#[cfg(test)]
fn backdrop_effect_cache_key(
    layer: &BackdropLayer,
    input_content_hash: u64,
    visible_rect: Rect,
    pixel_size: (u32, u32),
    root_scale: f32,
) -> Option<LayerRasterCacheKey> {
    backdrop_effect_cache_key_for_effect_hash(
        layer,
        input_content_hash,
        retained_render_effect_hash(&layer.effect),
        visible_rect,
        pixel_size,
        root_scale,
    )
}

fn backdrop_effect_cache_key_for_effect_hash(
    layer: &BackdropLayer,
    input_content_hash: u64,
    effect_hash: u64,
    visible_rect: Rect,
    pixel_size: (u32, u32),
    root_scale: f32,
) -> Option<LayerRasterCacheKey> {
    Some(LayerRasterCacheKey::backdrop_effect(
        layer.node_id,
        input_content_hash,
        effect_hash,
        visible_rect,
        pixel_size,
        ScaleBucket::from_scale(root_scale),
    ))
}

fn split_backdrop_effect(effect: &RenderEffect) -> (Option<&RenderEffect>, Option<&RuntimeShader>) {
    match effect {
        RenderEffect::Shader { shader } => (None, Some(shader)),
        RenderEffect::Chain { first, second } => match second.as_ref() {
            RenderEffect::Shader { shader } => (Some(first.as_ref()), Some(shader)),
            _ => (Some(effect), None),
        },
        _ => (Some(effect), None),
    }
}

fn retained_render_effect_hash(effect: &RenderEffect) -> u64 {
    effect.render_hash()
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

    let diag = backdrop_diag_enabled();
    let mut draw_op_count = 0usize;
    for draw_op in scene_range_draw_ops(&scene.draw_ops, 0, z_end)
        .iter()
        .filter(|draw_op| {
            draw_op_visible_bounds(scene, **draw_op)
                .is_none_or(|bounds| rects_intersect(bounds, capture_rect))
        })
    {
        0u8.hash(&mut hasher);
        hash_draw_op(scene, draw_op, &mut hasher);
        draw_op_count += 1;
    }
    let after_draw_ops = hasher.finish();
    let mut effect_layer_count = 0usize;
    for effect_layer in scene
        .effect_layers
        .iter()
        .filter(|layer| layer.z_end <= z_end && rects_intersect(layer.rect, capture_rect))
    {
        1u8.hash(&mut hasher);
        hash_effect_layer(effect_layer, &mut hasher);
        effect_layer_count += 1;
    }
    let after_effect_layers = hasher.finish();
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
    let after_backdrop_layers = hasher.finish();
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
    let result = hasher.finish();
    if diag {
        eprintln!(
            "[backdrop-diag] prefix-hash z_end={z_end} capture=({:.1},{:.1}) ops={draw_op_count}:{after_draw_ops} effects={effect_layer_count}:{after_effect_layers} backdrops={after_backdrop_layers} full={result}",
            capture_rect.x, capture_rect.y
        );
    }
    result
}

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
    shadow.rounded_clip.is_some().hash(state);
    if let Some(rounded_clip) = shadow.rounded_clip {
        hash_rect(rounded_clip.rect, state);
        for radius in rounded_clip.radii {
            hash_f32_bits(radius, state);
        }
    }
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

fn next_composite_seq(counter: &mut usize) -> usize {
    let seq = *counter;
    *counter += 1;
    seq
}

struct PendingLayerComposite {
    z_index: usize,
    seq: usize,
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
    seq: usize,
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
    seq: usize,
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
        seq,
        surface,
        shader,
        dest_quad,
        scissor,
        dest_viewport,
    })
}

fn pending_shader_layer_composite_batch_items(
    pending: &[PendingShaderLayerComposite],
) -> Vec<(usize, usize, ShaderCompositeBatchItem<'_>)> {
    pending
        .iter()
        .map(|pending| {
            let source = pending.surface.target.target();
            (
                pending.z_index,
                pending.seq,
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
        .map(|(_, _, item)| item)
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
) -> Vec<(usize, usize, CompositeBatchItem<'_>)> {
    pending
        .iter()
        .map(|pending| {
            (
                pending.z_index,
                pending.seq,
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

fn pending_write_intersects_rect(
    dest_quad: [[f32; 2]; 4],
    scissor: Option<(u32, u32, u32, u32)>,
    rect: Rect,
) -> bool {
    if !dest_quad_intersects_rect(dest_quad, rect) {
        return false;
    }
    scissor.is_none_or(|(x, y, width, height)| {
        rects_intersect(
            Rect {
                x: x as f32,
                y: y as f32,
                width: width as f32,
                height: height as f32,
            },
            rect,
        )
    })
}

fn pending_load_op_holds_clear(load_op: &Option<wgpu::LoadOp<wgpu::Color>>) -> bool {
    matches!(load_op, Some(wgpu::LoadOp::Clear(_)))
}

fn backdrop_dependency_rect(
    effect_rect: Rect,
    clip: Option<Rect>,
    effect: &RenderEffect,
    root_scale: f32,
    target_size: (u32, u32),
) -> Option<Rect> {
    let visible_rect =
        visible_layer_rect(effect_rect, clip, root_scale, target_size.0, target_size.1)?;
    let capture = backdrop_capture_rect(visible_rect, clip, effect, root_scale, target_size);
    let output = backdrop_output_rect(visible_rect, clip, effect, root_scale, target_size);
    union_rect(Some(capture), output)
}

/// The pending writes a capture of `dependency_pixels` would have to see
/// first: composites and shader composites whose destination touches the
/// rect, and a clear one of the queues still holds, which means the target
/// carries nothing yet.
struct CaptureConflicts {
    composites: usize,
    shaders: usize,
    clear_held: bool,
}

impl CaptureConflicts {
    fn any(&self) -> bool {
        self.composites > 0 || self.shaders > 0 || self.clear_held
    }
}

fn pending_capture_conflicts(
    pending_composites: &[PendingLayerComposite],
    pending_composite_load_op: &Option<wgpu::LoadOp<wgpu::Color>>,
    pending_shader_composites: &[PendingShaderLayerComposite],
    pending_shader_load_op: &Option<wgpu::LoadOp<wgpu::Color>>,
    dependency_pixels: Rect,
) -> CaptureConflicts {
    CaptureConflicts {
        composites: pending_composites
            .iter()
            .filter(|pending| {
                pending_write_intersects_rect(pending.dest_quad, pending.scissor, dependency_pixels)
            })
            .count(),
        shaders: pending_shader_composites
            .iter()
            .filter(|pending| {
                pending_write_intersects_rect(pending.dest_quad, pending.scissor, dependency_pixels)
            })
            .count(),
        clear_held: pending_load_op_holds_clear(pending_composite_load_op)
            || pending_load_op_holds_clear(pending_shader_load_op),
    }
}

#[allow(clippy::too_many_arguments)]
fn flush_pending_queues_for_backdrop_capture<B: SurfaceExecutionBackend>(
    backend: &mut B,
    pending_composites: &mut Vec<PendingLayerComposite>,
    pending_composite_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    pending_shader_composites: &mut Vec<PendingShaderLayerComposite>,
    pending_shader_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    target_view: &wgpu::TextureView,
    viewport: (u32, u32),
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
    dependency_rect: Rect,
    root_scale: f32,
) -> Result<(), String> {
    let dependency_pixels = surface_pixel_rect(dependency_rect, root_scale);
    let conflicts = pending_capture_conflicts(
        pending_composites,
        pending_composite_load_op,
        pending_shader_composites,
        pending_shader_load_op,
        dependency_pixels,
    );
    let must_flush = conflicts.any();
    if backdrop_diag_enabled() {
        eprintln!(
            "[backdrop-diag] capture-flush must={must_flush} dep={:?} pending={} hit={} shader_pending={} shader_hit={} clear_held={}",
            dependency_pixels,
            pending_composites.len(),
            conflicts.composites,
            pending_shader_composites.len(),
            conflicts.shaders,
            conflicts.clear_held
        );
    }
    if must_flush {
        flush_pending_composite_queues_fused(
            backend,
            pending_composites,
            pending_composite_load_op,
            pending_shader_composites,
            pending_shader_load_op,
            target_view,
            viewport,
            next_load_op,
        )?;
    }
    flush_pending_clear(backend, target_view, next_load_op);
    Ok(())
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
            .map(|(_, _, item)| item)
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

/// The composite queues a render loop batches between passes, lent to the
/// helpers that push into them or flush them.
struct PendingQueues<'a, 's> {
    composites: &'a mut Vec<PendingLayerComposite>,
    composite_load_op: &'a mut Option<wgpu::LoadOp<wgpu::Color>>,
    shader_composites: &'a mut Vec<PendingShaderLayerComposite>,
    shader_load_op: &'a mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &'a mut wgpu::LoadOp<wgpu::Color>,
    composite_seq: &'a mut usize,
    deferred: &'a mut DeferredDirectRun<'s>,
}

/// The direct draw ops of a scene that have been walked but not drawn: they
/// stay pending alongside the composites so that one fused pass draws every
/// z-ordered range and composite together, instead of one pass per child.
/// A capture that reads their pixels, an immediate draw that must land above
/// them, or the end of the walk flushes the run.
struct DeferredDirectRun<'s> {
    scene: &'s CompositorScene,
    run: DirectChunkRunCoalescer,
    excluded: Vec<Range<usize>>,
}

impl<'s> DeferredDirectRun<'s> {
    fn new(scene: &'s CompositorScene) -> Self {
        Self {
            scene,
            run: DirectChunkRunCoalescer::default(),
            excluded: Vec::new(),
        }
    }

    /// Takes the draw op at `z_index` out of the run: it is drawn as a pending
    /// composite instead, so segments and dependency checks skip it.
    fn exclude(&mut self, z_index: usize) {
        self.excluded.push(z_index..z_index.saturating_add(1));
    }

    fn next_excluded_after(&self, z_index: usize) -> Option<usize> {
        self.excluded
            .iter()
            .map(|range| range.start)
            .filter(|start| *start > z_index)
            .min()
    }

    fn intersects(&self, dependency_rect: Rect, boundary: usize) -> bool {
        self.run.peek(boundary).is_some_and(|(start, end)| {
            deferred_range_bounds(self.scene, start, end, &self.excluded)
                .is_some_and(|bounds| rects_intersect(bounds, dependency_rect))
        })
    }
}

/// Draws the deferred run up to `boundary` fused with every pending composite,
/// so nothing drawn into the target afterwards can land beneath it.
#[allow(clippy::too_many_arguments)]
fn flush_deferred_run<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target_view: &wgpu::TextureView,
    viewport: (u32, u32),
    root_scale: f32,
    boundary: usize,
    deferred: &mut DeferredDirectRun<'_>,
    pending_composites: &mut Vec<PendingLayerComposite>,
    pending_composite_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    pending_shader_composites: &mut Vec<PendingShaderLayerComposite>,
    pending_shader_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
) -> Result<(), String> {
    let Some((run_start, run_end)) = deferred.run.flush_at(boundary) else {
        return Ok(());
    };
    render_non_effect_range_with_pending_composites(
        backend,
        target_view,
        deferred.scene,
        run_start,
        run_end,
        viewport.0,
        viewport.1,
        root_scale,
        pending_composites,
        pending_composite_load_op,
        pending_shader_composites,
        pending_shader_load_op,
        next_load_op,
        &deferred.excluded,
    )?;
    deferred.excluded.retain(|range| range.start >= run_end);
    Ok(())
}

fn flush_deferred_run_for_dependency<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target_view: &wgpu::TextureView,
    viewport: (u32, u32),
    root_scale: f32,
    dependency_rect: Rect,
    boundary: usize,
    queues: &mut PendingQueues<'_, '_>,
) -> Result<(), String> {
    if !queues.deferred.intersects(dependency_rect, boundary) {
        return Ok(());
    }
    flush_deferred_run(
        backend,
        target_view,
        viewport,
        root_scale,
        boundary,
        queues.deferred,
        queues.composites,
        queues.composite_load_op,
        queues.shader_composites,
        queues.shader_load_op,
        queues.next_load_op,
    )
}

/// Composites a root child's own backdrop into the root target: capture,
/// effect and either a pending composite, a deferred shader composite or a
/// direct application, all ahead of the child's surface so an underlay
/// sampled beneath the child already carries it. A child whose surface bakes
/// an underlay gets the effect materialized rather than deferred: its
/// composite is then replayed into the underlay copy and drawn onto the root
/// as two blits of one effect result instead of two shader runs.
#[allow(clippy::too_many_arguments)]
fn composite_root_child_backdrop<B: SurfaceExecutionBackend>(
    backend: &mut B,
    root_target: Option<&OffscreenTarget>,
    surface_view: &wgpu::TextureView,
    local_scene: &CompositorScene,
    prior_child_contributions: &[BackdropPrefixChildContribution],
    child: &ChildLayerComposite,
    resolved_child: &ResolvedChildSurfaceComposite,
    viewport: (u32, u32),
    root_scale: f32,
    queues: &mut PendingQueues<'_, '_>,
) -> Result<(), String> {
    let (width, height) = viewport;
    if let Some(backdrop) = child.backdrop.clone() {
        let Some(root_target) = root_target else {
            return Err("root direct path does not support backdrop child surfaces".to_string());
        };
        let backdrop_layer = BackdropLayer {
            node_id: child.node_id,
            rect: resolved_child.backdrop_rect,
            clip: child.visual_clip,
            snap_anchor: resolved_child.snap_anchor,
            effect: backdrop,
            z_index: child.z_index,
        };
        let replay_pixels = match backdrop_dependency_rect(
            resolved_child.backdrop_rect,
            child.visual_clip,
            &backdrop_layer.effect,
            root_scale,
            (width, height),
        ) {
            Some(dependency_rect) => prepare_capture_source(
                backend,
                root_target,
                &backdrop_layer,
                dependency_rect,
                (width, height),
                root_scale,
                queues,
            )?,
            None => None,
        };
        let backdrop_input_hash = child_backdrop_input_hash(
            local_scene,
            prior_child_contributions,
            child,
            resolved_child,
            &backdrop_layer.effect,
            (width, height),
            root_scale,
        );
        if backdrop_diag_enabled() {
            eprintln!(
                "[backdrop-diag] root backdrop node={:?} input_hash={} contributions={}",
                child.node_id,
                backdrop_input_hash,
                prior_child_contributions.len()
            );
        }
        if let Some(prepared) = prepare_cached_backdrop_layer_composite(
            backend,
            root_target,
            &backdrop_layer,
            None,
            width,
            height,
            root_scale,
            Some(backdrop_input_hash),
            !child.needs_nested_underlay,
            replay_pixels.map(|dependency_pixels| PendingReplaySource {
                composites: queues.composites,
                shader_composites: queues.shader_composites,
                dependency_pixels,
            }),
        )? {
            let PreparedBackdropComposite {
                surface: prepared_surface,
                dest_quad: prepared_dest_quad,
                scissor: prepared_scissor,
            } = prepared;
            if prepared_surface.deferred_effect.is_some() {
                match direct_shader_layer_composite(
                    prepared_surface,
                    child.z_index,
                    next_composite_seq(queues.composite_seq),
                    prepared_dest_quad,
                    prepared_scissor,
                ) {
                    Ok(pending) => {
                        if queues.shader_composites.is_empty() {
                            *queues.shader_load_op = Some(*queues.next_load_op);
                        }
                        queues.shader_composites.push(pending);
                        *queues.next_load_op = wgpu::LoadOp::Load;
                    }
                    Err(prepared_surface) => {
                        composite_layer_surface_to_view(
                            backend,
                            &prepared_surface,
                            surface_view,
                            (width, height),
                            prepared_dest_quad,
                            *queues.next_load_op,
                            prepared_scissor,
                        )?;
                        *queues.next_load_op = wgpu::LoadOp::Load;
                        backend.release_layer_surface_target(prepared_surface.target);
                    }
                }
            } else {
                if queues.composites.is_empty() {
                    *queues.composite_load_op = Some(*queues.next_load_op);
                }
                queues.composites.push(PendingLayerComposite {
                    z_index: child.z_index,
                    seq: next_composite_seq(queues.composite_seq),
                    surface: prepared_surface,
                    dest_quad: prepared_dest_quad,
                    scissor: prepared_scissor,
                });
                *queues.next_load_op = wgpu::LoadOp::Load;
            }
        } else {
            if replay_pixels.is_some() {
                flush_pending_composite_queues_fused(
                    backend,
                    queues.composites,
                    queues.composite_load_op,
                    queues.shader_composites,
                    queues.shader_load_op,
                    surface_view,
                    (width, height),
                    queues.next_load_op,
                )?;
            }
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
            if !queues.composites.is_empty() {
                *queues.composite_load_op = Some(wgpu::LoadOp::Load);
            }
        }
    }
    Ok(())
}

struct NestedBackdropContext<'a> {
    backdrop_underlay: Option<&'a OffscreenTarget>,
    baked: bool,
    underlay_identity: Option<u64>,
}

/// The nested counterpart of [`composite_root_child_backdrop`]: the child's
/// own backdrop goes into the enclosing surface's target, reading through the
/// surface's external underlay unless local content covers it, and keyed on
/// the baked underlay identity when the surface bakes one.
#[allow(clippy::too_many_arguments)]
fn composite_nested_child_backdrop<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target: &OffscreenTarget,
    local_scene: &CompositorScene,
    prior_child_contributions: &[BackdropPrefixChildContribution],
    child: &ChildLayerComposite,
    resolved_child: &ResolvedChildSurfaceComposite,
    viewport: (u32, u32),
    target_scale: f32,
    context: NestedBackdropContext<'_>,
    queues: &mut PendingQueues<'_, '_>,
) -> Result<(), String> {
    let (width, height) = viewport;

    if !resolved_child.shadow_draws.is_empty() {
        flush_pending_composite_queues_fused(
            backend,
            queues.composites,
            queues.composite_load_op,
            queues.shader_composites,
            queues.shader_load_op,
            &target.view,
            (width, height),
            queues.next_load_op,
        )?;
        flush_pending_clear(backend, &target.view, queues.next_load_op);
    }

    if let Some(backdrop) = &child.backdrop {
        let backdrop_layer = BackdropLayer {
            node_id: child.node_id,
            rect: resolved_child.backdrop_rect,
            clip: child.visual_clip,
            snap_anchor: resolved_child.snap_anchor,
            effect: backdrop.clone(),
            z_index: child.z_index,
        };
        let replay_pixels = match backdrop_dependency_rect(
            resolved_child.backdrop_rect,
            child.visual_clip,
            backdrop,
            target_scale,
            (width, height),
        ) {
            Some(dependency_rect) => prepare_capture_source(
                backend,
                target,
                &backdrop_layer,
                dependency_rect,
                (width, height),
                target_scale,
                queues,
            )?,
            None => None,
        };
        let backdrop_input_hash = nested_child_backdrop_input_hash(
            local_scene,
            prior_child_contributions,
            child,
            resolved_child,
            &backdrop_layer.effect,
            (width, height),
            target_scale,
            context.baked,
            context.underlay_identity,
        );
        let effective_backdrop_underlay = if context.backdrop_underlay.is_some()
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
            context.backdrop_underlay
        };
        if let Some(prepared) = prepare_cached_backdrop_layer_composite(
            backend,
            target,
            &backdrop_layer,
            effective_backdrop_underlay,
            width,
            height,
            target_scale,
            Some(backdrop_input_hash),
            false,
            replay_pixels.map(|dependency_pixels| PendingReplaySource {
                composites: queues.composites,
                shader_composites: queues.shader_composites,
                dependency_pixels,
            }),
        )? {
            if queues.composites.is_empty() {
                *queues.composite_load_op = Some(*queues.next_load_op);
            }
            queues.composites.push(PendingLayerComposite {
                z_index: child.z_index,
                seq: next_composite_seq(queues.composite_seq),
                surface: prepared.surface,
                dest_quad: prepared.dest_quad,
                scissor: prepared.scissor,
            });
            *queues.next_load_op = wgpu::LoadOp::Load;
        } else {
            if replay_pixels.is_some() {
                flush_pending_composite_queues_fused(
                    backend,
                    queues.composites,
                    queues.composite_load_op,
                    queues.shader_composites,
                    queues.shader_load_op,
                    &target.view,
                    (width, height),
                    queues.next_load_op,
                )?;
            }
            apply_backdrop_layer_to_target(
                backend,
                target,
                &backdrop_layer,
                effective_backdrop_underlay,
                width,
                height,
                target_scale,
                Some(backdrop_input_hash),
            )?;
            if !queues.composites.is_empty() {
                *queues.composite_load_op = Some(wgpu::LoadOp::Load);
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn flush_pending_composite_queues_fused<B: SurfaceExecutionBackend>(
    backend: &mut B,
    pending_composites: &mut Vec<PendingLayerComposite>,
    pending_composite_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    pending_shader_composites: &mut Vec<PendingShaderLayerComposite>,
    pending_shader_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    target_view: &wgpu::TextureView,
    viewport: (u32, u32),
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
) -> Result<(), String> {
    if pending_shader_composites.is_empty() {
        flush_pending_layer_composites(
            backend,
            pending_composites,
            target_view,
            viewport,
            pending_composite_load_op,
            next_load_op,
        );
        return Ok(());
    }
    if pending_composites.is_empty() {
        return flush_pending_shader_layer_composites(
            backend,
            pending_shader_composites,
            target_view,
            viewport,
            pending_shader_load_op,
            next_load_op,
        );
    }

    let load_op = take_ordered_pending_composite_load_op(
        pending_composites,
        pending_composite_load_op,
        pending_shader_composites,
        pending_shader_load_op,
        *next_load_op,
    );
    let mut order: Vec<(usize, usize, bool, usize)> = pending_composites
        .iter()
        .enumerate()
        .map(|(index, pending)| (pending.z_index, pending.seq, false, index))
        .chain(
            pending_shader_composites
                .iter()
                .enumerate()
                .map(|(index, pending)| (pending.z_index, pending.seq, true, index)),
        )
        .collect();
    order.sort_by_key(|&(z_index, seq, _, _)| (z_index, seq));

    let encoded = {
        let blit_items = pending_layer_composite_batch_items(pending_composites);
        let shader_items = pending_shader_layer_composite_batch_items(pending_shader_composites);
        let items: Vec<FusedCompositeItem<'_>> = order
            .iter()
            .map(|&(_, _, is_shader, index)| {
                if is_shader {
                    FusedCompositeItem::Shader(shader_items[index].2)
                } else {
                    FusedCompositeItem::Blit(blit_items[index].2)
                }
            })
            .collect();
        backend.fused_composite_batch_to_view(target_view, viewport, load_op, &items)
    };
    if encoded {
        release_pending_layer_composites(backend, pending_composites);
        release_pending_shader_layer_composites(backend, pending_shader_composites);
        *next_load_op = wgpu::LoadOp::Load;
        return Ok(());
    }

    let mut blits: Vec<Option<PendingLayerComposite>> = std::mem::take(pending_composites)
        .into_iter()
        .map(Some)
        .collect();
    let mut shaders: Vec<Option<PendingShaderLayerComposite>> =
        std::mem::take(pending_shader_composites)
            .into_iter()
            .map(Some)
            .collect();
    let mut load_op = load_op;
    for &(_, _, is_shader, index) in &order {
        let (surface, dest_quad, scissor) = if is_shader {
            let pending = shaders[index].take().expect("shader pending drawn once");
            (pending.surface, pending.dest_quad, pending.scissor)
        } else {
            let pending = blits[index].take().expect("blit pending drawn once");
            (pending.surface, pending.dest_quad, pending.scissor)
        };
        composite_layer_surface_to_view(
            backend,
            &surface,
            target_view,
            viewport,
            dest_quad,
            load_op,
            scissor,
        )?;
        load_op = wgpu::LoadOp::Load;
        backend.release_layer_surface_target(surface.target);
    }
    *next_load_op = wgpu::LoadOp::Load;
    Ok(())
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
    excluded: &[Range<usize>],
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
            excluded,
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
        excluded,
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
                "[wgpu-render-stage:direct-scene-cache] skip reason=admission-budget z_start={z_start} z_end={z_end} draw_ops={draw_ops} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{} bytes={target_bytes}",
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
        if !backend.admit_layer_surface_cache_miss(&cache_key, CacheAdmission::OnRepeat) {
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
        record_layer_cache_miss(
            backend,
            "scene-range",
            &cache_key,
            target_width,
            target_height,
        );
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

#[allow(clippy::too_many_arguments)]
fn queue_cached_direct_scene_range<B: SurfaceExecutionBackend>(
    backend: &mut B,
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    root_scale: f32,
    snapped_bounds: Option<Rect>,
    pending_composites: &mut Vec<PendingLayerComposite>,
    composite_seq: &mut usize,
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
        seq: next_composite_seq(composite_seq),
        surface,
        dest_quad,
        scissor: None,
    });
    *next_load_op = wgpu::LoadOp::Load;
    Ok(true)
}

/// The last z (exclusive) of the scene prefix a snapshot may cover: ops from
/// the bottom up to the first one whose replay could be wrong — a retained
/// batch (its transform lives outside the content hash) or a shape whose
/// feed capture this frame needs the ordinary conversion stream. Everything
/// else is eligible, shadows and non-SrcOver blends included: a snapshot
/// replays captured bytes, so the flatten path's transparency-safety rules
/// do not apply to it.
fn prefix_snapshot_range_end(scene: &CompositorScene, z_end: usize) -> usize {
    for op in scene_range_draw_ops(&scene.draw_ops, 0, z_end) {
        let stable = match op.kind {
            DrawOpKind::Retained(_) => false,
            DrawOpKind::Shape(index) => !shape_index_pending_feed_capture(index),
            DrawOpKind::Image(_) | DrawOpKind::Text(_) | DrawOpKind::Shadow(_) => true,
        };
        if !stable {
            return op.z_index.min(z_end);
        }
    }
    z_end
}

fn prefix_snapshot_key(
    scene: &CompositorScene,
    prefix_end: usize,
    width: u32,
    height: u32,
    root_scale: f32,
    clear: &wgpu::Color,
) -> Option<(LayerRasterCacheKey, Rect)> {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }
    if scene_range_draw_op_count(scene, 0, prefix_end) < MIN_PREFIX_SNAPSHOT_DRAW_OPS {
        return None;
    }
    let logical_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32 / root_scale,
        height: height as f32 / root_scale,
    };
    // The clear color is part of the captured bytes (the prefix rendered
    // over it), so it belongs in the key with the content.
    let clear_bits = clear.r.to_bits()
        ^ clear.g.to_bits().rotate_left(16)
        ^ clear.b.to_bits().rotate_left(32)
        ^ clear.a.to_bits().rotate_left(48);
    let content_hash =
        scene_range_content_hash(scene, 0, prefix_end, (width, height), root_scale) ^ clear_bits;
    Some((
        LayerRasterCacheKey::prefix_snapshot(
            content_hash,
            prefix_end as u64,
            logical_rect,
            (width, height),
            ScaleBucket::from_scale(root_scale),
        ),
        logical_rect,
    ))
}

/// Serves the scene's stable bottom prefix from a byte-exact snapshot, or
/// captures one.
///
/// On a hit the snapshot replays as a full-viewport composite riding inside
/// the segment's first render pass; premultiplied SrcOver over the pass's
/// own clear reproduces the captured bytes exactly (the dst term vanishes
/// against the very color the capture embedded). On an admitted miss the
/// prefix renders through the ordinary pipeline — every chained rounding
/// happens exactly as an uncached frame — and the produced texels are then
/// copied out of the target, so the entry IS the direct output rather than
/// a flattened approximation of it. First sightings render normally and
/// only observe, mirroring the flatten cache's two-sighting admission.
#[allow(clippy::too_many_arguments)]
/// What the prefix snapshot stage did with the walk's bottom range.
enum PrefixSnapshotOutcome {
    /// No prefix key exists this frame; the walk owns the whole range.
    Inert,
    /// The prefix owns `[0, end)`: the walk renders it as one plain direct
    /// run and the chunk flatten cache never sees it. The claim starts on
    /// the FIRST sighting, not on the store: the chunker's grid downstream
    /// of the prefix must be the same in every frame of a scene's life, or
    /// chunk admissions restart mid-life and same-content frames stop being
    /// byte-identical. Claimed frames, store frames (which also render the
    /// entry offscreen), and served frames all produce the direct path's
    /// bytes for the prefix range.
    Claimed(usize),
    /// A cached entry was composited in place of `[0, end)`.
    Served(usize),
}

/// Runs the prefix stage and folds its outcome into the walk: a served
/// prefix advances the cursor past its composite, a claimed one seeds the
/// direct run so the range renders as plain direct ops ahead of the
/// chunker, and an inert stage leaves the walk untouched. Returns the z
/// the chunk walk starts from.
#[allow(clippy::too_many_arguments)]
fn stage_prefix_snapshot_into_walk<B: SurfaceExecutionBackend>(
    backend: &mut B,
    snapshot_source: Option<&OffscreenTarget>,
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    width: u32,
    height: u32,
    root_scale: f32,
    pending_composites: &mut Vec<PendingLayerComposite>,
    composite_seq: &mut usize,
    pending_composite_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
    direct_run: &mut DirectChunkRunCoalescer,
) -> Result<usize, String> {
    match serve_or_capture_prefix_snapshot(
        backend,
        snapshot_source,
        scene,
        z_start,
        z_end,
        width,
        height,
        root_scale,
        pending_composites,
        composite_seq,
        pending_composite_load_op,
        next_load_op,
    )? {
        PrefixSnapshotOutcome::Inert => Ok(z_start),
        PrefixSnapshotOutcome::Served(end) => Ok(end),
        PrefixSnapshotOutcome::Claimed(end) => {
            direct_run.absorb(z_start);
            Ok(end)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn serve_or_capture_prefix_snapshot<B: SurfaceExecutionBackend>(
    backend: &mut B,
    snapshot_source: Option<&OffscreenTarget>,
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    width: u32,
    height: u32,
    root_scale: f32,
    pending_composites: &mut Vec<PendingLayerComposite>,
    composite_seq: &mut usize,
    pending_composite_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
) -> Result<PrefixSnapshotOutcome, String> {
    let inert = |reason: &str| {
        if backdrop_diag_enabled() {
            eprintln!("[backdrop-diag] prefix-snapshot inert: {reason} z_end={z_end}");
        }
        Ok(PrefixSnapshotOutcome::Inert)
    };
    if z_start != 0 || !prefix_snapshot_enabled() {
        return inert("z_start or disabled");
    }
    let Some(source) = snapshot_source else {
        return inert("no source");
    };
    let wgpu::LoadOp::Clear(clear) = *next_load_op else {
        return inert("load op");
    };
    if source.width != width || source.height != height {
        return inert("size");
    }
    // A pending feed capture copies specific shape slots out of this frame's
    // conversion stream; both replaying past them and splitting the frame's
    // batches around them would corrupt what the store captures. Captures
    // are one-shot events, so sitting the whole frame out is free.
    if any_pending_feed_captures() {
        return inert("pending feed captures");
    }
    let prefix_end = prefix_snapshot_range_end(scene, z_end);
    let Some((key, logical_rect)) =
        prefix_snapshot_key(scene, prefix_end, width, height, root_scale, &clear)
    else {
        return inert(&format!("no key prefix_end={prefix_end}"));
    };

    if let Some((cached_target, cached_rect)) = backend.cached_layer_surface(&key) {
        if pending_composites.is_empty() {
            *pending_composite_load_op = Some(*next_load_op);
        }
        let dest_quad = anchored_composite_dest_quad(
            crate::rect_to_quad(cached_rect),
            None,
            None,
            root_scale,
            CompositeSampleMode::Nearest,
        );
        // Src, not SrcOver: the entry is the target's whole state after the
        // prefix — clear included — so replay is a verbatim texel copy. A
        // SrcOver replay is off by one bit wherever the stored alpha rounds
        // below one at an anti-aliased pixel and the dst term leaks through.
        pending_composites.push(PendingLayerComposite {
            z_index: 0,
            seq: next_composite_seq(composite_seq),
            surface: LayerSurface {
                target: LayerSurfaceTexture::Cached(cached_target),
                logical_rect: cached_rect,
                composite_alpha: 1.0,
                blend_mode: BlendMode::Src,
                rounded_clip: None,
                backdrop: None,
                deferred_effect: None,
                effect_content_rect: None,
                sample_mode: CompositeSampleMode::Nearest,
            },
            dest_quad,
            scissor: None,
        });
        *next_load_op = wgpu::LoadOp::Load;
        return Ok(PrefixSnapshotOutcome::Served(prefix_end));
    }

    if !backend.admit_layer_surface_cache_miss(&key, CacheAdmission::OnRepeat) {
        if crate::layer_surface_cache::cache_diag_enabled() {
            log::warn!("[layer-cache-diag] prefix-snapshot observe key={key:?}");
        }
        return Ok(PrefixSnapshotOutcome::Claimed(prefix_end));
    }
    if crate::layer_surface_cache::cache_diag_enabled() {
        log::warn!("[layer-cache-diag] prefix-snapshot admit key={key:?}");
    }

    // The entry renders through the same segment pipeline as the frame,
    // over the same clear: an identical op sequence from an identical
    // initial state reproduces the direct path's chained roundings bit for
    // bit, which a flatten over transparent cannot. The claim makes the
    // frame render the same range as one plain direct run, so the store
    // frame's on-screen bytes, the entry's, and every later replay's are
    // all the same bytes; its cost is this one ordered offscreen pass.
    let entry = backend.acquire_retained_surface(width, height);
    let render_result = backend.render_non_effect_segment(
        &entry.view,
        &scene.shapes,
        &scene.brushes,
        &scene.images,
        &scene.texts,
        &scene.shadow_draws,
        &scene.retained_draws,
        &scene.draw_ops,
        0,
        prefix_end,
        &[],
        width,
        height,
        root_scale,
        wgpu::LoadOp::Clear(clear),
    );
    match render_result {
        Ok(()) => {
            record_layer_cache_miss(backend, "prefix-snapshot", &key, width, height);
            backend.insert_cached_layer_surface(key, entry, logical_rect);
            Ok(PrefixSnapshotOutcome::Claimed(prefix_end))
        }
        Err(_) => {
            backend.release_layer_surface_target(LayerSurfaceTexture::Owned(entry));
            Ok(PrefixSnapshotOutcome::Claimed(prefix_end))
        }
    }
}

/// Turns every cacheable blurred drop shadow of the range into pending
/// composites of its cached shadow texture, one per band, and takes the ops
/// out of the deferred run. A capture above the shadow then replays the
/// composite into its copy instead of forcing the whole run onto the target.
#[allow(clippy::too_many_arguments)]
fn queue_blurred_shadow_composites<B: SurfaceExecutionBackend>(
    backend: &mut B,
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    viewport: (u32, u32),
    root_scale: f32,
    pending_composites: &mut Vec<PendingLayerComposite>,
    composite_seq: &mut usize,
    pending_composite_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
    deferred: &mut DeferredDirectRun<'_>,
) {
    if !root_scale.is_finite() || root_scale <= 0.0 || !shadow_composite_queue_enabled() {
        return;
    }
    for op in scene_range_draw_ops(&scene.draw_ops, z_start, z_end) {
        let DrawOpKind::Shadow(index) = op.kind else {
            continue;
        };
        let Some(shadow) = scene.shadow_draws.get(index) else {
            continue;
        };
        if !shadow_draw_is_blurred_drop(shadow) {
            continue;
        }
        let Some(prepared) =
            backend.prepare_shadow_composite(shadow, viewport.0, viewport.1, root_scale)
        else {
            continue;
        };
        deferred.exclude(op.z_index);
        let (x, y, width, height) = prepared.dest_viewport;
        let dest_quad = crate::rect_to_quad(Rect {
            x,
            y,
            width,
            height,
        });
        let logical_rect = Rect {
            x: x / root_scale,
            y: y / root_scale,
            width: width / root_scale,
            height: height / root_scale,
        };
        for band in prepared.bands {
            if pending_composites.is_empty() {
                *pending_composite_load_op = Some(*next_load_op);
            }
            pending_composites.push(PendingLayerComposite {
                z_index: op.z_index,
                seq: next_composite_seq(composite_seq),
                surface: LayerSurface {
                    target: LayerSurfaceTexture::Cached(Rc::clone(&prepared.source)),
                    logical_rect,
                    composite_alpha: 1.0,
                    blend_mode: BlendMode::SrcOver,
                    rounded_clip: shadow.rounded_clip,
                    backdrop: None,
                    deferred_effect: None,
                    effect_content_rect: None,
                    sample_mode: CompositeSampleMode::Nearest,
                },
                dest_quad,
                scissor: Some(band),
            });
            *next_load_op = wgpu::LoadOp::Load;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_direct_scene_range_with_pending_composites<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target_view: &wgpu::TextureView,
    snapshot_source: Option<&OffscreenTarget>,
    scene: &CompositorScene,
    z_start: usize,
    z_end: usize,
    width: u32,
    height: u32,
    root_scale: f32,
    pending_composites: &mut Vec<PendingLayerComposite>,
    composite_seq: &mut usize,
    pending_composite_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    pending_shader_composites: &mut Vec<PendingShaderLayerComposite>,
    pending_shader_load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
    next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
    deferred: &mut DeferredDirectRun<'_>,
    flush_at_end: bool,
) -> Result<(), String> {
    let cache_enabled = direct_scene_range_cache_enabled();
    let mut cursor_z = z_start;
    if cache_enabled {
        cursor_z = stage_prefix_snapshot_into_walk(
            backend,
            snapshot_source,
            scene,
            cursor_z,
            z_end,
            width,
            height,
            root_scale,
            pending_composites,
            composite_seq,
            pending_composite_load_op,
            next_load_op,
            &mut deferred.run,
        )?;
    }
    queue_blurred_shadow_composites(
        backend,
        scene,
        cursor_z,
        z_end,
        (width, height),
        root_scale,
        pending_composites,
        composite_seq,
        pending_composite_load_op,
        next_load_op,
        deferred,
    );
    while cursor_z < z_end {
        if is_in_effect_range(cursor_z, &deferred.excluded) {
            cursor_z = cursor_z.saturating_add(1);
            continue;
        }
        let mut chunk_end = direct_scene_range_cache_chunk_end(scene, cursor_z, z_end, root_scale);
        if let Some(next_excluded) = deferred.next_excluded_after(cursor_z) {
            chunk_end = chunk_end.min(next_excluded);
        }
        if chunk_end <= cursor_z {
            return Err("direct scene cache chunk did not advance".to_string());
        }
        let chunk_bounds = if cache_enabled {
            direct_scene_range_snapped_bounds(scene, cursor_z, chunk_end, root_scale)
        } else {
            None
        };
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
                composite_seq,
                pending_composite_load_op,
                next_load_op,
            )?
        {
            if let Some((run_start, run_end)) = deferred.run.flush_at(cursor_z) {
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
                    &deferred.excluded,
                )?;
            }
            cursor_z = chunk_end;
            continue;
        }
        deferred.run.absorb(cursor_z);
        cursor_z = chunk_end;
        if !direct_scene_range_coalesce_enabled()
            && let Some((run_start, run_end)) = deferred.run.flush_at(cursor_z)
        {
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
                &deferred.excluded,
            )?;
        }
    }
    if flush_at_end && let Some((run_start, run_end)) = deferred.run.flush_at(z_end) {
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
            &deferred.excluded,
        )?;
    }
    Ok(())
}

fn child_backdrop_input_hash(
    local_scene: &CompositorScene,
    prior_child_contributions: &[BackdropPrefixChildContribution],
    child: &ChildLayerComposite,
    resolved_child: &ResolvedChildSurfaceComposite,
    effect: &RenderEffect,
    viewport: (u32, u32),
    scale: f32,
) -> u64 {
    let capture_rect = visible_backdrop_capture_rect(
        resolved_child.backdrop_rect,
        child.visual_clip,
        effect,
        scale,
        viewport,
    );
    backdrop_scene_prefix_hash(
        local_scene,
        prior_child_contributions,
        child.z_index,
        capture_rect.unwrap_or(resolved_child.backdrop_rect),
        viewport,
        scale,
    )
}

#[allow(clippy::too_many_arguments)]
fn nested_child_backdrop_input_hash(
    local_scene: &CompositorScene,
    prior_child_contributions: &[BackdropPrefixChildContribution],
    child: &ChildLayerComposite,
    resolved_child: &ResolvedChildSurfaceComposite,
    effect: &RenderEffect,
    viewport: (u32, u32),
    scale: f32,
    baked: bool,
    underlay_identity: Option<u64>,
) -> u64 {
    let local_hash = child_backdrop_input_hash(
        local_scene,
        prior_child_contributions,
        child,
        resolved_child,
        effect,
        viewport,
        scale,
    );
    if baked {
        content_hash_over_underlay(local_hash, underlay_identity)
    } else {
        local_hash
    }
}

/// Renders the surface's direct ops in `[z_start, z_end)` together with the
/// composites queued so far, one fused pass when the range carries no layer
/// events, otherwise the queues flush first and the range renders through
/// the layer-event path. The caller's loop defers this until a child reads
/// the target, so the composites of the children in between ride in the
/// same pass as the ops around them.
#[allow(clippy::too_many_arguments)]
fn render_nested_range<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target: &OffscreenTarget,
    local_scene: &CompositorScene,
    prior_child_contributions: &[BackdropPrefixChildContribution],
    z_start: usize,
    z_end: usize,
    viewport: (u32, u32),
    target_scale: f32,
    context: NestedBackdropContext<'_>,
    queues: &mut PendingQueues<'_, '_>,
) -> Result<(), String> {
    let (width, height) = viewport;
    if !range_contains_layer_events(
        &local_scene.effect_layers,
        &local_scene.backdrop_layers,
        z_start,
        z_end,
    ) {
        return render_non_effect_range_with_pending_composites(
            backend,
            &target.view,
            local_scene,
            z_start,
            z_end,
            width,
            height,
            target_scale,
            queues.composites,
            queues.composite_load_op,
            queues.shader_composites,
            queues.shader_load_op,
            queues.next_load_op,
            &queues.deferred.excluded,
        );
    }
    flush_pending_composite_queues_fused(
        backend,
        queues.composites,
        queues.composite_load_op,
        queues.shader_composites,
        queues.shader_load_op,
        &target.view,
        viewport,
        queues.next_load_op,
    )?;
    let mut local_backdrop_hashes = scene_backdrop_input_hashes(
        local_scene,
        prior_child_contributions,
        viewport,
        target_scale,
    );
    if context.baked {
        for hash in &mut local_backdrop_hashes {
            *hash = content_hash_over_underlay(*hash, context.underlay_identity);
        }
    }
    backend.render_range_with_layer_events_to_target(
        target,
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
        z_start,
        z_end,
        None,
        width,
        height,
        target_scale,
        context.backdrop_underlay,
        *queues.next_load_op,
    )?;
    *queues.next_load_op = wgpu::LoadOp::Load;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_layer_source_uncached<B: SurfaceExecutionBackend>(
    backend: &mut B,
    local_scene: &CompositorScene,
    child_layers: Vec<ChildLayerComposite>,
    target_scale: f32,
    backdrop_underlay: Option<&OffscreenTarget>,
    underlay_identity: Option<u64>,
    bake_underlay: bool,
    width: u32,
    height: u32,
    effective_translated_content_context: bool,
    effective_translated_content_axes: TranslatedContentAxes,
    translation_context: TranslationRenderContext,
) -> Result<OffscreenTarget, String> {
    let target = backend.acquire_retained_surface(width, height);
    let baked = bake_underlay
        && backdrop_underlay.is_some_and(|underlay| {
            underlay.width == width
                && underlay.height == height
                && backend.copy_texture_region_to_target(underlay, (0, 0), &target, (width, height))
        });
    let backdrop_underlay = if baked { None } else { backdrop_underlay };
    let mut cursor_z = 0usize;
    let mut next_load_op = if baked {
        wgpu::LoadOp::Load
    } else {
        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
    };
    let mut pending_composites = Vec::new();
    let mut pending_composite_load_op = None;
    let mut pending_shader_composites = Vec::new();
    let mut pending_shader_load_op = None;
    let mut composite_seq = 0usize;
    let mut prior_child_contributions = Vec::new();
    let mut deferred = DeferredDirectRun::new(local_scene);
    for mut child in child_layers {
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
            continue;
        }
        let child_reads_target = child.backdrop.is_some()
            || child.needs_nested_underlay
            || !resolved_child.shadow_draws.is_empty();
        if cursor_z < child.z_index
            && (child_reads_target
                || range_contains_layer_events(
                    &local_scene.effect_layers,
                    &local_scene.backdrop_layers,
                    cursor_z,
                    child.z_index,
                ))
        {
            render_nested_range(
                backend,
                &target,
                local_scene,
                &prior_child_contributions,
                cursor_z,
                child.z_index,
                (width, height),
                target_scale,
                NestedBackdropContext {
                    backdrop_underlay,
                    baked,
                    underlay_identity,
                },
                &mut PendingQueues {
                    composites: &mut pending_composites,
                    composite_load_op: &mut pending_composite_load_op,
                    shader_composites: &mut pending_shader_composites,
                    shader_load_op: &mut pending_shader_load_op,
                    next_load_op: &mut next_load_op,
                    composite_seq: &mut composite_seq,
                    deferred: &mut deferred,
                },
            )?;
            cursor_z = child.z_index;
        }
        let child_translation_context = TranslationRenderContext {
            inherited_content_translation: effective_translated_content_context,
            translated_content_axes: effective_translated_content_axes,
            surface_capture_active: translation_context.surface_capture_active,
            local_picture_capture_active: translation_context.local_picture_capture_active,
        };
        let wants_underlay = child.needs_nested_underlay;
        let child_underlay_identity = wants_underlay
            .then(|| {
                underlay_identity_before(
                    local_scene,
                    &prior_child_contributions,
                    child.z_index,
                    quad_bounds(child_dest_quad),
                    (width, height),
                    target_scale,
                    underlay_identity,
                    backdrop_underlay.is_some(),
                )
            })
            .flatten();
        composite_nested_child_backdrop(
            backend,
            &target,
            local_scene,
            &prior_child_contributions,
            &child,
            &resolved_child,
            (width, height),
            target_scale,
            NestedBackdropContext {
                backdrop_underlay,
                baked,
                underlay_identity,
            },
            &mut PendingQueues {
                composites: &mut pending_composites,
                composite_load_op: &mut pending_composite_load_op,
                shader_composites: &mut pending_shader_composites,
                shader_load_op: &mut pending_shader_load_op,
                next_load_op: &mut next_load_op,
                composite_seq: &mut composite_seq,
                deferred: &mut deferred,
            },
        )?;
        let mut child_bake_underlay = false;
        if child_is_bare_backdrop(&child) {
            let scissor = child
                .visual_clip
                .and_then(|clip| scissor_rect_for_rect(clip, target_scale, width, height));
            prior_child_contributions.push(bare_backdrop_child_contribution(
                &child,
                &resolved_child,
                scissor,
            ));
            cursor_z = child.z_index.saturating_add(1);
            continue;
        }
        let child_underlay = if wants_underlay {
            let underlay = sample_child_underlay(
                backend,
                &target,
                backdrop_underlay,
                &child,
                ChildUnderlayPlacement {
                    logical_rect: resolved_child.logical_rect,
                    dest_quad: resolved_child.dest_quad,
                    snapped_dest_quad: child_dest_quad,
                    snap_anchor: resolved_child.snap_anchor,
                    composite_snap_origin: resolved_child.composite_snap_origin,
                },
                child_underlay_identity,
                target_scale,
                child_translation_context,
                UnderlayCaptureSource {
                    target_view: &target.view,
                    viewport: (width, height),
                    dependency_rect: quad_bounds(child_dest_quad),
                    queues: &mut PendingQueues {
                        composites: &mut pending_composites,
                        composite_load_op: &mut pending_composite_load_op,
                        shader_composites: &mut pending_shader_composites,
                        shader_load_op: &mut pending_shader_load_op,
                        next_load_op: &mut next_load_op,
                        composite_seq: &mut composite_seq,
                        deferred: &mut deferred,
                    },
                },
            )?;
            child_bake_underlay = underlay.baked;
            Some(underlay.target)
        } else {
            None
        };
        let child_surface = render_layer_surface(
            backend,
            &mut child,
            LayerSurfaceRequest {
                root_scale: target_scale,
                backdrop_underlay: child_underlay.as_ref(),
                backdrop_underlay_identity: child_underlay_identity,
                bake_underlay: child_bake_underlay,
                allow_runtime_cache: true,
                logical_rect_override: Some(resolved_child.logical_rect),
                capture_clip_override: resolved_child.surface_clip,
                activates_nested_capture: true,
                translation_context: child_translation_context,
            },
        )?;

        if !resolved_child.shadow_draws.is_empty() {
            flush_pending_composite_queues_fused(
                backend,
                &mut pending_composites,
                &mut pending_composite_load_op,
                &mut pending_shader_composites,
                &mut pending_shader_load_op,
                &target.view,
                (width, height),
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
        let dest_quad = texel_aligned_dest_quad(
            dest_quad,
            {
                let target = child_surface.target.target();
                (target.width, target.height)
            },
            resolved_child.snap_anchor,
            child_surface_target_scale(&child, target_scale, child_translation_context),
            target_scale,
        );
        let scissor = child
            .visual_clip
            .and_then(|clip| scissor_rect_for_rect(clip, target_scale, width, height));
        let child_prefix_contribution = backdrop_prefix_child_contribution(
            &child,
            &child_surface,
            dest_quad,
            scissor,
            child_underlay_identity,
        );
        if child_surface.deferred_effect.is_none() && axis_aligned_quad_rect(dest_quad).is_some() {
            if pending_composites.is_empty() {
                pending_composite_load_op = Some(next_load_op);
            }
            pending_composites.push(PendingLayerComposite {
                z_index: child.z_index,
                seq: next_composite_seq(&mut composite_seq),
                surface: child_surface,
                dest_quad,
                scissor,
            });
            next_load_op = wgpu::LoadOp::Load;
        } else {
            match direct_shader_layer_composite(
                child_surface,
                child.z_index,
                next_composite_seq(&mut composite_seq),
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
                    if cursor_z < child.z_index {
                        render_nested_range(
                            backend,
                            &target,
                            local_scene,
                            &prior_child_contributions,
                            cursor_z,
                            child.z_index,
                            (width, height),
                            target_scale,
                            NestedBackdropContext {
                                backdrop_underlay,
                                baked,
                                underlay_identity,
                            },
                            &mut PendingQueues {
                                composites: &mut pending_composites,
                                composite_load_op: &mut pending_composite_load_op,
                                shader_composites: &mut pending_shader_composites,
                                shader_load_op: &mut pending_shader_load_op,
                                next_load_op: &mut next_load_op,
                                composite_seq: &mut composite_seq,

                                deferred: &mut deferred,
                            },
                        )?;
                        cursor_z = child.z_index;
                    }
                    flush_pending_composite_queues_fused(
                        backend,
                        &mut pending_composites,
                        &mut pending_composite_load_op,
                        &mut pending_shader_composites,
                        &mut pending_shader_load_op,
                        &target.view,
                        (width, height),
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
        if cursor_z >= child.z_index {
            cursor_z = child.z_index.saturating_add(1);
        }
    }

    if cursor_z < local_scene.next_z {
        render_nested_range(
            backend,
            &target,
            local_scene,
            &prior_child_contributions,
            cursor_z,
            local_scene.next_z,
            (width, height),
            target_scale,
            NestedBackdropContext {
                backdrop_underlay,
                baked,
                underlay_identity,
            },
            &mut PendingQueues {
                composites: &mut pending_composites,
                composite_load_op: &mut pending_composite_load_op,
                shader_composites: &mut pending_shader_composites,
                shader_load_op: &mut pending_shader_load_op,
                next_load_op: &mut next_load_op,
                composite_seq: &mut composite_seq,
                deferred: &mut deferred,
            },
        )?;
    } else if matches!(next_load_op, wgpu::LoadOp::Clear(_)) {
        backend.clear_target_view_with_load_op(&target.view, next_load_op);
    } else {
        flush_pending_composite_queues_fused(
            backend,
            &mut pending_composites,
            &mut pending_composite_load_op,
            &mut pending_shader_composites,
            &mut pending_shader_load_op,
            &target.view,
            (width, height),
            &mut next_load_op,
        )?;
    }

    Ok(target)
}

/// Whether a source-content miss earns a cache slot: a miss the upstream probe
/// already paid for is stored unconditionally, any other miss goes through
/// the admission policy and is recorded when it is admitted.
fn admit_source_content_miss<B: SurfaceExecutionBackend>(
    backend: &mut B,
    cache_key: &LayerRasterCacheKey,
    admission: CacheAdmission,
    missed_upstream: bool,
    size: (u32, u32),
) -> bool {
    if missed_upstream {
        return true;
    }
    let admitted = backend.admit_layer_surface_cache_miss(cache_key, admission);
    if admitted {
        record_layer_cache_miss(backend, "source-content", cache_key, size.0, size.1);
    }
    admitted
}

fn store_source_content<B: SurfaceExecutionBackend>(
    backend: &mut B,
    admitted: bool,
    cache_key: LayerRasterCacheKey,
    rendered: OffscreenTarget,
    surface_rect: Rect,
) -> LayerSurfaceTexture {
    if admitted
        && offscreen_byte_size(rendered.width, rendered.height) <= MAX_LAYER_SURFACE_CACHE_BYTES
    {
        LayerSurfaceTexture::Cached(backend.insert_cached_layer_surface(
            cache_key,
            rendered,
            surface_rect,
        ))
    } else {
        LayerSurfaceTexture::Owned(rendered)
    }
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
        backdrop_underlay_identity,
        bake_underlay,
        allow_runtime_cache,
        mut cache_candidate,
        logical_rect_override,
        capture_clip_override,
        composite_sample_mode,
        translation_context,
    } = options;
    let isolation = child.isolation.clone();
    let cache_admission = surface_cache_admission(child);
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
        if effective_requirements.contains(SurfaceRequirement::MotionStableCapture)
            && let Some(visible_bounds) = collected_layer_bounds(&local_scene, &child_layers, true)
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
        let candidate_source_key = cache_candidate
            .take_if(|(key, _)| key.is_source_content())
            .map(|(key, _)| key);
        let source_probe_missed_upstream = candidate_source_key.is_some();
        let source_cache_key = candidate_source_key.or_else(|| {
            layer_source_cache_key(
                child,
                effective_requirements,
                surface_rect,
                (width, height),
                target_scale,
                source_uses_external_backdrop,
                backdrop_underlay_identity,
                allow_runtime_cache,
            )
        });
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
            let cached = if source_probe_missed_upstream {
                None
            } else {
                backend.cached_layer_surface(&cache_key)
            };
            if let Some((cached_target, _)) = cached {
                LayerSurfaceTexture::Cached(cached_target)
            } else {
                let admitted = admit_source_content_miss(
                    backend,
                    &cache_key,
                    cache_admission,
                    source_probe_missed_upstream,
                    (width, height),
                );
                let rendered = render_layer_source_uncached(
                    backend,
                    &local_scene,
                    child_layers,
                    target_scale,
                    backdrop_underlay,
                    backdrop_underlay_identity,
                    bake_underlay,
                    width,
                    height,
                    effective_translated_content_context,
                    effective_translated_content_axes,
                    translation_context,
                )?;
                store_source_content(backend, admitted, cache_key, rendered, surface_rect)
            }
        } else {
            LayerSurfaceTexture::Owned(render_layer_source_uncached(
                backend,
                &local_scene,
                child_layers,
                target_scale,
                backdrop_underlay,
                backdrop_underlay_identity,
                bake_underlay,
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

        if let Some(effect) = deferred_effect.as_ref()
            && can_materialize_cached_effect(effect, backdrop.as_ref())
            && offscreen_byte_size(width, height) <= MAX_LAYER_SURFACE_CACHE_BYTES
            && let Some((cache_key, logical_rect)) = cache_candidate.take()
        {
            let effect_target = backend.acquire_retained_surface(width, height);
            materialize_render_effect_to_target(
                backend,
                target.target(),
                effect,
                &effect_target,
                content_effect_pixel_rect(Some(child.local_bounds), logical_rect, width, height),
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

        if rounded_clip.is_some()
            && let Some(effect) = deferred_effect.take()
        {
            let effect_target = backend.acquire_frame_surface(width, height);
            let materialized = materialize_render_effect_to_target(
                backend,
                target.target(),
                &effect,
                &effect_target,
                content_effect_pixel_rect(Some(child.local_bounds), surface_rect, width, height),
                composite_sample_mode,
            );
            if let Err(error) = materialized {
                backend.release_frame_surface(effect_target);
                return Err(error);
            }
            backend.release_layer_surface_target(target);
            target = LayerSurfaceTexture::Owned(effect_target);
        }

        if deferred_effect.is_none()
            && let Some((cache_key, logical_rect)) = cache_candidate
            && let LayerSurfaceTexture::Owned(owned_target) = target
        {
            if offscreen_byte_size(owned_target.width, owned_target.height)
                <= MAX_LAYER_SURFACE_CACHE_BYTES
            {
                let cached_target =
                    backend.insert_cached_layer_surface(cache_key, owned_target, logical_rect);
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
    let composite_result = composite_captured_effect_layer(
        backend,
        &source,
        &target.view,
        &layer,
        dest_quad,
        scissor,
        capture_rect,
        effect_width,
        effect_height,
        width,
        height,
        sample_mode,
    );
    backend.release_frame_surface(source);
    composite_result
}

/// The device geometry of a backdrop capture: what of the layer is visible,
/// the padded rect its effect reads, the scale the capture renders at and,
/// when that scale is the target's own, the whole-pixel copy that samples it.
struct BackdropCaptureGeometry {
    layer_rect: Rect,
    layer_clip: Option<Rect>,
    visible_rect: Rect,
    capture_rect: Rect,
    backdrop_scale: f32,
    copy_plan: Option<BackdropSnapshotCopyPlan>,
}

fn backdrop_capture_geometry(
    layer: &BackdropLayer,
    root_scale: f32,
    width: u32,
    height: u32,
    target_size: (u32, u32),
    max_texture_dim: u32,
) -> Option<BackdropCaptureGeometry> {
    let (layer_rect, layer_clip) = snapped_backdrop_geometry(layer, root_scale);
    let visible_rect = visible_layer_rect(layer_rect, layer_clip, root_scale, width, height)?;
    let capture_rect = backdrop_capture_rect(
        visible_rect,
        layer_clip,
        &layer.effect,
        root_scale,
        (width, height),
    );
    let backdrop_scale =
        clamp_effect_surface_scale(capture_rect, root_scale, root_scale, max_texture_dim);
    let copy_plan = ((backdrop_scale - root_scale).abs() <= 0.01)
        .then(|| {
            axis_aligned_backdrop_snapshot_copy_plan(
                capture_rect,
                layer_rect,
                root_scale,
                target_size,
                max_texture_dim,
            )
        })
        .flatten();
    Some(BackdropCaptureGeometry {
        layer_rect,
        layer_clip,
        visible_rect,
        capture_rect,
        backdrop_scale,
        copy_plan,
    })
}

/// The composites still queued for the target when a capture copies out of
/// it: the ones touching `dependency_pixels` are replayed into the copy.
struct PendingReplaySource<'a> {
    composites: &'a [PendingLayerComposite],
    shader_composites: &'a [PendingShaderLayerComposite],
    dependency_pixels: Rect,
}

/// Copies the capture's whole-pixel window out of the target and replays the
/// queued composites that touch it into the copy. A capture that was promised
/// a replay cannot fall back to sampling the target, which lacks them.
fn copy_backdrop_capture<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target: &OffscreenTarget,
    copy_plan: Option<&BackdropSnapshotCopyPlan>,
    destination: &OffscreenTarget,
    replay: Option<&PendingReplaySource<'_>>,
) -> Result<bool, String> {
    let Some(plan) = copy_plan else {
        if replay.is_some() {
            return Err("a backdrop capture promised a replay has no copy plan".to_string());
        }
        return Ok(false);
    };
    let copied =
        backend.copy_texture_region_to_target(target, plan.source_origin, destination, plan.size);
    match replay {
        Some(source) if copied => replay_pending_into_copy(
            backend,
            destination,
            plan.source_origin,
            source.composites,
            source.shader_composites,
            source.dependency_pixels,
        )?,
        Some(_) => {
            return Err(
                "a backdrop capture promised a replay could not copy its window".to_string(),
            );
        }
        None => {}
    }
    Ok(copied)
}

/// Decides how a backdrop capture of `layer` sees the composites queued for
/// `target`. With a whole-pixel copy plan and no clear held by a queue, the
/// queues stay pending and the capture replays the conflicting ones into its
/// copy; otherwise the queues flush first. Returns the dependency pixels the
/// capture must replay, when it must.
#[allow(clippy::too_many_arguments)]
fn prepare_capture_source<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target: &OffscreenTarget,
    layer: &BackdropLayer,
    dependency_rect: Rect,
    viewport: (u32, u32),
    scale: f32,
    queues: &mut PendingQueues<'_, '_>,
) -> Result<Option<Rect>, String> {
    flush_deferred_run_for_dependency(
        backend,
        &target.view,
        viewport,
        scale,
        dependency_rect,
        layer.z_index,
        queues,
    )?;
    let dependency_pixels = surface_pixel_rect(dependency_rect, scale);
    let conflicts = pending_capture_conflicts(
        queues.composites,
        queues.composite_load_op,
        queues.shader_composites,
        queues.shader_load_op,
        dependency_pixels,
    );
    let can_replay = backend.underlay_replay_enabled()
        && !conflicts.clear_held
        && backdrop_capture_geometry(
            layer,
            scale,
            viewport.0,
            viewport.1,
            (target.width, target.height),
            backend.max_texture_dim(),
        )
        .is_some_and(|geometry| geometry.copy_plan.is_some());
    if !can_replay {
        flush_pending_queues_for_backdrop_capture(
            backend,
            queues.composites,
            queues.composite_load_op,
            queues.shader_composites,
            queues.shader_load_op,
            &target.view,
            viewport,
            queues.next_load_op,
            dependency_rect,
            scale,
        )?;
        return Ok(None);
    }
    flush_pending_clear(backend, &target.view, queues.next_load_op);
    Ok(conflicts.any().then_some(dependency_pixels))
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
    allow_deferred_tail: bool,
    replay: Option<PendingReplaySource<'_>>,
) -> Result<Option<PreparedBackdropComposite>, String> {
    let diag = backdrop_diag_enabled();
    let Some(BackdropCaptureGeometry {
        layer_rect,
        layer_clip,
        visible_rect,
        capture_rect,
        backdrop_scale,
        copy_plan,
    }) = backdrop_capture_geometry(
        layer,
        root_scale,
        width,
        height,
        (target.width, target.height),
        backend.max_texture_dim(),
    )
    else {
        if diag {
            eprintln!(
                "[backdrop-diag] prepare SKIP: no visible rect for {:?}",
                layer.rect
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
    if diag {
        eprintln!(
            "[backdrop-diag] prepare node={:?} visible={visible_rect:?} capture={capture_rect:?} scissor={scissor:?} in_pad={} out_pad={} replay={}",
            layer.node_id,
            layer.effect.input_padding(),
            layer.effect.output_padding(),
            replay.is_some(),
        );
    }
    let (backdrop_width, backdrop_height) = copy_plan.map(|plan| plan.size).unwrap_or_else(|| {
        surface_target_size(capture_rect, backdrop_scale, backend.max_texture_dim())
    });
    let (materialized_effect, deferred_tail) = if allow_deferred_tail {
        split_backdrop_effect(&layer.effect)
    } else {
        (Some(&layer.effect), None)
    };
    let materialized_effect_hash = materialized_effect
        .map(retained_render_effect_hash)
        .unwrap_or(0);
    let Some(cache_key) = input_content_hash.and_then(|hash| {
        (backdrop_underlay.is_none()
            && backend.is_render_effect_supported(&layer.effect)
            && offscreen_byte_size(backdrop_width, backdrop_height)
                <= MAX_LAYER_SURFACE_CACHE_BYTES)
            .then(|| {
                backdrop_effect_cache_key_for_effect_hash(
                    layer,
                    hash,
                    materialized_effect_hash,
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
        record_layer_cache_miss(
            backend,
            "backdrop",
            &cache_key,
            backdrop_width,
            backdrop_height,
        );
        let effect_target = backend.acquire_retained_surface(backdrop_width, backdrop_height);
        if let Some(effect) = materialized_effect {
            let snapshot = backend.acquire_frame_surface(backdrop_width, backdrop_height);
            let copied_snapshot = copy_backdrop_capture(
                backend,
                target,
                copy_plan.as_ref(),
                &snapshot,
                replay.as_ref(),
            )?;
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
                effect,
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
        } else {
            let copied_capture = copy_backdrop_capture(
                backend,
                target,
                copy_plan.as_ref(),
                &effect_target,
                replay.as_ref(),
            )?;
            if !copied_capture {
                copy_projective_backdrop_inputs_to_view(
                    backend,
                    None,
                    target,
                    capture_rect,
                    &effect_target.view,
                    (backdrop_width, backdrop_height),
                    backdrop_scale,
                )?;
            }
            if diag {
                eprintln!(
                    "[backdrop-diag] prepare MISS copied={copied_capture} deferred-tail capture backdrop_size=({backdrop_width},{backdrop_height}) scale={backdrop_scale}"
                );
            }
        }
        LayerSurfaceTexture::Cached(backend.insert_cached_layer_surface(
            cache_key,
            effect_target,
            capture_rect,
        ))
    };

    let (surface_logical_rect, deferred_effect, effect_content_rect) = match deferred_tail {
        Some(shader) => {
            let window = copy_plan
                .map(|plan| Rect {
                    x: plan.source_origin.0 as f32 / root_scale,
                    y: plan.source_origin.1 as f32 / root_scale,
                    width: plan.size.0 as f32 / root_scale,
                    height: plan.size.1 as f32 / root_scale,
                })
                .unwrap_or(capture_rect);
            (
                window,
                Some(RenderEffect::Shader {
                    shader: shader.clone(),
                }),
                Some(layer_rect),
            )
        }
        None => (capture_rect, None, None),
    };
    Ok(Some(PreparedBackdropComposite {
        surface: LayerSurface {
            target: target_texture,
            logical_rect: surface_logical_rect,
            composite_alpha: 1.0,
            blend_mode: BlendMode::SrcOver,
            rounded_clip: None,
            backdrop: None,
            deferred_effect,
            effect_content_rect,
            sample_mode: CompositeSampleMode::Linear,
        },
        dest_quad: scaled_quad(crate::rect_to_quad(surface_logical_rect), root_scale),
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
        false,
        None,
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
        let sample_mode =
            exact_translation_sample_mode(dest_rect, source.width, source.height, sample_mode);
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
    use std::sync::Arc;

    use cranpose_core::{NodeId, collections::map::HashMap};
    use cranpose_render_common::graph::{
        DrawPrimitiveNode, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
        ProjectiveTransform, RenderNode, TextPrimitiveNode,
    };
    use cranpose_ui::{
        TextLayoutOptions,
        text::{AnnotatedString, TextStyle},
    };
    use cranpose_ui_graphics::{
        BlendMode, Brush, Color, GraphicsLayer, ImageBitmap, ImageSampling, Point, Rect,
        RenderEffect, RuntimeShader,
    };

    use super::{
        BackdropPrefixChildContribution, DIRECT_SCENE_RANGE_CACHE_FLOOR_BYTES,
        DirectChunkRunCoalescer, MAX_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
        MAX_MOTION_SENSITIVE_DIRECT_SCENE_CACHE_DRAW_BYTES, MIN_DIRECT_SCENE_RANGE_CACHE_DRAW_OPS,
        SceneBrush, anchored_composite_dest_quad, axis_aligned_backdrop_snapshot_copy_plan,
        backdrop_effect_cache_key, backdrop_effect_cache_key_for_effect_hash,
        backdrop_scene_prefix_hash, backdrop_underlay_is_covered_by_local_content,
        child_composite_visible, composite_dest_viewport, dest_quad_intersects_rect,
        direct_scene_range_cache_chunk_end, direct_scene_range_cache_enabled_for_policy,
        direct_scene_range_cache_key, direct_scene_range_chunk_fits_cache_entry,
        direct_scene_range_snapped_bounds, exact_translation_sample_mode, layer_source_cache_key,
        layer_source_uses_external_backdrop_underlay, layer_surface_dest_quad,
        layer_surface_translation_context, minimum_surface_scale_for_composite,
        prefix_snapshot_key, prefix_snapshot_range_end, quad_bounds_rect, rects_intersect,
        render_string_scene_hash, retained_render_effect_hash, rounded_fill_covers_rect,
        scene_backdrop_input_hashes, snapped_backdrop_geometry, split_backdrop_effect,
        surface_target_size, underlay_fill_scissor, underlay_sample_rect,
        visible_backdrop_capture_rect,
    };
    use crate::{
        effect_renderer::CompositeSampleMode,
        normalized_scene::TranslateBy,
        scene::{
            BackdropLayer, CompositorScene, DrawOp, DrawOpKind, DrawShape, ImageDraw, SnapAnchor,
        },
        surface_plan::{TranslationRenderContext, layer_surface_requirements_cached},
        surface_requirements::{SurfaceRequirement, SurfaceRequirementSet},
    };

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
        run.absorb(0);
        run.absorb(64);
        run.absorb(128);
        assert_eq!(run.flush_at(192), Some((0, 192)));
        assert_eq!(run.flush_at(192), None);
    }

    #[test]
    fn direct_chunk_run_coalescer_flushes_below_a_composited_chunk() {
        let mut run = DirectChunkRunCoalescer::default();
        run.absorb(0);
        assert_eq!(run.flush_at(64), Some((0, 64)));
        run.absorb(128);
        assert_eq!(run.flush_at(256), Some((128, 256)));
    }

    #[test]
    fn direct_chunk_run_coalescer_is_quiet_without_direct_chunks() {
        let mut run = DirectChunkRunCoalescer::default();
        assert_eq!(run.flush_at(64), None);
        assert_eq!(run.flush_at(128), None);
        run.absorb(64);
        assert_eq!(run.flush_at(64), None);
        assert_eq!(run.flush_at(128), Some((64, 128)));
    }

    #[test]
    fn the_flatten_floor_keeps_entries_small_and_the_kill_switch_refuses_all() {
        let small_entry = DIRECT_SCENE_RANGE_CACHE_FLOOR_BYTES;
        let large_entry = DIRECT_SCENE_RANGE_CACHE_FLOOR_BYTES + 1;

        assert!(direct_scene_range_cache_enabled_for_policy(
            false,
            small_entry
        ));
        assert!(!direct_scene_range_cache_enabled_for_policy(
            false,
            large_entry
        ));
        assert!(!direct_scene_range_cache_enabled_for_policy(
            true,
            small_entry
        ));
    }

    #[test]
    fn a_prefix_snapshot_stops_at_a_retained_batch() {
        let mut scene = CompositorScene::new();
        for z_index in 0..10usize {
            let shape = prefix_shape(z_index, Color::BLACK);
            scene.shapes.push(shape);
            scene.draw_ops.push(DrawOp {
                z_index,
                kind: DrawOpKind::Shape(z_index),
            });
        }
        scene.retained_draws.push(crate::scene::RetainedDraw {
            slot: 0,
            transform: crate::scene::SimilarityTransform::IDENTITY,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            first_shape: 0,
            shape_count: 1,
        });
        scene.draw_ops.push(DrawOp {
            z_index: 10,
            kind: DrawOpKind::Retained(0),
        });
        scene.next_z = 11;

        assert_eq!(prefix_snapshot_range_end(&scene, scene.next_z), 10);
    }

    #[test]
    fn a_prefix_snapshot_stops_before_a_feed_captured_shape() {
        crate::shape_replay::clear_pending_feed_captures_for_tests();
        let mut scene = CompositorScene::new();
        for z_index in 0..10usize {
            let shape = prefix_shape(z_index, Color::BLACK);
            scene.shapes.push(shape);
            scene.draw_ops.push(DrawOp {
                z_index,
                kind: DrawOpKind::Shape(z_index),
            });
        }
        scene.next_z = 10;

        assert_eq!(prefix_snapshot_range_end(&scene, scene.next_z), 10);

        crate::shape_replay::inject_feed_capture_for_tests(
            cranpose_render_common::graph::DrawCommandId {
                node_id: 7,
                command_index: 0,
                placement: cranpose_render_common::style_shared::DrawPlacement::Behind,
            },
            0,
            6,
            2,
        );
        assert_eq!(
            prefix_snapshot_range_end(&scene, scene.next_z),
            6,
            "a capture queued this frame copies those shape slots out of the \
             ordinary conversion stream; a snapshot replay would starve it"
        );
        crate::shape_replay::clear_pending_feed_captures_for_tests();
    }

    #[test]
    fn a_prefix_snapshot_key_carries_the_clear_color() {
        let scene = scene_with_cacheable_prefix_shapes(Color::BLACK);
        let clear_a = wgpu::Color::BLACK;
        let clear_b = wgpu::Color {
            r: 0.5,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let key_a = prefix_snapshot_key(&scene, scene.next_z, 400, 400, 1.0, &clear_a)
            .expect("prefix over enough ops must key")
            .0;
        let key_b = prefix_snapshot_key(&scene, scene.next_z, 400, 400, 1.0, &clear_b)
            .expect("prefix over enough ops must key")
            .0;
        assert_ne!(
            key_a, key_b,
            "the captured bytes embed the clear the prefix rendered over"
        );
    }

    #[test]
    fn a_prefix_snapshot_key_never_collides_with_a_flatten_key() {
        let scene = scene_with_cacheable_prefix_shapes(Color::BLACK);
        let prefix = prefix_snapshot_key(&scene, scene.next_z, 400, 400, 1.0, &wgpu::Color::BLACK)
            .expect("prefix over enough ops must key")
            .0;
        let flatten = direct_scene_range_cache_key(
            &scene,
            0,
            scene.next_z,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 400.0,
            },
            (400, 400),
            1.0,
        )
        .expect("the same range must also produce a flatten key");
        assert_ne!(
            prefix, flatten,
            "a byte-exact snapshot and a flattened approximation must never \
             serve each other's probes"
        );
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

    fn lower_test_layer(layer: &LayerNode) -> crate::normalized_scene::ChildLayerComposite {
        use cranpose_render_common::layer_composition::effective_layer_isolation;

        use crate::surface_plan::{
            layer_contains_descendant_backdrop, layer_surface_scale,
            translated_content_axes_for_layer,
        };

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
    fn every_translation_phase_pays_the_same_one_texel_snapshot_margin() {
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
        assert_eq!(plan.size.1, ceiled.1 + 1);
        assert_eq!(
            plan.size.0,
            ceiled.0 + 1,
            "the margin is constant, not phase-dependent — an integral axis pays it too"
        );
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
    fn a_bare_shader_backdrop_defers_entirely_and_materializes_nothing() {
        let effect = RenderEffect::runtime_shader(RuntimeShader::new("// lens"));
        let (materialized, tail) = split_backdrop_effect(&effect);
        assert!(materialized.is_none(), "a lens caches the raw capture");
        assert!(tail.is_some(), "the lens shader must defer to the batch");
    }

    #[test]
    fn a_frost_chain_materializes_the_blur_and_defers_the_shader_tail() {
        let effect = RenderEffect::blur(12.0).then(RenderEffect::runtime_shader(
            RuntimeShader::new("// frost tail"),
        ));
        let (materialized, tail) = split_backdrop_effect(&effect);
        assert!(
            matches!(materialized, Some(RenderEffect::Blur { .. })),
            "the blur prefix alone names the cached surface"
        );
        assert!(tail.is_some(), "the frost tail must defer to the batch");
    }

    #[test]
    fn an_effect_without_a_shader_tail_materializes_whole() {
        let bare_blur = RenderEffect::blur(8.0);
        let (materialized, tail) = split_backdrop_effect(&bare_blur);
        assert_eq!(materialized, Some(&bare_blur));
        assert!(tail.is_none());

        let blur_chain = RenderEffect::blur(8.0).then(RenderEffect::blur(4.0));
        let (materialized, tail) = split_backdrop_effect(&blur_chain);
        assert_eq!(
            materialized,
            Some(&blur_chain),
            "a chain that does not end in a shader stays intact"
        );
        assert!(tail.is_none());
    }

    #[test]
    fn deferring_the_tail_keys_the_cache_off_the_prefix_not_the_dynamics() {
        let layer_rect = Rect {
            x: 10.0,
            y: 10.0,
            width: 60.0,
            height: 40.0,
        };
        let mut morning = RuntimeShader::new("// frost tail");
        morning.set_float(0, 0.25);
        let mut evening = RuntimeShader::new("// frost tail");
        evening.set_float(0, 0.75);
        let key_for = |tail: RuntimeShader| {
            let effect = RenderEffect::blur(12.0).then(RenderEffect::runtime_shader(tail));
            let layer = BackdropLayer {
                effect,
                ..test_backdrop_layer(layer_rect)
            };
            let (materialized, _) = split_backdrop_effect(&layer.effect);
            backdrop_effect_cache_key_for_effect_hash(
                &layer,
                7,
                materialized.map(retained_render_effect_hash).unwrap_or(0),
                layer.rect,
                (60, 40),
                1.0,
            )
        };
        assert_eq!(
            key_for(morning),
            key_for(evening),
            "per-frame tail uniforms must not invalidate the blurred capture"
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
            "a full-surface chunk is far past the flatten entry budget"
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
        let large_side = (MAX_MOTION_SENSITIVE_DIRECT_SCENE_CACHE_DRAW_BYTES as f32
            / crate::offscreen::composition_bytes_per_pixel() as f32)
            .sqrt()
            + 16.0;
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
        assert!(
            direct_scene_range_cache_key(
                &scene,
                0,
                motion_z,
                viewport,
                (large_side as u32, large_side as u32),
                root_scale,
            )
            .is_some()
        );
        assert!(
            direct_scene_range_cache_key(
                &scene,
                motion_z,
                suffix_start,
                viewport,
                (large_side as u32, large_side as u32),
                root_scale,
            )
            .is_some()
        );
        assert!(
            direct_scene_range_cache_key(
                &scene,
                suffix_start,
                scene.next_z,
                viewport,
                (large_side as u32, large_side as u32),
                root_scale,
            )
            .is_some()
        );
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
                None,
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
                None,
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
                None,
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
                None,
                true,
            ),
            None,
            "nested backdrop sources depend on the external underlay until the descendant input is isolated"
        );
    }

    #[test]
    fn motion_stable_translated_text_source_cache_tracks_scroll_offset() {
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
        assert!(
            base_lowered
                .surface_requirements
                .surface_requirements
                .contains(SurfaceRequirement::MotionStableCapture)
        );

        let moved_lowered = lower_test_layer(&moved);
        let base_key = layer_source_cache_key(
            &base_lowered,
            base_lowered.surface_requirements.surface_requirements,
            base.local_bounds,
            (220, 96),
            1.0,
            false,
            None,
            true,
        );
        let moved_key = layer_source_cache_key(
            &moved_lowered,
            moved_lowered.surface_requirements.surface_requirements,
            moved.local_bounds,
            (220, 96),
            1.0,
            false,
            None,
            true,
        );

        assert_ne!(base_key, moved_key);
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
    fn integer_and_fractional_phases_share_one_enclosing_copy_plan() {
        let plan_at = |x: f32| {
            let capture = Rect {
                x,
                y: 20.0,
                width: 140.0,
                height: 100.0,
            };
            axis_aligned_backdrop_snapshot_copy_plan(capture, capture, 2.0, (800, 600), 4096)
                .expect("in-viewport capture stays on the copy path at every phase")
        };
        let integer = plan_at(12.0);
        let fractional = plan_at(12.25);

        assert_eq!(integer.size, fractional.size);
        for (plan, x) in [(integer, 12.0f32), (fractional, 12.25)] {
            let left_px = (x * 2.0).floor() as u32;
            let right_px = ((x + 140.0) * 2.0).ceil() as u32;
            assert!(plan.source_origin.0 <= left_px);
            assert!(plan.source_origin.0 + plan.size.0 >= right_px);
        }
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
        assert_eq!(plan.size, (281, 201));
        assert_eq!(plan.effect_pixel_rect, [0.5, 0.0, 280.0, 200.0]);
        assert_eq!(plan.dest_viewport, (24.0, 41.0, 281.0, 201.0));
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
    fn a_backdrop_copy_plan_size_never_depends_on_translation_phase() {
        let mut sizes = std::collections::BTreeSet::new();
        for step in 0..12 {
            let capture = Rect {
                x: 260.0,
                y: 100.0 + step as f32 * 0.1,
                width: 92.0,
                height: 92.0,
            };
            let plan =
                axis_aligned_backdrop_snapshot_copy_plan(capture, capture, 3.0, (1080, 2244), 4096)
                    .expect("mid-viewport capture must stay on the copy path at every phase");
            sizes.insert(plan.size);
        }
        assert_eq!(
            sizes.len(),
            1,
            "translation phase leaked into the snapshot size: {sizes:?} — a moving \
             backdrop churns cache keys and defeats pool recycling"
        );
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
        assert_eq!(plan.size, (329, 249));
        assert_eq!(plan.effect_pixel_rect, [24.0, 24.0, 280.0, 200.0]);
        assert_eq!(plan.dest_viewport, (4.0, 28.0, 329.0, 249.0));
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

    #[test]
    fn one_to_one_integer_translation_uses_texel_sampling() {
        assert_eq!(
            exact_translation_sample_mode(
                Rect {
                    x: 12.0,
                    y: -1.0,
                    width: 100.0,
                    height: 40.0,
                },
                100,
                40,
                CompositeSampleMode::Box4,
            ),
            CompositeSampleMode::Nearest
        );
        assert_eq!(
            exact_translation_sample_mode(
                Rect {
                    x: 12.0,
                    y: -1.0,
                    width: 100.0,
                    height: 40.2,
                },
                100,
                40,
                CompositeSampleMode::Linear,
            ),
            CompositeSampleMode::Nearest
        );
        assert_eq!(
            exact_translation_sample_mode(
                Rect {
                    x: 12.5,
                    y: -1.0,
                    width: 100.0,
                    height: 40.0,
                },
                100,
                40,
                CompositeSampleMode::Box4,
            ),
            CompositeSampleMode::Box4
        );
        assert_eq!(
            exact_translation_sample_mode(
                Rect {
                    x: 12.0,
                    y: -1.0,
                    width: 100.0,
                    height: 40.0,
                },
                100,
                40,
                CompositeSampleMode::Linear,
            ),
            CompositeSampleMode::Nearest
        );
    }
}
