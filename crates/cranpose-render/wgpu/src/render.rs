use std::{
    borrow::Cow,
    collections::HashMap,
    hash::{Hash, Hasher},
    rc::Rc,
    sync::{Arc, mpsc},
    time::Duration,
};

use bytemuck::{Pod, Zeroable};
use cranpose_core::{NodeId, hash::default as default_hash};
use cranpose_render_common::{
    bounded_lru_cache::BoundedLruCache,
    geometry::blur_reach,
    graph::{DrawCommandId, quad_bounds},
    software_text_raster::{
        SoftwareGlyphAtlasGlyph, SoftwareGlyphAtlasKey, SoftwareGlyphAtlasPlacement,
        SoftwareGlyphAtlasRunGlyph, SoftwareGlyphRasterCache, SoftwareTextFontSet,
        collect_solid_text_atlas_run, measure_text_with_font,
        rasterize_annotated_text_to_image_with_glyph_cache,
        rasterize_text_to_image_with_glyph_cache,
    },
};
use cranpose_ui_graphics::{
    BlendMode, ColorFilter, FxHasher, ImageBitmap, ImageSampling, Point, RecordSegment, Rect,
    RenderHash, TileMode,
};
use smallvec::SmallVec;
use web_time::Instant;

use crate::{
    DebugCpuAllocationStats,
    ablation::Ablation,
    collect::LayerScene,
    debug_toggles::DebugToggle,
    draw_pass::{PassSegment, PassTarget, ResolvedComposite, ResolvedCompositeKind, SourceContent},
    effect_renderer::{CompositeSampleMode, EffectRenderer, RoundedCompositeMask},
    frame::{AdmissionGate, FrameExecutor},
    frame_graph::{
        BufferUpload, FrameCommandRecorder, FrameCommandStats, FrameTextureDescriptor,
        UploadAllocatorSpec, WgpuFrameGraph, WgpuFrameGraphExecutor, write_buffer,
    },
    frame_packet::{CancelReason, FramePacket, PresentOutcome, RenderReturns},
    geometry::{
        DevicePixelBounds, anchored_device_rect, axis_aligned_quad_rect,
        canonicalize_device_coordinate, canonicalized_scaled_quad, offscreen_byte_size,
        scaled_quad, snap_delta_for_anchor, translate_quad,
        translation_stable_anchored_device_pixel_bounds,
    },
    gpu_stats::{self, gpu_stats_enabled},
    layer_cache::LayerCache,
    lazy_resource::LazyGpuResource,
    offscreen::{OffscreenTarget, composition_bytes_per_pixel, composition_format},
    output_conversion::OutputConverter,
    record_columns::record_vertex_layouts,
    rect_to_quad,
    run_store::{ArenaBinding, PlacementData, RunBufferMode, RunDrawCall, RunStore},
    scene::{
        CompositorScene, DrawOp, DrawOpKind, ImageDraw, RunDraw, ShadowDraw, SnapAnchor, TextDraw,
    },
    shaders,
    shape_pipelines::{ShapePipelineFactory, ShapePipelines},
};
const MAX_SHADOW_SURFACE_CACHE_ITEMS: usize = 512;
const MAX_TRANSPARENT_SOURCES: usize = 16;
const MAX_SHADOW_SURFACE_CACHE_BYTES: u64 = 384 * 1024 * 1024;

static SKIP_SHADOWS: DebugToggle = DebugToggle::new("CRANPOSE_SKIP_SHADOWS");

fn skip_shadow_draws() -> bool {
    SKIP_SHADOWS.flag()
}
const MAX_TEXT_IMAGE_CACHE_ITEMS: usize = 1024;
const MAX_TEXT_GLYPH_MASK_CACHE_ITEMS: usize = 8192;
const MAX_TEXT_GLYPH_ATLAS_ITEMS: usize = 8192;
const MAX_TEXT_GLYPH_RUN_CACHE_ITEMS: usize = 1024;
const MAX_TEXT_GLYPH_GPU_RUN_CACHE_ITEMS: usize = 1024;
const MIN_RETAINED_TEXT_GLYPH_QUADS: usize = 192;

const TEXT_GLYPH_ATLAS_MIN_SIZE: u32 = 512;
const TEXT_GLYPH_ATLAS_MAX_SIZE: u32 = 4096;
const TEXT_GLYPH_ATLAS_PADDING: u32 = 1;
const MAX_TEXT_LINE_INDEX_CACHE_ITEMS: usize = 512;
const MIN_MULTILINE_TEXT_LINES_FOR_CLIPPED_RASTER: usize = 2;

const CACHE_MISS_WARMUP_FRAMES: u8 = 1;
pub(crate) const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: cranpose_render_common::FRAME_CLEAR_COLOR[0] as f64,
    g: cranpose_render_common::FRAME_CLEAR_COLOR[1] as f64,
    b: cranpose_render_common::FRAME_CLEAR_COLOR[2] as f64,
    a: cranpose_render_common::FRAME_CLEAR_COLOR[3] as f64,
};
const INITIAL_VIEWPORT_UNIFORM_SLOTS: usize = 128;
const MAX_TEXTURE_CACHE_ITEMS: usize = 256;
const MAX_IMAGE_TEXTURE_CACHE_BYTES: usize = 256 * 1024 * 1024;

const DEFAULT_WGPU_RENDER_STAGE_TELEMETRY_THRESHOLD_MS: f64 = 4.0;

fn wgpu_render_stage_telemetry_threshold_ms() -> Option<f64> {
    static THRESHOLD_MS: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *THRESHOLD_MS.get_or_init(|| {
        let explicit =
            crate::debug_toggles::debug_toggle("CRANPOSE_WGPU_RENDER_STAGE_TELEMETRY_MS")
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value >= 0.0);
        explicit.or_else(|| {
            std::env::var_os("CRANPOSE_WGPU_RENDER_STAGE_TELEMETRY")
                .is_some()
                .then_some(DEFAULT_WGPU_RENDER_STAGE_TELEMETRY_THRESHOLD_MS)
        })
    })
}

pub(crate) fn instant_ms(start: Instant, end: Instant) -> f64 {
    end.duration_since(start).as_secs_f64() * 1000.0
}

pub(crate) fn should_log_wgpu_render_stage(start: Instant, end: Instant) -> Option<f64> {
    let threshold_ms = wgpu_render_stage_telemetry_threshold_ms()?;
    let total_ms = instant_ms(start, end);
    (total_ms >= threshold_ms).then_some(total_ms)
}

pub static PRESENTED_FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn frames_presented() -> u64 {
    PRESENTED_FRAMES.load(std::sync::atomic::Ordering::Relaxed)
}

fn text_atlas_fallback_diag_enabled() -> bool {
    cranpose_core::env_flag!("CRANPOSE_TEXT_ATLAS_FALLBACK_DIAG")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ShadowSurfaceCacheKey {
    content_hash: u64,
    pixel_size: [u32; 2],
    root_scale_bits: u32,
    blur_radius_bits: u32,
}

struct CachedShadowSurface {
    target: Rc<OffscreenTarget>,
    byte_size: u64,
}

type DeviceRect4 = (f32, f32, f32, f32);

/// A draw's scissor cut down to the pixels its pass segment may touch;
/// `None` when nothing of it remains.
pub(crate) fn bounded_scissor(
    scissor: (u32, u32, u32, u32),
    bound: Option<(u32, u32, u32, u32)>,
) -> Option<(u32, u32, u32, u32)> {
    let Some((bx, by, bw, bh)) = bound else {
        return Some(scissor);
    };
    let (x, y, width, height) = scissor;
    let left = x.max(bx);
    let top = y.max(by);
    let right = (x + width).min(bx + bw);
    let bottom = (y + height).min(by + bh);
    (right > left && bottom > top).then(|| (left, top, right - left, bottom - top))
}

fn intersect_device_rects(a: DeviceRect4, b: DeviceRect4) -> Option<DeviceRect4> {
    let left = a.0.max(b.0);
    let top = a.1.max(b.1);
    let right = (a.0 + a.2).min(b.0 + b.2);
    let bottom = (a.1 + a.3).min(b.1 + b.3);
    (right > left && bottom > top).then_some((left, top, right - left, bottom - top))
}

fn anchored_rect_to_device(
    rect: Rect,
    snap_anchor: Option<SnapAnchor>,
    root_scale: f32,
) -> DeviceRect4 {
    let device = anchored_device_rect(rect, snap_anchor, root_scale);
    (device.x, device.y, device.width, device.height)
}

fn mask_rect(rect: Rect) -> [f32; 4] {
    [rect.x, rect.y, rect.width, rect.height]
}

/// The parts of a shadow's covered device rect that lie outside its
/// occluder: up to four disjoint bands (above, below, left of and right of
/// the occluder) that together tile the coverage minus the occluder's whole
/// interior pixels. A fractional occluder shrinks inward so no covered pixel
/// is skipped.
fn shadow_bands(
    coverage: DeviceRect4,
    occluder: Option<DeviceRect4>,
) -> SmallVec<[DeviceRect4; 4]> {
    let mut bands = SmallVec::new();
    let (cx, cy, cw, ch) = coverage;
    let (cr, cb) = (cx + cw, cy + ch);
    let Some((ox, oy, ow, oh)) = occluder else {
        bands.push(coverage);
        return bands;
    };
    let left = ox.ceil().max(cx);
    let top = oy.ceil().max(cy);
    let right = (ox + ow).floor().min(cr);
    let bottom = (oy + oh).floor().min(cb);
    if right <= left || bottom <= top {
        bands.push(coverage);
        return bands;
    }
    if top > cy {
        bands.push((cx, cy, cw, top - cy));
    }
    if bottom < cb {
        bands.push((cx, bottom, cw, cb - bottom));
    }
    if left > cx {
        bands.push((cx, top, left - cx, bottom - top));
    }
    if right < cr {
        bands.push((right, top, cr - right, bottom - top));
    }
    bands
}

fn banded_pixels(bands: &[DeviceRect4]) -> u64 {
    bands
        .iter()
        .map(|band| (band.2 as u64).saturating_mul(band.3 as u64))
        .sum()
}

#[cfg(test)]
mod shadow_band_tests {
    use super::*;

    fn area(bands: &[DeviceRect4]) -> f32 {
        bands.iter().map(|band| band.2 * band.3).sum()
    }

    fn disjoint(bands: &[DeviceRect4]) -> bool {
        bands.iter().enumerate().all(|(index, a)| {
            bands
                .iter()
                .skip(index + 1)
                .all(|b| intersect_device_rects(*a, *b).is_none())
        })
    }

    #[test]
    fn an_interior_occluder_leaves_four_disjoint_bands_that_tile_the_ring() {
        let bands = shadow_bands((0.0, 0.0, 100.0, 80.0), Some((20.0, 10.0, 50.0, 40.0)));
        assert_eq!(bands.len(), 4);
        assert!(disjoint(&bands));
        assert_eq!(area(&bands), 100.0 * 80.0 - 50.0 * 40.0);
    }

    #[test]
    fn an_occluder_outside_the_coverage_changes_nothing() {
        let coverage = (0.0, 0.0, 100.0, 80.0);
        let bands = shadow_bands(coverage, Some((200.0, 200.0, 10.0, 10.0)));
        assert_eq!(bands.as_slice(), &[coverage]);
    }

    #[test]
    fn an_occluder_swallowing_the_coverage_leaves_nothing_to_draw() {
        let bands = shadow_bands((10.0, 10.0, 20.0, 20.0), Some((0.0, 0.0, 100.0, 100.0)));
        assert!(bands.is_empty());
    }

    #[test]
    fn a_fractional_occluder_shrinks_inward_so_no_covered_pixel_is_skipped() {
        let bands = shadow_bands((0.0, 0.0, 100.0, 80.0), Some((20.4, 10.6, 50.2, 40.1)));
        assert!(disjoint(&bands));
        let ring = 100.0 * 80.0 - (70.0 - 21.0) * (50.0 - 11.0);
        assert_eq!(area(&bands), ring);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextImageCacheKey(u64);

struct CachedTextImage {
    image: ImageBitmap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextGlyphRunCacheKey(u64);

#[derive(Clone, Copy)]
struct CachedTextGlyphQuad {
    x: i32,
    y: i32,
    width: usize,
    height: usize,
    color: (f32, f32, f32, f32),
    uv: ImageUvRect,
}

struct CachedTextGlyphRun {
    glyphs: Rc<[SoftwareGlyphAtlasPlacement]>,
    quads: Option<Rc<[CachedTextGlyphQuad]>>,
    atlas_generation: u64,
}

struct CachedGpuTextGlyphRun {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    atlas_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextLineIndexCacheKey(usize);

struct CachedTextLineIndex {
    text: std::sync::Weak<cranpose_ui::text::RenderString>,
    len: usize,
    starts: Rc<[usize]>,
}

struct TextLineIndexCache {
    entries: BoundedLruCache<TextLineIndexCacheKey, CachedTextLineIndex>,
}

impl TextLineIndexCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: BoundedLruCache::with_capacity_at_least_one(capacity),
        }
    }

    fn line_starts(&mut self, text: &Arc<cranpose_ui::text::RenderString>) -> Rc<[usize]> {
        let key = TextLineIndexCacheKey(Arc::as_ptr(text) as usize);
        if let Some(cached) = self.entries.get(&key)
            && cached.len == text.text.len()
            && cached
                .text
                .upgrade()
                .is_some_and(|cached_text| Arc::ptr_eq(&cached_text, text))
        {
            return cached.starts.clone();
        }

        let starts = Rc::<[usize]>::from(line_start_offsets(text.text.as_str()));
        self.entries.put(
            key,
            CachedTextLineIndex {
                text: Arc::downgrade(text),
                len: text.text.len(),
                starts: starts.clone(),
            },
        );
        starts
    }
}

#[derive(Default)]
struct DeviceErrorSentry {
    errors: std::sync::atomic::AtomicU64,
    poisoned: std::sync::atomic::AtomicBool,
}

impl DeviceErrorSentry {
    fn record(&self, error: &wgpu::Error) {
        use std::sync::atomic::Ordering;
        self.poisoned.store(true, Ordering::Release);
        let count = self.errors.fetch_add(1, Ordering::Relaxed) + 1;
        if count.is_power_of_two() {
            log::error!("[gpu-device] uncaptured wgpu error #{count}: {error}");
        }
    }

    fn take_poison(&self) -> bool {
        self.poisoned
            .swap(false, std::sync::atomic::Ordering::AcqRel)
    }

    fn error_count(&self) -> u64 {
        self.errors.load(std::sync::atomic::Ordering::Relaxed)
    }
}

fn is_blend_mode_supported(mode: BlendMode) -> bool {
    matches!(
        mode,
        BlendMode::Src | BlendMode::SrcOver | BlendMode::DstOut
    )
}

fn blend_state_for_mode(mode: BlendMode) -> wgpu::BlendState {
    match mode {
        BlendMode::Src => wgpu::BlendState::REPLACE,
        BlendMode::DstOut => wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        },
        _ => wgpu::BlendState::ALPHA_BLENDING,
    }
}

pub(crate) fn supported_blend_mode(mode: BlendMode) -> BlendMode {
    if is_blend_mode_supported(mode) {
        return mode;
    }

    BlendMode::SrcOver
}

pub(crate) fn hash_f32_for_cache<H: Hasher>(value: f32, state: &mut H) {
    value.to_bits().hash(state);
}

fn hash_text_raster_geometry_for_cache<H: Hasher>(
    rect: Rect,
    static_text_motion: bool,
    state: &mut H,
) {
    hash_f32_for_cache(rect.width, state);
    hash_f32_for_cache(rect.height, state);
    static_text_motion.hash(state);
    if !static_text_motion {
        hash_f32_for_cache(rect.x.fract(), state);
        hash_f32_for_cache(rect.y.fract(), state);
    }
}

fn text_raster_geometry_for_draw(
    text_draw: &TextDraw,
    root_scale: f32,
) -> Option<(Rect, Rect, Option<Rect>, f32, bool)> {
    if text_draw.text.is_empty()
        || text_draw.rect.width <= 0.0
        || text_draw.rect.height <= 0.0
        || !root_scale.is_finite()
        || root_scale <= 0.0
    {
        return None;
    }

    let text_scale = text_draw.scale * root_scale;
    if !text_scale.is_finite() || text_scale <= 0.0 {
        return None;
    }

    let static_text_motion = text_draw
        .text_style
        .paragraph_style
        .text_motion
        .unwrap_or(cranpose_ui::text::TextMotion::Static)
        == cranpose_ui::text::TextMotion::Static;
    let snap_delta = text_draw
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
        .unwrap_or_default();
    let logical_rect = text_draw.rect.translate(snap_delta.x, snap_delta.y);
    let clip = text_draw.clip;
    let mut raster_rect = Rect {
        x: logical_rect.x * root_scale,
        y: logical_rect.y * root_scale,
        width: logical_rect.width * root_scale,
        height: logical_rect.height * root_scale,
    };
    if text_draw.snap_anchor.is_some() {
        raster_rect.x = canonicalize_device_coordinate(raster_rect.x);
        raster_rect.y = canonicalize_device_coordinate(raster_rect.y);
    }
    if static_text_motion {
        raster_rect.x = raster_rect.x.round();
        raster_rect.y = raster_rect.y.round();
    }
    raster_rect.width = raster_rect.width.ceil().max(1.0);
    raster_rect.height = raster_rect.height.ceil().max(1.0);
    Some((
        logical_rect,
        raster_rect,
        clip,
        text_scale,
        static_text_motion,
    ))
}

fn text_draw_is_visible_in_viewport(
    logical_rect: Rect,
    clip: Option<Rect>,
    viewport: ViewportUniformParams,
    root_scale: f32,
) -> bool {
    draw_rect_is_visible_in_viewport(logical_rect, clip, viewport, root_scale)
}

fn expand_rect(rect: Rect, margin_x: f32, margin_y: f32) -> Rect {
    Rect {
        x: rect.x - margin_x,
        y: rect.y - margin_y,
        width: rect.width + margin_x * 2.0,
        height: rect.height + margin_y * 2.0,
    }
}

fn draw_rect_is_visible_in_viewport(
    rect: Rect,
    clip: Option<Rect>,
    viewport: ViewportUniformParams,
    root_scale: f32,
) -> bool {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return false;
    }
    let viewport_rect = Rect {
        x: viewport.offset[0] / root_scale,
        y: viewport.offset[1] / root_scale,
        width: viewport.width as f32 / root_scale,
        height: viewport.height as f32 / root_scale,
    };
    rect_is_visible_in_rect(rect, clip, viewport_rect)
}

fn rect_is_visible_in_rect(rect: Rect, clip: Option<Rect>, viewport_rect: Rect) -> bool {
    let visible_rect = match clip {
        Some(clip) => clip.intersect(viewport_rect),
        None => Some(viewport_rect),
    };
    visible_rect.is_some_and(|visible| rect.intersect(visible).is_some())
}

fn snapped_quad_bounds(quad: [[f32; 2]; 4], anchor: Option<SnapAnchor>, root_scale: f32) -> Rect {
    let snap_delta = anchor
        .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
        .unwrap_or_default();
    quad_bounds(translate_quad(quad, snap_delta))
}

/// The logical rect a draw may touch: its snapped bounds within its clip,
/// `None` when the clip leaves nothing.
fn clipped_bounds(rect: Rect, clip: Option<Rect>) -> Option<Rect> {
    match clip {
        Some(clip) => rect.intersect(clip),
        None => Some(rect),
    }
}

pub(crate) fn text_draw_bounds(text: &TextDraw, root_scale: f32) -> Option<Rect> {
    text_raster_geometry_for_draw(text, root_scale)
        .and_then(|(logical_rect, _, clip, _, _)| clipped_bounds(logical_rect, clip))
}

pub(crate) fn image_draw_bounds(image: &ImageDraw, root_scale: f32) -> Option<Rect> {
    clipped_bounds(
        snapped_quad_bounds(image.quad, image.snap_anchor, root_scale),
        image.clip,
    )
}

pub(crate) fn run_draw_bounds(run: &RunDraw, root_scale: f32) -> Option<Rect> {
    let snap_delta = run
        .placement
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
        .unwrap_or_default();
    clipped_bounds(
        run.bounds.translate(snap_delta.x, snap_delta.y),
        run.placement.clip,
    )
}

pub(crate) fn text_draw_is_visible_in_rect(
    text: &TextDraw,
    viewport_rect: Rect,
    root_scale: f32,
) -> bool {
    text_draw_bounds(text, root_scale)
        .is_some_and(|bounds| bounds.intersect(viewport_rect).is_some())
}

pub(crate) fn run_draw_is_visible_in_rect(
    run: &RunDraw,
    viewport_rect: Rect,
    root_scale: f32,
) -> bool {
    run_draw_bounds(run, root_scale).is_some_and(|bounds| bounds.intersect(viewport_rect).is_some())
}

fn cached_text_glyph_quad(
    glyph: &SoftwareGlyphAtlasPlacement,
    entry: GlyphAtlasEntry,
    atlas_size: u32,
) -> CachedTextGlyphQuad {
    CachedTextGlyphQuad {
        x: glyph.x,
        y: glyph.y,
        width: glyph.width,
        height: glyph.height,
        color: (
            glyph.color.0.clamp(0.0, 1.0),
            glyph.color.1.clamp(0.0, 1.0),
            glyph.color.2.clamp(0.0, 1.0),
            glyph.color.3.clamp(0.0, 1.0),
        ),
        uv: glyph_atlas_uv_rect(entry, atlas_size),
    }
}

fn append_cached_text_glyph_quad(
    source_raster_rect: Rect,
    quad: &CachedTextGlyphQuad,
    image_vertices: &mut Vec<Vertex>,
    image_indices: &mut Vec<u32>,
) -> bool {
    if quad.width == 0 || quad.height == 0 || quad.color.3 <= 0.0 {
        return false;
    }

    let base_vertex = image_vertices.len() as u32;
    image_indices.extend_from_slice(&[
        base_vertex,
        base_vertex + 1,
        base_vertex + 2,
        base_vertex + 2,
        base_vertex + 1,
        base_vertex + 3,
    ]);

    let x0 = source_raster_rect.x + quad.x as f32;
    let y0 = source_raster_rect.y + quad.y as f32;
    let x1 = x0 + quad.width as f32;
    let y1 = y0 + quad.height as f32;
    let color = [quad.color.0, quad.color.1, quad.color.2, quad.color.3];

    image_vertices.extend_from_slice(&[
        Vertex {
            position: [x0, y0],
            color,
            uv: [quad.uv.min[0], quad.uv.min[1]],
            uv_bounds: quad.uv.sample_bounds,
        },
        Vertex {
            position: [x1, y0],
            color,
            uv: [quad.uv.max[0], quad.uv.min[1]],
            uv_bounds: quad.uv.sample_bounds,
        },
        Vertex {
            position: [x0, y1],
            color,
            uv: [quad.uv.min[0], quad.uv.max[1]],
            uv_bounds: quad.uv.sample_bounds,
        },
        Vertex {
            position: [x1, y1],
            color,
            uv: [quad.uv.max[0], quad.uv.max[1]],
            uv_bounds: quad.uv.sample_bounds,
        },
    ]);
    true
}

fn cached_text_glyph_quad_logical_rect(
    source_raster_rect: Rect,
    quad: &CachedTextGlyphQuad,
    root_scale: f32,
) -> Option<Rect> {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }
    Some(Rect {
        x: (source_raster_rect.x + quad.x as f32) / root_scale,
        y: (source_raster_rect.y + quad.y as f32) / root_scale,
        width: quad.width as f32 / root_scale,
        height: quad.height as f32 / root_scale,
    })
}

fn cached_text_glyph_quad_is_visible_in_viewport(
    source_raster_rect: Rect,
    quad: &CachedTextGlyphQuad,
    clip: Option<Rect>,
    viewport: ViewportUniformParams,
    root_scale: f32,
) -> bool {
    cached_text_glyph_quad_logical_rect(source_raster_rect, quad, root_scale)
        .is_some_and(|rect| draw_rect_is_visible_in_viewport(rect, clip, viewport, root_scale))
}

fn should_use_retained_text_glyph_run(quads_len: usize, clip: Option<Rect>) -> bool {
    clip.is_none() && quads_len >= MIN_RETAINED_TEXT_GLYPH_QUADS
}

const SHADOW_CACHE_DEVICE_QUANT: f32 = 16.0;

pub(crate) fn hash_shadow_device_offset<H: Hasher>(
    value: f32,
    origin: f32,
    root_scale: f32,
    state: &mut H,
) {
    let quantized = ((value - origin) * root_scale * SHADOW_CACHE_DEVICE_QUANT).round();
    (quantized as i64).hash(state);
}

pub(crate) fn hash_shadow_device_rect<H: Hasher>(
    rect: Rect,
    origin_x: f32,
    origin_y: f32,
    root_scale: f32,
    state: &mut H,
) {
    hash_shadow_device_offset(rect.x, origin_x, root_scale, state);
    hash_shadow_device_offset(rect.y, origin_y, root_scale, state);
    hash_shadow_device_offset(rect.width, 0.0, root_scale, state);
    hash_shadow_device_offset(rect.height, 0.0, root_scale, state);
}

fn hash_placement<H: Hasher>(
    placement: &crate::scene::Placement,
    origin_x: f32,
    origin_y: f32,
    root_scale: f32,
    state: &mut H,
) {
    hash_shadow_device_offset(placement.offset.x, origin_x, root_scale, state);
    hash_shadow_device_offset(placement.offset.y, origin_y, root_scale, state);
    match placement.snap_anchor {
        Some(anchor) => {
            1u8.hash(state);
            hash_shadow_device_offset(anchor.origin.x, origin_x, root_scale, state);
            hash_shadow_device_offset(anchor.origin.y, origin_y, root_scale, state);
            hash_f32_for_cache(anchor.device_pixel_step, state);
        }
        None => 0u8.hash(state),
    }
    match placement.clip {
        Some(clip) => {
            1u8.hash(state);
            hash_shadow_device_rect(clip, origin_x, origin_y, root_scale, state);
        }
        None => 0u8.hash(state),
    }
    hash_f32_for_cache(placement.alpha, state);
    match placement.color_filter {
        Some(filter) => {
            1u8.hash(state);
            filter.render_hash().hash(state);
        }
        None => 0u8.hash(state),
    }
}

/// Hashes what a run draws relative to `origin`: its records by
/// fingerprint and segment range, and its placement in device units, so a
/// run moving rigidly by whole pixels hashes the same.
pub(crate) fn hash_run_item<H: Hasher>(
    run: &RunDraw,
    origin_x: f32,
    origin_y: f32,
    root_scale: f32,
    state: &mut H,
) {
    run.tables().fingerprint().hash(state);
    run.segments.start.hash(state);
    run.segments.end.hash(state);
    hash_shadow_device_rect(run.bounds, origin_x, origin_y, root_scale, state);
    hash_placement(&run.placement, origin_x, origin_y, root_scale, state);
}

/// What a shadow's casters draw, independent of where the shadow sits to
/// the whole device pixel: the recordings, and the placement relative to
/// the casters' bounds.
pub(crate) fn shadow_content_hash(shadow: &ShadowDraw, root_scale: f32) -> u64 {
    let mut hasher = FxHasher::default();
    let origin = shape_shadow_bounds(shadow).unwrap_or(Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    });
    for run in shadow.shapes.iter().chain(&shadow.post_blur_cutouts) {
        hash_run_item(run, origin.x, origin.y, root_scale, &mut hasher);
    }
    hasher.finish()
}

fn shape_shadow_surface_cache_key(
    shadow: &ShadowDraw,
    device_bounds: DevicePixelBounds,
    pixel_radius: f32,
    root_scale: f32,
) -> Option<ShadowSurfaceCacheKey> {
    (root_scale.is_finite() && root_scale > 0.0).then(|| ShadowSurfaceCacheKey {
        content_hash: shadow_content_hash(shadow, root_scale),
        pixel_size: [device_bounds.width, device_bounds.height],
        root_scale_bits: root_scale.to_bits(),
        blur_radius_bits: pixel_radius.to_bits(),
    })
}

fn shape_shadow_bounds(shadow: &ShadowDraw) -> Option<Rect> {
    shadow.shapes.as_ref().map(|run| run.bounds)
}

pub(crate) fn shadow_draw_bounds(shadow: &ShadowDraw) -> Option<Rect> {
    shape_shadow_bounds(shadow)
        .into_iter()
        .chain(shadow.texts.iter().map(|text| text.rect))
        .reduce(|a, b| Rect {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            width: (a.x + a.width).max(b.x + b.width) - a.x.min(b.x),
            height: (a.y + a.height).max(b.y + b.height) - a.y.min(b.y),
        })
}

fn shape_shader_source(mode: RunBufferMode) -> Cow<'static, str> {
    if mode.storage {
        Cow::Owned(shaders::storage_shape_shader())
    } else {
        Cow::Borrowed(shaders::SHADER)
    }
}

/// A pipeline that draws one full-screen triangle strip from `fullscreen_vs`
/// into a single color target, the shape every effect and composite pass
/// shares; `constants` fixes the shader's override constants.
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_fullscreen_strip_pipeline(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    log_label: &str,
    label: &'static str,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    fragment_entry: &'static str,
    constants: &[(&str, f64)],
    target: wgpu::ColorTargetState,
) -> wgpu::RenderPipeline {
    create_render_pipeline_logged(
        device,
        cache,
        log_label,
        wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("fullscreen_vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants,
                    ..wgpu::PipelineCompilationOptions::default()
                },
            },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some(fragment_entry),
                targets: &[Some(target)],
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants,
                    ..wgpu::PipelineCompilationOptions::default()
                },
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        },
    )
}

pub(crate) fn create_render_pipeline_logged<'a>(
    device: &wgpu::Device,
    cache: Option<&'a wgpu::PipelineCache>,
    tag: &str,
    mut descriptor: wgpu::RenderPipelineDescriptor<'a>,
) -> wgpu::RenderPipeline {
    descriptor.cache = cache;
    let started = Instant::now();
    let pipeline = device.create_render_pipeline(&descriptor);
    log::info!(
        "[pipeline-create] {tag} {:.1}ms",
        instant_ms(started, Instant::now())
    );
    #[cfg(not(target_arch = "wasm32"))]
    crate::pipeline_disk_cache::note_pipeline_created();
    pipeline
}

/// Which tier's tables a shape pipeline reads: a stored run under the
/// placement uniform, or the frame arena where each record names its
/// placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RunTier {
    Store,
    Arena,
}

/// What every record of a draw agrees on, fixed into its pipeline as
/// shader constants so the fragment program carries only the branches
/// the draw can take: the shape kind when the segment holds one kind, a
/// solid-only brush, and whether the placement clips. The general
/// pipeline (`ShapeVariant::GENERAL`) keeps every branch and shades every
/// record the same, which `shape_variant_parity.rs` pins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ShapeVariant {
    kind: Option<u8>,
    solid: bool,
    clipped: bool,
}

impl ShapeVariant {
    const GENERAL: Self = Self {
        kind: None,
        solid: false,
        clipped: true,
    };

    pub(crate) fn of_segment(segment: &RecordSegment, clipped: bool) -> Self {
        if !shape_variants_enabled() {
            return Self::GENERAL;
        }
        Self {
            kind: segment.uniform_kind().map(|kind| kind as u8),
            solid: !segment.gradient,
            clipped,
        }
    }
}

static SHAPE_VARIANTS: DebugToggle = DebugToggle::new("CRANPOSE_SHAPE_VARIANTS");

fn shape_variants_enabled() -> bool {
    !SHAPE_VARIANTS.equals("0")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ShapePipelineKey {
    pub(crate) blend_mode: BlendMode,
    pub(crate) tier: RunTier,
    pub(crate) variant: ShapeVariant,
}

impl ShapePipelineKey {
    pub(crate) fn general_for(blend_mode: BlendMode, tier: RunTier) -> Self {
        Self {
            blend_mode,
            tier,
            variant: ShapeVariant::GENERAL,
        }
    }

    pub(crate) fn general(self) -> Self {
        Self::general_for(self.blend_mode, self.tier)
    }

    pub(crate) fn is_general(self) -> bool {
        self.variant == ShapeVariant::GENERAL
    }
}

pub(crate) fn create_shape_pipeline(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    surface_format: wgpu::TextureFormat,
    uniform_layout: &wgpu::BindGroupLayout,
    run_layout: &wgpu::BindGroupLayout,
    key: ShapePipelineKey,
    mode: RunBufferMode,
) -> wgpu::RenderPipeline {
    let ShapePipelineKey {
        blend_mode,
        tier,
        variant,
    } = key;
    let constants = [
        ("SHAPE_KIND_FIXED", variant.kind.map_or(-1.0, f64::from)),
        ("SHAPE_SOLID", f64::from(u8::from(variant.solid))),
        ("SHAPE_CLIPPED", f64::from(u8::from(variant.clipped))),
        ("TIER_ARENA", f64::from(u8::from(tier == RunTier::Arena))),
        ("SHAPE_BANDS", f64::from(u8::from(mode.storage))),
    ];
    let vertex_entry = if variant.solid {
        "vs_record_solid"
    } else {
        "vs_record"
    };
    let fragment_entry = if variant.solid { "fs_solid" } else { "fs_main" };
    let instance_layout = record_vertex_layouts();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Shape Shader"),
        source: wgpu::ShaderSource::Wgsl(shape_shader_source(mode)),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Shape Pipeline Layout"),
        bind_group_layouts: &[Some(uniform_layout), Some(run_layout)],
        immediate_size: 0,
    });

    create_render_pipeline_logged(
        device,
        cache,
        &format!("shape blend={blend_mode:?} tier={tier:?} variant={variant:?}"),
        wgpu::RenderPipelineDescriptor {
            label: Some("Shape Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(vertex_entry),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..wgpu::PipelineCompilationOptions::default()
                },
                buffers: &instance_layout,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(fragment_entry),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..wgpu::PipelineCompilationOptions::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(blend_state_for_mode(blend_mode)),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        },
    )
}
fn create_image_pipeline(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    surface_format: wgpu::TextureFormat,
    uniform_layout: &wgpu::BindGroupLayout,
    image_layout: &wgpu::BindGroupLayout,
    blend_mode: BlendMode,
) -> wgpu::RenderPipeline {
    let image_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Image Shader"),
        source: wgpu::ShaderSource::Wgsl(shaders::IMAGE_SHADER.into()),
    });

    let image_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Image Pipeline Layout"),
        bind_group_layouts: &[Some(uniform_layout), Some(image_layout)],
        immediate_size: 0,
    });

    create_render_pipeline_logged(
        device,
        cache,
        &format!("image blend={blend_mode:?}"),
        wgpu::RenderPipelineDescriptor {
            label: Some("Image Pipeline"),
            layout: Some(&image_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &image_shader,
                entry_point: Some("image_vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &image_shader,
                entry_point: Some("image_fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(blend_state_for_mode(blend_mode)),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        },
    )
}

fn create_glyph_atlas_pipeline(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    surface_format: wgpu::TextureFormat,
    uniform_layout: &wgpu::BindGroupLayout,
    image_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Glyph Atlas Shader"),
        source: wgpu::ShaderSource::Wgsl(shaders::GLYPH_ATLAS_SHADER.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Glyph Atlas Pipeline Layout"),
        bind_group_layouts: &[Some(uniform_layout), Some(image_layout)],
        immediate_size: 0,
    });

    create_render_pipeline_logged(
        device,
        cache,
        "glyph-atlas",
        wgpu::RenderPipelineDescriptor {
            label: Some("Glyph Atlas Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("glyph_atlas_vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("glyph_atlas_fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(blend_state_for_mode(BlendMode::SrcOver)),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        },
    )
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
    uv: [f32; 2],
    uv_bounds: [f32; 4],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x4,
        2 => Float32x2,
        3 => Float32x4
    ];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Uniforms {
    viewport: [f32; 2],
    viewport_offset: [f32; 2],
    placement: PlacementData,
}

static SURVIVE_GPU_ERRORS: DebugToggle = DebugToggle::new("CRANPOSE_SURVIVE_GPU_ERRORS");

fn survive_gpu_errors_enabled() -> bool {
    !SURVIVE_GPU_ERRORS.equals("0")
}

struct CachedImageTexture {
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    nearest_bind_group: wgpu::BindGroup,
    linear_bind_group: wgpu::BindGroup,
    bytes: usize,
}

impl CachedImageTexture {
    fn bind_group(&self, sampling: ImageSampling) -> &wgpu::BindGroup {
        match sampling {
            ImageSampling::Nearest => &self.nearest_bind_group,
            ImageSampling::Linear => &self.linear_bind_group,
        }
    }
}

#[derive(Clone, Copy)]
struct GlyphAtlasEntry {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn next_glyph_atlas_size(current: u32, max: u32) -> u32 {
    current.saturating_mul(2).clamp(1, max.max(1))
}

struct TextGlyphAtlas {
    texture: wgpu::Texture,
    _view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    entries: BoundedLruCache<SoftwareGlyphAtlasKey, GlyphAtlasEntry>,
    generation: u64,
    size: u32,
    max_size: u32,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    upload_scratch: Vec<u8>,
}

impl TextGlyphAtlas {
    fn new(
        device: &wgpu::Device,
        image_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        size: u32,
    ) -> Self {
        let max_size = TEXT_GLYPH_ATLAS_MAX_SIZE.min(device.limits().max_texture_dimension_2d);
        let size = size.clamp(TEXT_GLYPH_ATLAS_MIN_SIZE.min(max_size), max_size);
        let texture = Self::create_texture(device, size);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Text Glyph Atlas Bind Group"),
            layout: image_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        Self {
            texture,
            _view: view,
            bind_group,
            entries: BoundedLruCache::with_capacity_at_least_one(MAX_TEXT_GLYPH_ATLAS_ITEMS),
            generation: 0,
            size,
            max_size,
            cursor_x: TEXT_GLYPH_ATLAS_PADDING,
            cursor_y: TEXT_GLYPH_ATLAS_PADDING,
            row_height: 0,
            upload_scratch: Vec::new(),
        }
    }

    fn create_texture(device: &wgpu::Device, size: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Text Glyph Atlas Texture"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn reset(
        &mut self,
        device: &wgpu::Device,
        image_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) {
        let generation = self.generation.wrapping_add(1);
        let grown = next_glyph_atlas_size(self.size, self.max_size);
        let mut next = Self::new(device, image_layout, sampler, grown);
        next.generation = generation;
        *self = next;
    }

    fn generation(&self) -> u64 {
        self.generation
    }

    fn size(&self) -> u32 {
        self.size
    }

    fn entry(&mut self, key: &SoftwareGlyphAtlasKey) -> Option<GlyphAtlasEntry> {
        self.entries.get(key).copied()
    }

    fn allocate(&mut self, width: u32, height: u32) -> Option<GlyphAtlasEntry> {
        if width == 0
            || height == 0
            || width + TEXT_GLYPH_ATLAS_PADDING * 2 > self.size
            || height + TEXT_GLYPH_ATLAS_PADDING * 2 > self.size
        {
            return None;
        }

        if self.cursor_x + width + TEXT_GLYPH_ATLAS_PADDING > self.size {
            self.cursor_x = TEXT_GLYPH_ATLAS_PADDING;
            self.cursor_y = self
                .cursor_y
                .saturating_add(self.row_height)
                .saturating_add(TEXT_GLYPH_ATLAS_PADDING);
            self.row_height = 0;
        }
        if self.cursor_y + height + TEXT_GLYPH_ATLAS_PADDING > self.size {
            return None;
        }

        let entry = GlyphAtlasEntry {
            x: self.cursor_x,
            y: self.cursor_y,
            width,
            height,
        };
        self.cursor_x = self
            .cursor_x
            .saturating_add(width)
            .saturating_add(TEXT_GLYPH_ATLAS_PADDING);
        self.row_height = self.row_height.max(height);
        Some(entry)
    }

    fn upload_glyph(
        &mut self,
        key: SoftwareGlyphAtlasKey,
        glyph: &SoftwareGlyphAtlasGlyph,
        queue: &wgpu::Queue,
        executor: &mut WgpuFrameGraphExecutor,
        frame_stats: &mut gpu_stats::FrameStats,
    ) -> Option<GlyphAtlasEntry> {
        if let Some(entry) = self.entry(&key) {
            frame_stats.record_text_glyph_atlas_hit();
            return Some(entry);
        }

        let width = u32::try_from(glyph.mask.width).ok()?;
        let height = u32::try_from(glyph.mask.height).ok()?;
        let entry = self.allocate(width, height)?;
        self.upload_scratch.clear();
        self.upload_scratch.reserve(
            glyph
                .mask
                .alpha
                .len()
                .saturating_sub(self.upload_scratch.capacity()),
        );
        self.upload_scratch.extend(
            glyph
                .mask
                .alpha
                .iter()
                .map(|alpha| (alpha.clamp(0.0, 1.0) * 255.0).round() as u8),
        );

        let upload_stats = executor.upload_texture(
            queue,
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: entry.x,
                    y: entry.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &self.upload_scratch,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(entry.width),
                rows_per_image: Some(entry.height),
            },
            wgpu::Extent3d {
                width: entry.width,
                height: entry.height,
                depth_or_array_layers: 1,
            },
        );
        frame_stats.record_command_stats(upload_stats);
        frame_stats.record_text_glyph_atlas_miss(entry.width, entry.height);
        self.entries.put(key, entry);
        Some(entry)
    }
}

pub(crate) struct ImageDrawCmd {
    index_start: u32,
    scissor: (u32, u32, u32, u32),
    image_id: u64,
    sampling: ImageSampling,
}

#[derive(Clone, Copy)]
enum GlyphDrawSource {
    Shared {
        index_start: u32,
        index_count: u32,
    },
    Retained {
        cache_key: TextGlyphRunCacheKey,
        uniform_slot: usize,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct GlyphDrawCmd {
    source: GlyphDrawSource,
    scissor: (u32, u32, u32, u32),
}

impl GlyphDrawCmd {
    fn shared(index_start: u32, index_count: u32, scissor: (u32, u32, u32, u32)) -> Self {
        Self {
            source: GlyphDrawSource::Shared {
                index_start,
                index_count,
            },
            scissor,
        }
    }

    fn retained(
        cache_key: TextGlyphRunCacheKey,
        uniform_slot: usize,
        scissor: (u32, u32, u32, u32),
    ) -> Self {
        Self {
            source: GlyphDrawSource::Retained {
                cache_key,
                uniform_slot,
            },
            scissor,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ImageUvRect {
    min: [f32; 2],
    max: [f32; 2],
    sample_bounds: [f32; 4],
}

/// A growable vertex buffer and index buffer pair.
/// One pass's image and shared glyph quads: the frame's vertex and index
/// uploads they were appended to.
pub(crate) struct ImageSlot {
    vertices: BufferUpload,
    indices: BufferUpload,
}

fn image_vertex_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::vertex("Image Vertex Buffer", std::mem::size_of::<Vertex>() as u64)
}

fn image_index_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::index("Image Index Buffer", std::mem::size_of::<u32>() as u64)
}

/// One frame's viewport uniforms. Every pass, and every retained glyph run
/// (which carries its own raster-space origin), claims a slot; the slots are
/// dynamic offsets into one buffer and reach the GPU in a single write when
/// the frame is flushed.
struct ViewportUniformRing {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    stride: usize,
    capacity: usize,
    cursor: usize,
    written: Vec<u8>,
}

impl ViewportUniformRing {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, capacity: usize) -> Self {
        let stride = align_usize_to(
            std::mem::size_of::<Uniforms>(),
            (device.limits().min_uniform_buffer_offset_alignment as usize)
                .max(wgpu::COPY_BUFFER_ALIGNMENT as usize),
        );
        let (buffer, bind_group) = Self::create(device, layout, stride, capacity);
        Self {
            buffer,
            bind_group,
            stride,
            capacity,
            cursor: 0,
            written: Vec::new(),
        }
    }

    fn create(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        stride: usize,
        capacity: usize,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Viewport Uniform Buffer"),
            size: (stride * capacity) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Viewport Uniform Bind Group"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                }),
            }],
        });
        (buffer, bind_group)
    }

    fn begin_frame(&mut self) {
        self.cursor = 0;
        self.written.clear();
    }

    fn claim(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniforms: &Uniforms,
    ) -> usize {
        let slot = self.cursor;
        self.cursor += 1;
        if slot >= self.capacity {
            self.capacity = (self.capacity * 2).max(slot + 1);
            let (buffer, bind_group) = Self::create(device, layout, self.stride, self.capacity);
            self.buffer = buffer;
            self.bind_group = bind_group;
        }
        let offset = self.stride * slot;
        self.written.resize(offset + self.stride, 0);
        let bytes = bytemuck::bytes_of(uniforms);
        self.written[offset..offset + bytes.len()].copy_from_slice(bytes);
        slot
    }

    fn dynamic_offset(&self, slot: usize) -> Result<u32, String> {
        if slot >= self.cursor {
            return Err("viewport uniform slot was never claimed this frame".to_string());
        }
        u32::try_from(self.stride * slot)
            .map_err(|_| "viewport uniform offset exceeds the dynamic offset range".to_string())
    }

    fn flush(&self, queue: &wgpu::Queue) -> FrameCommandStats {
        write_buffer(queue, &self.buffer, 0, &self.written)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewportUniformParams {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) offset: [f32; 2],
}

/// A stored run's draws for one pass: its tables by command, the uniform
/// slot holding its placement, and the pipeline and vertex range of each
/// segment's quads and bands.
pub(crate) struct StoreRunBatch {
    pub(crate) command: DrawCommandId,
    pub(crate) uniform_slot: usize,
    pub(crate) draws: SmallVec<[RunDrawCall; 8]>,
}

struct CompositionTarget {
    target: Rc<OffscreenTarget>,
    output_bind_group: wgpu::BindGroup,
}

/// Where a frame renders: straight into the presentable image, or into the
/// reusable composition target that the output conversion then copies out.
enum FrameRoot {
    Surface(Rc<OffscreenTarget>),
    Composition(CompositionTarget),
}

impl FrameRoot {
    fn target(&self) -> &Rc<OffscreenTarget> {
        match self {
            Self::Surface(target) => target,
            Self::Composition(composition) => &composition.target,
        }
    }

    /// The output conversion's destination and source bind group; nothing
    /// when the frame already rendered into the presentable image.
    fn output<'a>(
        &'a self,
        output_view: Option<&'a wgpu::TextureView>,
        screenshot_bind_group: Option<&'a wgpu::BindGroup>,
    ) -> Option<(&'a wgpu::TextureView, &'a wgpu::BindGroup)> {
        match self {
            Self::Surface(_) => None,
            Self::Composition(composition) => output_view.map(|view| {
                (
                    view,
                    screenshot_bind_group.unwrap_or(&composition.output_bind_group),
                )
            }),
        }
    }
}

const DIRECT_SURFACE_ROOT_USAGES: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT
    .union(wgpu::TextureUsages::TEXTURE_BINDING)
    .union(wgpu::TextureUsages::COPY_SRC)
    .union(wgpu::TextureUsages::COPY_DST);

/// The usages a presentable image needs to serve as the frame's root
/// target: rendering plus the capture usages the composition target has.
/// Callers configuring a surface ask for them when the surface offers them
/// all; a partial set falls back to the composition copy, so nothing is
/// requested in that case beyond rendering.
pub fn presentable_root_usages(supported: wgpu::TextureUsages) -> wgpu::TextureUsages {
    if supported.contains(DIRECT_SURFACE_ROOT_USAGES) {
        DIRECT_SURFACE_ROOT_USAGES
    } else {
        wgpu::TextureUsages::RENDER_ATTACHMENT
    }
}

/// Whether the presented image can be the frame's root target: its bytes
/// are the composition format (so the 8-bit output conversion would be an
/// identity), it can be captured and sampled the way the composition target
/// is, and it is the viewport's size.
fn surface_is_direct_root(
    texture: &wgpu::Texture,
    composition_format: wgpu::TextureFormat,
    viewport: (u32, u32),
) -> bool {
    texture.format().remove_srgb_suffix() == composition_format
        && texture.usage().contains(DIRECT_SURFACE_ROOT_USAGES)
        && (texture.width(), texture.height()) == viewport
}

#[derive(Clone, Copy)]
enum OutputMode {
    Display,
    Screenshot,
}

pub struct GpuRenderer {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    device_errors: Arc<DeviceErrorSentry>,
    renderer_epoch: u64,
    pub(crate) composition_format: wgpu::TextureFormat,
    #[cfg(not(target_arch = "wasm32"))]
    display_format: wgpu::TextureFormat,
    composition_target: Option<CompositionTarget>,
    output_converter: OutputConverter,
    screenshot_converter: OutputConverter,
    adapter_backend: wgpu::Backend,
    pipeline_cache: Option<wgpu::PipelineCache>,
    shape_pipelines: ShapePipelines,
    image_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    image_pipeline_dst_out: LazyGpuResource<wgpu::RenderPipeline>,
    glyph_atlas_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    uniform_bind_group_layout: wgpu::BindGroupLayout,
    image_bind_group_layout: wgpu::BindGroupLayout,
    image_nearest_sampler: wgpu::Sampler,
    image_linear_sampler: wgpu::Sampler,
    text_fonts: SoftwareTextFontSet,
    viewport_uniforms: ViewportUniformRing,
    run_store: RunStore,
    image_texture_cache: BoundedLruCache<u64, CachedImageTexture>,
    image_texture_cache_bytes: usize,
    text_image_cache: BoundedLruCache<TextImageCacheKey, CachedTextImage>,
    text_glyph_atlas: TextGlyphAtlas,
    text_glyph_run_cache: BoundedLruCache<TextGlyphRunCacheKey, CachedTextGlyphRun>,
    text_glyph_gpu_run_cache: BoundedLruCache<TextGlyphRunCacheKey, CachedGpuTextGlyphRun>,
    text_glyph_mask_cache: SoftwareGlyphRasterCache,
    text_line_index_cache: TextLineIndexCache,
    pub(crate) scratch_image_vertices: Vec<Vertex>,
    pub(crate) scratch_image_indices: Vec<u32>,
    pub(crate) scratch_image_cmds: Vec<ImageDrawCmd>,
    pub(crate) scratch_glyph_cmds: Vec<GlyphDrawCmd>,
    scratch_text_glyph_run: Vec<SoftwareGlyphAtlasRunGlyph>,
    scratch_text_glyph_placements: Vec<SoftwareGlyphAtlasPlacement>,
    scratch_text_glyph_quads: Vec<CachedTextGlyphQuad>,
    frame_graph_executor: WgpuFrameGraphExecutor,
    deferred_offscreen_releases: Vec<OffscreenTarget>,
    pub(crate) effect_renderer: EffectRenderer,
    pub(crate) layer_cache: LayerCache,
    pub(crate) ablation: Ablation,
    pub(crate) ablation_frames: u32,
    pub(crate) backdrop_gates: HashMap<NodeId, AdmissionGate>,
    pub(crate) fill_gates: HashMap<DrawCommandId, AdmissionGate>,
    transparent_sources: HashMap<(u32, u32), Rc<OffscreenTarget>>,
    shadow_surface_cache: BoundedLruCache<ShadowSurfaceCacheKey, CachedShadowSurface>,
    shadow_surface_cache_bytes: u64,
    pub(crate) frame_stats: gpu_stats::FrameStats,
    last_frame_stats: Option<gpu_stats::FrameStatsSnapshot>,
    pending_frame_warmup_frames: u8,
    frame_count: u64,
}

fn image_sampler_descriptor(sampling: ImageSampling) -> wgpu::SamplerDescriptor<'static> {
    let filter = match sampling {
        ImageSampling::Nearest => wgpu::FilterMode::Nearest,
        ImageSampling::Linear => wgpu::FilterMode::Linear,
    };
    wgpu::SamplerDescriptor {
        label: Some(match sampling {
            ImageSampling::Nearest => "Nearest Image Sampler",
            ImageSampling::Linear => "Linear Image Sampler",
        }),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    }
}

impl GpuRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        adapter_backend: wgpu::Backend,
        adapter_downlevel: wgpu::DownlevelFlags,
        text_fonts: SoftwareTextFontSet,
        renderer_epoch: u64,
    ) -> Self {
        let display_format = surface_format;
        let composition_format = composition_format();
        let construction_started = Instant::now();
        let device_errors = Arc::new(DeviceErrorSentry::default());
        if survive_gpu_errors_enabled() {
            let sentry = Arc::clone(&device_errors);
            device.on_uncaptured_error(Arc::new(move |error| sentry.record(&error)));
        }
        device.set_device_lost_callback(|reason, message| {
            log::error!("[gpu-device] device lost ({reason:?}): {message}");
        });
        let run_store = RunStore::new(
            &device,
            RunBufferMode::for_device(&device, adapter_downlevel),
        );
        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Viewport Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<Uniforms>() as u64
                        ),
                    },
                    count: None,
                }],
            });
        let image_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Image Texture Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let image_nearest_sampler =
            device.create_sampler(&image_sampler_descriptor(ImageSampling::Nearest));
        let image_linear_sampler =
            device.create_sampler(&image_sampler_descriptor(ImageSampling::Linear));
        let text_glyph_atlas = TextGlyphAtlas::new(
            &device,
            &image_bind_group_layout,
            &image_nearest_sampler,
            TEXT_GLYPH_ATLAS_MIN_SIZE,
        );
        let viewport_uniforms = ViewportUniformRing::new(
            &device,
            &uniform_bind_group_layout,
            INITIAL_VIEWPORT_UNIFORM_SLOTS,
        );

        #[cfg(not(target_arch = "wasm32"))]
        let pipeline_cache = crate::pipeline_disk_cache::load(&device);
        #[cfg(target_arch = "wasm32")]
        let pipeline_cache: Option<wgpu::PipelineCache> = None;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(cache) = pipeline_cache.clone() {
            crate::pipeline_disk_cache::spawn_persist_watcher(cache);
        }

        let effects_started = Instant::now();
        let effect_renderer = EffectRenderer::new(
            &device,
            pipeline_cache.clone(),
            composition_format,
            adapter_backend,
        );
        let output_converter = OutputConverter::new(&device, display_format);
        let screenshot_converter = OutputConverter::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        let effects_ms = instant_ms(effects_started, Instant::now());
        let mut frame_graph_executor = WgpuFrameGraphExecutor::new();
        frame_graph_executor.init_pass_timing(&device, &queue);
        let shape_pipelines = ShapePipelines::new(
            ShapePipelineFactory {
                device: Arc::clone(&device),
                cache: pipeline_cache.clone(),
                format: composition_format,
                uniform_layout: uniform_bind_group_layout.clone(),
                run_layout: run_store.layout().clone(),
                mode: run_store.mode(),
            },
            adapter_backend,
        );

        let renderer = Self {
            device,
            queue,
            device_errors,
            renderer_epoch,
            composition_format,
            #[cfg(not(target_arch = "wasm32"))]
            display_format,
            composition_target: None,
            output_converter,
            screenshot_converter,
            adapter_backend,
            pipeline_cache,
            shape_pipelines,
            image_pipeline: LazyGpuResource::new("image/src-over"),
            image_pipeline_dst_out: LazyGpuResource::new("image/dst-out"),
            glyph_atlas_pipeline: LazyGpuResource::new("glyph/atlas"),
            uniform_bind_group_layout,
            image_bind_group_layout,
            image_nearest_sampler,
            image_linear_sampler,
            text_fonts,
            viewport_uniforms,
            run_store,
            image_texture_cache: BoundedLruCache::with_capacity_at_least_one(
                MAX_TEXTURE_CACHE_ITEMS,
            ),
            image_texture_cache_bytes: 0,
            text_image_cache: BoundedLruCache::with_capacity_at_least_one(
                MAX_TEXT_IMAGE_CACHE_ITEMS,
            ),
            text_glyph_atlas,
            text_glyph_run_cache: BoundedLruCache::with_capacity_at_least_one(
                MAX_TEXT_GLYPH_RUN_CACHE_ITEMS,
            ),
            text_glyph_gpu_run_cache: BoundedLruCache::with_capacity_at_least_one(
                MAX_TEXT_GLYPH_GPU_RUN_CACHE_ITEMS,
            ),
            text_glyph_mask_cache: SoftwareGlyphRasterCache::with_capacity_at_least_one(
                MAX_TEXT_GLYPH_MASK_CACHE_ITEMS,
            ),
            text_line_index_cache: TextLineIndexCache::new(MAX_TEXT_LINE_INDEX_CACHE_ITEMS),
            scratch_image_vertices: Vec::new(),
            scratch_image_indices: Vec::new(),
            scratch_image_cmds: Vec::new(),
            scratch_glyph_cmds: Vec::new(),
            scratch_text_glyph_run: Vec::new(),
            scratch_text_glyph_placements: Vec::new(),
            scratch_text_glyph_quads: Vec::new(),
            frame_graph_executor,
            deferred_offscreen_releases: Vec::new(),
            effect_renderer,
            layer_cache: LayerCache::new(),
            ablation: Ablation::default(),
            ablation_frames: 0,
            backdrop_gates: HashMap::new(),
            fill_gates: HashMap::new(),
            transparent_sources: HashMap::new(),
            shadow_surface_cache: BoundedLruCache::with_capacity_at_least_one(
                MAX_SHADOW_SURFACE_CACHE_ITEMS,
            ),
            shadow_surface_cache_bytes: 0,
            frame_stats: gpu_stats::FrameStats::default(),
            last_frame_stats: None,
            pending_frame_warmup_frames: 0,
            frame_count: 0,
        };
        log::info!(
            "[gpu-init] {:?} renderer ready in {:.1} ms (effects {:.1} ms)",
            adapter_backend,
            instant_ms(construction_started, Instant::now()),
            effects_ms,
        );
        renderer
    }

    fn ensure_shape_pipeline(&mut self, key: ShapePipelineKey) {
        self.shape_pipelines.ensure(key);
    }

    fn image_pipeline(&self, blend_mode: BlendMode) -> &wgpu::RenderPipeline {
        let resource = match blend_mode {
            BlendMode::DstOut => &self.image_pipeline_dst_out,
            _ => &self.image_pipeline,
        };
        resource.get_or_init(self.adapter_backend, || {
            create_image_pipeline(
                &self.device,
                self.pipeline_cache.as_ref(),
                self.composition_format,
                &self.uniform_bind_group_layout,
                &self.image_bind_group_layout,
                blend_mode,
            )
        })
    }

    fn glyph_atlas_pipeline(&self) -> &wgpu::RenderPipeline {
        self.glyph_atlas_pipeline
            .get_or_init(self.adapter_backend, || {
                create_glyph_atlas_pipeline(
                    &self.device,
                    self.pipeline_cache.as_ref(),
                    self.composition_format,
                    &self.uniform_bind_group_layout,
                    &self.image_bind_group_layout,
                )
            })
    }

    fn ensure_image_cached(&mut self, image: &ImageBitmap) -> Result<(), String> {
        if self.image_texture_cache.get(&image.id()).is_some() {
            return Ok(());
        }

        let size = wgpu::Extent3d {
            width: image.width(),
            height: image.height(),
            depth_or_array_layers: 1,
        };

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Image Texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let upload_stats = self.frame_graph_executor.upload_texture(
            &self.queue,
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * image.width()),
                rows_per_image: Some(image.height()),
            },
            size,
        );
        self.frame_stats.record_command_stats(upload_stats);

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let nearest_bind_group = self.image_bind_group(&view, &self.image_nearest_sampler);
        let linear_bind_group = self.image_bind_group(&view, &self.image_linear_sampler);

        let bytes = image.width() as usize * image.height() as usize * 4;
        if let Some(replaced) = self.image_texture_cache.put(
            image.id(),
            CachedImageTexture {
                _texture: texture,
                _view: view,
                nearest_bind_group,
                linear_bind_group,
                bytes,
            },
        ) {
            self.image_texture_cache_bytes = self
                .image_texture_cache_bytes
                .saturating_sub(replaced.bytes);
        }
        self.image_texture_cache_bytes += bytes;
        while self.image_texture_cache_bytes > MAX_IMAGE_TEXTURE_CACHE_BYTES
            && self.image_texture_cache.len() > 1
        {
            let Some((_, evicted)) = self.image_texture_cache.pop_lru() else {
                break;
            };
            self.image_texture_cache_bytes =
                self.image_texture_cache_bytes.saturating_sub(evicted.bytes);
        }
        Ok(())
    }

    fn image_bind_group(
        &self,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Image Texture Bind Group"),
            layout: &self.image_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    pub(crate) fn max_texture_dim(&self) -> u32 {
        self.effect_renderer.max_texture_dim()
    }

    /// A pooled texture that outlives the frame: layer cache entries and
    /// cached shadow surfaces.
    pub(crate) fn acquire_retained_surface(&mut self, width: u32, height: u32) -> OffscreenTarget {
        self.effect_renderer
            .acquire_offscreen(&self.device, width, height, Some(&self.frame_stats))
    }

    fn frame_root(
        &mut self,
        output_mode: OutputMode,
        output_view: Option<&wgpu::TextureView>,
        output_texture: Option<&wgpu::Texture>,
        viewport: (u32, u32),
    ) -> FrameRoot {
        if let (OutputMode::Display, Some(view), Some(texture)) =
            (output_mode, output_view, output_texture)
            && surface_is_direct_root(texture, self.composition_format, viewport)
        {
            return FrameRoot::Surface(Rc::new(OffscreenTarget::from_surface(
                texture.clone(),
                view.clone(),
            )));
        }
        FrameRoot::Composition(self.take_composition_target(viewport.0.max(1), viewport.1.max(1)))
    }

    fn take_composition_target(&mut self, width: u32, height: u32) -> CompositionTarget {
        if let Some(target) = self.composition_target.take()
            && target.target.width == width
            && target.target.height == height
        {
            return target;
        }
        let target = Rc::new(OffscreenTarget::new(
            &self.device,
            self.composition_format,
            width,
            height,
        ));
        let output_bind_group = self.output_converter.bind_group(&self.device, &target.view);
        CompositionTarget {
            target,
            output_bind_group,
        }
    }

    fn transient_offscreen_descriptor(
        &self,
        label: &'static str,
        width: u32,
        height: u32,
    ) -> FrameTextureDescriptor {
        let max_texture_dim = self.max_texture_dim();
        FrameTextureDescriptor::render_attachment(
            label,
            width.min(max_texture_dim),
            height.min(max_texture_dim),
            self.composition_format,
        )
    }

    /// A texture of the given size that stays transparent: the input of a
    /// runtime shader whose layer draws nothing itself, so the shader needs
    /// no surface pass and reads the same empty content every frame.
    pub(crate) fn transparent_source<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        width: u32,
        height: u32,
    ) -> Rc<OffscreenTarget> {
        if let Some(source) = self.transparent_sources.get(&(width, height)) {
            return Rc::clone(source);
        }
        if self.transparent_sources.len() >= MAX_TRANSPARENT_SOURCES {
            for (_, source) in self.transparent_sources.drain() {
                if let Ok(target) = Rc::try_unwrap(source) {
                    self.deferred_offscreen_releases.push(target);
                }
            }
        }
        let source = Rc::new(self.acquire_retained_surface(width, height));
        self.clear_target(
            recorder,
            &source.view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        self.transparent_sources
            .insert((width, height), Rc::clone(&source));
        source
    }

    fn defer_offscreen_release(&mut self, target: OffscreenTarget) {
        self.deferred_offscreen_releases.push(target);
    }

    fn flush_deferred_offscreen_releases(&mut self) {
        self.backdrop_gates.retain(|_, gate| gate.end_frame());
        self.fill_gates.retain(|_, gate| gate.end_frame());
        for target in self.deferred_offscreen_releases.drain(..) {
            self.effect_renderer.release_offscreen(target);
        }
        for (transient, target) in self.layer_cache.take_released() {
            match transient {
                Some(descriptor) => self
                    .frame_graph_executor
                    .release_transient(descriptor, target),
                None => self.effect_renderer.release_offscreen(target),
            }
        }
    }

    fn insert_cached_shadow_surface(
        &mut self,
        key: ShadowSurfaceCacheKey,
        target: Rc<OffscreenTarget>,
    ) {
        let byte_size = offscreen_byte_size(target.width, target.height);
        while self.shadow_surface_cache_bytes + byte_size > MAX_SHADOW_SURFACE_CACHE_BYTES {
            let Some((_, evicted)) = self.shadow_surface_cache.pop_lru() else {
                break;
            };
            self.shadow_surface_cache_bytes = self
                .shadow_surface_cache_bytes
                .saturating_sub(evicted.byte_size);
        }
        let cached = CachedShadowSurface { target, byte_size };
        if let Some((_, replaced)) = self.shadow_surface_cache.push(key, cached) {
            self.shadow_surface_cache_bytes = self
                .shadow_surface_cache_bytes
                .saturating_sub(replaced.byte_size);
        }
        self.shadow_surface_cache_bytes = self.shadow_surface_cache_bytes.saturating_add(byte_size);
    }
}
fn frame_stats_need_warmup_frame(snapshot: &gpu_stats::FrameStatsSnapshot) -> bool {
    snapshot.layer_cache_misses > 0
        || snapshot.shadow_shape_cache_misses > 0
        || snapshot.text_image_cache_misses > 0
        || snapshot.text_glyph_atlas_misses > 0
}

fn update_frame_warmup_budget(pending_frames: &mut u8, snapshot: &gpu_stats::FrameStatsSnapshot) {
    if *pending_frames > 0 {
        *pending_frames = pending_frames.saturating_sub(1);
    } else if frame_stats_need_warmup_frame(snapshot) {
        *pending_frames = CACHE_MISS_WARMUP_FRAMES;
    }
}

impl GpuRenderer {
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        texture: &wgpu::Texture,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        packet: FramePacket,
        surface_epoch: u64,
        returns: &mut RenderReturns,
    ) -> Result<(), String> {
        self.render_internal(
            width,
            height,
            packet,
            surface_epoch,
            returns,
            OutputMode::Display,
            Some(view),
            Some(texture),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_internal(
        &mut self,
        width: u32,
        height: u32,
        packet: FramePacket,
        surface_epoch: u64,
        returns: &mut RenderReturns,
        output_mode: OutputMode,
        output_view: Option<&wgpu::TextureView>,
        output_texture: Option<&wgpu::Texture>,
    ) -> Result<(), String> {
        let cancel_reason = if packet.renderer_epoch != self.renderer_epoch {
            Some(CancelReason::RendererEpoch)
        } else if packet.surface_epoch != surface_epoch {
            Some(CancelReason::SurfaceEpoch)
        } else if packet.viewport != (width, height) {
            Some(CancelReason::Viewport)
        } else {
            None
        };
        if let Some(reason) = cancel_reason {
            return Self::cancel_packet(packet, reason, returns);
        }
        if self.device_errors.take_poison() {
            return Self::cancel_packet(packet, CancelReason::DeviceError, returns);
        }
        returns.frame_id = packet.frame_id;
        let render_start = Instant::now();
        self.shape_pipelines.begin_frame();
        self.viewport_uniforms.begin_frame();
        self.run_store.begin_frame(gpu_stats_enabled());

        let text_cache_len = packet.text_cache_len;
        let frame_root = self.frame_root(output_mode, output_view, output_texture, (width, height));
        let root = frame_root.target();
        let screenshot_bind_group = output_view.and_then(|_| {
            matches!(output_mode, OutputMode::Screenshot).then(|| {
                self.screenshot_converter
                    .bind_group(&self.device, &root.view)
            })
        });
        let output = frame_root.output(output_view, screenshot_bind_group.as_ref());
        let result = self.render_graph(root, packet, returns, output_mode, output);
        if let FrameRoot::Composition(composition) = frame_root {
            self.composition_target = Some(composition);
        }
        let after_graph = Instant::now();
        self.flush_deferred_offscreen_releases();

        self.frame_stats
            .layer_cache_size
            .set(self.layer_cache.len() as u32);
        self.frame_stats
            .layer_cache_bytes
            .set(self.layer_cache.bytes());
        self.frame_stats.offscreen_pool_size.set(
            self.effect_renderer
                .retained_offscreen_count()
                .saturating_add(self.frame_graph_executor.retained_texture_count())
                .saturating_add(usize::from(self.composition_target.is_some())) as u32,
        );
        self.frame_stats.offscreen_pool_bytes.set(
            (self.effect_renderer.retained_offscreen_bytes() as u64)
                .saturating_add(self.frame_graph_executor.retained_texture_bytes())
                .saturating_add(
                    self.composition_target
                        .as_ref()
                        .map(|target| {
                            u64::from(target.target.width)
                                .saturating_mul(u64::from(target.target.height))
                                .saturating_mul(composition_bytes_per_pixel())
                        })
                        .unwrap_or(0),
                ),
        );
        self.frame_stats
            .text_pool_size
            .set(self.text_image_cache.len() as u32);
        self.frame_stats
            .image_cache_size
            .set(self.image_texture_cache.len() as u32);
        self.frame_stats.text_cache_size.set(text_cache_len as u32);
        self.effect_renderer
            .merge_and_reset_debug_counters(&self.frame_stats);
        self.frame_graph_executor.reset_upload_allocators();
        let snapshot = self.frame_stats.snapshot();
        if crate::frame_graph::frame_graph_pass_telemetry_threshold_ms().is_some() {
            log::warn!(
                "[wgpu-render-stage:frame-stats] layer_hit={} layer_miss={} miss_px={} \
                 offscreen_acq={} offscreen_new={} isolated={} draws={}",
                snapshot.layer_cache_hits,
                snapshot.layer_cache_misses,
                snapshot.layer_cache_miss_pixels,
                snapshot.offscreen_acquires,
                snapshot.offscreen_news,
                snapshot.isolated_layer_renders,
                snapshot.draw_calls,
            );
        }
        self.last_frame_stats = Some(snapshot);
        PRESENTED_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        update_frame_warmup_budget(&mut self.pending_frame_warmup_frames, &snapshot);
        let gpu_stats_on = gpu_stats_enabled();
        self.frame_stats
            .maybe_print_snapshot(snapshot, &mut self.frame_count, gpu_stats_on);
        if gpu_stats_on && self.frame_count.is_multiple_of(60) {
            gpu_stats::print_gpu_memory_report(&self.device, self.frame_count);
        }
        self.frame_graph_executor
            .end_pass_timing_frame(&self.device, &self.queue);
        self.frame_stats.reset();
        let after_stats = Instant::now();
        if let Some(total_ms) = should_log_wgpu_render_stage(render_start, after_stats) {
            log::warn!(
                "[wgpu-render-stage:render] total_ms={total_ms:.2} graph_ms={:.2} cleanup_stats_ms={:.2}",
                instant_ms(render_start, after_graph),
                instant_ms(after_graph, after_stats),
            );
        }
        if result.is_ok() {
            returns.outcome = PresentOutcome::Presented;
        }
        result
    }

    /// Returns a packet unrendered, handing its scene back for recycling.
    pub(crate) fn cancel_packet(
        packet: FramePacket,
        reason: CancelReason,
        returns: &mut RenderReturns,
    ) -> Result<(), String> {
        returns.scene = Some(packet.root.scene);
        returns.frame_id = packet.frame_id;
        returns.outcome = PresentOutcome::Cancelled(reason);
        Ok(())
    }

    pub fn last_frame_stats(&self) -> Option<gpu_stats::FrameStatsSnapshot> {
        self.last_frame_stats
    }

    pub fn gpu_pass_timings(&self) -> crate::pass_timing::GpuPassTimingReport {
        self.frame_graph_executor.pass_timing_report()
    }

    pub fn needs_frame_warmup(&self) -> bool {
        self.pending_frame_warmup_frames > 0
    }

    pub fn debug_cpu_allocation_stats(&self) -> DebugCpuAllocationStats {
        DebugCpuAllocationStats {
            scene_graph_node_count: 0,
            scene_graph_heap_bytes: 0,
            scene_hits_len: 0,
            scene_hits_cap: 0,
            scene_node_index_len: 0,
            scene_node_index_cap: 0,
            text_renderer_pool_len: self.text_image_cache.len(),
            text_renderer_pool_cap: self.text_image_cache.cap().get(),
            image_texture_cache_len: self.image_texture_cache.len(),
            image_texture_cache_cap: self.image_texture_cache.cap().get(),
            run_arena_staging_bytes: self.run_store.arena_staging_bytes(),
            run_store_bytes: self.run_store.stored_bytes(),
            run_store_runs: self.run_store.stored_count(),
            scratch_image_vertices_cap: self.scratch_image_vertices.capacity(),
            scratch_image_indices_cap: self.scratch_image_indices.capacity(),
            scratch_image_cmds_cap: self.scratch_image_cmds.capacity(),
            layer_cache_len: self.layer_cache.len(),
            layer_cache_bytes: self.layer_cache.bytes(),
        }
    }
    pub fn render_to_rgba_pixels(
        &mut self,
        width: u32,
        height: u32,
        packet: FramePacket,
        surface_epoch: u64,
        returns: &mut RenderReturns,
    ) -> Result<Vec<u8>, String> {
        if width == 0 || height == 0 {
            return Err("Screenshot size must be non-zero".to_string());
        }

        let output_texture = crate::offscreen::create_2d_texture(
            &self.device,
            wgpu::TextureFormat::Rgba8Unorm,
            width,
            height,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            Some("Screenshot Output Texture"),
        );
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.render_internal(
            width,
            height,
            packet,
            surface_epoch,
            returns,
            OutputMode::Screenshot,
            Some(&output_view),
            None,
        )?;

        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = width
            .checked_mul(bytes_per_pixel)
            .ok_or_else(|| "Screenshot row byte size overflow".to_string())?;
        let padded_bytes_per_row =
            align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let output_buffer_size = padded_bytes_per_row as u64 * height as u64;

        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Screenshot Readback Buffer"),
            size: output_buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let device = self.device.clone();
        let queue = self.queue.clone();
        let mut graph = WgpuFrameGraph::new(Some("Screenshot Copy Encoder"));
        let source = graph.import_surface("screenshot-copy-source");
        graph.add_fallible_command_pass(Some("Screenshot Copy Pass"), &[source], &[], |context| {
            context.encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &output_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &output_buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_bytes_per_row),
                        rows_per_image: Some(height),
                    },
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            Ok(())
        });
        let mut executor = std::mem::take(&mut self.frame_graph_executor);
        let execution = executor.execute_recorded_graph(&device, &queue, graph);
        self.frame_graph_executor = executor;
        let execution = execution.map_err(|error| error.to_string())?;
        let submission_index = execution.submission;
        let copy_stats = execution.stats;
        self.last_frame_stats = self
            .last_frame_stats
            .map(|snapshot| snapshot.with_command_stats_added(copy_stats));

        let buffer_slice = output_buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission_index),
            timeout: None,
        });

        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Err(format!("Screenshot map_async failed: {err:?}")),
            Err(err) => return Err(format!("Screenshot readback timed out: {err}")),
        }

        let mapped = buffer_slice.get_mapped_range();
        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];

        let src_row_len = padded_bytes_per_row as usize;
        let dst_row_len = unpadded_bytes_per_row as usize;
        for row in 0..height as usize {
            let src_offset = row * src_row_len;
            let dst_offset = row * dst_row_len;
            pixels[dst_offset..dst_offset + dst_row_len]
                .copy_from_slice(&mapped[src_offset..src_offset + dst_row_len]);
        }
        drop(mapped);
        output_buffer.unmap();

        self.convert_surface_pixels_to_rgba(&pixels)
    }

    fn render_graph(
        &mut self,
        root_target: &Rc<OffscreenTarget>,
        packet: FramePacket,
        returns: &mut RenderReturns,
        output_mode: OutputMode,
        output: Option<(&wgpu::TextureView, &wgpu::BindGroup)>,
    ) -> Result<(), String> {
        let device = self.device.clone();
        let queue = self.queue.clone();
        let graph_start = Instant::now();
        let FramePacket {
            root,
            overlay,
            root_scale,
            ..
        } = packet;
        let page = Rc::clone(root_target);

        #[cfg(not(target_arch = "wasm32"))]
        let (result, submitted) = {
            let mut executor = std::mem::take(&mut self.frame_graph_executor);
            let mut frame_graph = WgpuFrameGraph::new(Some("Renderer Frame Graph"));
            let surface = frame_graph.import_surface("renderer-surface");
            frame_graph.add_fallible_recorded_command_pass(
                Some("Renderer Frame Pass"),
                &[],
                &[surface],
                |frame_encoder| {
                    self.encode_frame(
                        frame_encoder,
                        &root,
                        overlay.as_ref(),
                        Rc::clone(&page),
                        root_scale,
                        output_mode,
                        output,
                    )
                },
            );
            let after_build = Instant::now();
            let execution = executor.execute_recorded_graph(&device, &queue, frame_graph);
            let after_execute = Instant::now();
            self.frame_graph_executor = executor;
            if let Some(total_ms) = should_log_wgpu_render_stage(graph_start, after_execute) {
                log::warn!(
                    "[wgpu-render-stage:graph] total_ms={total_ms:.2} build_ms={:.2} execute_ms={:.2}",
                    instant_ms(graph_start, after_build),
                    instant_ms(after_build, after_execute),
                );
            }
            match execution {
                Ok(execution) => {
                    if execution.stats.pass_count > 0 {
                        self.frame_stats.record_command_stats(execution.stats);
                    }
                    (Ok(()), true)
                }
                Err(crate::frame_graph::FrameGraphError::NoDeclaredPasses) => (Ok(()), false),
                Err(error) => (Err(error.to_string()), false),
            }
        };

        #[cfg(target_arch = "wasm32")]
        let (result, submitted) = {
            let mut executor = std::mem::take(&mut self.frame_graph_executor);
            let (result, execution) = {
                let mut frame_encoder =
                    executor.begin(&device, &queue, Some("Renderer Frame Encoder"));
                let initial_pass_count = frame_encoder.recorded_pass_count();
                let result = self.encode_frame(
                    &mut frame_encoder,
                    &root,
                    overlay.as_ref(),
                    Rc::clone(&page),
                    root_scale,
                    output_mode,
                    output,
                );
                let execution =
                    if result.is_ok() && frame_encoder.recorded_pass_count() > initial_pass_count {
                        Some(frame_encoder.finish())
                    } else {
                        None
                    };
                (result, execution)
            };
            let after_execute = Instant::now();
            self.frame_graph_executor = executor;
            if let Some(total_ms) = should_log_wgpu_render_stage(graph_start, after_execute) {
                log::warn!("[wgpu-render-stage:graph] total_ms={total_ms:.2}",);
            }
            let submitted = execution.is_some();
            if let Some(execution) = execution {
                self.frame_stats.record_command_stats(execution.stats);
            }
            (result, submitted)
        };
        if !submitted {
            self.run_store.invalidate_uploads();
        }
        returns.scene = Some(root.scene);
        result
    }

    /// Records the frame: the root and overlay layer scenes into the frame's
    /// target, the output conversion when the target is not the presented
    /// image, and the viewport uniforms the recorded passes claimed.
    #[allow(clippy::too_many_arguments)]
    fn encode_frame<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        root: &LayerScene,
        overlay: Option<&LayerScene>,
        page: Rc<OffscreenTarget>,
        root_scale: f32,
        output_mode: OutputMode,
        output: Option<(&wgpu::TextureView, &wgpu::BindGroup)>,
    ) -> Result<(), String> {
        FrameExecutor::new(self, recorder).render_frame(
            root,
            overlay,
            page,
            root_scale,
            wgpu::LoadOp::Clear(CLEAR_COLOR),
        )?;
        if let Some((output_view, bind_group)) = output {
            match output_mode {
                OutputMode::Display => &self.output_converter,
                OutputMode::Screenshot => &self.screenshot_converter,
            }
            .encode(
                &self.device,
                recorder,
                output_view,
                bind_group,
                self.adapter_backend,
            );
            recorder.record_pass();
        }
        let mut upload = self.viewport_uniforms.flush(&self.queue);
        upload += self.run_store.flush(&self.queue);
        self.frame_stats.record_command_stats(upload);
        Ok(())
    }
    fn viewport_uniforms(params: ViewportUniformParams) -> Uniforms {
        Uniforms {
            viewport: [params.width as f32, params.height as f32],
            viewport_offset: params.offset,
            placement: PlacementData::zeroed(),
        }
    }

    /// Claims this frame's next viewport uniform slot for `params`.
    pub(crate) fn claim_uniform_slot(&mut self, params: ViewportUniformParams) -> usize {
        let uniforms = Self::viewport_uniforms(params);
        self.viewport_uniforms
            .claim(&self.device, &self.uniform_bind_group_layout, &uniforms)
    }

    /// Resolves a blurred shadow at `z` into a texture and queues its
    /// composites. The shadow's shapes and texts render into a source the
    /// size of their blur footprint, blur in place and take the post-blur
    /// cutouts; the source is then blitted in bands around the occluder.
    /// Shape-only shadows live in the shadow cache, keyed by their content
    /// and device placement, so a scrolling card re-blits its cached blur.
    #[allow(clippy::too_many_arguments)]
    /// The blurred shadow texture and whether the cache held it: a
    /// shape-only shadow is cached by content and placement, a shadow with
    /// text renders every frame.
    fn blurred_shadow_source<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        shadow: &ShadowDraw,
        source_device: DevicePixelBounds,
        pixel_radius: f32,
        root_scale: f32,
        transients: &mut Vec<(FrameTextureDescriptor, Rc<OffscreenTarget>)>,
    ) -> Option<(Rc<OffscreenTarget>, bool, SourceContent)> {
        let shape_only = shadow.texts.is_empty();
        let key = if shape_only {
            shape_shadow_surface_cache_key(shadow, source_device, pixel_radius, root_scale)
        } else {
            None
        };
        let content = key.map_or(SourceContent::Transient, |key| {
            SourceContent::retained(&key)
        });
        if let Some(entry) = key.and_then(|key| self.shadow_surface_cache.get(&key)) {
            return Some((Rc::clone(&entry.target), true, content));
        }
        if !shape_only {
            self.frame_stats.record_shadow_text_blur_fallback();
        }
        let source = self.render_shadow_source(
            recorder,
            shadow,
            source_device,
            pixel_radius,
            root_scale,
            key.is_some(),
            transients,
        )?;
        if let Some(key) = key {
            self.frame_stats
                .record_shadow_shape_cache_miss(source_device.width, source_device.height);
            self.frame_stats.maybe_print_shadow_shape_cache_miss(
                source_device.width,
                source_device.height,
                key.content_hash,
                pixel_radius,
                [source_device.x, source_device.y],
                shadow.shapes.as_ref().map_or(0, RunDraw::record_count) as usize,
                shadow.clip,
            );
            self.insert_cached_shadow_surface(key, Rc::clone(&source));
        }
        Some((source, false, content))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve_blurred_shadow<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        shadow: &ShadowDraw,
        z: usize,
        root_scale: f32,
        target_rect: DeviceRect4,
        transients: &mut Vec<(FrameTextureDescriptor, Rc<OffscreenTarget>)>,
        resolved: &mut Vec<ResolvedComposite>,
    ) {
        if shadow.blur_radius <= 0.0
            || skip_shadow_draws()
            || !root_scale.is_finite()
            || root_scale <= 0.0
        {
            return;
        }
        let Some(bounds) = shadow_draw_bounds(shadow) else {
            return;
        };
        let margin = blur_reach(shadow.blur_radius, root_scale);
        let source_bounds = expand_rect(bounds, margin, margin);
        let mut visible = source_bounds;
        if let Some(clip) = shadow.clip {
            let Some(clipped) = visible.intersect(expand_rect(clip, margin, margin)) else {
                return;
            };
            visible = clipped;
        }
        let target_logical = Rect {
            x: target_rect.0 / root_scale,
            y: target_rect.1 / root_scale,
            width: target_rect.2 / root_scale,
            height: target_rect.3 / root_scale,
        };
        let Some(visible) = visible.intersect(target_logical) else {
            return;
        };
        let max_texture_dim = self.max_texture_dim();
        let shape_only = shadow.texts.is_empty();
        let anchor = shadow
            .shapes
            .as_ref()
            .and_then(|run| run.placement.snap_anchor);
        let source_device = shape_only
            .then(|| {
                translation_stable_anchored_device_pixel_bounds(
                    source_bounds,
                    anchor,
                    root_scale,
                    max_texture_dim,
                )
            })
            .flatten()
            .or_else(|| device_pixel_bounds(visible, root_scale, max_texture_dim));
        let Some(source_device) = source_device else {
            return;
        };
        let pixel_radius = shadow.blur_radius * root_scale;
        let Some((source, hit, content)) = self.blurred_shadow_source(
            recorder,
            shadow,
            source_device,
            pixel_radius,
            root_scale,
            transients,
        ) else {
            return;
        };
        let dest = (
            source_device.x,
            source_device.y,
            source_device.width as f32,
            source_device.height as f32,
        );
        let mut coverage = intersect_device_rects(dest, target_rect);
        if let Some(clip) = shadow.clip {
            coverage = coverage.and_then(|coverage| {
                intersect_device_rects(coverage, anchored_rect_to_device(clip, anchor, root_scale))
            });
        }
        let Some(coverage) = coverage else {
            return;
        };
        let bands = shadow_bands(
            coverage,
            shadow
                .occluder
                .map(|occluder| anchored_rect_to_device(occluder, anchor, root_scale)),
        );
        if bands.is_empty() {
            self.frame_stats.record_shadow_fully_occluded();
            return;
        }
        if hit {
            self.frame_stats
                .record_shadow_shape_cache_hit(banded_pixels(&bands));
        }
        let rounded_mask = shadow_composite_mask(shadow, anchor, root_scale);
        let downscaled =
            (source.width, source.height) != (source_device.width, source_device.height);
        let (sample_mode, source_viewport) = if downscaled {
            (
                CompositeSampleMode::Linear,
                Some((0.0, 0.0, source.width as f32, source.height as f32)),
            )
        } else {
            (CompositeSampleMode::Nearest, None)
        };
        for band in bands {
            resolved.push(ResolvedComposite {
                z_index: z,
                source: Rc::clone(&source),
                content,
                dest,
                scissor: Some(band),
                kind: ResolvedCompositeKind::Blit {
                    alpha: 1.0,
                    blend_mode: BlendMode::SrcOver,
                    rounded_mask,
                    sample_mode,
                    source_viewport,
                },
            });
        }
    }

    /// Draws a shadow's shapes and texts into a surface covering `bounds`
    /// and blurs it. A wide blur runs at its scratch size and its result
    /// stays there, read bilinearly by the composite; a post-blur cutout
    /// needs the surface's full size, so the blurred result is interpolated
    /// back into it first and the cutout drawn at that size. A retained
    /// result feeds the shadow cache; a transient one is registered with
    /// the frame's transients and released with them. `None` when the
    /// shadow draws nothing.
    #[allow(clippy::too_many_arguments)]
    fn render_shadow_source<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        shadow: &ShadowDraw,
        bounds: DevicePixelBounds,
        pixel_radius: f32,
        root_scale: f32,
        retained: bool,
        transients: &mut Vec<(FrameTextureDescriptor, Rc<OffscreenTarget>)>,
    ) -> Option<Rc<OffscreenTarget>> {
        let (width, height) = (bounds.width, bounds.height);
        let device = self.device.clone();
        let (scratch_width, scratch_height) =
            crate::effect_renderer::blur_scratch_size(pixel_radius, pixel_radius, width, height);
        let full_size_result = shadow.post_blur_cutouts.is_some()
            || (scratch_width, scratch_height) == (width, height);
        let (result_width, result_height) = if full_size_result {
            (width, height)
        } else {
            (scratch_width, scratch_height)
        };
        let result = if retained {
            Rc::new(self.acquire_retained_surface(result_width, result_height))
        } else {
            self.shadow_transient(
                recorder,
                transients,
                "Shadow Result",
                result_width,
                result_height,
            )
        };
        let source = if full_size_result {
            Rc::clone(&result)
        } else {
            self.shadow_transient(recorder, transients, "Shadow Source", width, height)
        };
        let offset = [bounds.x, bounds.y];
        let target = PassTarget {
            view: &source.view,
            width,
            height,
            offset,
        };
        let scene = shadow_scene(shadow.shapes.as_ref(), &shadow.texts);
        let segment = PassSegment {
            scene: &scene,
            ops: &scene.draw_ops,
            composites: &[],
            offset,
            scissor: None,
            first_run_window: None,
        };
        let drew = self.encode_pass(
            recorder,
            target,
            std::slice::from_ref(&segment),
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            root_scale,
            "Shadow Source Pass",
        );
        match drew {
            Ok(true) => {}
            Ok(false) => {
                drop(source);
                if retained && let Ok(target) = Rc::try_unwrap(result) {
                    self.defer_offscreen_release(target);
                }
                return None;
            }
            Err(error) => {
                log::error!("shadow source pass failed: {error}");
                return None;
            }
        }
        let scratch_descriptor = self.transient_offscreen_descriptor(
            "Shadow Blur Scratch",
            scratch_width,
            scratch_height,
        );
        let scratch = recorder.acquire_transient_offscreen(&device, scratch_descriptor);
        let blurred = if full_size_result && (scratch_width, scratch_height) != (width, height) {
            Some(self.shadow_transient(
                recorder,
                transients,
                "Shadow Blur Result",
                scratch_width,
                scratch_height,
            ))
        } else {
            None
        };
        let blur_dest = match &blurred {
            Some(blurred) => (&blurred.view, (scratch_width, scratch_height)),
            None => (&result.view, (result_width, result_height)),
        };
        let passes = self.effect_renderer.encode_blur_scissored_ping_pong_passes(
            recorder,
            &device,
            &source,
            &scratch,
            blur_dest,
            pixel_radius,
            pixel_radius,
            TileMode::Decal,
            None,
        );
        recorder.record_passes(passes);
        self.effect_renderer.record_blur_pass();
        recorder.release_transient_offscreen(scratch_descriptor, scratch);
        if let Some(blurred) = &blurred {
            self.effect_renderer
                .encode_upscale_pass(recorder, &device, blurred, &result.view);
            recorder.record_pass();
        }
        if let Some(cutout_run) = &shadow.post_blur_cutouts {
            let cutouts = shadow_scene(Some(cutout_run), &[]);
            let segment = PassSegment {
                scene: &cutouts,
                ops: &cutouts.draw_ops,
                composites: &[],
                offset,
                scissor: None,
                first_run_window: None,
            };
            if let Err(error) = self.encode_pass(
                recorder,
                target,
                std::slice::from_ref(&segment),
                wgpu::LoadOp::Load,
                root_scale,
                "Shadow Cutout Pass",
            ) {
                log::error!("shadow cutout pass failed: {error}");
            }
        }
        Some(result)
    }

    /// A transient surface of a shadow's frame, released with the frame's
    /// transients.
    fn shadow_transient<C: FrameCommandRecorder>(
        &self,
        recorder: &mut C,
        transients: &mut Vec<(FrameTextureDescriptor, Rc<OffscreenTarget>)>,
        label: &'static str,
        width: u32,
        height: u32,
    ) -> Rc<OffscreenTarget> {
        let descriptor = self.transient_offscreen_descriptor(label, width, height);
        let target = Rc::new(recorder.acquire_transient_offscreen(&self.device, descriptor));
        transients.push((descriptor, Rc::clone(&target)));
        target
    }

    /// Whether `run` draws from retained buffers keyed by its command.
    pub(crate) fn run_is_stored(&self, run: &RunDraw) -> bool {
        self.run_store.is_stored(run)
    }

    fn run_pipeline_key(segment: &RecordSegment, clipped: bool, tier: RunTier) -> ShapePipelineKey {
        ShapePipelineKey {
            blend_mode: supported_blend_mode(segment.blend),
            tier,
            variant: ShapeVariant::of_segment(segment, clipped),
        }
    }

    /// Brings a stored run's tables up to date and records its draws under
    /// a placement uniform of its own.
    pub(crate) fn prepare_store_run<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        run: &RunDraw,
        viewport: ViewportUniformParams,
        root_scale: f32,
        window: &std::ops::Range<u32>,
    ) -> StoreRunBatch {
        let command = run.command.expect("a stored run has a command");
        let upload_start = Instant::now();
        let (upload, fill) = self
            .run_store
            .upload_stored(&self.device, recorder, run, root_scale);
        if let Some(total_ms) = should_log_wgpu_render_stage(upload_start, Instant::now()) {
            log::warn!(
                "[wgpu-render-stage:run-upload] total_ms={total_ms:.2} bytes={} records={}",
                upload.upload_bytes,
                run.tables().shapes.len()
            );
        }
        self.frame_stats.record_command_stats(upload);
        if let Some(fill) = fill {
            self.frame_stats.add_shape_fill(fill);
        }
        let uniforms = Uniforms {
            viewport: [viewport.width as f32, viewport.height as f32],
            viewport_offset: viewport.offset,
            placement: PlacementData::of(&run.placement, root_scale),
        };
        let uniform_slot =
            self.viewport_uniforms
                .claim(&self.device, &self.uniform_bind_group_layout, &uniforms);
        let clipped = run.placement.clip.is_some();
        let mut draws = SmallVec::new();
        self.run_store.stored_run_draws(
            &self.device,
            run,
            &mut |segment| Self::run_pipeline_key(segment, clipped, RunTier::Store),
            &mut draws,
        );
        window_draws(&mut draws, window);
        for draw in &draws {
            self.ensure_shape_pipeline(draw.key);
        }
        StoreRunBatch {
            command,
            uniform_slot,
            draws,
        }
    }

    pub(crate) fn open_arena(&mut self) -> usize {
        self.run_store.open_arena()
    }

    pub(crate) fn arena_accepts(&self, chunk: usize, run: &RunDraw) -> bool {
        self.run_store.arena_accepts(chunk, run)
    }

    pub(crate) fn append_arena_run(
        &mut self,
        chunk: usize,
        run: &RunDraw,
        window: std::ops::Range<u32>,
        root_scale: f32,
    ) -> u32 {
        let clipped = run.placement.clip.is_some();
        let mut keys: SmallVec<[ShapePipelineKey; 4]> = SmallVec::new();
        let taken = self
            .run_store
            .append_arena(chunk, run, window, root_scale, &mut |segment| {
                let key = Self::run_pipeline_key(segment, clipped, RunTier::Arena);
                if !keys.contains(&key) {
                    keys.push(key);
                }
                key
            });
        for key in keys {
            self.ensure_shape_pipeline(key);
        }
        taken
    }

    /// Uploads the open chunk and returns its draws.
    pub(crate) fn close_arena(&mut self, chunk: usize) -> Vec<RunDrawCall> {
        let (draws, fill) = self.run_store.close_arena(&self.device, chunk);
        if let Some(fill) = fill {
            self.frame_stats.add_shape_fill(fill);
        }
        draws
    }

    pub(crate) fn draw_run_calls(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        tables: ArenaBinding<'_>,
        uniform_slot: usize,
        draws: &[RunDrawCall],
        target_size: (u32, u32),
        scissor: Option<(u32, u32, u32, u32)>,
    ) -> Result<(), String> {
        if draws.is_empty() {
            return Ok(());
        }
        self.frame_stats.bump_shapes();
        self.frame_stats.add_draw_calls(draws.len() as u32);
        let (x, y, width, height) = scissor.unwrap_or((0, 0, target_size.0, target_size.1));
        pass.set_scissor_rect(x, y, width, height);
        pass.set_bind_group(
            0,
            &self.viewport_uniforms.bind_group,
            &[self.viewport_uniforms.dynamic_offset(uniform_slot)?],
        );
        pass.set_bind_group(1, tables.bind_group, &tables.offsets[2..]);
        for (slot, buffer) in tables.records.into_iter().enumerate() {
            pass.set_vertex_buffer(slot as u32, buffer.slice(u64::from(tables.offsets[slot])..));
        }
        let mut bound_class = None;
        for draw in draws {
            let (pipeline, fallback) = self
                .shape_pipelines
                .get(draw.key)
                .ok_or_else(|| format!("shape pipeline {:?} was not prepared", draw.key))?;
            if fallback {
                self.frame_stats
                    .shape_pipeline_fallback_draws
                    .set(self.frame_stats.shape_pipeline_fallback_draws.get() + 1);
            } else if !draw.key.is_general() {
                self.frame_stats
                    .shape_specialized_draws
                    .set(self.frame_stats.shape_specialized_draws.get() + 1);
            }
            if bound_class != Some(draw.band_class) {
                pass.set_index_buffer(
                    self.run_store.strip_index_buffer(draw.band_class).slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                bound_class = Some(draw.band_class);
            }
            pass.set_pipeline(pipeline);
            pass.draw_indexed(draw.indices(), 0, draw.records.clone());
        }
        Ok(())
    }

    pub(crate) fn draw_store_run(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        batch: &StoreRunBatch,
        target_size: (u32, u32),
        scissor: Option<(u32, u32, u32, u32)>,
    ) -> Result<(), String> {
        let stored = self
            .run_store
            .stored(&batch.command)
            .ok_or_else(|| "a stored run left the store before its draw".to_string())?;
        self.draw_run_calls(
            pass,
            stored.buffers.binding(),
            batch.uniform_slot,
            &batch.draws,
            target_size,
            scissor,
        )
    }

    pub(crate) fn draw_arena(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        chunk: usize,
        uniform_slot: usize,
        draws: &[RunDrawCall],
        target_size: (u32, u32),
        scissor: Option<(u32, u32, u32, u32)>,
    ) -> Result<(), String> {
        self.draw_run_calls(
            pass,
            self.run_store.arena_binding(chunk),
            uniform_slot,
            draws,
            target_size,
            scissor,
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn surface_format(&self) -> wgpu::TextureFormat {
        self.display_format
    }

    pub fn device_error_count(&self) -> u64 {
        self.device_errors.error_count()
    }

    /// A pass that only applies `load_op` to the target, for a scene with
    /// nothing to draw that still needs its clear.
    pub(crate) fn clear_target<C: FrameCommandRecorder>(
        &self,
        recorder: &mut C,
        view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) {
        let pass = recorder.begin_color_pass("Clear Pass", view, load_op);
        drop(pass);
        recorder.record_pass();
    }
    pub(crate) fn draw_image_cmds(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        image_slot: &ImageSlot,
        uniform_slot: usize,
        cmds: &[ImageDrawCmd],
        blend_mode: BlendMode,
        bound: Option<(u32, u32, u32, u32)>,
    ) -> Result<(), String> {
        if cmds.is_empty() {
            return Ok(());
        }
        self.frame_stats.bump_images();
        self.frame_stats.add_draw_calls(cmds.len() as u32);
        pass.set_pipeline(self.image_pipeline(blend_mode));
        pass.set_bind_group(
            0,
            &self.viewport_uniforms.bind_group,
            &[self.viewport_uniforms.dynamic_offset(uniform_slot)?],
        );
        pass.set_index_buffer(image_slot.indices.slice(), wgpu::IndexFormat::Uint32);
        pass.set_vertex_buffer(0, image_slot.vertices.slice());
        for cmd in cmds {
            let Some((x, y, width, height)) = bounded_scissor(cmd.scissor, bound) else {
                continue;
            };
            pass.set_scissor_rect(x, y, width, height);
            let cached = self
                .image_texture_cache
                .peek(&cmd.image_id)
                .ok_or_else(|| "image texture missing from cache".to_string())?;
            pass.set_bind_group(1, cached.bind_group(cmd.sampling), &[]);
            pass.draw_indexed(cmd.index_start..(cmd.index_start + 6), 0, 0..1);
        }
        Ok(())
    }

    pub(crate) fn draw_glyph_cmds(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        image_slot: Option<&ImageSlot>,
        uniform_slot: usize,
        cmds: &[GlyphDrawCmd],
        bound: Option<(u32, u32, u32, u32)>,
    ) -> Result<(), String> {
        if cmds.is_empty() {
            return Ok(());
        }
        self.frame_stats.bump_text();
        self.frame_stats.add_draw_calls(cmds.len() as u32);
        pass.set_pipeline(self.glyph_atlas_pipeline());
        pass.set_bind_group(1, &self.text_glyph_atlas.bind_group, &[]);
        let uniform_offset = self.viewport_uniforms.dynamic_offset(uniform_slot)?;
        let mut shared_bound = false;
        for cmd in cmds {
            let Some((x, y, width, height)) = bounded_scissor(cmd.scissor, bound) else {
                continue;
            };
            pass.set_scissor_rect(x, y, width, height);
            match cmd.source {
                GlyphDrawSource::Shared {
                    index_start,
                    index_count,
                } => {
                    if !shared_bound {
                        let slot = image_slot
                            .ok_or_else(|| "shared glyph draw without an image slot".to_string())?;
                        pass.set_bind_group(
                            0,
                            &self.viewport_uniforms.bind_group,
                            &[uniform_offset],
                        );
                        pass.set_index_buffer(slot.indices.slice(), wgpu::IndexFormat::Uint32);
                        pass.set_vertex_buffer(0, slot.vertices.slice());
                        shared_bound = true;
                    }
                    pass.draw_indexed(index_start..(index_start + index_count), 0, 0..1);
                }
                GlyphDrawSource::Retained {
                    cache_key,
                    uniform_slot: retained_slot,
                } => {
                    shared_bound = false;
                    let cached = self
                        .text_glyph_gpu_run_cache
                        .peek(&cache_key)
                        .ok_or_else(|| "retained glyph buffer missing from cache".to_string())?;
                    pass.set_bind_group(
                        0,
                        &self.viewport_uniforms.bind_group,
                        &[self.viewport_uniforms.dynamic_offset(retained_slot)?],
                    );
                    pass.set_index_buffer(cached.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.set_vertex_buffer(0, cached.vertex_buffer.slice(..));
                    pass.draw_indexed(0..cached.index_count, 0, 0..1);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn append_image_draw_cmd(
        &mut self,
        image_draw: &ImageDraw,
        viewport: ViewportUniformParams,
        root_scale: f32,
        image_vertices: &mut Vec<Vertex>,
        image_indices: &mut Vec<u32>,
        image_cmds: &mut Vec<ImageDrawCmd>,
    ) -> Result<(), String> {
        let snap_delta = image_draw
            .snap_anchor
            .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
            .unwrap_or_default();
        let rect = image_draw.rect.translate(snap_delta.x, snap_delta.y);
        if rect.width <= 0.0 || rect.height <= 0.0 || image_draw.alpha <= 0.0 {
            return Ok(());
        }

        let (tint, cpu_filter) = tint_for_image(image_draw.color_filter, image_draw.alpha);
        if tint[3] <= 0.0 {
            return Ok(());
        }

        let prepared_image = if let Some(filter) = cpu_filter {
            apply_filter_to_bitmap(&image_draw.image, filter)?
        } else {
            image_draw.image.clone()
        };
        self.ensure_image_cached(&prepared_image)?;

        let mut adjusted_image = ImageDraw {
            rect,
            local_rect: image_draw.local_rect.translate(snap_delta.x, snap_delta.y),
            quad: translate_quad(image_draw.quad, snap_delta),
            snap_anchor: image_draw.snap_anchor,
            image: image_draw.image.clone(),
            alpha: image_draw.alpha,
            color_filter: image_draw.color_filter,
            sampling: image_draw.sampling,
            z_index: image_draw.z_index,
            clip: image_draw.clip,
            blend_mode: image_draw.blend_mode,
            src_rect: image_draw.src_rect,
            motion_context_animated: image_draw.motion_context_animated,
        };
        snap_nearest_image_to_device_pixels(&mut adjusted_image, root_scale);
        let Some(scissor) = scissor_rect_for_image(&adjusted_image, root_scale, viewport) else {
            return Ok(());
        };

        let Some(uv_rect) = image_uv_rect(&image_draw.image, image_draw.src_rect) else {
            return Ok(());
        };
        let device_quad =
            nearest_image_device_quad(&adjusted_image, root_scale).unwrap_or_else(|| {
                if adjusted_image.snap_anchor.is_some() {
                    canonicalized_scaled_quad(adjusted_image.quad, root_scale)
                } else {
                    scaled_quad(adjusted_image.quad, root_scale)
                }
            });

        let base_vertex = image_vertices.len() as u32;
        let index_start = image_indices.len() as u32;
        image_indices.extend_from_slice(&[
            base_vertex,
            base_vertex + 1,
            base_vertex + 2,
            base_vertex + 2,
            base_vertex + 1,
            base_vertex + 3,
        ]);
        image_vertices.extend_from_slice(&[
            Vertex {
                position: device_quad[0],
                color: tint,
                uv: [uv_rect.min[0], uv_rect.min[1]],
                uv_bounds: uv_rect.sample_bounds,
            },
            Vertex {
                position: device_quad[1],
                color: tint,
                uv: [uv_rect.max[0], uv_rect.min[1]],
                uv_bounds: uv_rect.sample_bounds,
            },
            Vertex {
                position: device_quad[2],
                color: tint,
                uv: [uv_rect.min[0], uv_rect.max[1]],
                uv_bounds: uv_rect.sample_bounds,
            },
            Vertex {
                position: device_quad[3],
                color: tint,
                uv: [uv_rect.max[0], uv_rect.max[1]],
                uv_bounds: uv_rect.sample_bounds,
            },
        ]);

        image_cmds.push(ImageDrawCmd {
            index_start,
            scissor,
            image_id: prepared_image.id(),
            sampling: image_draw.sampling,
        });
        Ok(())
    }

    /// Uploads a pass's image and glyph quads into the frame's buffers.
    pub(crate) fn upload_image_slot<C: FrameCommandRecorder>(
        &self,
        recorder: &mut C,
        vertices: &[Vertex],
        indices: &[u32],
    ) -> ImageSlot {
        ImageSlot {
            vertices: recorder.upload_buffer(
                image_vertex_spec(),
                &self.device,
                bytemuck::cast_slice(vertices),
            ),
            indices: recorder.upload_buffer(
                image_index_spec(),
                &self.device,
                bytemuck::cast_slice(indices),
            ),
        }
    }
    fn glyph_atlas_entry_for(
        &mut self,
        glyph: &SoftwareGlyphAtlasGlyph,
    ) -> Result<GlyphAtlasEntry, String> {
        if let Some(entry) = self.text_glyph_atlas.upload_glyph(
            glyph.key,
            glyph,
            &self.queue,
            &mut self.frame_graph_executor,
            &mut self.frame_stats,
        ) {
            return Ok(entry);
        }

        self.text_glyph_atlas.reset(
            &self.device,
            &self.image_bind_group_layout,
            &self.image_nearest_sampler,
        );
        Err("text glyph atlas filled and was reset".to_string())
    }

    fn glyph_atlas_entry_for_cached(
        &mut self,
        glyph: &SoftwareGlyphAtlasPlacement,
    ) -> Option<GlyphAtlasEntry> {
        let entry = self.text_glyph_atlas.entry(&glyph.key)?;
        self.frame_stats.record_text_glyph_atlas_hit();
        Some(entry)
    }

    fn glyph_atlas_entry_for_placement(
        &mut self,
        glyph: &SoftwareGlyphAtlasPlacement,
    ) -> Result<GlyphAtlasEntry, String> {
        if let Some(entry) = self.glyph_atlas_entry_for_cached(glyph) {
            return Ok(entry);
        }

        let Some(upload_glyph) = self.text_glyph_mask_cache.atlas_glyph_for_placement(glyph) else {
            return Err("text glyph placement has no retained raster mask".to_string());
        };
        self.glyph_atlas_entry_for(&upload_glyph)
    }

    fn prepare_text_glyph_quads(
        &mut self,
        run_key: TextGlyphRunCacheKey,
        atlas_generation: u64,
        cached_glyph_run: Option<&[SoftwareGlyphAtlasPlacement]>,
        collected_run: &[SoftwareGlyphAtlasRunGlyph],
        generated_quads: &mut Vec<CachedTextGlyphQuad>,
    ) -> Result<Rc<[CachedTextGlyphQuad]>, String> {
        generated_quads.clear();
        if let Some(glyph_run) = cached_glyph_run {
            for glyph in glyph_run {
                if glyph.width == 0 || glyph.height == 0 || glyph.color.3 <= 0.0 {
                    continue;
                }
                let entry = self.glyph_atlas_entry_for_placement(glyph)?;
                generated_quads.push(cached_text_glyph_quad(
                    glyph,
                    entry,
                    self.text_glyph_atlas.size(),
                ));
            }
        } else {
            for run_glyph in collected_run {
                let placement = run_glyph.placement();
                if placement.width == 0 || placement.height == 0 || placement.color.3 <= 0.0 {
                    continue;
                }
                let entry = match run_glyph {
                    SoftwareGlyphAtlasRunGlyph::Cached(placement) => {
                        self.glyph_atlas_entry_for_placement(placement)?
                    }
                    SoftwareGlyphAtlasRunGlyph::New(glyph) => self.glyph_atlas_entry_for(glyph)?,
                };
                generated_quads.push(cached_text_glyph_quad(
                    &placement,
                    entry,
                    self.text_glyph_atlas.size(),
                ));
            }
        }

        let quads: Rc<[CachedTextGlyphQuad]> = Rc::from(generated_quads.clone().into_boxed_slice());
        if let Some(cached) = self.text_glyph_run_cache.get_mut(&run_key) {
            cached.quads = Some(Rc::clone(&quads));
            cached.atlas_generation = atlas_generation;
        }
        Ok(quads)
    }

    #[allow(clippy::too_many_arguments)]
    fn append_text_glyph_quad_run(
        &mut self,
        source_raster_rect: Rect,
        quads: &[CachedTextGlyphQuad],
        clip: Option<Rect>,
        viewport: ViewportUniformParams,
        root_scale: f32,
        image_vertices: &mut Vec<Vertex>,
        image_indices: &mut Vec<u32>,
        record_cached_hits: bool,
    ) -> usize {
        let mut appended = 0usize;
        for quad in quads {
            if !cached_text_glyph_quad_is_visible_in_viewport(
                source_raster_rect,
                quad,
                clip,
                viewport,
                root_scale,
            ) {
                continue;
            }
            if append_cached_text_glyph_quad(
                source_raster_rect,
                quad,
                image_vertices,
                image_indices,
            ) {
                if record_cached_hits {
                    self.frame_stats.record_text_glyph_atlas_hit();
                }
                appended = appended.saturating_add(1);
            }
        }
        appended
    }

    fn retained_glyph_viewport(
        viewport: ViewportUniformParams,
        source_raster_rect: Rect,
    ) -> ViewportUniformParams {
        ViewportUniformParams {
            width: viewport.width,
            height: viewport.height,
            offset: [
                viewport.offset[0] - source_raster_rect.x,
                viewport.offset[1] - source_raster_rect.y,
            ],
        }
    }

    fn retained_text_glyph_run_ready(&self, cache_key: TextGlyphRunCacheKey) -> bool {
        let atlas_generation = self.text_glyph_atlas.generation();
        self.text_glyph_gpu_run_cache
            .peek(&cache_key)
            .is_some_and(|cached| cached.atlas_generation == atlas_generation)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_retained_text_glyph_run_if_ready(
        &mut self,
        cache_key: TextGlyphRunCacheKey,
        quads: &[CachedTextGlyphQuad],
        clip: Option<Rect>,
        viewport: ViewportUniformParams,
        source_raster_rect: Rect,
        scissor: (u32, u32, u32, u32),
        glyph_cmds: &mut Vec<GlyphDrawCmd>,
    ) -> bool {
        if !should_use_retained_text_glyph_run(quads.len(), clip) {
            return false;
        }
        if !self.retained_text_glyph_run_ready(cache_key)
            && !self.ensure_retained_text_glyph_run(cache_key, quads)
        {
            return false;
        }
        let uniform_slot =
            self.claim_uniform_slot(Self::retained_glyph_viewport(viewport, source_raster_rect));
        glyph_cmds.push(GlyphDrawCmd::retained(cache_key, uniform_slot, scissor));
        true
    }

    fn ensure_retained_text_glyph_run(
        &mut self,
        cache_key: TextGlyphRunCacheKey,
        quads: &[CachedTextGlyphQuad],
    ) -> bool {
        let atlas_generation = self.text_glyph_atlas.generation();
        if self
            .text_glyph_gpu_run_cache
            .peek(&cache_key)
            .is_some_and(|cached| cached.atlas_generation == atlas_generation)
        {
            return true;
        }

        let mut vertices = Vec::with_capacity(quads.len().saturating_mul(4));
        let mut indices = Vec::with_capacity(quads.len().saturating_mul(6));
        let origin = Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
        for quad in quads {
            append_cached_text_glyph_quad(origin, quad, &mut vertices, &mut indices);
        }
        if indices.is_empty() {
            return false;
        }

        let vertex_bytes = bytemuck::cast_slice(&vertices);
        let index_bytes = bytemuck::cast_slice(&indices);
        let vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Retained Text Glyph Vertex Buffer"),
            size: vertex_bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Retained Text Glyph Index Buffer"),
            size: index_bytes.len() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut upload = write_buffer(&self.queue, &vertex_buffer, 0, vertex_bytes);
        upload.upload_bytes +=
            write_buffer(&self.queue, &index_buffer, 0, index_bytes).upload_bytes;
        self.frame_stats.record_command_stats(upload);

        self.text_glyph_gpu_run_cache.put(
            cache_key,
            CachedGpuTextGlyphRun {
                vertex_buffer,
                index_buffer,
                index_count: indices.len() as u32,
                atlas_generation,
            },
        );
        true
    }
    /// Appends the glyph atlas draws of `layer_texts` visible in `viewport`.
    /// `Ok(false)` when a text cannot draw from the atlas (animated motion,
    /// or a run the atlas cannot hold): nothing was appended, and the caller
    /// draws the texts as rasterized images instead.
    pub(crate) fn append_text_glyph_draws<'a, I>(
        &mut self,
        layer_texts: I,
        viewport: ViewportUniformParams,
        root_scale: f32,
        image_vertices: &mut Vec<Vertex>,
        image_indices: &mut Vec<u32>,
        glyph_cmds: &mut Vec<GlyphDrawCmd>,
    ) -> Result<bool, String>
    where
        I: IntoIterator<Item = &'a TextDraw>,
    {
        let append_start = Instant::now();
        let initial_vertex_len = image_vertices.len();
        let initial_index_len = image_indices.len();
        let initial_cmd_len = glyph_cmds.len();
        let mut collected_run = std::mem::take(&mut self.scratch_text_glyph_run);
        let mut collected_placements = std::mem::take(&mut self.scratch_text_glyph_placements);
        let mut generated_quads = std::mem::take(&mut self.scratch_text_glyph_quads);
        generated_quads.clear();
        let mut visited = 0usize;
        let mut emitted_glyphs = 0usize;
        let mut run_hits = 0usize;
        let mut run_misses = 0usize;
        let mut fallback = false;

        for text_draw in layer_texts {
            visited = visited.saturating_add(1);
            let Some((logical_rect, raster_rect, clip, text_scale, static_text_motion)) =
                self.text_raster_geometry(text_draw, root_scale)
            else {
                continue;
            };
            if !static_text_motion {
                fallback = true;
                break;
            }
            if !text_draw_is_visible_in_viewport(logical_rect, clip, viewport, root_scale) {
                continue;
            }

            let raster_source = text_glyph_raster_source(text_draw, raster_rect);
            let source_draw = raster_source.draw.as_ref();
            let source_raster_rect = raster_source.raster_rect;

            let run_key = Self::text_glyph_run_cache_key(
                source_draw,
                source_raster_rect,
                text_scale,
                static_text_motion,
            );
            let atlas_generation = self.text_glyph_atlas.generation();
            let mut cached_quad_run = None;
            let cached_glyph_run = if let Some(cached) = self.text_glyph_run_cache.get(&run_key) {
                run_hits = run_hits.saturating_add(1);
                if cached.atlas_generation == atlas_generation {
                    cached_quad_run = cached.quads.as_ref().map(Rc::clone);
                }
                Some(Rc::clone(&cached.glyphs))
            } else {
                run_misses = run_misses.saturating_add(1);
                collected_run.clear();
                let collected = collect_solid_text_atlas_run(
                    source_draw.text.as_ref(),
                    source_raster_rect,
                    &source_draw.text_style,
                    source_draw.color,
                    source_draw.font_size,
                    text_scale,
                    &self.text_fonts,
                    &mut self.text_glyph_mask_cache,
                    &mut collected_run,
                );
                if collected.is_none() {
                    if text_atlas_fallback_diag_enabled() {
                        let preview: String = source_draw.text.text.chars().take(96).collect();
                        log::warn!(
                            "[text-atlas-fallback] node={:?} spans={} links={} text_len={} preview={:?} span_style={:?} paragraph_style={:?}",
                            source_draw.node_id,
                            source_draw.text.span_styles.len(),
                            source_draw.text.links.len(),
                            source_draw.text.text.len(),
                            preview,
                            source_draw.text_style.span_style,
                            source_draw.text_style.paragraph_style,
                        );
                    }
                    fallback = true;
                    break;
                }
                collected_placements.clear();
                collected_placements.extend(
                    collected_run
                        .iter()
                        .map(SoftwareGlyphAtlasRunGlyph::placement),
                );
                let glyphs: Rc<[SoftwareGlyphAtlasPlacement]> =
                    Rc::from(collected_placements.clone().into_boxed_slice());
                self.text_glyph_run_cache.put(
                    run_key,
                    CachedTextGlyphRun {
                        glyphs,
                        quads: None,
                        atlas_generation: 0,
                    },
                );
                None
            };

            let draw_rect = Rect {
                x: source_raster_rect.x / root_scale,
                y: source_raster_rect.y / root_scale,
                width: source_raster_rect.width / root_scale,
                height: source_raster_rect.height / root_scale,
            };
            let Some(scissor) =
                scissor_rect_for_layer(draw_rect, source_draw.clip, root_scale, viewport)
            else {
                continue;
            };

            if let Some(quad_run) = cached_quad_run.as_ref()
                && self.emit_retained_text_glyph_run_if_ready(
                    run_key,
                    quad_run.as_ref(),
                    source_draw.clip,
                    viewport,
                    source_raster_rect,
                    scissor,
                    glyph_cmds,
                )
            {
                emitted_glyphs = emitted_glyphs.saturating_add(quad_run.len());
                continue;
            }

            let index_start = image_indices.len() as u32;
            let (quad_run, cached) = match cached_quad_run {
                Some(quad_run) => (quad_run, true),
                None => {
                    let Ok(quad_run) = self.prepare_text_glyph_quads(
                        run_key,
                        atlas_generation,
                        cached_glyph_run.as_deref(),
                        &collected_run,
                        &mut generated_quads,
                    ) else {
                        fallback = true;
                        break;
                    };
                    (quad_run, false)
                }
            };
            emitted_glyphs = emitted_glyphs.saturating_add(self.append_text_glyph_quad_run(
                source_raster_rect,
                quad_run.as_ref(),
                source_draw.clip,
                viewport,
                root_scale,
                image_vertices,
                image_indices,
                cached,
            ));
            let index_count = image_indices.len() as u32 - index_start;
            if index_count > 0 {
                glyph_cmds.push(GlyphDrawCmd::shared(index_start, index_count, scissor));
            }
        }

        self.scratch_text_glyph_run = collected_run;
        self.scratch_text_glyph_placements = collected_placements;
        self.scratch_text_glyph_quads = generated_quads;
        if fallback {
            image_vertices.truncate(initial_vertex_len);
            image_indices.truncate(initial_index_len);
            glyph_cmds.truncate(initial_cmd_len);
            return Ok(false);
        }
        let append_end = Instant::now();
        if let Some(total_ms) = should_log_wgpu_render_stage(append_start, append_end) {
            log::warn!(
                "[wgpu-render-stage:text-glyph-atlas] total_ms={total_ms:.2} visited={} cmds={} glyphs={} run_hits={} run_misses={}",
                visited,
                glyph_cmds.len().saturating_sub(initial_cmd_len),
                emitted_glyphs,
                run_hits,
                run_misses,
            );
        }
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn append_image_bitmap_draw_cmd(
        &mut self,
        image: &ImageBitmap,
        rect: Rect,
        clip: Option<Rect>,
        sampling: ImageSampling,
        viewport: ViewportUniformParams,
        root_scale: f32,
        image_vertices: &mut Vec<Vertex>,
        image_indices: &mut Vec<u32>,
        image_cmds: &mut Vec<ImageDrawCmd>,
    ) -> Result<(), String> {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return Ok(());
        }

        self.ensure_image_cached(image)?;

        let (device_quad, scissor_rect) =
            if sampling == ImageSampling::Nearest && root_scale.is_finite() && root_scale > 0.0 {
                let left_px = (rect.x * root_scale).round();
                let top_px = (rect.y * root_scale).round();
                let width_px = (rect.width * root_scale).round().max(1.0);
                let height_px = (rect.height * root_scale).round().max(1.0);
                let snapped_rect = Rect {
                    x: left_px / root_scale,
                    y: top_px / root_scale,
                    width: width_px / root_scale,
                    height: height_px / root_scale,
                };
                let right_px = left_px + width_px;
                let bottom_px = top_px + height_px;
                (
                    [
                        [left_px, top_px],
                        [right_px, top_px],
                        [left_px, bottom_px],
                        [right_px, bottom_px],
                    ],
                    snapped_rect,
                )
            } else {
                (
                    rect_to_quad(rect).map(|[x, y]| [x * root_scale, y * root_scale]),
                    rect,
                )
            };

        let Some(scissor) = scissor_rect_for_layer(scissor_rect, clip, root_scale, viewport) else {
            return Ok(());
        };
        let Some(uv_rect) = image_uv_rect(image, None) else {
            return Ok(());
        };

        let base_vertex = image_vertices.len() as u32;
        let index_start = image_indices.len() as u32;
        image_indices.extend_from_slice(&[
            base_vertex,
            base_vertex + 1,
            base_vertex + 2,
            base_vertex + 2,
            base_vertex + 1,
            base_vertex + 3,
        ]);
        let color = [1.0, 1.0, 1.0, 1.0];
        image_vertices.extend_from_slice(&[
            Vertex {
                position: device_quad[0],
                color,
                uv: [uv_rect.min[0], uv_rect.min[1]],
                uv_bounds: uv_rect.sample_bounds,
            },
            Vertex {
                position: device_quad[1],
                color,
                uv: [uv_rect.max[0], uv_rect.min[1]],
                uv_bounds: uv_rect.sample_bounds,
            },
            Vertex {
                position: device_quad[2],
                color,
                uv: [uv_rect.min[0], uv_rect.max[1]],
                uv_bounds: uv_rect.sample_bounds,
            },
            Vertex {
                position: device_quad[3],
                color,
                uv: [uv_rect.max[0], uv_rect.max[1]],
                uv_bounds: uv_rect.sample_bounds,
            },
        ]);
        image_cmds.push(ImageDrawCmd {
            index_start,
            scissor,
            image_id: image.id(),
            sampling,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn append_text_image_draw_cmds<'a, I>(
        &mut self,
        layer_texts: I,
        viewport: ViewportUniformParams,
        root_scale: f32,
        image_vertices: &mut Vec<Vertex>,
        image_indices: &mut Vec<u32>,
        image_cmds: &mut Vec<ImageDrawCmd>,
    ) -> Result<(), String>
    where
        I: Iterator<Item = &'a TextDraw>,
    {
        let append_start = Instant::now();
        let initial_len = image_cmds.len();
        let mut visited = 0usize;
        let mut hit_count = 0usize;
        let mut miss_count = 0usize;
        for text_draw in layer_texts {
            visited = visited.saturating_add(1);
            let _ = text_draw.node_id;
            let Some((logical_rect, raster_rect, clip, text_scale, static_text_motion)) =
                self.text_raster_geometry(text_draw, root_scale)
            else {
                continue;
            };
            if !text_draw_is_visible_in_viewport(logical_rect, clip, viewport, root_scale) {
                continue;
            }

            let raster_source = self.text_image_raster_source(
                text_draw,
                logical_rect,
                raster_rect,
                clip,
                root_scale,
                static_text_motion,
            );
            let source_draw = raster_source.draw.as_ref();
            let source_raster_rect = raster_source.raster_rect;

            let cache_key = Self::text_image_cache_key(
                source_draw,
                source_raster_rect,
                text_scale,
                static_text_motion,
            );
            let image = if let Some(cached) = self.text_image_cache.get(&cache_key) {
                self.frame_stats
                    .record_text_image_cache_hit(cached.image.width(), cached.image.height());
                hit_count = hit_count.saturating_add(1);
                cached.image.clone()
            } else {
                let Some(image) =
                    self.rasterize_text_draw_to_image(source_draw, source_raster_rect, text_scale)
                else {
                    continue;
                };
                self.frame_stats
                    .record_text_image_cache_miss(image.width(), image.height());
                miss_count = miss_count.saturating_add(1);
                self.text_image_cache.put(
                    cache_key,
                    CachedTextImage {
                        image: image.clone(),
                    },
                );
                image
            };

            let draw_origin = if static_text_motion {
                Point::new(
                    source_raster_rect.x / root_scale,
                    source_raster_rect.y / root_scale,
                )
            } else {
                Point::new(logical_rect.x, logical_rect.y)
            };
            let draw_rect = Rect {
                x: draw_origin.x,
                y: draw_origin.y,
                width: image.width() as f32 / root_scale,
                height: image.height() as f32 / root_scale,
            };
            self.append_image_bitmap_draw_cmd(
                &image,
                draw_rect,
                clip,
                ImageSampling::Nearest,
                viewport,
                root_scale,
                image_vertices,
                image_indices,
                image_cmds,
            )?;
        }
        let append_end = Instant::now();
        if let Some(total_ms) = should_log_wgpu_render_stage(append_start, append_end) {
            log::warn!(
                "[wgpu-render-stage:text-images] total_ms={total_ms:.2} visited={} emitted={} hits={} misses={}",
                visited,
                image_cmds.len().saturating_sub(initial_len),
                hit_count,
                miss_count,
            );
        }
        Ok(())
    }

    fn text_image_raster_source<'a>(
        &mut self,
        text_draw: &'a TextDraw,
        logical_rect: Rect,
        raster_rect: Rect,
        clip: Option<Rect>,
        root_scale: f32,
        static_text_motion: bool,
    ) -> TextRasterSource<'a> {
        let Some(clip) = clip else {
            return TextRasterSource {
                draw: Cow::Borrowed(text_draw),
                raster_rect,
            };
        };
        if !static_text_motion || text_draw.text.text.as_str().find('\n').is_none() {
            return TextRasterSource {
                draw: Cow::Borrowed(text_draw),
                raster_rect,
            };
        }

        let line_starts = self.text_line_index_cache.line_starts(&text_draw.text);
        clipped_text_raster_source_with_line_starts(
            text_draw,
            logical_rect,
            raster_rect,
            clip,
            root_scale,
            line_starts.as_ref(),
        )
    }

    fn text_raster_geometry(
        &self,
        text_draw: &TextDraw,
        root_scale: f32,
    ) -> Option<(Rect, Rect, Option<Rect>, f32, bool)> {
        text_raster_geometry_for_draw(text_draw, root_scale)
    }

    fn text_image_cache_key(
        text_draw: &TextDraw,
        raster_rect: Rect,
        text_scale: f32,
        static_text_motion: bool,
    ) -> TextImageCacheKey {
        let mut state = default_hash::new();
        text_draw.text.render_hash().hash(&mut state);
        text_draw.text_style.render_hash().hash(&mut state);
        text_draw.color.render_hash().hash(&mut state);
        hash_text_raster_geometry_for_cache(raster_rect, static_text_motion, &mut state);
        text_draw.font_size.to_bits().hash(&mut state);
        text_scale.to_bits().hash(&mut state);
        text_draw.layout_options.hash(&mut state);
        TextImageCacheKey(state.finish())
    }

    fn text_glyph_run_cache_key(
        text_draw: &TextDraw,
        raster_rect: Rect,
        text_scale: f32,
        static_text_motion: bool,
    ) -> TextGlyphRunCacheKey {
        TextGlyphRunCacheKey(
            Self::text_image_cache_key(text_draw, raster_rect, text_scale, static_text_motion).0,
        )
    }

    fn rasterize_text_draw_to_image(
        &mut self,
        text_draw: &TextDraw,
        raster_rect: Rect,
        text_scale: f32,
    ) -> Option<ImageBitmap> {
        if text_draw.text.span_styles.is_empty() {
            let font = self.text_fonts.resolve(&text_draw.text_style)?;
            return rasterize_text_to_image_with_glyph_cache(
                text_draw.text.text.as_str(),
                raster_rect,
                &text_draw.text_style,
                text_draw.color,
                text_draw.font_size,
                text_scale,
                font,
                &mut self.text_glyph_mask_cache,
            );
        }

        if let Some(image) = rasterize_annotated_text_to_image_with_glyph_cache(
            text_draw.text.as_ref(),
            raster_rect,
            &text_draw.text_style,
            text_draw.color,
            text_draw.font_size,
            text_scale,
            &self.text_fonts,
            &mut self.text_glyph_mask_cache,
        ) {
            return Some(image);
        }

        rasterize_spanned_text_to_image(
            text_draw,
            raster_rect,
            text_scale,
            &self.text_fonts,
            &mut self.text_glyph_mask_cache,
        )
    }
}

fn rasterize_spanned_text_to_image(
    text_draw: &TextDraw,
    raster_rect: Rect,
    text_scale: f32,
    fonts: &SoftwareTextFontSet,
    glyph_cache: &mut SoftwareGlyphRasterCache,
) -> Option<ImageBitmap> {
    let width = raster_rect.width.ceil().max(1.0) as u32;
    let height = raster_rect.height.ceil().max(1.0) as u32;
    let mut canvas = vec![0_u8; (width as usize) * (height as usize) * 4];
    let boundaries = text_draw.text.span_boundaries();
    let base_line_height = text_draw
        .text_style
        .resolve_line_height(14.0, text_draw.font_size)
        .max(1.0);
    let mut current_line_height = base_line_height;
    let mut cursor_x = raster_rect.x;
    let mut cursor_y = raster_rect.y;

    for window in boundaries.windows(2) {
        let start = window[0];
        let end = window[1];
        if start == end {
            continue;
        }

        let chunk = &text_draw.text.text[start..end];
        let mut merged_span = text_draw.text_style.span_style.clone();
        for span in &text_draw.text.span_styles {
            if span.range.start <= start && span.range.end >= end {
                merged_span = merged_span.merge(&span.item);
            }
        }

        let mut chunk_style = text_draw.text_style.clone();
        chunk_style.span_style = merged_span;

        for part in chunk.split_inclusive('\n') {
            let has_newline = part.ends_with('\n');
            let content = if has_newline {
                &part[..part.len().saturating_sub(1)]
            } else {
                part
            };

            if !content.is_empty() {
                let chunk_font_size = chunk_style.resolve_font_size(text_draw.font_size);
                let Some(font) = fonts.resolve(&chunk_style) else {
                    continue;
                };
                let metrics = measure_text_with_font(content, &chunk_style, chunk_font_size, font);
                let segment_rect = Rect {
                    x: cursor_x,
                    y: cursor_y,
                    width: (metrics.width * text_scale).ceil().max(1.0),
                    height: (metrics.height * text_scale).ceil().max(1.0),
                };
                if let Some(segment_image) = rasterize_text_to_image_with_glyph_cache(
                    content,
                    segment_rect,
                    &chunk_style,
                    chunk_style.resolve_text_color(text_draw.color),
                    chunk_font_size,
                    text_scale,
                    font,
                    glyph_cache,
                ) {
                    composite_text_segment(
                        &mut canvas,
                        width,
                        height,
                        raster_rect,
                        segment_rect,
                        &segment_image,
                    );
                }
                cursor_x += metrics.width * text_scale;
                current_line_height = current_line_height.max(metrics.line_height.max(1.0));
            }

            if has_newline {
                cursor_x = raster_rect.x;
                cursor_y += current_line_height * text_scale;
                current_line_height = base_line_height;
            }
        }
    }

    ImageBitmap::from_rgba8(width, height, canvas).ok()
}

struct TextRasterSource<'a> {
    draw: Cow<'a, TextDraw>,
    raster_rect: Rect,
}

fn text_glyph_raster_source(text_draw: &TextDraw, raster_rect: Rect) -> TextRasterSource<'_> {
    TextRasterSource {
        draw: Cow::Borrowed(text_draw),
        raster_rect,
    }
}

fn clipped_text_raster_source_with_line_starts<'a>(
    text_draw: &'a TextDraw,
    logical_rect: Rect,
    raster_rect: Rect,
    clip: Rect,
    root_scale: f32,
    line_starts: &[usize],
) -> TextRasterSource<'a> {
    if line_starts.len() < MIN_MULTILINE_TEXT_LINES_FOR_CLIPPED_RASTER {
        return TextRasterSource {
            draw: Cow::Borrowed(text_draw),
            raster_rect,
        };
    }

    let Some(visible_rect) = logical_rect.intersect(clip) else {
        return TextRasterSource {
            draw: Cow::Borrowed(text_draw),
            raster_rect,
        };
    };

    let line_count = line_starts.len().max(1);
    let line_height = logical_rect.height / line_count as f32;
    if !line_height.is_finite() || line_height <= 0.0 {
        return TextRasterSource {
            draw: Cow::Borrowed(text_draw),
            raster_rect,
        };
    }

    let visible_top = ((visible_rect.y - logical_rect.y) / line_height).floor() as isize;
    let visible_bottom =
        ((visible_rect.y + visible_rect.height - logical_rect.y) / line_height).ceil() as isize;
    let start_line = visible_top.saturating_sub(1).max(0) as usize;
    let end_line = (visible_bottom + 1).max(start_line as isize + 1) as usize;
    let end_line = end_line.min(line_count);
    if start_line == 0 && end_line >= line_count {
        return TextRasterSource {
            draw: Cow::Borrowed(text_draw),
            raster_rect,
        };
    }

    let byte_start = line_starts[start_line];
    let byte_end = line_end_offset(text_draw.text.text.as_str(), line_starts, end_line - 1);
    if byte_start >= byte_end {
        return TextRasterSource {
            draw: Cow::Borrowed(text_draw),
            raster_rect,
        };
    }

    let slice_y = logical_rect.y + start_line as f32 * line_height;
    let slice_height = (end_line - start_line) as f32 * line_height;
    let mut slice_raster_rect = Rect {
        x: logical_rect.x * root_scale,
        y: slice_y * root_scale,
        width: logical_rect.width * root_scale,
        height: slice_height * root_scale,
    };
    slice_raster_rect.x = slice_raster_rect.x.round();
    slice_raster_rect.y = slice_raster_rect.y.round();
    slice_raster_rect.width = slice_raster_rect.width.ceil().max(1.0);
    slice_raster_rect.height = slice_raster_rect.height.ceil().max(1.0);

    let mut sliced_draw = text_draw.clone();
    sliced_draw.rect = Rect {
        x: logical_rect.x,
        y: slice_y,
        width: logical_rect.width,
        height: slice_height,
    };
    sliced_draw.text = Arc::new(text_draw.text.subsequence(byte_start..byte_end));

    TextRasterSource {
        draw: Cow::Owned(sliced_draw),
        raster_rect: slice_raster_rect,
    }
}

fn line_start_offsets(text: &str) -> Vec<usize> {
    let mut starts =
        Vec::with_capacity(text.as_bytes().iter().filter(|b| **b == b'\n').count() + 1);
    starts.push(0);
    starts.extend(
        text.char_indices()
            .filter_map(|(index, ch)| (ch == '\n').then_some(index + ch.len_utf8())),
    );
    starts
}

fn line_end_offset(text: &str, line_starts: &[usize], line: usize) -> usize {
    line_starts.get(line + 1).copied().unwrap_or(text.len())
}

fn composite_text_segment(
    canvas: &mut [u8],
    canvas_width: u32,
    canvas_height: u32,
    canvas_rect: Rect,
    segment_rect: Rect,
    segment_image: &ImageBitmap,
) {
    let offset_x = (segment_rect.x - canvas_rect.x).round() as i32;
    let offset_y = (segment_rect.y - canvas_rect.y).round() as i32;
    let src = segment_image.pixels();
    for sy in 0..segment_image.height() as i32 {
        let dy = offset_y + sy;
        if dy < 0 || dy >= canvas_height as i32 {
            continue;
        }
        for sx in 0..segment_image.width() as i32 {
            let dx = offset_x + sx;
            if dx < 0 || dx >= canvas_width as i32 {
                continue;
            }
            let src_index = ((sy as u32 * segment_image.width() + sx as u32) * 4) as usize;
            let dst_index = ((dy as u32 * canvas_width + dx as u32) * 4) as usize;
            blend_rgba_pixel(
                &mut canvas[dst_index..dst_index + 4],
                &src[src_index..src_index + 4],
            );
        }
    }
}

fn blend_rgba_pixel(dst: &mut [u8], src: &[u8]) {
    let src_alpha = src[3] as f32 / 255.0;
    if src_alpha <= 0.0 {
        return;
    }
    let dst_alpha = dst[3] as f32 / 255.0;
    let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);
    if out_alpha <= f32::EPSILON {
        dst.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }

    for channel in 0..3 {
        let src_channel = src[channel] as f32 / 255.0;
        let dst_channel = dst[channel] as f32 / 255.0;
        let src_premult = src_channel * src_alpha;
        let dst_premult = dst_channel * dst_alpha;
        dst[channel] =
            (((src_premult + dst_premult * (1.0 - src_alpha)) / out_alpha).clamp(0.0, 1.0) * 255.0)
                .round() as u8;
    }
    dst[3] = (out_alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
}

fn align_to(value: u32, alignment: u32) -> u32 {
    debug_assert!(alignment > 0);
    value.div_ceil(alignment) * alignment
}

fn align_usize_to(value: usize, alignment: usize) -> usize {
    debug_assert!(alignment > 0);
    value.div_ceil(alignment) * alignment
}

impl GpuRenderer {
    fn convert_surface_pixels_to_rgba(&self, pixels: &[u8]) -> Result<Vec<u8>, String> {
        if !pixels.len().is_multiple_of(4) {
            return Err("Screenshot readback has an incomplete pixel".to_string());
        }
        Ok(pixels.to_vec())
    }
}

/// The scissor of a logical rect in a target whose origin sits at
/// `viewport.offset` of the scene's device space, clamped to the target.
/// `None` when nothing of the rect lands in the target.
pub(crate) fn scissor_rect_for_rect(
    rect: Rect,
    root_scale: f32,
    viewport: ViewportUniformParams,
) -> Option<(u32, u32, u32, u32)> {
    let width = viewport.width as f32;
    let height = viewport.height as f32;
    let left = (canonicalize_device_coordinate(rect.x * root_scale) - viewport.offset[0])
        .clamp(0.0, width)
        .floor();
    let top = (canonicalize_device_coordinate(rect.y * root_scale) - viewport.offset[1])
        .clamp(0.0, height)
        .floor();
    let right = (canonicalize_device_coordinate((rect.x + rect.width) * root_scale)
        - viewport.offset[0])
        .clamp(0.0, width)
        .ceil();
    let bottom = (canonicalize_device_coordinate((rect.y + rect.height) * root_scale)
        - viewport.offset[1])
        .clamp(0.0, height)
        .ceil();
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

fn scissor_rect_for_layer(
    rect: Rect,
    clip: Option<Rect>,
    root_scale: f32,
    viewport: ViewportUniformParams,
) -> Option<(u32, u32, u32, u32)> {
    let clipped_rect = match clip {
        Some(clip_rect) => rect.intersect(clip_rect)?,
        None => rect,
    };
    scissor_rect_for_rect(clipped_rect, root_scale, viewport)
}
fn tint_for_image(
    color_filter: Option<ColorFilter>,
    alpha: f32,
) -> ([f32; 4], Option<ColorFilter>) {
    let alpha = alpha.clamp(0.0, 1.0);
    match color_filter {
        Some(filter) if filter.supports_gpu_vertex_modulation() => {
            let Some(tint) = filter.gpu_vertex_tint() else {
                return ([1.0, 1.0, 1.0, alpha], Some(filter));
            };
            (
                [
                    tint[0].clamp(0.0, 1.0),
                    tint[1].clamp(0.0, 1.0),
                    tint[2].clamp(0.0, 1.0),
                    (tint[3] * alpha).clamp(0.0, 1.0),
                ],
                None,
            )
        }
        Some(filter) => ([1.0, 1.0, 1.0, alpha], Some(filter)),
        None => ([1.0, 1.0, 1.0, alpha], None),
    }
}

fn image_uv_rect(image: &ImageBitmap, src_rect: Option<Rect>) -> Option<ImageUvRect> {
    let Some(src) = src_rect else {
        return Some(ImageUvRect {
            min: [0.0, 0.0],
            max: [1.0, 1.0],
            sample_bounds: [0.0, 0.0, 1.0, 1.0],
        });
    };

    let (u_min, u_max, u_bound_min, u_bound_max) =
        source_axis_uv(src.x, src.width, image.width() as f32)?;
    let (v_min, v_max, v_bound_min, v_bound_max) =
        source_axis_uv(src.y, src.height, image.height() as f32)?;

    Some(ImageUvRect {
        min: [u_min, v_min],
        max: [u_max, v_max],
        sample_bounds: [u_bound_min, v_bound_min, u_bound_max, v_bound_max],
    })
}

fn glyph_atlas_uv_rect(entry: GlyphAtlasEntry, atlas_size: u32) -> ImageUvRect {
    let atlas_width = atlas_size as f32;
    let atlas_height = atlas_size as f32;
    let min = [entry.x as f32 / atlas_width, entry.y as f32 / atlas_height];
    let max = [
        (entry.x + entry.width) as f32 / atlas_width,
        (entry.y + entry.height) as f32 / atlas_height,
    ];
    let center_min = [
        (entry.x as f32 + 0.5) / atlas_width,
        (entry.y as f32 + 0.5) / atlas_height,
    ];
    let center_max = [
        (entry.x as f32 + entry.width as f32 - 0.5).max(entry.x as f32 + 0.5) / atlas_width,
        (entry.y as f32 + entry.height as f32 - 0.5).max(entry.y as f32 + 0.5) / atlas_height,
    ];
    ImageUvRect {
        min,
        max,
        sample_bounds: [center_min[0], center_min[1], center_max[0], center_max[1]],
    }
}

fn snap_nearest_image_to_device_pixels(image: &mut ImageDraw, root_scale: f32) {
    if image.sampling != ImageSampling::Nearest || !root_scale.is_finite() || root_scale <= 0.0 {
        return;
    }

    let Some(rect) = axis_aligned_quad_rect(image.quad) else {
        return;
    };

    let left_px = (rect.x * root_scale).round();
    let top_px = (rect.y * root_scale).round();
    let width_px = (rect.width * root_scale).round().max(1.0);
    let height_px = (rect.height * root_scale).round().max(1.0);
    let snapped = Rect {
        x: left_px / root_scale,
        y: top_px / root_scale,
        width: width_px / root_scale,
        height: height_px / root_scale,
    };

    image.rect = snapped;
    image.local_rect = Rect {
        x: image.local_rect.x + snapped.x - rect.x,
        y: image.local_rect.y + snapped.y - rect.y,
        width: snapped.width,
        height: snapped.height,
    };
    image.quad = crate::rect_to_quad(snapped);
}

fn nearest_image_device_quad(image: &ImageDraw, root_scale: f32) -> Option<[[f32; 2]; 4]> {
    if image.sampling != ImageSampling::Nearest || !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }

    let rect = axis_aligned_quad_rect(image.quad)?;
    let left_px = (rect.x * root_scale).round();
    let top_px = (rect.y * root_scale).round();
    let width_px = (rect.width * root_scale).round().max(1.0);
    let height_px = (rect.height * root_scale).round().max(1.0);
    let right_px = left_px + width_px;
    let bottom_px = top_px + height_px;
    Some([
        [left_px, top_px],
        [right_px, top_px],
        [left_px, bottom_px],
        [right_px, bottom_px],
    ])
}

fn source_axis_uv(start: f32, extent: f32, image_extent: f32) -> Option<(f32, f32, f32, f32)> {
    if !start.is_finite()
        || !extent.is_finite()
        || !image_extent.is_finite()
        || extent == 0.0
        || image_extent <= 0.0
    {
        return None;
    }

    let end = start + extent;
    let edge_min = start.min(end).clamp(0.0, image_extent);
    let edge_max = start.max(end).clamp(0.0, image_extent);
    if edge_max <= edge_min {
        return None;
    }

    let center_min = edge_min + 0.5;
    let center_max = edge_max - 0.5;
    let (bound_min, bound_max) = if center_min <= center_max {
        (center_min, center_max)
    } else {
        let center = (edge_min + edge_max) * 0.5;
        (center, center)
    };

    Some((
        edge_min / image_extent,
        edge_max / image_extent,
        bound_min / image_extent,
        bound_max / image_extent,
    ))
}

fn apply_filter_to_bitmap(image: &ImageBitmap, filter: ColorFilter) -> Result<ImageBitmap, String> {
    let mut filtered = Vec::with_capacity(image.pixels().len());
    for pixel in image.pixels().as_chunks::<4>().0 {
        let rgba = [
            pixel[0] as f32 / 255.0,
            pixel[1] as f32 / 255.0,
            pixel[2] as f32 / 255.0,
            pixel[3] as f32 / 255.0,
        ];
        let out = filter.apply_rgba(rgba);
        filtered.push((out[0].clamp(0.0, 1.0) * 255.0).round() as u8);
        filtered.push((out[1].clamp(0.0, 1.0) * 255.0).round() as u8);
        filtered.push((out[2].clamp(0.0, 1.0) * 255.0).round() as u8);
        filtered.push((out[3].clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    ImageBitmap::from_rgba8(image.width(), image.height(), filtered)
        .map_err(|error| format!("failed to build filtered bitmap: {error}"))
}

fn scissor_rect_for_image(
    image: &ImageDraw,
    root_scale: f32,
    viewport: ViewportUniformParams,
) -> Option<(u32, u32, u32, u32)> {
    scissor_rect_for_layer(image.rect, image.clip, root_scale, viewport)
}

/// The rounded mask a shadow's composite applies, in the target's pixels: an
/// inner shadow masks itself to its fill shape, and a shadow lowered out of a
/// clipped layer masks itself to that layer's rounded clip.
/// The rounded mask a shadow's composite applies, in the scene's device
/// pixels: an inner shadow masks itself to its fill shape, and a shadow
/// lowered out of a clipped layer masks itself to that layer's rounded clip.
fn shadow_composite_mask(
    shadow: &ShadowDraw,
    snap_anchor: Option<SnapAnchor>,
    root_scale: f32,
) -> Option<RoundedCompositeMask> {
    inner_shadow_composite_mask(shadow, root_scale).or_else(|| {
        shadow.rounded_clip.map(|clip| RoundedCompositeMask {
            rect: mask_rect(anchored_device_rect(clip.rect, snap_anchor, root_scale)),
            radii: clip.radii.map(|radius| radius * root_scale),
        })
    })
}

/// A scene holding just a shadow's own draws, in the order they arrive, so
/// the shadow source renders through the same pass encoder as everything
/// else.
fn shadow_scene(shapes: Option<&RunDraw>, texts: &[TextDraw]) -> CompositorScene {
    let mut scene = CompositorScene::new();
    if let Some(run) = shapes {
        scene.push_run(run.clone());
    }
    for text in texts {
        let z_index = scene.next_z();
        scene.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::Text(scene.texts.len()),
        });
        scene.texts.push(text.clone());
        scene.next_z += 1;
    }
    scene
}

/// The whole device pixels a logical rect covers, or `None` when it covers
/// none or more than a texture can hold.
fn device_pixel_bounds(
    rect: Rect,
    root_scale: f32,
    max_texture_dim: u32,
) -> Option<DevicePixelBounds> {
    let x = (rect.x * root_scale).floor();
    let y = (rect.y * root_scale).floor();
    let right = ((rect.x + rect.width) * root_scale).ceil();
    let bottom = ((rect.y + rect.height) * root_scale).ceil();
    let width = (right - x).max(0.0) as u32;
    let height = (bottom - y).max(0.0) as u32;
    if width == 0 || height == 0 || width > max_texture_dim || height > max_texture_dim {
        return None;
    }
    Some(DevicePixelBounds {
        x,
        y,
        width,
        height,
    })
}
fn inner_shadow_composite_mask(
    shadow: &ShadowDraw,
    root_scale: f32,
) -> Option<RoundedCompositeMask> {
    let run = shadow.shapes.as_ref()?;
    if !run
        .tables()
        .shapes
        .iter()
        .any(|record| record.blend_mode() == BlendMode::DstOut)
    {
        return None;
    }
    let fill = run.tables().shapes.get(0)?;
    let rect = run.placement.translated_bounds(fill.stored_rect());
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let resolved =
        cranpose_ui_graphics::RoundedCornerShape::with_radii(cranpose_ui_graphics::CornerRadii {
            top_left: fill.radii[0],
            top_right: fill.radii[1],
            bottom_right: fill.radii[2],
            bottom_left: fill.radii[3],
        })
        .resolve(rect.width, rect.height);
    let radii = [
        resolved.top_left * root_scale,
        resolved.top_right * root_scale,
        resolved.bottom_left * root_scale,
        resolved.bottom_right * root_scale,
    ];

    Some(RoundedCompositeMask {
        rect: mask_rect(anchored_device_rect(
            rect,
            run.placement.snap_anchor,
            root_scale,
        )),
        radii,
    })
}

fn window_draws(draws: &mut SmallVec<[RunDrawCall; 8]>, window: &std::ops::Range<u32>) {
    let mut relative = 0u32;
    draws.retain(|draw| {
        let count = draw.records.end - draw.records.start;
        let first = relative;
        relative += count;
        let keep_start = window.start.max(first).min(first + count);
        let keep_end = window.end.min(first + count).max(keep_start);
        draw.records =
            draw.records.start + (keep_start - first)..draw.records.start + (keep_end - first);
        draw.records.start < draw.records.end
    });
}
