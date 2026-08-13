//! GPU rendering implementation using WGPU

use crate::effect_renderer::{
    projective_dest_bounds_rect, CompositeBatchItem, CompositeSampleMode, EffectRenderer,
    EffectScratchTargetProvider, ProjectiveSurfaceComposite, RoundedCompositeMask,
    ShaderCompositeBatchItem,
};
use crate::frame_graph::{
    FrameCommandRecorder, FrameTextureDescriptor, WgpuFrameGraph, WgpuFrameGraphExecutor,
};
use crate::frame_packet::{
    CancelReason, FramePacket, PacketRoot, PresentOutcome, RenderReturns, RootSurfacePacket,
};
use crate::layer_events::{
    collect_effect_ranges, collect_layer_events, LayerEvent, LayerEventKind,
};
use crate::layer_surface_cache::LayerSurfaceCache;
use crate::lazy_resource::LazyGpuResource;
#[cfg(test)]
use crate::normalized_scene::{
    build_scene_window, collect_layer_contents, collect_layer_contents_with_translation_context,
    filtered_effect_layer_index, scene_bounds, SceneWindowSource,
};
#[cfg(test)]
use crate::normalized_scene::{estimate_layer_surface_rect, motion_stable_capture_bounds};
use crate::normalized_scene::{translate_quad, ChildLayerComposite, CollectedLayer};
use crate::offscreen::OffscreenTarget;
use crate::rect_to_quad;
use crate::scene::{
    BackdropLayer, CompositorScene, DrawOp, DrawOpKind, DrawShape, EffectLayer, ImageDraw,
    RetainedDraw, ShadowDraw, SimilarityTransform, SnapAnchor, TextDraw,
};
use crate::shaders;
#[cfg(test)]
use crate::surface_executor::surface_target_size;
use crate::surface_executor::{
    apply_backdrop_layer_to_target as execute_apply_backdrop_layer_to_target,
    axis_aligned_quad_rect, backdrop_underlay_is_covered_by_local_content,
    canonicalize_device_coordinate, canonicalized_scaled_quad, canonicalized_scaled_rect,
    composite_surface_to_view as execute_composite_surface_to_view, device_pixel_bounds_for_rect,
    offscreen_byte_size, render_effect_layer_to_target as execute_render_effect_layer_to_target,
    render_layer_surface as execute_render_layer_surface,
    render_root_direct as execute_render_root_direct, root_direct_scene_events_are_supported,
    scaled_quad, snap_delta_for_anchor, snap_motion_stable_dest_quad,
    translation_stable_anchored_device_pixel_bounds, DevicePixelBounds, LayerSurfaceTexture,
    SurfaceExecutionBackend,
};
#[cfg(test)]
use crate::surface_executor::{clamp_effect_surface_scale, visible_layer_rect};
#[cfg(test)]
use crate::surface_plan::root_can_render_directly_cached;
#[cfg(test)]
use crate::surface_plan::{
    composite_sample_mode_for_effect_layer, composite_sample_mode_for_requirements,
    direct_translation, effect_layer_target_scale, layer_contains_descendant_backdrop,
    layer_surface_requirements, layer_surface_requirements_cached, layer_surface_scale,
    layer_surface_target_scale, layer_uses_external_backdrop_input, TranslatedContentAxes,
};
use crate::surface_plan::{LayerSurfaceRequest, TranslationRenderContext};
#[cfg(test)]
use crate::surface_requirements::SurfaceRequirement;
use crate::surface_requirements::SurfaceRequirementSet;
use crate::DebugCpuAllocationStats;
use bytemuck::{Pod, Zeroable};
#[cfg(any(not(target_arch = "wasm32"), test))]
use cranpose_core::collections::map::HashMap;
use cranpose_core::{hash::default as default_hash, NodeId};
use cranpose_render_common::bounded_lru_cache::BoundedLruCache;
use cranpose_render_common::geometry::blur_extent_margin;
use cranpose_render_common::graph::quad_bounds;
#[cfg(test)]
use cranpose_render_common::graph::{
    CachePolicy, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform,
    RenderNode,
};
use cranpose_render_common::raster_cache::LayerRasterCacheKey;
#[cfg(test)]
use cranpose_render_common::raster_cache::ScaleBucket;
use cranpose_render_common::software_text_raster::{
    collect_solid_text_atlas_run, measure_text_with_font,
    rasterize_annotated_text_to_image_with_glyph_cache, rasterize_text_to_image_with_glyph_cache,
    SoftwareGlyphAtlasGlyph, SoftwareGlyphAtlasKey, SoftwareGlyphAtlasPlacement,
    SoftwareGlyphAtlasRunGlyph, SoftwareGlyphRasterCache, SoftwareTextFontSet,
};
#[cfg(test)]
use cranpose_ui_graphics::GraphicsLayer;
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, ColorFilter, FxHasher, ImageBitmap, ImageSampling, Point, Rect,
    RenderEffect, RenderHash, RuntimeShader, StrokeCap, StrokeJoin, TileMode,
};
use std::borrow::Cow;
use std::cell::Cell;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use web_time::Instant;

use crate::gpu_stats;
use crate::gpu_stats::gpu_stats_enabled;
use crate::pipeline::push_layer_shadow;

/// Must equal the `array<ShapeData, N>` literal in `shape.wgsl`: on wasm the
/// shader source is used verbatim, so a larger batch cap here would index past
/// the declared array. 102 x 160-byte ShapeData = 16320 bytes, the most that
/// fits WebGL's 16 KiB uniform-binding floor.
#[cfg(target_arch = "wasm32")]
const MAX_SHAPES_PER_BATCH: usize = 102;
#[cfg(not(target_arch = "wasm32"))]
const MAX_SHAPES_PER_BATCH: usize = 768;
#[cfg(target_arch = "wasm32")]
const MAX_GRADIENT_STOPS: usize = 256;
#[cfg(not(target_arch = "wasm32"))]
const MAX_GRADIENT_STOPS: usize = 1024;

/// Per-pass ceilings when the shape and gradient arrays live in storage
/// buffers instead of uniforms. These are not hardware limits — storage
/// bindings are hundreds of megabytes everywhere — they bound worst-case
/// buffer growth: 65 536 shapes is a 7 MiB shape buffer and a 12 MiB vertex
/// buffer, far past any real scene, while still forcing a batch split before
/// a pathological one can ask for gigabytes.
#[cfg(not(target_arch = "wasm32"))]
const MAX_SHAPES_PER_STORAGE_BATCH: usize = 1 << 16;
#[cfg(not(target_arch = "wasm32"))]
const MAX_GRADIENT_STOPS_PER_STORAGE_BATCH: usize = 1 << 16;

/// How many shapes/stops the storage-mode buffers start out sized for. In
/// uniform mode the initial capacity must equal the cap (a uniform binding
/// smaller than the shader's fixed-length array fails validation), but a
/// runtime-sized storage array binds at any size, so start small and let
/// `ensure_capacity` double toward the cap as scenes demand.
#[cfg(not(target_arch = "wasm32"))]
const INITIAL_STORAGE_BATCH_CAPACITY: usize = 1024;

/// Shape/gradient batch capacities derived from the actual device limits.
///
/// Where storage buffers are available (any real Vulkan/Metal/D3D device, and
/// GL only when it exposes SSBOs to fragment shaders) the arrays are bound as
/// read-only storage and a whole scene fits one batch. Otherwise they fall
/// back to uniform arrays: the compile-time `MAX_*` constants assume
/// desktop-class 64 KiB uniform bindings, while Android downlevel and
/// GLES-class devices may only offer the 16 KiB spec minimum; sizing the
/// buffers (and the matching WGSL array lengths) past
/// `max_uniform_buffer_binding_size` makes the very first "Shape Bind Group"
/// fail validation and aborts the app.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ShapeBatchLimits {
    max_shapes_per_batch: usize,
    max_gradient_stops: usize,
    storage: bool,
}

impl ShapeBatchLimits {
    fn for_device(device: &wgpu::Device) -> Self {
        let limits = device.limits();
        #[cfg(not(target_arch = "wasm32"))]
        if limits.max_storage_buffers_per_shader_stage >= 2 {
            return Self::for_storage_binding_size(limits.max_storage_buffer_binding_size);
        }
        Self::for_uniform_binding_size(limits.max_uniform_buffer_binding_size)
    }

    fn for_uniform_binding_size(max_uniform_buffer_binding_size: u64) -> Self {
        let binding = max_uniform_buffer_binding_size as usize;
        Self {
            max_shapes_per_batch: (binding / std::mem::size_of::<ShapeData>())
                .clamp(1, MAX_SHAPES_PER_BATCH),
            max_gradient_stops: (binding / std::mem::size_of::<GradientStop>())
                .clamp(1, MAX_GRADIENT_STOPS),
            storage: false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn for_storage_binding_size(max_storage_buffer_binding_size: u64) -> Self {
        let binding = max_storage_buffer_binding_size as usize;
        Self {
            max_shapes_per_batch: (binding / std::mem::size_of::<ShapeData>())
                .clamp(1, MAX_SHAPES_PER_STORAGE_BATCH),
            max_gradient_stops: (binding / std::mem::size_of::<GradientStop>())
                .clamp(1, MAX_GRADIENT_STOPS_PER_STORAGE_BATCH),
            storage: true,
        }
    }

    fn initial_shape_capacity(&self) -> usize {
        #[cfg(not(target_arch = "wasm32"))]
        if self.storage {
            return self
                .max_shapes_per_batch
                .min(INITIAL_STORAGE_BATCH_CAPACITY);
        }
        self.max_shapes_per_batch
    }

    fn initial_gradient_capacity(&self) -> usize {
        #[cfg(not(target_arch = "wasm32"))]
        if self.storage {
            return self.max_gradient_stops.min(INITIAL_STORAGE_BATCH_CAPACITY);
        }
        self.max_gradient_stops
    }

    fn data_buffer_usage(&self) -> wgpu::BufferUsages {
        if self.storage {
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST
        } else {
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST
        }
    }

    fn data_binding_type(&self) -> wgpu::BufferBindingType {
        if self.storage {
            wgpu::BufferBindingType::Storage { read_only: true }
        } else {
            wgpu::BufferBindingType::Uniform
        }
    }

    #[cfg(test)]
    fn desktop() -> Self {
        Self::for_uniform_binding_size(wgpu::Limits::default().max_uniform_buffer_binding_size)
    }
}
#[cfg(target_arch = "wasm32")]
const HARD_MAX_BUFFER_MB: usize = 64; // Maximum 64MB per buffer (image vertex/index only)
const MAX_SHADOW_SURFACE_CACHE_ITEMS: usize = 512;
// Sized for HiDPI: a 4K fractional-scale screen full of shadowed panels needs
// ~10-15 rasters of 4-12MB each; a 64MB budget made the large entries evict
// each other every frame during scroll, re-blurring tens of megapixels.
const MAX_SHADOW_SURFACE_CACHE_BYTES: u64 = 192 * 1024 * 1024;
const MAX_TEXT_IMAGE_CACHE_ITEMS: usize = 1024;
const MAX_TEXT_GLYPH_MASK_CACHE_ITEMS: usize = 8192;
const MAX_TEXT_GLYPH_ATLAS_ITEMS: usize = 8192;
const MAX_TEXT_GLYPH_RUN_CACHE_ITEMS: usize = 1024;
#[cfg(not(target_arch = "wasm32"))]
const MAX_TEXT_GLYPH_GPU_RUN_CACHE_ITEMS: usize = 1024;
#[cfg(not(target_arch = "wasm32"))]
const MIN_RETAINED_TEXT_GLYPH_QUADS: usize = 192;
#[cfg(not(target_arch = "wasm32"))]
const OFFSCREEN_TEXT_GLYPH_PREWARM_BUDGET_MS: f64 = 0.75;
#[cfg(not(target_arch = "wasm32"))]
const MAX_OFFSCREEN_TEXT_GLYPH_PREWARM_CANDIDATES: usize = 2;
#[cfg(not(target_arch = "wasm32"))]
const MAX_OFFSCREEN_TEXT_GLYPH_PREWARM_UNCACHED_CHARS: usize = 160;
#[cfg(not(target_arch = "wasm32"))]
const MAX_OFFSCREEN_TEXT_GLYPH_PREWARM_CACHED_GLYPHS: usize = 160;
/// Side length the glyph atlas starts at, and the one it doubles towards.
///
/// The atlas is square and `R8Unorm`, so the maximum is a 16 MiB texture. That
/// was also the starting size until it became the single largest resource the
/// renderer allocated: a 454x454 watch face draws a couple of hundred distinct
/// glyphs and needs well under a megabyte of them, but paid the full 16 MiB at
/// renderer construction, before a single glyph had been rastered. Starting at
/// `MIN` and doubling on overflow (see `TextGlyphAtlas::reset`) costs at most
/// three extra resets for a workload that genuinely needs the large atlas —
/// which then behaves exactly as the fixed 4096 atlas did — and costs a
/// text-light screen 256 KiB instead of 16 MiB, permanently.
const TEXT_GLYPH_ATLAS_MIN_SIZE: u32 = 512;
const TEXT_GLYPH_ATLAS_MAX_SIZE: u32 = 4096;
const TEXT_GLYPH_ATLAS_PADDING: u32 = 1;
const MAX_TEXT_LINE_INDEX_CACHE_ITEMS: usize = 512;
const MIN_MULTILINE_TEXT_LINES_FOR_CLIPPED_RASTER: usize = 2;
const MAX_OBSERVED_SCENE_RANGE_CACHE_MISSES: usize = 128;
const CACHE_MISS_WARMUP_FRAMES: u8 = 1;
pub(crate) const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: cranpose_render_common::FRAME_CLEAR_COLOR[0] as f64,
    g: cranpose_render_common::FRAME_CLEAR_COLOR[1] as f64,
    b: cranpose_render_common::FRAME_CLEAR_COLOR[2] as f64,
    a: cranpose_render_common::FRAME_CLEAR_COLOR[3] as f64,
};
#[cfg(not(target_arch = "wasm32"))]
const INITIAL_UPLOAD_BUFFER_BYTES: u64 = 4 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const INITIAL_RETAINED_GLYPH_UNIFORM_SLOTS: usize = 128;
const MAX_TEXTURE_CACHE_ITEMS: usize = 256;
/// Byte ceiling for `image_texture_cache` (see `CachedImageTexture::bytes`).
/// Generous enough for a screenful of full-page images plus thumbnails;
/// small enough that a camera preview stream can never pin gigabytes.
const MAX_IMAGE_TEXTURE_CACHE_BYTES: usize = 256 * 1024 * 1024;
const RETAINED_STAGED_UPLOAD_BYTES: usize = 256 * 1024;
const RETAINED_STAGED_UPLOAD_COPIES: usize = 128;
pub(crate) const RETAINED_LAYER_REQUIREMENTS_CAPACITY: usize = 512;
const DEFAULT_WGPU_RENDER_STAGE_TELEMETRY_THRESHOLD_MS: f64 = 4.0;
#[cfg(not(target_arch = "wasm32"))]
static SEGMENT_DIAG_LINES: AtomicUsize = AtomicUsize::new(0);
// Reclaim oversized text scratch allocations only after a meaningful 4x collapse
// from a previously large frame; smaller swings are left alone to avoid churn.

fn wgpu_render_stage_telemetry_threshold_ms() -> Option<f64> {
    static THRESHOLD_MS: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *THRESHOLD_MS.get_or_init(|| {
        let explicit = std::env::var("CRANPOSE_WGPU_RENDER_STAGE_TELEMETRY_MS")
            .ok()
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

fn admit_layer_surface_cache_miss_impl(
    key: &LayerRasterCacheKey,
    observed_scene_range_misses: &mut BoundedLruCache<LayerRasterCacheKey, ()>,
) -> bool {
    if !key.is_scene_range() {
        return true;
    }
    if observed_scene_range_misses.contains(key) {
        return true;
    }
    observed_scene_range_misses.put(*key, ());
    false
}

#[cfg(test)]
fn first_cache_miss_admission(key: &LayerRasterCacheKey) -> bool {
    let mut observed_scene_range_misses =
        BoundedLruCache::with_capacity_at_least_one(MAX_OBSERVED_SCENE_RANGE_CACHE_MISSES);
    admit_layer_surface_cache_miss_impl(key, &mut observed_scene_range_misses)
}

#[cfg(test)]
fn repeated_cache_miss_admission(key: &LayerRasterCacheKey) -> bool {
    let mut observed_scene_range_misses =
        BoundedLruCache::with_capacity_at_least_one(MAX_OBSERVED_SCENE_RANGE_CACHE_MISSES);
    let _ = admit_layer_surface_cache_miss_impl(key, &mut observed_scene_range_misses);
    admit_layer_surface_cache_miss_impl(key, &mut observed_scene_range_misses)
}

pub static PRESENTED_FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn frames_presented() -> u64 {
    PRESENTED_FRAMES.load(std::sync::atomic::Ordering::Relaxed)
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

fn text_atlas_fallback_diag_enabled() -> bool {
    cranpose_core::env_flag!("CRANPOSE_TEXT_ATLAS_FALLBACK_DIAG")
}

fn text_glyph_run_diag_enabled() -> bool {
    cranpose_core::env_flag!("CRANPOSE_TEXT_GLYPH_RUN_DIAG")
}

fn root_direct_diag_enabled() -> bool {
    cranpose_core::env_flag!("CRANPOSE_ROOT_DIRECT_DIAG")
}

fn scene_layer_events_precede_z(scene: &CompositorScene, z_index: usize) -> bool {
    scene
        .effect_layers
        .iter()
        .any(|layer| layer.z_start < z_index && 0 < layer.z_end)
        || scene
            .backdrop_layers
            .iter()
            .any(|layer| layer.z_index < z_index)
}

fn direct_root_child_can_be_replayed_into_later_underlay(child: &ChildLayerComposite) -> bool {
    child.backdrop.is_none()
        && !child.has_effect
        && child.shadow_draws.is_empty()
        && axis_aligned_quad_rect(child.dest_quad).is_some()
}

fn rects_overlap(a: Rect, b: Rect) -> bool {
    let a_right = a.x + a.width;
    let a_bottom = a.y + a.height;
    let b_right = b.x + b.width;
    let b_bottom = b.y + b.height;
    a.x < b_right && b.x < a_right && a.y < b_bottom && b.y < a_bottom
}

pub(crate) fn direct_root_child_underlays_are_supported(collected: &CollectedLayer) -> bool {
    for (child_index, child) in collected.child_layers.iter().enumerate() {
        if child.backdrop.is_some() {
            if root_direct_diag_enabled() {
                log::warn!(
                    "[root-direct-diag] reject self-backdrop child node={:?}",
                    child.node_id
                );
            }
            return false;
        }
        if child.needs_nested_underlay {
            let Some(dest_rect) = axis_aligned_quad_rect(child.dest_quad) else {
                if root_direct_diag_enabled() {
                    log::warn!(
                        "[root-direct-diag] reject projective underlay child node={:?}",
                        child.node_id
                    );
                }
                return false;
            };
            let translation_only = (dest_rect.width - child.logical_rect.width).abs() <= 0.001
                && (dest_rect.height - child.logical_rect.height).abs() <= 0.001;
            let unsupported_preceding_child_layer = collected.child_layers[..child_index]
                .iter()
                .any(|preceding| {
                    if direct_root_child_can_be_replayed_into_later_underlay(preceding) {
                        return false;
                    }
                    axis_aligned_quad_rect(preceding.dest_quad)
                        .is_none_or(|preceding_rect| rects_overlap(preceding_rect, dest_rect))
                });
            let preceding_scene_events =
                scene_layer_events_precede_z(&collected.scene, child.z_index);
            if unsupported_preceding_child_layer || preceding_scene_events || !translation_only {
                if root_direct_diag_enabled() {
                    log::warn!(
                        "[root-direct-diag] reject underlay child node={:?} unsupported_preceding_child_layer={} preceding_scene_events={} translation_only={} dest=({:.1},{:.1},{:.1},{:.1}) logical=({:.1},{:.1},{:.1},{:.1})",
                        child.node_id,
                        unsupported_preceding_child_layer,
                        preceding_scene_events,
                        translation_only,
                        dest_rect.x,
                        dest_rect.y,
                        dest_rect.width,
                        dest_rect.height,
                        child.logical_rect.x,
                        child.logical_rect.y,
                        child.logical_rect.width,
                        child.logical_rect.height
                    );
                }
                return false;
            }
        }
    }
    true
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

struct CachedShadowComposite {
    source: Rc<OffscreenTarget>,
    scissor: Option<(u32, u32, u32, u32)>,
    rounded_mask: Option<RoundedCompositeMask>,
    dest_viewport: Option<(f32, f32, f32, f32)>,
}

impl CachedShadowComposite {
    fn batch_item(&self) -> CompositeBatchItem<'_> {
        CompositeBatchItem {
            source: &self.source,
            alpha: 1.0,
            scissor: self.scissor,
            rounded_mask: self.rounded_mask,
            blend_mode: BlendMode::SrcOver,
            dest_viewport: self.dest_viewport,
            source_viewport: None,
            sample_mode: CompositeSampleMode::Nearest,
        }
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

const TEXT_GLYPH_PREWARM_VIEWPORT_MULTIPLIER: f32 = 2.0;

#[cfg(not(target_arch = "wasm32"))]
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
        if let Some(cached) = self.entries.get(&key) {
            if cached.len == text.text.len()
                && cached
                    .text
                    .upgrade()
                    .is_some_and(|cached_text| Arc::ptr_eq(&cached_text, text))
            {
                return cached.starts.clone();
            }
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct ShapeShadowSurfacePlan {
    source_device_bounds: DevicePixelBounds,
    processing_scissor: Option<(u32, u32, u32, u32)>,
    pixel_radius: f32,
}

#[derive(Default)]
struct RendererWarningState {
    unsupported_effect_reported: Cell<bool>,
}

impl RendererWarningState {
    fn warn_unsupported_effect_once(&self) {
        if !self.unsupported_effect_reported.replace(true) {
            log::warn!(
                "WGPU renderer received an unsupported RenderEffect variant; falling back to passthrough compositing"
            );
        }
    }
}

fn is_blend_mode_supported(mode: BlendMode) -> bool {
    matches!(mode, BlendMode::SrcOver | BlendMode::DstOut)
}

fn blend_state_for_mode(mode: BlendMode) -> wgpu::BlendState {
    match mode {
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

fn supported_blend_mode(mode: BlendMode) -> BlendMode {
    if is_blend_mode_supported(mode) {
        return mode;
    }

    BlendMode::SrcOver
}

fn direct_shader_composite_viewport(
    alpha: f32,
    blend_mode: BlendMode,
    dest_viewport: Option<(f32, f32, f32, f32)>,
    sample_mode: CompositeSampleMode,
    source_size: (u32, u32),
) -> Option<(f32, f32, f32, f32)> {
    if alpha != 1.0 || supported_blend_mode(blend_mode) != BlendMode::SrcOver {
        return None;
    }
    let viewport = dest_viewport?;
    if viewport.2 <= 0.0 || viewport.3 <= 0.0 {
        return None;
    }
    match sample_mode {
        CompositeSampleMode::Linear | CompositeSampleMode::Nearest => Some(viewport),
        CompositeSampleMode::Box4
            if shader_composite_preserves_source_pixel_grid(viewport, source_size) =>
        {
            Some(viewport)
        }
        CompositeSampleMode::Box4 => None,
    }
}

fn shader_composite_preserves_source_pixel_grid(
    viewport: (f32, f32, f32, f32),
    source_size: (u32, u32),
) -> bool {
    const EPSILON: f32 = 0.01;
    let (x, y, width, height) = viewport;
    let (source_width, source_height) = source_size;
    (x - x.round()).abs() <= EPSILON
        && (y - y.round()).abs() <= EPSILON
        && (width - source_width as f32).abs() <= EPSILON
        && (height - source_height as f32).abs() <= EPSILON
}

type DirectShaderTailComposite<'a> = (&'a RenderEffect, &'a RuntimeShader, (f32, f32, f32, f32));

fn direct_shader_tail_composite(
    effect: &RenderEffect,
    alpha: f32,
    blend_mode: BlendMode,
    dest_viewport: Option<(f32, f32, f32, f32)>,
    sample_mode: CompositeSampleMode,
    source_size: (u32, u32),
) -> Option<DirectShaderTailComposite<'_>> {
    let viewport = direct_shader_composite_viewport(
        alpha,
        blend_mode,
        dest_viewport,
        sample_mode,
        source_size,
    )?;
    let RenderEffect::Chain { first, second } = effect else {
        return None;
    };
    let RenderEffect::Shader { shader } = second.as_ref() else {
        return None;
    };
    Some((first.as_ref(), shader, viewport))
}

fn hash_f32_for_cache<H: Hasher>(value: f32, state: &mut H) {
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
    // Clips are resolved in scene space from their own layer ancestry. A draw
    // item's raster snap must never move a fixed ancestor clip.
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

fn text_draw_should_prewarm_in_viewport(
    logical_rect: Rect,
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
    let margin_x = viewport_rect.width * TEXT_GLYPH_PREWARM_VIEWPORT_MULTIPLIER;
    let margin_y = viewport_rect.height * TEXT_GLYPH_PREWARM_VIEWPORT_MULTIPLIER;
    let prewarm_viewport = expand_rect(viewport_rect, margin_x, margin_y);
    let prewarm_rect = match clip {
        Some(clip) => expand_rect(clip, margin_x, margin_y).intersect(prewarm_viewport),
        None => Some(prewarm_viewport),
    };
    prewarm_rect.is_some_and(|rect| logical_rect.intersect(rect).is_some())
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
    let visible_rect = match clip {
        Some(clip) => clip.intersect(viewport_rect),
        None => Some(viewport_rect),
    };
    visible_rect.is_some_and(|visible| rect.intersect(visible).is_some())
}

fn shape_draw_is_visible_in_viewport(
    shape: &DrawShape,
    viewport: ViewportUniformParams,
    root_scale: f32,
) -> bool {
    let snap_delta = shape
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
        .unwrap_or_default();
    let rect = quad_bounds(translate_quad(shape.quad, snap_delta));
    let clip = shape.clip;
    draw_rect_is_visible_in_viewport(rect, clip, viewport, root_scale)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextGlyphDrawAction {
    DrawVisible,
    PrewarmOffscreen,
    Skip,
}

fn text_glyph_draw_action(
    is_visible: bool,
    is_prewarm_candidate: bool,
    allow_offscreen_prewarm: bool,
) -> TextGlyphDrawAction {
    if is_visible {
        TextGlyphDrawAction::DrawVisible
    } else if allow_offscreen_prewarm && is_prewarm_candidate {
        TextGlyphDrawAction::PrewarmOffscreen
    } else {
        TextGlyphDrawAction::Skip
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn should_use_retained_text_glyph_run(quads_len: usize, clip: Option<Rect>) -> bool {
    clip.is_none() && quads_len >= MIN_RETAINED_TEXT_GLYPH_QUADS
}

#[cfg(not(target_arch = "wasm32"))]
fn offscreen_text_glyph_prewarm_work_is_bounded(
    cached_glyphs: Option<usize>,
    text_len: usize,
) -> bool {
    match cached_glyphs {
        Some(glyphs) => glyphs <= MAX_OFFSCREEN_TEXT_GLYPH_PREWARM_CACHED_GLYPHS,
        None => text_len <= MAX_OFFSCREEN_TEXT_GLYPH_PREWARM_UNCACHED_CHARS,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn offscreen_text_glyph_prewarm_budget_exhausted(
    start: Instant,
    admitted_candidates: usize,
) -> bool {
    admitted_candidates >= MAX_OFFSCREEN_TEXT_GLYPH_PREWARM_CANDIDATES
        || instant_ms(start, Instant::now()) >= OFFSCREEN_TEXT_GLYPH_PREWARM_BUDGET_MS
}

fn text_draws_for_ordered_range<'a>(
    ordered_items: &'a [(usize, SegmentDrawItem)],
    texts: &'a [TextDraw],
    start: usize,
    end: usize,
) -> Result<impl Iterator<Item = &'a TextDraw>, String> {
    let range_items = ordered_items
        .get(start..end)
        .ok_or_else(|| format!("text batch range {start}..{end} is outside ordered draw items"))?;
    for (_, item) in range_items {
        match item {
            SegmentDrawItem::Text(text_index) if *text_index < texts.len() => {}
            SegmentDrawItem::Text(text_index) => {
                return Err(format!(
                    "text batch references missing text draw index: {text_index}"
                ));
            }
            _ => return Err(format!("text batch contains non-text draw item: {item:?}")),
        }
    }

    Ok(range_items.iter().filter_map(move |(_, item)| match item {
        SegmentDrawItem::Text(text_index) => texts.get(*text_index),
        _ => None,
    }))
}

/// Shadow geometry is hashed in device pixels quantized to 1/16 px so rigid
/// translations reuse the cached blurred raster. The cached surface is
/// composited one-to-one with texel-exact sampling; translation may not change
/// either the blur or its sampling phase.
const SHADOW_CACHE_DEVICE_QUANT: f32 = 16.0;

fn hash_shadow_device_offset<H: Hasher>(value: f32, origin: f32, root_scale: f32, state: &mut H) {
    let quantized = ((value - origin) * root_scale * SHADOW_CACHE_DEVICE_QUANT).round();
    (quantized as i64).hash(state);
}

fn hash_shadow_device_rect<H: Hasher>(
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

fn hash_shape_shadow_item<H: Hasher>(
    shape: &DrawShape,
    blend_mode: BlendMode,
    origin_x: f32,
    origin_y: f32,
    root_scale: f32,
    state: &mut H,
) {
    hash_shadow_device_rect(shape.rect, origin_x, origin_y, root_scale, state);
    hash_shadow_device_rect(shape.local_rect, origin_x, origin_y, root_scale, state);
    for point in shape.quad {
        hash_shadow_device_offset(point[0], origin_x, root_scale, state);
        hash_shadow_device_offset(point[1], origin_y, root_scale, state);
    }
    match shape.snap_anchor {
        Some(anchor) => {
            1u8.hash(state);
            hash_shadow_device_offset(anchor.origin.x, origin_x, root_scale, state);
            hash_shadow_device_offset(anchor.origin.y, origin_y, root_scale, state);
            hash_f32_for_cache(anchor.device_pixel_step, state);
        }
        None => 0u8.hash(state),
    }
    shape.brush.render_hash().hash(state);
    match shape.shape {
        Some(corner_shape) => {
            1u8.hash(state);
            corner_shape.radii().render_hash().hash(state);
        }
        None => 0u8.hash(state),
    }
    match shape.clip {
        Some(clip) => {
            1u8.hash(state);
            hash_shadow_device_rect(clip, origin_x, origin_y, root_scale, state);
        }
        None => 0u8.hash(state),
    }
    blend_mode.hash(state);
    shape.blend_mode.hash(state);
}

fn shape_shadow_content_hash(shapes: &[(DrawShape, BlendMode)], root_scale: f32) -> u64 {
    let mut hasher = FxHasher::default();
    // Anchor the hash to the shapes' own (unfloored) bounds so rigid translation
    // cancels out exactly. Anchoring to floored device-pixel bounds would leak
    // the device subpixel phase into the hash and defeat the cache at
    // fractional display scales.
    let origin = shape_shadow_bounds(shapes).unwrap_or(Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    });

    shapes.len().hash(&mut hasher);
    for (shape, blend_mode) in shapes {
        hash_shape_shadow_item(
            shape,
            *blend_mode,
            origin.x,
            origin.y,
            root_scale,
            &mut hasher,
        );
    }
    hasher.finish()
}

fn shape_shadow_surface_cache_key(
    shapes: &[(DrawShape, BlendMode)],
    device_bounds: DevicePixelBounds,
    pixel_radius: f32,
    root_scale: f32,
) -> Option<ShadowSurfaceCacheKey> {
    (root_scale.is_finite() && root_scale > 0.0).then(|| ShadowSurfaceCacheKey {
        content_hash: shape_shadow_content_hash(shapes, root_scale),
        pixel_size: [device_bounds.width, device_bounds.height],
        root_scale_bits: root_scale.to_bits(),
        blur_radius_bits: pixel_radius.to_bits(),
    })
}

fn shape_shadow_bounds(shapes: &[(DrawShape, BlendMode)]) -> Option<Rect> {
    shapes
        .iter()
        .map(|(shape, _)| shape.rect)
        .reduce(|a, b| Rect {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            width: (a.x + a.width).max(b.x + b.width) - a.x.min(b.x),
            height: (a.y + a.height).max(b.y + b.height) - a.y.min(b.y),
        })
}

fn shared_shape_shadow_snap_anchor(shapes: &[(DrawShape, BlendMode)]) -> Option<SnapAnchor> {
    let anchor = shapes.first()?.0.snap_anchor?;
    shapes
        .iter()
        .all(|(shape, _)| shape.snap_anchor == Some(anchor))
        .then_some(anchor)
}

fn shadow_draw_bounds(shadow: &ShadowDraw) -> Option<Rect> {
    shadow
        .shapes
        .iter()
        .map(|(shape, _)| shape.rect)
        .chain(shadow.texts.iter().map(|text| text.rect))
        .reduce(|a, b| Rect {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            width: (a.x + a.width).max(b.x + b.width) - a.x.min(b.x),
            height: (a.y + a.height).max(b.y + b.height) - a.y.min(b.y),
        })
}

fn shadow_draw_may_render(
    shadow: &ShadowDraw,
    width: u32,
    height: u32,
    root_scale: f32,
    max_texture_dim: u32,
) -> bool {
    if shadow.texts.is_empty() && !shadow.shapes.is_empty() && shadow.blur_radius > 0.0 {
        return shape_shadow_surface_plan(
            &shadow.shapes,
            shadow.clip,
            shadow.blur_radius,
            width,
            height,
            root_scale,
            max_texture_dim,
        )
        .is_some();
    }

    let Some(bounds) = shadow_draw_bounds(shadow) else {
        return false;
    };
    let blur_margin = blur_extent_margin(shadow.blur_radius);
    let mut visible_bounds = Rect {
        x: bounds.x - blur_margin,
        y: bounds.y - blur_margin,
        width: bounds.width + blur_margin * 2.0,
        height: bounds.height + blur_margin * 2.0,
    };
    if let Some(clip) = shadow.clip {
        let clip_expanded = Rect {
            x: clip.x - blur_margin,
            y: clip.y - blur_margin,
            width: clip.width + blur_margin * 2.0,
            height: clip.height + blur_margin * 2.0,
        };
        let Some(intersection) = visible_bounds.intersect(clip_expanded) else {
            return false;
        };
        visible_bounds = intersection;
    }

    scissor_rect_for_rect(visible_bounds, root_scale, width, height).is_some()
}

fn shape_shadow_surface_plan(
    shapes: &[(DrawShape, BlendMode)],
    clip: Option<Rect>,
    blur_radius: f32,
    width: u32,
    height: u32,
    root_scale: f32,
    max_texture_dim: u32,
) -> Option<ShapeShadowSurfacePlan> {
    let shape_bounds = shape_shadow_bounds(shapes)?;
    let blur_margin = blur_extent_margin(blur_radius);
    let source_blur_bounds = Rect {
        x: shape_bounds.x - blur_margin,
        y: shape_bounds.y - blur_margin,
        width: shape_bounds.width + blur_margin * 2.0,
        height: shape_bounds.height + blur_margin * 2.0,
    };

    let mut visible_blur_bounds = source_blur_bounds;
    if let Some(clip) = clip {
        let clip_expanded = Rect {
            x: clip.x - blur_margin,
            y: clip.y - blur_margin,
            width: clip.width + blur_margin * 2.0,
            height: clip.height + blur_margin * 2.0,
        };
        visible_blur_bounds = visible_blur_bounds.intersect(clip_expanded)?;
    }

    let processing_scissor = scissor_rect_for_rect(visible_blur_bounds, root_scale, width, height);
    processing_scissor?;
    let visible_device_bounds =
        device_pixel_bounds_for_rect(visible_blur_bounds, width, height, root_scale)?;
    let source_device_bounds = translation_stable_anchored_device_pixel_bounds(
        source_blur_bounds,
        shared_shape_shadow_snap_anchor(shapes),
        root_scale,
        max_texture_dim,
    )
    .unwrap_or(visible_device_bounds);

    Some(ShapeShadowSurfacePlan {
        source_device_bounds,
        processing_scissor,
        pixel_radius: blur_radius * root_scale,
    })
}

fn is_render_effect_supported(effect: &RenderEffect) -> bool {
    match effect {
        RenderEffect::Blur { .. } => true,
        RenderEffect::Offset { .. } => true,
        RenderEffect::Shader { .. } => true,
        RenderEffect::Chain { first, second } => {
            is_render_effect_supported(first) && is_render_effect_supported(second)
        }
    }
}

fn resolve_gradient_point(origin: f32, extent: f32, value: f32) -> f32 {
    if value.is_finite() {
        origin + value
    } else if value.is_sign_positive() {
        origin + extent
    } else {
        origin
    }
}

fn gradient_tile_mode_value(tile_mode: TileMode) -> u32 {
    match tile_mode {
        TileMode::Clamp => 0,
        TileMode::Repeated => 1,
        TileMode::Mirror => 2,
        TileMode::Decal => 3,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn shape_shader_source(batch_limits: ShapeBatchLimits) -> Cow<'static, str> {
    // These literals must stay in sync with `shape.wgsl`; a mismatch makes
    // the substitution silently no-op and leaves the shader sized for the
    // downlevel floor.
    if batch_limits.storage {
        return Cow::Owned(
            shaders::SHADER
                .replace(
                    "var<uniform> shape_data: array<ShapeData, 102>;",
                    "var<storage, read> shape_data: array<ShapeData>;",
                )
                .replace(
                    "var<uniform> gradient_stops: array<GradientStop, 256>;",
                    // Also inject the retained-paint array here: one mutable
                    // color per shape, read when `similarity.paint_select`
                    // is set, so recolor patches upload 16-byte colors
                    // instead of whole ShapeData records. The base text
                    // never declares it — uniform-mode devices cannot bind
                    // storage and never host retained slots.
                    "var<storage, read> gradient_stops: array<GradientStop>;\n\n\
                     @group(1) @binding(3)\n\
                     var<storage, read> paint: array<vec4<f32>>;",
                )
                .replace(
                    "output.color = shape.color;",
                    "output.color = \
                     select(shape.color, paint[shape_idx], similarity.paint_select > 0.5);",
                ),
        );
    }
    Cow::Owned(
        shaders::SHADER
            .replace(
                "array<ShapeData, 102>",
                &format!("array<ShapeData, {}>", batch_limits.max_shapes_per_batch),
            )
            .replace(
                "array<GradientStop, 256>",
                &format!("array<GradientStop, {}>", batch_limits.max_gradient_stops),
            ),
    )
}

#[cfg(target_arch = "wasm32")]
fn shape_shader_source(_batch_limits: ShapeBatchLimits) -> Cow<'static, str> {
    Cow::Borrowed(shaders::SHADER)
}

fn create_shape_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    uniform_layout: &wgpu::BindGroupLayout,
    shape_layout: &wgpu::BindGroupLayout,
    blend_mode: BlendMode,
    batch_limits: ShapeBatchLimits,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Shape Shader"),
        source: wgpu::ShaderSource::Wgsl(shape_shader_source(batch_limits)),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[Some(uniform_layout), Some(shape_layout)],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            // No vertex buffer: `vs_main` pulls quad corners from ShapeData
            // by `vertex_index`.
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
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
    })
}

/// Storage-mode pipeline for retained slots that captured a conservative arc
/// mesh: `vs_mesh` consumes `{position, uv, shape_idx}` vertices instead of
/// expanding six corners per shape. Fragment stage, bind group layouts
/// (including the dynamic-offset similarity binding and the retained paint
/// binding) and the SrcOver blend are exactly the ones the quad-expansion retained
/// path uses — only the vertex fetch differs.
#[cfg(not(target_arch = "wasm32"))]
fn create_mesh_shape_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    uniform_layout: &wgpu::BindGroupLayout,
    shape_layout: &wgpu::BindGroupLayout,
    batch_limits: ShapeBatchLimits,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Shape Mesh Shader"),
        source: wgpu::ShaderSource::Wgsl(shape_shader_source(batch_limits)),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Mesh Render Pipeline Layout"),
        bind_group_layouts: &[Some(uniform_layout), Some(shape_layout)],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Retained Mesh Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_mesh"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[MeshVertex::desc()],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
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
    })
}

/// Storage-mode pipeline for ordinary shape batches drawn as instanced
/// indexed quads (`vs_shape_instanced`): four vertex executions per shape
/// through the static `[0, 1, 2, 2, 1, 3]` index buffer instead of six
/// unindexed corner expansions. Everything but the vertex entry point is
/// exactly `create_shape_pipeline` — same fragment stage, same layouts,
/// same blend per mode — so a draw-time fallback to `vs_main` (the
/// `CRANPOSE_INSTANCED_QUADS=0` kill switch) changes nothing else.
#[cfg(not(target_arch = "wasm32"))]
fn create_instanced_shape_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    uniform_layout: &wgpu::BindGroupLayout,
    shape_layout: &wgpu::BindGroupLayout,
    blend_mode: BlendMode,
    batch_limits: ShapeBatchLimits,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Shape Instanced Shader"),
        source: wgpu::ShaderSource::Wgsl(shape_shader_source(batch_limits)),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Instanced Render Pipeline Layout"),
        bind_group_layouts: &[Some(uniform_layout), Some(shape_layout)],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Instanced Render Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_shape_instanced"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            // No vertex buffer: like `vs_main`, the corners come from
            // ShapeData; only the shape index source differs
            // (`instance_index` instead of `vertex_index / 6`).
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
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
    })
}

fn create_image_pipeline(
    device: &wgpu::Device,
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

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
    })
}

fn create_glyph_atlas_pipeline(
    device: &wgpu::Device,
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

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
    })
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Vertex {
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
}

/// Mirror of `struct ShapeData` in `shape.wgsl`. Field order and sizes must
/// match exactly: 10 x 16 bytes = 160 bytes, every member 16-byte aligned as
/// the uniform address space requires. The quad corners and vertex color ride
/// in here because the shape pipeline has no vertex buffer: the vertex shader
/// pulls all six corners of a shape straight from this struct.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ShapeData {
    rect: [f32; 4], // x, y, width, height
    /// Rects: top_left, top_right, bottom_left, bottom_right corner radii.
    /// Arcs: (sin, cos) of the mid angle and of the half sweep — the shader's
    /// per-shape trig, precomputed so `sdf_arc_band` needs none per fragment.
    radii: [f32; 4],
    gradient_params: [f32; 4], // linear: start.xy,end.xy; radial: center.xy,radius,unused
    clip_rect: [f32; 4],       // clip_x, clip_y, clip_width, clip_height (0,0,0,0 = no clip)
    /// stroke width, packed flags (see [`pack_shape_flags`]), arc outer radius,
    /// arc inner radius. All zero for a plain fill.
    stroke_params: [f32; 4],
    /// arc center.xy, start angle, sweep angle (radians, 0 = +X, clockwise).
    arc_params: [f32; 4],
    /// Device-space quad corners 0 (xy) and 1 (zw).
    quad01: [f32; 4],
    /// Device-space quad corners 2 (xy) and 3 (zw).
    quad23: [f32; 4],
    /// Vertex color: the solid brush color, or the first gradient stop.
    color: [f32; 4],
    brush_type: u32,         // 0=solid, 1=linear_gradient, 2=radial_gradient
    gradient_start: u32,     // Starting index in gradient buffer
    gradient_count: u32,     // Number of gradient stops
    gradient_tile_mode: u32, // 0=Clamp, 1=Repeated, 2=Mirror, 3=Decal
}

/// Shape kinds understood by `shape.wgsl`.
const SHAPE_KIND_FILL: u32 = 0;
const SHAPE_KIND_STROKE: u32 = 1;
const SHAPE_KIND_ARC: u32 = 2;

fn stroke_cap_code(cap: StrokeCap) -> u32 {
    match cap {
        StrokeCap::Butt => 0,
        StrokeCap::Round => 1,
        StrokeCap::Square => 2,
    }
}

fn stroke_join_code(join: StrokeJoin) -> u32 {
    match join {
        StrokeJoin::Miter => 0,
        StrokeJoin::Round => 1,
        StrokeJoin::Bevel => 2,
    }
}

/// Packs kind/cap/join into the single float `ShapeData::stroke_params[1]`.
///
/// Three 2-bit fields fit in one f32 exactly (integers below 2^24 are exact),
/// which keeps `ShapeData` a slot smaller than it would be if each field got
/// its own float — batch capacity is set by this size on uniform backends.
fn pack_shape_flags(kind: u32, cap: StrokeCap, join: StrokeJoin) -> f32 {
    ((kind & 3) | (stroke_cap_code(cap) << 2) | (stroke_join_code(join) << 4)) as f32
}

/// Whether a batch conversion fans out is decided by measurement — see
/// [`crate::cost_tuner::CostTuner`]. The floor of 256 matters: a device
/// whose uniform binding caps batches at ~409 shapes never crossed the old
/// fixed threshold of 512, so conversion ran serial on exactly the class of
/// hardware (watch-grade in-order cores) where fanning out pays most. The
/// 400 µs cheap floor keeps a big phone core, which clears such a batch in
/// well under that, from ever paying for a spawn wave.
#[cfg(not(target_arch = "wasm32"))]
static SHAPE_CONVERT_TUNER: crate::cost_tuner::CostTuner =
    crate::cost_tuner::CostTuner::new("shape-convert", 256, 400_000);

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn shape_convert_worker_count() -> usize {
    static WORKERS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *WORKERS.get_or_init(|| {
        let cpus = std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1);
        let workers = cpus.clamp(1, 4);
        // One line per process: on devices whose scheduler confines the
        // process (affinity masks, cpusets), this is the number that
        // explains why fan-out stages stayed serial.
        log::info!("[shape-convert] fan-out width {workers} (available parallelism {cpus})");
        workers
    })
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn shape_convert_worker_count() -> usize {
    1
}

fn shape_gradient_stop_count(shape: &DrawShape) -> usize {
    match &shape.brush {
        Brush::Solid(_) => 0,
        Brush::LinearGradient { colors, .. }
        | Brush::RadialGradient { colors, .. }
        | Brush::SweepGradient { colors, .. } => colors.len(),
    }
}

/// Converts one [`DrawShape`] into its GPU representation, writing into
/// pre-sized slots so a batch can convert in parallel across disjoint
/// sub-slices. `gradient_start` is the shape's global offset into the batch
/// gradient buffer; `gradient_out` is exactly its span of that buffer.
fn convert_shape_into_slots(
    shape: &DrawShape,
    root_scale: f32,
    gradient_start: u32,
    shape_out: &mut ShapeData,
    gradient_out: &mut [GradientStop],
) {
    let snap_delta = shape
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
        .unwrap_or_default();
    let local_rect = shape.local_rect.translate(snap_delta.x, snap_delta.y);
    let quad = translate_quad(shape.quad, snap_delta);
    // Clips are resolved in scene space from their own layer ancestry. A draw
    // item's raster snap must never move a fixed ancestor clip.
    let clip = shape.clip;
    let canonicalize = shape.snap_anchor.is_some();
    let device_local_rect = if canonicalize {
        canonicalized_scaled_rect(local_rect, root_scale)
    } else {
        Rect {
            x: local_rect.x * root_scale,
            y: local_rect.y * root_scale,
            width: local_rect.width * root_scale,
            height: local_rect.height * root_scale,
        }
    };
    let device_quad = if canonicalize {
        canonicalized_scaled_quad(quad, root_scale)
    } else {
        scaled_quad(quad, root_scale)
    };
    let canonicalize_brush_coordinate = |value| {
        if canonicalize {
            canonicalize_device_coordinate(value)
        } else {
            value
        }
    };

    // Clip rect (scaled to physical pixels)
    let clip_rect = if let Some(clip) = clip {
        let device_clip = if canonicalize {
            canonicalized_scaled_rect(clip, root_scale)
        } else {
            Rect {
                x: clip.x * root_scale,
                y: clip.y * root_scale,
                width: clip.width * root_scale,
                height: clip.height * root_scale,
            }
        };
        [
            device_clip.x,
            device_clip.y,
            device_clip.width,
            device_clip.height,
        ]
    } else {
        [0.0, 0.0, 0.0, 0.0]
    };

    // Gradient parameters
    let mut fill_gradient_entries = |colors: &[Color], stops: Option<&[f32]>| {
        let count = colors.len();
        let explicit_stops = stops.filter(|values| values.len() == count);
        for (index, color) in colors.iter().enumerate() {
            let position = explicit_stops
                .map(|values| values[index])
                .unwrap_or_else(|| {
                    if count <= 1 {
                        0.0
                    } else {
                        index as f32 / (count - 1) as f32
                    }
                });
            gradient_out[index] = GradientStop {
                color: [color.r(), color.g(), color.b(), color.a()],
                position: [position, 0.0, 0.0, 0.0],
            };
        }
        count as u32
    };
    let mut gradient_params = [0.0f32; 4];
    let (brush_type, gradient_count, gradient_tile_mode) = match &shape.brush {
        Brush::Solid(_) => (0u32, 0u32, gradient_tile_mode_value(TileMode::Clamp)),
        Brush::LinearGradient {
            colors,
            stops,
            start,
            end,
            tile_mode,
        } => {
            let count = fill_gradient_entries(colors, stops.as_deref());
            gradient_params = [
                canonicalize_brush_coordinate(resolve_gradient_point(
                    device_local_rect.x,
                    device_local_rect.width,
                    start.x * root_scale,
                )),
                canonicalize_brush_coordinate(resolve_gradient_point(
                    device_local_rect.y,
                    device_local_rect.height,
                    start.y * root_scale,
                )),
                canonicalize_brush_coordinate(resolve_gradient_point(
                    device_local_rect.x,
                    device_local_rect.width,
                    end.x * root_scale,
                )),
                canonicalize_brush_coordinate(resolve_gradient_point(
                    device_local_rect.y,
                    device_local_rect.height,
                    end.y * root_scale,
                )),
            ];
            (1u32, count, gradient_tile_mode_value(*tile_mode))
        }
        Brush::RadialGradient {
            colors,
            stops,
            center,
            radius,
            tile_mode,
        } => {
            let count = fill_gradient_entries(colors, stops.as_deref());
            gradient_params = [
                canonicalize_brush_coordinate(device_local_rect.x + center.x * root_scale),
                canonicalize_brush_coordinate(device_local_rect.y + center.y * root_scale),
                (radius * root_scale).max(f32::EPSILON),
                0.0,
            ];
            (2u32, count, gradient_tile_mode_value(*tile_mode))
        }
        Brush::SweepGradient {
            colors,
            stops,
            center,
        } => {
            let count = fill_gradient_entries(colors, stops.as_deref());
            gradient_params = [
                canonicalize_brush_coordinate(device_local_rect.x + center.x * root_scale),
                canonicalize_brush_coordinate(device_local_rect.y + center.y * root_scale),
                0.0,
                0.0,
            ];
            (3u32, count, gradient_tile_mode_value(TileMode::Clamp))
        }
    };

    // A stroked rect/round-rect was emitted with `local_rect` already
    // inflated by half the stroke width, so corner radii must resolve
    // against the geometry that was actually asked for, not the
    // inflated box. The shader shrinks `half_size` by the same amount.
    let stroke_outset = shape
        .stroke
        .map(|stroke| stroke.half_width())
        .unwrap_or(0.0);
    let geometry_width = (local_rect.width - stroke_outset * 2.0).max(0.0);
    let geometry_height = (local_rect.height - stroke_outset * 2.0).max(0.0);

    let radii = if let Some(arc) = shape.arc {
        // Arcs never carry corner radii, so this slot ships the shader's
        // per-shape trig instead: (sin, cos) of the sweep's mid angle and of
        // the half sweep. Computing these here — once per shape — is what
        // lets `sdf_arc_band` run without a single transcendental per
        // fragment. A full ring is the common case (dots, particles) and
        // `ArcGeometry::new` normalizes it to start 0 / sweep TAU, whose
        // values are exact constants; the half-sweep sine is pinned to
        // non-negative just like the shader used to, so a closed ring keeps
        // its seam-free (0, -1) form.
        if arc.sweep_angle >= cranpose_ui_graphics::TAU && arc.start_angle == 0.0 {
            [0.0, -1.0, 0.0, -1.0]
        } else {
            let half_sweep = arc.sweep_angle.clamp(0.0, cranpose_ui_graphics::TAU) * 0.5;
            let (mid_sin, mid_cos) = (arc.start_angle + half_sweep).sin_cos();
            let (half_sin, half_cos) = half_sweep.sin_cos();
            [mid_sin, mid_cos, half_sin.max(0.0), half_cos]
        }
    } else if let Some(rounded) = shape.shape {
        let resolved = rounded.resolve(geometry_width, geometry_height);
        [
            resolved.top_left * root_scale,
            resolved.top_right * root_scale,
            resolved.bottom_left * root_scale,
            resolved.bottom_right * root_scale,
        ]
    } else {
        [0.0, 0.0, 0.0, 0.0]
    };

    let device_rect = [
        device_local_rect.x,
        device_local_rect.y,
        device_local_rect.width,
        device_local_rect.height,
    ];

    // Stroke/arc parameters ride in the same ShapeData and the same
    // pipeline as fills, so a stroked or arc shape never splits a
    // batch.
    let (stroke_params, arc_params) = match (shape.arc, shape.stroke) {
        (Some(arc), _) => (
            [
                0.0,
                pack_shape_flags(SHAPE_KIND_ARC, arc.cap, StrokeJoin::Miter),
                arc.outer_radius * root_scale,
                arc.inner_radius * root_scale,
            ],
            [
                (arc.center.x + snap_delta.x) * root_scale,
                (arc.center.y + snap_delta.y) * root_scale,
                arc.start_angle,
                arc.sweep_angle,
            ],
        ),
        (None, Some(stroke)) => (
            [
                stroke.width.max(0.0) * root_scale,
                pack_shape_flags(SHAPE_KIND_STROKE, stroke.cap, stroke.join),
                0.0,
                0.0,
            ],
            [0.0; 4],
        ),
        (None, None) => (
            [
                0.0,
                pack_shape_flags(SHAPE_KIND_FILL, StrokeCap::Butt, StrokeJoin::Miter),
                0.0,
                0.0,
            ],
            [0.0; 4],
        ),
    };

    let color = match &shape.brush {
        Brush::Solid(c) => [c.r(), c.g(), c.b(), c.a()],
        Brush::LinearGradient { colors, .. } => {
            let first = colors.first().unwrap_or(&Color(1.0, 1.0, 1.0, 1.0));
            [first.r(), first.g(), first.b(), first.a()]
        }
        Brush::RadialGradient { colors, .. } | Brush::SweepGradient { colors, .. } => {
            let first = colors.first().unwrap_or(&Color(1.0, 1.0, 1.0, 1.0));
            [first.r(), first.g(), first.b(), first.a()]
        }
    };

    *shape_out = ShapeData {
        rect: device_rect,
        radii,
        gradient_params,
        clip_rect,
        stroke_params,
        arc_params,
        quad01: [
            device_quad[0][0],
            device_quad[0][1],
            device_quad[1][0],
            device_quad[1][1],
        ],
        quad23: [
            device_quad[2][0],
            device_quad[2][1],
            device_quad[3][0],
            device_quad[3][1],
        ],
        color,
        brush_type,
        gradient_start,
        gradient_count,
        gradient_tile_mode,
    };
}

/// `CRANPOSE_QUAD_AREA_DIAG=1` prints, per shape batch, how many device
/// pixels the emitted quads cover — split into arc quads, the true arc band
/// coverage inside them, and everything else. Fill cost is the product of
/// fragment count and shader cost, and this is the fragment-count half: it
/// is how the MEGA scene's ~10x overdraw (and the ~50% of arc-quad area that
/// the SDF discards) was measured.
fn quad_area_diag_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_QUAD_AREA_DIAG").is_some())
}

/// Converts a batch of shapes into pre-sized output slices, fanning the work
/// across scoped threads when the batch is large enough to pay for spawns.
/// The outputs may be scratch vectors or mapped GPU staging memory; each
/// shape writes only its own disjoint slots, so chunked `split_at_mut`
/// hand-off keeps the parallel path free of any synchronization.
fn convert_shapes_into_outputs(
    shape_refs: &[&DrawShape],
    gradient_offsets: &[u32],
    root_scale: f32,
    shape_data_out: &mut [ShapeData],
    gradients_out: &mut [GradientStop],
) {
    let shape_count = shape_refs.len();
    #[cfg(not(target_arch = "wasm32"))]
    let convert_started = Instant::now();
    #[cfg(not(target_arch = "wasm32"))]
    let parallel =
        SHAPE_CONVERT_TUNER.choose_parallel(shape_count) && shape_convert_worker_count() > 1;
    if quad_area_diag_enabled() {
        let quad_area = |q: [[f32; 2]; 4]| {
            // Shoelace over the quad polygon TL, TR, BR, BL (corners 0,1,3,2).
            let poly = [q[0], q[1], q[3], q[2]];
            let mut twice = 0.0f64;
            for i in 0..4 {
                let a = poly[i];
                let b = poly[(i + 1) % 4];
                twice += a[0] as f64 * b[1] as f64 - b[0] as f64 * a[1] as f64;
            }
            twice.abs() * 0.5
        };
        let mut arc_quad = 0.0f64; // quad px of arc shapes
        let mut arc_band = 0.0f64; // true band coverage of those arcs
        let mut arc_count = 0usize;
        let mut ring_count = 0usize;
        let mut other_quad = 0.0f64;
        let mut other_count = 0usize;
        // Largest non-arc quads: (area, index) so the tail of the diag can
        // name what the aggregate "other" fill actually is.
        let mut top_other: Vec<(f64, usize)> = Vec::new();
        for (index, shape) in shape_refs.iter().enumerate() {
            let area = quad_area(shape.quad);
            if let Some(arc) = shape.arc {
                arc_quad += area;
                arc_count += 1;
                if arc.sweep_angle >= cranpose_ui_graphics::TAU {
                    ring_count += 1;
                }
                let ra = arc.mid_radius() as f64;
                let rb = arc.half_thickness() as f64;
                arc_band +=
                    arc.sweep_angle as f64 * ra * (2.0 * rb) + std::f64::consts::PI * rb * rb;
            } else {
                other_quad += area;
                other_count += 1;
                top_other.push((area, index));
            }
        }
        let scale2 = (root_scale as f64) * (root_scale as f64);
        eprintln!(
            "[quad-area] arcs={arc_count} (rings={ring_count}) arc_quad_px={:.0} arc_band_px={:.0} | other={other_count} other_px={:.0}",
            arc_quad * scale2,
            arc_band * scale2,
            other_quad * scale2,
        );
        top_other.sort_by(|a, b| b.0.total_cmp(&a.0));
        for &(area, index) in top_other.iter().take(4) {
            let shape = shape_refs[index];
            let brush = match &shape.brush {
                cranpose_ui_graphics::Brush::Solid(color) => format!("solid a={:.2}", color.3),
                cranpose_ui_graphics::Brush::LinearGradient { colors, .. } => {
                    format!("linear n={}", colors.len())
                }
                cranpose_ui_graphics::Brush::RadialGradient { colors, .. } => {
                    format!("radial n={}", colors.len())
                }
                cranpose_ui_graphics::Brush::SweepGradient { colors, .. } => {
                    format!("sweep n={}", colors.len())
                }
            };
            eprintln!(
                "[quad-area]   top other: {:.0}px {}x{} at ({:.0},{:.0}) {} shape={} stroke={} clip={} blend={:?} z={}",
                area * scale2,
                shape.rect.width.round(),
                shape.rect.height.round(),
                shape.rect.x,
                shape.rect.y,
                brush,
                shape.shape.is_some(),
                shape.stroke.is_some(),
                shape.clip.is_some(),
                shape.blend_mode,
                shape.z_index,
            );
        }
    }
    #[cfg(target_arch = "wasm32")]
    let parallel = false;
    let workers = if parallel {
        shape_convert_worker_count()
    } else {
        1
    };
    if workers <= 1 {
        for (idx, shape) in shape_refs.iter().enumerate() {
            let gradient_start = gradient_offsets[idx];
            let gradient_end = gradient_offsets[idx + 1];
            convert_shape_into_slots(
                shape,
                root_scale,
                gradient_start,
                &mut shape_data_out[idx],
                &mut gradients_out[gradient_start as usize..gradient_end as usize],
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        SHAPE_CONVERT_TUNER.record(
            false,
            shape_count,
            convert_started.elapsed().as_nanos() as u64,
        );
        return;
    }

    let chunk_len = shape_count.div_ceil(workers);
    let mut shape_data_rest = shape_data_out;
    let mut gradients_rest = gradients_out;
    std::thread::scope(|scope| {
        let mut chunk_start = 0usize;
        while chunk_start < shape_count {
            let chunk_end = (chunk_start + chunk_len).min(shape_count);
            let count = chunk_end - chunk_start;
            let gradient_base = gradient_offsets[chunk_start];
            let gradient_span = (gradient_offsets[chunk_end] - gradient_base) as usize;
            let (shape_data_chunk, rest) = std::mem::take(&mut shape_data_rest).split_at_mut(count);
            shape_data_rest = rest;
            let (gradient_chunk, rest) =
                std::mem::take(&mut gradients_rest).split_at_mut(gradient_span);
            gradients_rest = rest;
            let chunk_refs = &shape_refs[chunk_start..chunk_end];
            let chunk_offsets = &gradient_offsets[chunk_start..=chunk_end];
            let mut convert_chunk = move || {
                for (j, shape) in chunk_refs.iter().enumerate() {
                    let gradient_start = chunk_offsets[j];
                    let local_start = (gradient_start - gradient_base) as usize;
                    let local_end = (chunk_offsets[j + 1] - gradient_base) as usize;
                    convert_shape_into_slots(
                        shape,
                        root_scale,
                        gradient_start,
                        &mut shape_data_chunk[j],
                        &mut gradient_chunk[local_start..local_end],
                    );
                }
            };
            if chunk_end == shape_count {
                // The caller would only block at the scope join; converting
                // the final chunk inline puts that time to work and saves a
                // spawn.
                convert_chunk();
            } else {
                scope.spawn(convert_chunk);
            }
            chunk_start = chunk_end;
        }
    });
    #[cfg(not(target_arch = "wasm32"))]
    SHAPE_CONVERT_TUNER.record(
        true,
        shape_count,
        convert_started.elapsed().as_nanos() as u64,
    );
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct GradientStop {
    color: [f32; 4],
    position: [f32; 4],
}

/// How many replay slots the shared transform buffer holds. Each slot's
/// transform lives at `slot * REPLAY_TRANSFORM_STRIDE`, aligned for the
/// strictest uniform-offset requirement any backend reports.
#[cfg(not(target_arch = "wasm32"))]
const MAX_REPLAY_SLOTS: u32 = 128;
#[cfg(not(target_arch = "wasm32"))]
const REPLAY_TRANSFORM_STRIDE: u64 = 256;

/// One retained replay batch: converted shape slots captured on an earlier
/// frame, kept on the GPU and re-drawn each frame under the similarity
/// transform staged at `transform_offset`.
///
/// The immutable `ShapeData` and gradient buffers hold no handle here:
/// nothing addresses them after capture, and `bind_group` keeps them alive.
#[cfg(not(target_arch = "wasm32"))]
struct ReplaySlot {
    /// One `vec4<f32>` color per shape — the mutable paint the shader reads
    /// under `paint_select`, split out so recolor patches upload 16 bytes
    /// per shape while the 160-byte `ShapeData` stays immutable on the GPU
    /// from capture to release.
    paint_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    shape_count: u32,
    /// CPU mirror of the paint buffer. Recolor patches apply here first
    /// and upload as one contiguous span per slot per frame — MEGA's
    /// twinkle field recolors ~1.7k dots a frame, and that many individual
    /// copy commands stall a mobile GPU for longer than the spans' extra
    /// bytes ever could.
    paint_mirror: Vec<[f32; 4]>,
    /// Conservative capture-space arc/ring mesh, built once at capture.
    /// `None` when the kill switch is off, the slot meshed no arcs, or the
    /// vertex budget overflowed — those slots replay through the quad-expansion
    /// six-vertices-per-shape path.
    mesh: Option<ReplaySlotMesh>,
    /// Which capture created this slot's buffers, from the store's global
    /// monotone counter. Retained bundle keys carry it so a slot id that is
    /// released and recaptured — new bind group, new buffers, same id — can
    /// never be drawn through a bundle recorded against the old capture.
    capture_epoch: u64,
}

/// Vertex geometry a retained slot replays instead of per-shape quads: arc
/// bands get trapezoid strips covering only their antialiasing footprint,
/// every other shape gets a passthrough pair of triangles identical to the
/// quad expansion. See [`build_arc_mesh_vertices`].
#[cfg(not(target_arch = "wasm32"))]
struct ReplaySlotMesh {
    vertex_buffer: wgpu::Buffer,
    /// `u32` triangle-list indices into `vertex_buffer`: band-boundary
    /// vertices are emitted once and shared by both adjacent trapezoids, so
    /// per-arc vertex-shader work drops from ~30 executions to the unique
    /// boundary vertices (~10-14) — the amplification that made the
    /// non-indexed mesh SLOWER than plain quads on the watch's Adreno 702.
    index_buffer: wgpu::Buffer,
    /// Prefix table, `shape_count + 1` entries: shape `i`'s triangles occupy
    /// indices `index_prefix[i]..index_prefix[i + 1]`, so a retained span
    /// draws `index_prefix[first]..index_prefix[first + count]` — one
    /// `draw_indexed` per op, identical shape order, z untouched.
    index_prefix: Vec<u32>,
}

/// Vertex of a retained slot's conservative arc mesh: capture-device-space
/// position, the uv reproducing `vs_main`'s affine rect map at that position,
/// and the shape index standing in for `vertex_index / 6`.
#[cfg(not(target_arch = "wasm32"))]
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct MeshVertex {
    position: [f32; 2],
    uv: [f32; 2],
    shape_idx: u32,
}

#[cfg(not(target_arch = "wasm32"))]
impl MeshVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Uint32];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// Kill switch, mirroring `command_feed_enabled`: default ON,
/// `CRANPOSE_ARC_MESH=0` (or the `debug.cranpose.arc_mesh` property on
/// Android) makes the next capture skip mesh building entirely, so a device
/// A/B needs no rebuild. Read per capture — captures are rare.
#[cfg(not(target_arch = "wasm32"))]
fn arc_mesh_enabled() -> bool {
    // Opt-in (CRANPOSE_ARC_MESH=1 / debug.cranpose.arc_mesh): the Gate 0
    // off-charger watch A/B measured the non-indexed mesh 5-7 fps SLOWER
    // than plain quads on the Adreno 702 — the 4-6x vertex amplification
    // outweighs the fragment savings on a small binning GPU (big desktop
    // GPUs and the at-vsync-ceiling Huawei masked it). Default returns to
    // quad expansion until indexed band-boundary geometry removes the
    // amplification; then the A/B is repeated.
    matches!(std::env::var("CRANPOSE_ARC_MESH").as_deref(), Ok(v) if v != "0")
}

/// Dilation applied to the band's half-thickness before meshing, in capture
/// device pixels. The fragment SDF feathers over ±0.5 px
/// (`smoothstep(-0.5, 0.5, dist)`), so every pixel the shader keeps sits
/// within 0.5 px of the band; the other 0.5 px absorbs f32 slop between this
/// builder's trig and the converted shape's precomputed (sin, cos) pairs.
#[cfg(not(target_arch = "wasm32"))]
const ARC_MESH_MARGIN: f32 = 1.0;

/// Chord overshoot budget in pixels: the segment count is chosen so pushing
/// outer edges tangent-outside the dilated outer circle overshoots it by
/// about this much at the chord ends.
#[cfg(not(target_arch = "wasm32"))]
const ARC_MESH_OVERSHOOT: f32 = 2.0;

#[cfg(not(target_arch = "wasm32"))]
const ARC_MESH_MIN_SEGMENTS: usize = 4;
#[cfg(not(target_arch = "wasm32"))]
const ARC_MESH_MAX_SEGMENTS: usize = 64;

/// Per-slot geometry budget in BYTES: 48 vertex-equivalents (~1 KB) per
/// shape, floored for tiny slots so a single huge ring still fits. The
/// non-indexed mesh spent this entirely on 20-byte vertices; the indexed
/// mesh counts vertices AND 4-byte indices against the same byte ceiling,
/// which indexed geometry fits with more headroom (MEGA's retained arcs
/// drop from ~30 vertices ≈ 600 B to ~12 unique vertices + ~30 indices
/// ≈ 360 B). Overflow falls back to whole-slot passthrough WITH a warning —
/// truncating silently would break the containment invariant.
#[cfg(not(target_arch = "wasm32"))]
const ARC_MESH_BUDGET_BYTES_PER_SHAPE: usize = 48 * std::mem::size_of::<MeshVertex>();
#[cfg(not(target_arch = "wasm32"))]
const ARC_MESH_BUDGET_FLOOR_BYTES: usize = 4096 * std::mem::size_of::<MeshVertex>();

/// The budget-relevant size of an indexed mesh: what the GPU buffers will
/// actually hold.
#[cfg(not(target_arch = "wasm32"))]
fn arc_mesh_bytes(vertices: usize, indices: usize) -> usize {
    vertices * std::mem::size_of::<MeshVertex>() + indices * std::mem::size_of::<u32>()
}

/// Band parameters of a captured arc that qualifies for a conservative mesh:
/// solid brush, no clip, and a quad that is exactly — tolerance zero — the
/// axis-aligned box of its rect. Everything else returns `None` and passes
/// through as today's two quad triangles.
#[cfg(not(target_arch = "wasm32"))]
struct ArcMeshBand {
    center: [f32; 2],
    inner: f32,
    outer: f32,
    start: f32,
    sweep: f32,
}

#[cfg(not(target_arch = "wasm32"))]
fn arc_mesh_band(shape: &ShapeData) -> Option<ArcMeshBand> {
    // Mirror the fragment shader's flag decode (`u32(max(x, 0.0))`).
    let flags = shape.stroke_params[1].max(0.0) as u32;
    if flags & 3 != SHAPE_KIND_ARC {
        return None;
    }
    // Solid brushes only: gradients also derive from `rect_pos` and would
    // mesh in principle, but the hot retained scenes are solid and a narrow
    // gate keeps the byte-exactness surface small.
    if shape.brush_type != 0 {
        return None;
    }
    // A live clip is a hard `world_pos` comparison in the fragment shader.
    // Meshed arcs interpolate `world_pos` across different triangles than
    // the quad would, and one ulp of difference at the clip boundary flips
    // whole pixels — clipped arcs pass through untouched.
    if shape.clip_rect[2] > 0.0 && shape.clip_rect[3] > 0.0 {
        return None;
    }
    let [_, _, w, h] = shape.rect;
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    // The quad must be an axis-aligned box, tolerance zero: the mesh is
    // clipped to the quad's own corners, so as long as the quad IS a box its
    // rasterized pixel set equals the mesh clip region and the tight-AABB
    // tangent-point crop is reproduced exactly. (Comparing against `rect`
    // instead is an over-tight gate: under a non-dyadic root scale
    // `(x + w) * s` differs from `x * s + w * s` by an ulp and every arc
    // fell back to passthrough — observed on the Huawei at scale 2.75.)
    let [left, top, right, _] = shape.quad01;
    let [bl_x, bottom, br_x, br_y] = shape.quad23;
    let axis_aligned = shape.quad01[3] == top
        && bl_x == left
        && br_x == right
        && br_y == bottom
        && left < right
        && top < bottom;
    if !axis_aligned {
        return None;
    }
    let center = [shape.arc_params[0], shape.arc_params[1]];
    let start = shape.arc_params[2];
    let sweep = shape.arc_params[3];
    let outer = shape.stroke_params[2];
    let inner = shape.stroke_params[3];
    let finite = center[0].is_finite()
        && center[1].is_finite()
        && start.is_finite()
        && sweep.is_finite()
        && outer.is_finite()
        && inner.is_finite();
    if !finite || outer <= 0.0 || sweep <= 0.0 {
        return None;
    }
    Some(ArcMeshBand {
        center,
        inner,
        outer,
        start,
        sweep,
    })
}

/// Emits the quad `vs_main` would expand for this shape as four shared
/// vertices plus the index pattern (0, 1, 2)(2, 1, 3) — the identical corner
/// order, corner uvs and positions straight from the captured quad, so a
/// passthrough shape rasterizes bit-identically to the quad-expansion
/// indexless path while spending four vertex executions instead of six.
#[cfg(not(target_arch = "wasm32"))]
fn emit_passthrough_quad(
    shape: &ShapeData,
    shape_idx: u32,
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
) {
    let base = vertices.len() as u32;
    let corners = [
        ([shape.quad01[0], shape.quad01[1]], [0.0, 0.0]),
        ([shape.quad01[2], shape.quad01[3]], [1.0, 0.0]),
        ([shape.quad23[0], shape.quad23[1]], [0.0, 1.0]),
        ([shape.quad23[2], shape.quad23[3]], [1.0, 1.0]),
    ];
    for (position, uv) in corners {
        vertices.push(MeshVertex {
            position,
            uv,
            shape_idx,
        });
    }
    indices.extend([0u32, 1, 2, 2, 1, 3].map(|corner| base + corner));
}

/// One Sutherland–Hodgman pass against an axis-aligned half-plane.
///
/// Two properties the byte-exactness bar depends on:
/// * the clipped coordinate is set to `bound` EXACTLY rather than recomputed
///   through `p + t * (q - p)`, so every clipped polygon's boundary lies
///   bitwise on the clip line;
/// * the intersection is computed on the lexicographically ordered endpoint
///   pair, so the shared radial edge of two adjacent trapezoids — traversed
///   in opposite directions — clips to bitwise-identical points, keeping the
///   strip watertight (no pixel shaded twice or missed along the seam).
#[cfg(not(target_arch = "wasm32"))]
fn clip_polygon_axis(
    input: &[[f32; 2]],
    axis: usize,
    bound: f32,
    keep_at_most: bool,
    output: &mut Vec<[f32; 2]>,
) {
    output.clear();
    let inside = |p: [f32; 2]| {
        if keep_at_most {
            p[axis] <= bound
        } else {
            p[axis] >= bound
        }
    };
    let intersect = |a: [f32; 2], b: [f32; 2]| {
        let (p, q) = if (b[0], b[1]) < (a[0], a[1]) {
            (b, a)
        } else {
            (a, b)
        };
        let t = (bound - p[axis]) / (q[axis] - p[axis]);
        let mut point = [0.0f32; 2];
        point[axis] = bound;
        point[1 - axis] = p[1 - axis] + t * (q[1 - axis] - p[1 - axis]);
        point
    };
    for (index, &current) in input.iter().enumerate() {
        let previous = input[(index + input.len() - 1) % input.len()];
        match (inside(previous), inside(current)) {
            (true, true) => output.push(current),
            (true, false) => output.push(intersect(previous, current)),
            (false, true) => {
                output.push(intersect(previous, current));
                output.push(current);
            }
            (false, false) => {}
        }
    }
}

/// Emits the conservative trapezoid-strip mesh for one qualifying arc band.
///
/// CONTAINMENT INVARIANT (the byte-exactness bar): the union of emitted
/// triangles is a superset of `{ p in the capture quad's box :
/// sdf_arc_band(p) <= 0.5 }` — every pixel the fragment shader would keep.
/// Over-inclusion is free (the SDF discards those pixels identically to
/// today's quad); only under-inclusion can diverge, and
/// `arc_mesh_contains_every_band_pixel` checks it never happens.
///
/// Geometry: outer vertices ride at `Ro / cos(step / 2)` so every chord is
/// tangent-outside the dilated outer circle; inner vertices ride at the
/// dilated inner radius, whose chords lie inside the hole. Cap coverage is
/// bounded by the round-cap disc about the band endpoint (butt/square caps
/// only cut that disc with planes — see `sdf_arc_band`), so padding the
/// angular range by the disc's angular half-extent contains every cap. Each
/// trapezoid is clipped to the quad box and fan-triangulated IN INDEX SPACE:
/// a trapezoid the clipper left untouched shares its two boundary vertices
/// with each neighbor through the index list (closed rings wrap the sharing
/// modulo the boundary count), so the strip is watertight by construction —
/// the seam edge is one vertex pair, not two bitwise-equal copies — and the
/// per-arc vertex count collapses from three-per-triangle to the unique
/// boundary vertices. Clipped trapezoids cannot share boundary vertices (the
/// clipper rewrote them), so their fan vertices are appended PRIVATELY after
/// the shared block and indexed directly; seams against neighbors still hold
/// because a boundary edge either survives the clip on both sides
/// bitwise-identically (same input edge, same planes, same float ops — see
/// `clip_polygon_axis`) or is cut on both sides identically. Triangles are
/// emitted in exact segment order either way, so the indexed mesh's
/// primitive stream is triangle-for-triangle the one the non-indexed
/// emitter produced.
///
/// Returns the emitted segment count, or `None` when the mesh came out empty
/// — the caller emits the passthrough quad instead (never risk
/// under-coverage).
#[cfg(not(target_arch = "wasm32"))]
fn emit_arc_band_mesh(
    shape: &ShapeData,
    shape_idx: u32,
    band: &ArcMeshBand,
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
) -> Option<usize> {
    let [cx, cy] = band.center;
    let ra = (band.outer + band.inner) * 0.5;
    let rb = ((band.outer - band.inner) * 0.5).max(0.0);
    let rb_m = rb + ARC_MESH_MARGIN;
    let ro = ra + rb_m;
    let ri = (ra - rb_m).max(0.0);
    let tau = cranpose_ui_graphics::TAU;

    let (range_start, range) = if band.sweep >= tau {
        (0.0, tau)
    } else {
        let pad = if rb_m < ra {
            (rb_m / ra).asin() + 0.05
        } else {
            // The cap disc wraps the center; such shapes are tiny, take the
            // whole circle.
            std::f32::consts::PI
        };
        let padded = band.sweep + pad + pad;
        if padded >= tau {
            (0.0, tau)
        } else {
            (band.start - pad, padded)
        }
    };
    let closed = range >= tau;

    let dtheta = (2.0 * (ro / (ro + ARC_MESH_OVERSHOOT)).acos()).clamp(tau / 64.0, tau / 6.0);
    let segments =
        ((range / dtheta).ceil() as usize).clamp(ARC_MESH_MIN_SEGMENTS, ARC_MESH_MAX_SEGMENTS);
    let step = range / segments as f32;
    let rc = ro / (step * 0.5).cos();

    // Boundary vertices are computed once and shared by both adjacent
    // trapezoids: bitwise-equal edge endpoints are what let the rasterizer's
    // fill rule shade each seam exactly once.
    let boundary_count = if closed { segments } else { segments + 1 };
    let mut boundaries = Vec::with_capacity(boundary_count);
    for j in 0..boundary_count {
        let (sin, cos) = (range_start + step * j as f32).sin_cos();
        boundaries.push((
            [cx + cos * ri, cy + sin * ri],
            [cx + cos * rc, cy + sin * rc],
        ));
    }

    let quad_min = [shape.quad01[0], shape.quad01[1]];
    let quad_max = [shape.quad23[2], shape.quad23[3]];

    /// One trapezoid's clip outcome (see the function docs): `Shared` means
    /// the clip output is bitwise the input quad, so its corners index the
    /// shared boundary block; `Fan` carries the clipped polygon for private
    /// fan triangulation; `Empty` was clipped away entirely.
    enum SegmentGeometry {
        Shared,
        Fan(Vec<[f32; 2]>),
        Empty,
    }

    // Phase 1: clip every trapezoid and classify it.
    let mut polygon: Vec<[f32; 2]> = Vec::with_capacity(8);
    let mut scratch: Vec<[f32; 2]> = Vec::with_capacity(8);
    let mut segment_geometry = Vec::with_capacity(segments);
    let mut boundary_used = vec![false; boundary_count];
    for j in 0..segments {
        let jb = (j + 1) % boundary_count;
        let (inner_a, outer_a) = boundaries[j];
        let (inner_b, outer_b) = boundaries[jb];
        polygon.clear();
        polygon.extend_from_slice(&[inner_a, outer_a, outer_b, inner_b]);
        clip_polygon_axis(&polygon, 0, quad_min[0], false, &mut scratch);
        clip_polygon_axis(&scratch, 0, quad_max[0], true, &mut polygon);
        clip_polygon_axis(&polygon, 1, quad_min[1], false, &mut scratch);
        clip_polygon_axis(&scratch, 1, quad_max[1], true, &mut polygon);
        // Collapse exact duplicates (an `Ri == 0` pie wedge duplicates the
        // center) before fanning.
        scratch.clear();
        for &point in polygon.iter() {
            if scratch.last() != Some(&point) {
                scratch.push(point);
            }
        }
        while scratch.len() > 1 && scratch.first() == scratch.last() {
            scratch.pop();
        }
        if scratch.len() < 3 {
            segment_geometry.push(SegmentGeometry::Empty);
        } else if scratch[..] == [inner_a, outer_a, outer_b, inner_b] {
            boundary_used[j] = true;
            boundary_used[jb] = true;
            segment_geometry.push(SegmentGeometry::Shared);
        } else {
            segment_geometry.push(SegmentGeometry::Fan(scratch.clone()));
        }
    }

    let push_vertex = |vertices: &mut Vec<MeshVertex>, position: [f32; 2]| -> u32 {
        let index = vertices.len() as u32;
        vertices.push(MeshVertex {
            position,
            uv: [
                (position[0] - shape.rect[0]) / shape.rect[2],
                (position[1] - shape.rect[1]) / shape.rect[3],
            ],
            shape_idx,
        });
        index
    };

    // Shared block: every boundary referenced by a surviving whole trapezoid
    // gets its (inner, outer) vertex pair exactly once, in boundary order.
    let mut boundary_vertex = vec![[0u32; 2]; boundary_count];
    for (j, used) in boundary_used.iter().enumerate() {
        if *used {
            let (inner, outer) = boundaries[j];
            boundary_vertex[j] = [push_vertex(vertices, inner), push_vertex(vertices, outer)];
        }
    }

    // Phase 2: indices in exact segment order — the primitive stream matches
    // the non-indexed emitter triangle for triangle.
    let start_len = indices.len();
    for (j, geometry) in segment_geometry.iter().enumerate() {
        match geometry {
            SegmentGeometry::Empty => {}
            SegmentGeometry::Shared => {
                let jb = (j + 1) % boundary_count;
                let [in_a, out_a] = boundary_vertex[j];
                let [in_b, out_b] = boundary_vertex[jb];
                // The fan the non-indexed emitter produced for an untouched
                // trapezoid: (in_a, out_a, out_b)(in_a, out_b, in_b) — the
                // same quad diagonal.
                indices.extend_from_slice(&[in_a, out_a, out_b, in_a, out_b, in_b]);
            }
            SegmentGeometry::Fan(points) => {
                let base = vertices.len() as u32;
                for &point in points {
                    push_vertex(vertices, point);
                }
                for i in 1..points.len() as u32 - 1 {
                    indices.extend_from_slice(&[base, base + i, base + i + 1]);
                }
            }
        }
    }
    if indices.len() == start_len {
        return None;
    }
    Some(segments)
}

/// Unsigned shoelace area of an emitted indexed triangle list, for
/// telemetry.
#[cfg(not(target_arch = "wasm32"))]
fn triangles_shoelace_area(vertices: &[MeshVertex], indices: &[u32]) -> f64 {
    indices
        .chunks_exact(3)
        .map(|tri| {
            let [a, b, c] = [
                vertices[tri[0] as usize].position,
                vertices[tri[1] as usize].position,
                vertices[tri[2] as usize].position,
            ];
            let cross = (b[0] as f64 - a[0] as f64) * (c[1] as f64 - a[1] as f64)
                - (b[1] as f64 - a[1] as f64) * (c[0] as f64 - a[0] as f64);
            cross.abs() * 0.5
        })
        .sum()
}

/// Unsigned area of the two triangles the quad-expansion path would rasterize for
/// this shape, for telemetry.
#[cfg(not(target_arch = "wasm32"))]
fn quad_shoelace_area(shape: &ShapeData) -> f64 {
    let corners = [
        [shape.quad01[0] as f64, shape.quad01[1] as f64],
        [shape.quad01[2] as f64, shape.quad01[3] as f64],
        [shape.quad23[0] as f64, shape.quad23[1] as f64],
        [shape.quad23[2] as f64, shape.quad23[3] as f64],
    ];
    let tri = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5
    };
    tri(corners[0], corners[1], corners[2]) + tri(corners[2], corners[1], corners[3])
}

#[cfg(not(target_arch = "wasm32"))]
struct ArcMeshBuild {
    vertices: Vec<MeshVertex>,
    /// Triangle-list indices into `vertices`; see [`ReplaySlotMesh`].
    indices: Vec<u32>,
    /// `shape_count + 1` entries; shape `i` owns triangles
    /// `indices[index_prefix[i]..index_prefix[i + 1]]`.
    index_prefix: Vec<u32>,
    meshed_arcs: usize,
    meshed_segments: usize,
    passthrough: usize,
    quad_area: f64,
    mesh_area: f64,
}

/// Builds a slot's conservative indexed mesh: arc bands become
/// vertex-sharing trapezoid strips, every other shape a passthrough quad
/// (four vertices, six indices), in the exact capture shape order. Returns
/// `None` when the byte budget overflows — the caller warns and the whole
/// slot replays through the quad-expansion path (silent truncation would
/// break the containment invariant).
#[cfg(not(target_arch = "wasm32"))]
fn build_arc_mesh_vertices(shape_data: &[ShapeData]) -> Option<ArcMeshBuild> {
    let budget_bytes =
        (shape_data.len() * ARC_MESH_BUDGET_BYTES_PER_SHAPE).max(ARC_MESH_BUDGET_FLOOR_BYTES);
    let mut build = ArcMeshBuild {
        vertices: Vec::new(),
        indices: Vec::new(),
        index_prefix: Vec::with_capacity(shape_data.len() + 1),
        meshed_arcs: 0,
        meshed_segments: 0,
        passthrough: 0,
        quad_area: 0.0,
        mesh_area: 0.0,
    };
    build.index_prefix.push(0);
    for (index, shape) in shape_data.iter().enumerate() {
        let start = build.indices.len();
        let meshed = arc_mesh_band(shape).and_then(|band| {
            emit_arc_band_mesh(
                shape,
                index as u32,
                &band,
                &mut build.vertices,
                &mut build.indices,
            )
        });
        match meshed {
            Some(segments) => {
                build.meshed_arcs += 1;
                build.meshed_segments += segments;
            }
            None => {
                emit_passthrough_quad(shape, index as u32, &mut build.vertices, &mut build.indices);
                build.passthrough += 1;
            }
        }
        if arc_mesh_bytes(build.vertices.len(), build.indices.len()) > budget_bytes {
            return None;
        }
        build.index_prefix.push(build.indices.len() as u32);
        build.quad_area += quad_shoelace_area(shape);
        build.mesh_area += triangles_shoelace_area(&build.vertices, &build.indices[start..]);
    }
    Some(build)
}

/// The renderer's registry of live replay slots. The replay cache (scene
/// side) owns slot LIFECYCLE decisions; this store owns the GPU resources.
#[cfg(not(target_arch = "wasm32"))]
struct ReplaySlotStore {
    slots: std::collections::HashMap<u32, ReplaySlot, cranpose_ui_graphics::FxBuildHasher>,
    transform_buffer: wgpu::Buffer,
    free_ids: Vec<u32>,
    /// Global capture counter feeding [`ReplaySlot::capture_epoch`]: bumped
    /// on every capture, never reused, so an epoch identifies one capture's
    /// buffers for the renderer's whole lifetime.
    next_capture_epoch: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl ReplaySlotStore {
    fn new(device: &wgpu::Device) -> Self {
        let transform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Replay Transform Buffer"),
            size: MAX_REPLAY_SLOTS as u64 * REPLAY_TRANSFORM_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            slots: std::collections::HashMap::default(),
            transform_buffer,
            free_ids: (0..MAX_REPLAY_SLOTS).rev().collect(),
            next_capture_epoch: 1,
        }
    }
}

/// Kill switch for cached retained render bundles, mirroring
/// `command_feed_enabled`: default ON, `CRANPOSE_RETAINED_BUNDLES=0` (or the
/// `debug.cranpose.retained_bundles` property on Android) drops the fused
/// retained arms back to direct per-op encoding, so a device A/B needs no
/// rebuild. Read per partition — the parity harness flips it between passes.
#[cfg(not(target_arch = "wasm32"))]
fn retained_bundles_enabled() -> bool {
    std::env::var("CRANPOSE_RETAINED_BUNDLES").as_deref() != Ok("0")
}

/// Kill switch for instanced ordinary-shape quads: default ON,
/// `CRANPOSE_INSTANCED_QUADS=0` (or the `debug.cranpose.instanced_quads`
/// property on Android) reverts every ordinary shape draw to the six-vertex
/// `vs_main` expansion. Unlike the per-partition bundle flag this is read
/// ONCE per [`GpuRenderer`] construction into a field: cached retained
/// bundles encode the selected pipeline, so a flag that moved per draw would
/// let a cached bundle replay a selection the direct path no longer makes.
#[cfg(not(target_arch = "wasm32"))]
fn instanced_quads_enabled() -> bool {
    std::env::var("CRANPOSE_INSTANCED_QUADS").as_deref() != Ok("0")
}

/// The index pattern of one instanced quad: the exact triangle pair
/// `vs_main`'s six-slot corner mapping produces — (0, 1, 2)(2, 1, 3), same
/// diagonal, same winding — shared by every instance.
#[cfg(not(target_arch = "wasm32"))]
const INSTANCED_QUAD_INDICES: [u16; 6] = [0, 1, 2, 2, 1, 3];

/// The latched instanced-quad selection: `Some` exactly when the renderer
/// was constructed in storage mode with [`instanced_quads_enabled`]. Both
/// blend variants exist because ordinary batches draw SrcOver and DstOut;
/// the `vs_main` pipelines coexist untouched so the `=0` revert (and the
/// uniform-mode path) still has its six-vertex draws.
#[cfg(not(target_arch = "wasm32"))]
struct InstancedQuadPipelines {
    pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    pipeline_dst_out: LazyGpuResource<wgpu::RenderPipeline>,
    /// Static `[0, 1, 2, 2, 1, 3]` u16 index buffer, created once and shared
    /// by every instanced draw.
    index_buffer: wgpu::Buffer,
}

/// Everything that decides the commands one retained op contributes to a
/// cached bundle. Equal op keys imply identical encoded commands:
/// `capture_epoch` pins the slot's bind group and buffers to one capture,
/// `has_mesh` pins the pipeline and vertex-buffer choice, `first..last` is
/// the clamped draw range, and `retained_index` is the dynamic transform
/// offset. Transforms and paints are NOT here — they are data-buffer
/// contents the bundle reads at execution.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RetainedBundleOpKey {
    slot: u32,
    /// The slot's capture epoch at key time, `None` while the slot is absent
    /// from the store (the op encodes nothing). Epochs are globally unique
    /// per capture, so a recaptured slot reusing its id can never satisfy a
    /// key recorded against the previous capture's buffers.
    capture_epoch: Option<u64>,
    first: u32,
    last: u32,
    retained_index: u32,
    has_mesh: bool,
}

/// Key of one maximal consecutive retained stretch: the op keys in draw
/// order. Any reorder, count change, range change, recapture, or slot
/// release changes the key and forces a rebuild.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
struct RetainedBundleKey {
    ops: Vec<RetainedBundleOpKey>,
}

#[cfg(not(target_arch = "wasm32"))]
struct RetainedBundleCacheEntry<B> {
    bundle: B,
    last_used_frame: u64,
}

/// Cache of encoded render bundles for retained stretches, generic over the
/// bundle payload so the reuse/invalidation/eviction logic is unit-testable
/// without a GPU. The full [`RetainedBundleKey`] is the map key — a fresh
/// key can only ever build a fresh bundle, never alias a stale one.
///
/// The surface format and the group-0 uniform bind group are deliberately
/// not part of the key: both are fixed for a `GpuRenderer`'s lifetime (a
/// surface reconfigure builds a new renderer, and with it an empty cache).
#[cfg(not(target_arch = "wasm32"))]
struct RetainedBundleCacheImpl<B> {
    entries: HashMap<RetainedBundleKey, RetainedBundleCacheEntry<B>>,
    frame: u64,
    rebuilds: u64,
    cached_executes: u64,
    window_rebuilds: u64,
    window_executes: u64,
}

#[cfg(not(target_arch = "wasm32"))]
type RetainedBundleCache = RetainedBundleCacheImpl<wgpu::RenderBundle>;

#[cfg(not(target_arch = "wasm32"))]
impl<B> RetainedBundleCacheImpl<B> {
    fn new() -> Self {
        Self {
            entries: HashMap::default(),
            frame: 0,
            rebuilds: 0,
            cached_executes: 0,
            window_rebuilds: 0,
            window_executes: 0,
        }
    }

    /// True when a bundle for `key` is cached; marks it used this frame and
    /// counts a cached execute.
    fn hit(&mut self, key: &RetainedBundleKey) -> bool {
        let frame = self.frame;
        match self.entries.get_mut(key) {
            Some(entry) => {
                entry.last_used_frame = frame;
                self.cached_executes += 1;
                self.window_executes += 1;
                true
            }
            None => false,
        }
    }

    /// Stores a freshly built bundle, counting a rebuild.
    fn insert(&mut self, key: RetainedBundleKey, bundle: B) {
        self.rebuilds += 1;
        self.window_rebuilds += 1;
        self.entries.insert(
            key,
            RetainedBundleCacheEntry {
                bundle,
                last_used_frame: self.frame,
            },
        );
    }

    fn get(&self, key: &RetainedBundleKey) -> Option<&B> {
        self.entries.get(key).map(|entry| &entry.bundle)
    }

    /// Drops every cached bundle. Called whenever a replay slot is released:
    /// the key compare already makes stale entries unreachable (their epochs
    /// can never recur), so this only releases the dropped capture's GPU
    /// resources promptly instead of one frame later via eviction.
    fn clear(&mut self) {
        self.entries.clear();
    }

    /// Frame boundary: evicts entries the frame did not use — a bundle
    /// holds references on its slot's buffers, so unused entries must not
    /// accumulate — and emits the rate-limited rebuild/execute telemetry.
    fn end_frame(&mut self) {
        let frame = self.frame;
        self.entries
            .retain(|_, entry| entry.last_used_frame >= frame);
        self.frame = self.frame.wrapping_add(1);
        // Always-on at a cadence that cannot spam; every perf window (120
        // frames) under the replay diagnostics flag so short A/B runs see
        // the counts. log::warn because log::info is invisible on the
        // desktop console.
        let due = self.frame.is_multiple_of(1024)
            || (cranpose_core::env_flag!("CRANPOSE_COMMAND_REPLAY_DIAG")
                && self.frame.is_multiple_of(120));
        if due && self.window_rebuilds + self.window_executes > 0 {
            log::warn!(
                "[retained-bundles] {} stretches, {} rebuilds, {} cached executes ({} live bundles)",
                self.window_rebuilds + self.window_executes,
                self.window_rebuilds,
                self.window_executes,
                self.entries.len(),
            );
            self.window_rebuilds = 0;
            self.window_executes = 0;
        }
    }

    /// Lifetime (rebuilds, cached executes) for tests and diagnostics.
    fn stats(&self) -> (u64, u64) {
        (self.rebuilds, self.cached_executes)
    }
}

struct CachedImageTexture {
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    nearest_bind_group: wgpu::BindGroup,
    linear_bind_group: wgpu::BindGroup,
    /// GPU bytes this entry pins (w×h×4): the cache is bounded by BYTES as
    /// well as count. A live camera publishes a new multi-MB bitmap id every
    /// frame; 256 count-slots of those is ~1.5GB of dead preview textures —
    /// which on iOS unified memory counts straight against the process's
    /// jetsam limit (measured: the app died mid-scan under an open camera
    /// with exactly that ballast).
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

/// Side length the glyph atlas should be rebuilt at after it overflowed at
/// `current`: one doubling, never past `max`.
///
/// Doubling (rather than jumping straight to `max`) is what makes the atlas
/// cost track the workload: an app that overflows once needs a little more
/// room, not sixteen times more.
fn next_glyph_atlas_size(current: u32, max: u32) -> u32 {
    current.saturating_mul(2).clamp(1, max.max(1))
}

struct TextGlyphAtlas {
    texture: wgpu::Texture,
    _view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    entries: BoundedLruCache<SoftwareGlyphAtlasKey, GlyphAtlasEntry>,
    generation: u64,
    /// Side length of `texture`, between `TEXT_GLYPH_ATLAS_MIN_SIZE` and the
    /// device's ceiling. Every UV is normalised against it, so it has to travel
    /// with the atlas rather than be read back off a constant.
    size: u32,
    /// Largest side length this atlas may grow to: the smaller of
    /// `TEXT_GLYPH_ATLAS_MAX_SIZE` and what the device grants. Mobile devices
    /// are requested `downlevel_defaults()` limits raised by `using_resolution`,
    /// so a device that only offers 2048 would otherwise fail to create the
    /// texture outright.
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

    /// Throws every cached glyph away and starts over on a texture one doubling
    /// larger, up to [`TextGlyphAtlas::max_size`].
    ///
    /// `allocate` is a one-way shelf cursor with no compaction, so the only
    /// recovery from a full atlas is to start again — and starting again at the
    /// same size makes a workload whose live glyph set genuinely does not fit
    /// re-raster every glyph every frame. Treating each overflow as the signal
    /// to double means the atlas converges on the size the workload actually
    /// needs: a text-heavy screen reaches the old fixed 4096 after at most three
    /// resets and behaves identically from then on, while a watch face that
    /// never overflows never pays for space it will not use.
    ///
    /// Bumping the generation is what invalidates the cached glyph runs, whose
    /// UVs are normalised against the previous size and would otherwise sample
    /// the wrong part of the new texture.
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

struct ImageDrawCmd {
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
    #[cfg(not(target_arch = "wasm32"))]
    Retained {
        cache_key: TextGlyphRunCacheKey,
        uniform_slot: usize,
    },
}

#[derive(Clone, Copy)]
struct GlyphDrawCmd {
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

    #[cfg(not(target_arch = "wasm32"))]
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

// Text raster cache is owned by GpuRenderer and backed by software text images
// between measurement and rendering to eliminate duplicate text shaping

/// Persistent GPU buffers for batched shape rendering. There is no vertex or
/// index buffer: the shape shader pulls quad corners straight out of
/// `ShapeData` by `vertex_index`, so the batch is drawn unindexed.
struct ShapeBatchBuffers {
    shape_buffer: wgpu::Buffer,
    gradient_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    shape_capacity: usize,
    gradient_capacity: usize,
    batch_limits: ShapeBatchLimits,
}

#[cfg(target_arch = "wasm32")]
struct UniformBatchBuffer {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

#[cfg(target_arch = "wasm32")]
struct ImageBatchBuffers {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    vertex_capacity: usize,
    index_capacity: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ViewportUniformParams {
    width: u32,
    height: u32,
    offset: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum UploadTarget {
    Uniform,
    ShapeData,
    ShapeGradient,
    ImageVertex,
    ImageIndex,
    #[cfg(not(target_arch = "wasm32"))]
    RetainedGlyphUniform,
    /// The shared replay-transform buffer; copies land at each slot's fixed
    /// 256-byte-aligned offset.
    #[cfg(not(target_arch = "wasm32"))]
    ReplayTransform,
    /// A replay slot's retained paint buffer (color patches land here).
    #[cfg(not(target_arch = "wasm32"))]
    ReplayPaintData(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct PendingBufferCopy {
    source_offset: u64,
    target_offset: u64,
    size: u64,
    target: UploadTarget,
}

#[derive(Default)]
struct StagedBufferUploads {
    bytes: Vec<u8>,
    copies: Vec<PendingBufferCopy>,
}

impl StagedBufferUploads {
    fn clear(&mut self) {
        self.bytes.clear();
        self.copies.clear();
    }

    fn shrink_retained_capacity(&mut self, max_bytes: usize, max_copies: usize) -> bool {
        let mut shrunk = false;
        if self.bytes.len() <= max_bytes && self.bytes.capacity() > max_bytes {
            self.bytes.shrink_to(max_bytes);
            shrunk = true;
        }
        if self.copies.len() <= max_copies && self.copies.capacity() > max_copies {
            self.copies.shrink_to(max_copies);
            shrunk = true;
        }
        shrunk
    }

    fn is_empty(&self) -> bool {
        self.copies.is_empty()
    }

    #[cfg(test)]
    fn payload_for_copy(&self, copy: PendingBufferCopy) -> &[u8] {
        let start = copy.source_offset as usize;
        let end = start + copy.size as usize;
        &self.bytes[start..end]
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn stage(&mut self, target: UploadTarget, bytes: &[u8]) {
        self.stage_at(target, 0, bytes);
    }

    /// Records a GPU copy whose source bytes were already written into the
    /// frame upload buffer (via `Queue::write_buffer_with`), so nothing is
    /// appended to `bytes`. `source_offset` is relative to the same base the
    /// caller later passes to `flush_staged_uploads_at`.
    #[cfg(not(target_arch = "wasm32"))]
    fn record_upload_copy(
        &mut self,
        target: UploadTarget,
        source_offset: u64,
        target_offset: u64,
        size: u64,
    ) {
        if size == 0 {
            return;
        }
        self.copies.push(PendingBufferCopy {
            source_offset,
            target_offset,
            size,
            target,
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn stage_at(&mut self, target: UploadTarget, target_offset: u64, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        debug_assert_eq!(
            bytes.len() % wgpu::COPY_BUFFER_ALIGNMENT as usize,
            0,
            "buffer uploads must be aligned to copy requirements"
        );

        let aligned_offset = align_usize_to(self.bytes.len(), wgpu::COPY_BUFFER_ALIGNMENT as usize);
        if aligned_offset > self.bytes.len() {
            self.bytes.resize(aligned_offset, 0);
        }

        let source_offset = self.bytes.len() as u64;
        self.bytes.extend_from_slice(bytes);
        self.copies.push(PendingBufferCopy {
            source_offset,
            target_offset,
            size: bytes.len() as u64,
            target,
        });
    }

    fn truncate(&mut self, bytes_len: usize, copies_len: usize) {
        self.bytes.truncate(bytes_len);
        self.copies.truncate(copies_len);
    }
}

/// The fresh-batch entry list for the shape bind group layout: the batch's
/// own data buffers, the shared identity similarity buffer, and — storage
/// mode only, where the layout carries the paint entry — the renderer-wide
/// dummy paint buffer (fresh draws leave `paint_select` at 0.0).
fn shape_batch_bind_group_entries<'a>(
    shape_buffer: &'a wgpu::Buffer,
    gradient_buffer: &'a wgpu::Buffer,
    similarity_buffer: &'a wgpu::Buffer,
    paint_buffer: Option<&'a wgpu::Buffer>,
) -> Vec<wgpu::BindGroupEntry<'a>> {
    let mut entries = vec![
        wgpu::BindGroupEntry {
            binding: 0,
            resource: shape_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: gradient_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: similarity_buffer.as_entire_binding(),
        },
    ];
    if let Some(paint_buffer) = paint_buffer {
        entries.push(wgpu::BindGroupEntry {
            binding: 3,
            resource: paint_buffer.as_entire_binding(),
        });
    }
    entries
}

impl ShapeBatchBuffers {
    fn new(
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        similarity_buffer: &wgpu::Buffer,
        paint_buffer: Option<&wgpu::Buffer>,
        batch_limits: ShapeBatchLimits,
    ) -> Self {
        debug_assert_eq!(
            paint_buffer.is_some(),
            batch_limits.storage,
            "the paint binding exists exactly when the layout is in storage mode"
        );
        let initial_shape_cap = batch_limits.initial_shape_capacity();
        let initial_gradient_cap = batch_limits.initial_gradient_capacity();

        let shape_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shape Data Buffer"),
            size: (std::mem::size_of::<ShapeData>() * initial_shape_cap) as u64,
            usage: batch_limits.data_buffer_usage(),
            mapped_at_creation: false,
        });

        let gradient_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Gradient Buffer"),
            size: (std::mem::size_of::<GradientStop>() * initial_gradient_cap) as u64,
            usage: batch_limits.data_buffer_usage(),
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shape Bind Group"),
            layout: bind_group_layout,
            entries: &shape_batch_bind_group_entries(
                &shape_buffer,
                &gradient_buffer,
                similarity_buffer,
                paint_buffer,
            ),
        });

        Self {
            shape_buffer,
            gradient_buffer,
            bind_group,
            shape_capacity: initial_shape_cap,
            gradient_capacity: initial_gradient_cap,
            batch_limits,
        }
    }

    /// Ensure buffers have enough capacity, resizing if needed.
    /// Clamps growth to prevent excessive allocations for huge scenes.
    fn ensure_capacity(
        &mut self,
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        similarity_buffer: &wgpu::Buffer,
        paint_buffer: Option<&wgpu::Buffer>,
        shapes_needed: usize,
        gradients_needed: usize,
    ) {
        let mut need_bind_group_update = false;

        // In uniform mode the shape and gradient buffers start at the cap
        // (the shader's fixed-size array length) so these never fire; in
        // storage mode they double toward the cap as scenes demand.
        if shapes_needed > self.shape_capacity
            && self.shape_capacity < self.batch_limits.max_shapes_per_batch
        {
            let new_cap = shapes_needed
                .next_power_of_two()
                .min(self.batch_limits.max_shapes_per_batch);
            self.shape_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Shape Data Buffer"),
                size: (std::mem::size_of::<ShapeData>() * new_cap) as u64,
                usage: self.batch_limits.data_buffer_usage(),
                mapped_at_creation: false,
            });
            self.shape_capacity = new_cap;
            need_bind_group_update = true;
        }

        if gradients_needed > self.gradient_capacity
            && self.gradient_capacity < self.batch_limits.max_gradient_stops
        {
            let new_cap = gradients_needed
                .max(1)
                .next_power_of_two()
                .min(self.batch_limits.max_gradient_stops);
            self.gradient_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Gradient Buffer"),
                size: (std::mem::size_of::<GradientStop>() * new_cap) as u64,
                usage: self.batch_limits.data_buffer_usage(),
                mapped_at_creation: false,
            });
            self.gradient_capacity = new_cap;
            need_bind_group_update = true;
        }

        if need_bind_group_update {
            self.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Shape Bind Group"),
                layout: bind_group_layout,
                entries: &shape_batch_bind_group_entries(
                    &self.shape_buffer,
                    &self.gradient_buffer,
                    similarity_buffer,
                    paint_buffer,
                ),
            });
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl UniformBatchBuffer {
    fn new(device: &wgpu::Device, bind_group_layout: &wgpu::BindGroupLayout) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Viewport Uniform Batch Buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Viewport Uniform Batch Bind Group"),
            layout: bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self { buffer, bind_group }
    }
}

#[cfg(target_arch = "wasm32")]
impl ImageBatchBuffers {
    fn new(device: &wgpu::Device) -> Self {
        let vertex_capacity = 4;
        let index_capacity = 6;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Image Vertex Batch Buffer"),
            size: (std::mem::size_of::<Vertex>() * vertex_capacity) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Image Index Batch Buffer"),
            size: (std::mem::size_of::<u32>() * index_capacity) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            vertex_buffer,
            index_buffer,
            vertex_capacity,
            index_capacity,
        }
    }

    fn ensure_capacity(
        &mut self,
        device: &wgpu::Device,
        vertices_needed: usize,
        indices_needed: usize,
    ) {
        let hard_max_bytes = HARD_MAX_BUFFER_MB * 1024 * 1024;
        if vertices_needed > self.vertex_capacity {
            let desired = vertices_needed.next_power_of_two();
            let max_count = hard_max_bytes / std::mem::size_of::<Vertex>();
            let new_cap = desired.min(max_count);
            self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Image Vertex Batch Buffer"),
                size: (std::mem::size_of::<Vertex>() * new_cap) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vertex_capacity = new_cap;
        }
        if indices_needed > self.index_capacity {
            let desired = indices_needed.next_power_of_two();
            let max_count = hard_max_bytes / std::mem::size_of::<u32>();
            let new_cap = desired.min(max_count);
            self.index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Image Index Batch Buffer"),
                size: (std::mem::size_of::<u32>() * new_cap) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.index_capacity = new_cap;
        }
    }
}

// Text image cache keys are local to rasterized WGPU text batches

pub struct GpuRenderer {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    /// This instance's renderer epoch, stamped by `init_gpu` at
    /// construction. A packet whose `renderer_epoch` differs was built
    /// against another instance and is cancelled at the head of
    /// [`Self::render`], never drawn.
    renderer_epoch: u64,
    /// The producer feed generation this store's slot universe belongs to:
    /// seeded at construction, advanced by `consume_replay_ops` when a
    /// higher-generation batch arrives (the batch itself carries the
    /// retirement releases). The store never reads the producer's
    /// thread-local — this field is its only generation authority.
    #[cfg(not(target_arch = "wasm32"))]
    store_feed_generation: u64,
    surface_format: wgpu::TextureFormat,
    adapter_backend: wgpu::Backend,
    shape_batch_limits: ShapeBatchLimits,
    pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    pipeline_dst_out: LazyGpuResource<wgpu::RenderPipeline>,
    /// `Some` exactly in storage mode: the retained-mesh pipeline (`vs_mesh`
    /// over a vertex buffer) that replay slots with a captured arc mesh draw
    /// through. Uniform-mode devices never host retained slots.
    #[cfg(not(target_arch = "wasm32"))]
    mesh_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    /// `Some` exactly when this renderer latched the instanced-quad path at
    /// construction (storage mode && `CRANPOSE_INSTANCED_QUADS` != 0). Read
    /// ONCE per renderer lifetime — cached retained bundles encode the
    /// selection, so it must never move under them (see
    /// [`instanced_quads_enabled`]).
    #[cfg(not(target_arch = "wasm32"))]
    instanced_quads: Option<InstancedQuadPipelines>,
    uniform_bind_group_layout: wgpu::BindGroupLayout,
    shape_bind_group_layout: wgpu::BindGroupLayout,
    /// `Some` exactly in storage mode: the 16-byte stand-in every fresh
    /// batch binds at the paint entry (see `shape_batch_bind_group_entries`).
    dummy_paint_buffer: Option<wgpu::Buffer>,
    /// Shared identity binding for `@group(1) @binding(2)`: every freshly
    /// converted shape batch draws untransformed through this one buffer.
    identity_similarity_buffer: wgpu::Buffer,
    #[cfg(not(target_arch = "wasm32"))]
    replay_slots: ReplaySlotStore,
    image_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    image_pipeline_dst_out: LazyGpuResource<wgpu::RenderPipeline>,
    glyph_atlas_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    #[cfg(not(target_arch = "wasm32"))]
    retained_glyph_atlas_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    image_bind_group_layout: wgpu::BindGroupLayout,
    #[cfg(not(target_arch = "wasm32"))]
    retained_glyph_uniform_bind_group_layout: wgpu::BindGroupLayout,
    image_nearest_sampler: wgpu::Sampler,
    image_linear_sampler: wgpu::Sampler,
    text_fonts: SoftwareTextFontSet,
    // Persistent GPU buffers (reused across frames)
    #[cfg(not(target_arch = "wasm32"))]
    upload_buffer: wgpu::Buffer,
    #[cfg(not(target_arch = "wasm32"))]
    uniform_buffer: wgpu::Buffer,
    #[cfg(not(target_arch = "wasm32"))]
    uniform_bind_group: wgpu::BindGroup,
    #[cfg(not(target_arch = "wasm32"))]
    shape_buffers: ShapeBatchBuffers,
    #[cfg(not(target_arch = "wasm32"))]
    image_vertex_buffer: wgpu::Buffer,
    #[cfg(not(target_arch = "wasm32"))]
    image_index_buffer: wgpu::Buffer,
    #[cfg(not(target_arch = "wasm32"))]
    retained_glyph_uniform_buffer: wgpu::Buffer,
    #[cfg(not(target_arch = "wasm32"))]
    retained_glyph_uniform_bind_group: wgpu::BindGroup,
    #[cfg(not(target_arch = "wasm32"))]
    retained_glyph_uniform_stride: u64,
    #[cfg(not(target_arch = "wasm32"))]
    retained_glyph_uniform_capacity: usize,
    #[cfg(not(target_arch = "wasm32"))]
    retained_glyph_uniform_cursor: usize,
    #[cfg(target_arch = "wasm32")]
    wasm_uniform_batches: Vec<UniformBatchBuffer>,
    #[cfg(target_arch = "wasm32")]
    wasm_uniform_batch_cursor: usize,
    #[cfg(target_arch = "wasm32")]
    wasm_shape_batches: Vec<ShapeBatchBuffers>,
    #[cfg(target_arch = "wasm32")]
    wasm_shape_batch_cursor: usize,
    #[cfg(target_arch = "wasm32")]
    wasm_image_batches: Vec<ImageBatchBuffers>,
    #[cfg(target_arch = "wasm32")]
    wasm_image_batch_cursor: usize,
    image_texture_cache: BoundedLruCache<u64, CachedImageTexture>,
    /// Total `CachedImageTexture::bytes` currently in the cache.
    image_texture_cache_bytes: usize,
    text_image_cache: BoundedLruCache<TextImageCacheKey, CachedTextImage>,
    text_glyph_atlas: TextGlyphAtlas,
    text_glyph_run_cache: BoundedLruCache<TextGlyphRunCacheKey, CachedTextGlyphRun>,
    #[cfg(not(target_arch = "wasm32"))]
    text_glyph_gpu_run_cache: BoundedLruCache<TextGlyphRunCacheKey, CachedGpuTextGlyphRun>,
    text_glyph_mask_cache: SoftwareGlyphRasterCache,
    text_line_index_cache: TextLineIndexCache,
    scratch_shape_data: Vec<ShapeData>,
    scratch_gradients: Vec<GradientStop>,
    scratch_image_vertices: Vec<Vertex>,
    scratch_image_indices: Vec<u32>,
    scratch_image_cmds: Vec<ImageDrawCmd>,
    scratch_glyph_cmds: Vec<GlyphDrawCmd>,
    scratch_text_glyph_run: Vec<SoftwareGlyphAtlasRunGlyph>,
    scratch_text_glyph_placements: Vec<SoftwareGlyphAtlasPlacement>,
    scratch_text_glyph_quads: Vec<CachedTextGlyphQuad>,
    scratch_segment_items: Vec<(usize, SegmentDrawItem)>,
    scratch_effect_ranges: Vec<Range<usize>>,
    scratch_layer_events: Vec<LayerEvent>,
    staged_uploads: StagedBufferUploads,
    frame_graph_executor: WgpuFrameGraphExecutor,
    deferred_offscreen_releases: Vec<OffscreenTarget>,
    effect_renderer: EffectRenderer,
    layer_surface_cache: LayerSurfaceCache,
    observed_scene_range_cache_misses: BoundedLruCache<LayerRasterCacheKey, ()>,
    shadow_surface_cache: BoundedLruCache<ShadowSurfaceCacheKey, CachedShadowSurface>,
    shadow_surface_cache_bytes: u64,
    frame_stats: gpu_stats::FrameStats,
    last_frame_stats: Option<gpu_stats::FrameStatsSnapshot>,
    pending_frame_warmup_frames: u8,
    frame_count: u64,
    gpu_stats_enabled: bool,
    warning_state: RendererWarningState,
    #[cfg(not(target_arch = "wasm32"))]
    replay_upload_stats: ReplayUploadStats,
    /// The frame's replay recolor patches, parked here by
    /// `consume_replay_ops` until the retained prepare arms drain them
    /// (`stage_replay_patches`). The vec this frame's ops displace is last
    /// frame's, already drained empty, and returns to the producer with
    /// the ack — capacity ping-pongs planner queue → packet ops → here →
    /// ack return, so neither side allocates per frame (P4b).
    #[cfg(not(target_arch = "wasm32"))]
    replay_color_patches: Vec<crate::scene::ColorPatch>,
    /// Drain arena for `replay_color_patches`: `stage_replay_patches`
    /// swaps against this instead of `mem::take`, so both keep their
    /// high-water capacity across frames. Always empty between drains.
    #[cfg(not(target_arch = "wasm32"))]
    color_patch_scratch: Vec<crate::scene::ColorPatch>,
    /// Recycled confirmations buffer for the next [`crate::frame_packet::ReplayAck`]:
    /// `consume_replay_ops` fills it, the planner drains it in `apply_ack`,
    /// and the render loop hands the emptied vec (capacity intact) back
    /// here — the ack channel's half of the P4b no-allocation contract.
    #[cfg(not(target_arch = "wasm32"))]
    replay_ack_confirmations: Vec<crate::frame_packet::ReplayConfirmation>,
    /// Lifetime count of replay-ops batches dropped whole by the
    /// generation check in `consume_replay_ops` — fail-closed against ops
    /// planned under a slot universe this store no longer holds.
    /// Synchronously impossible today; structural for the pipeline split.
    #[cfg(not(target_arch = "wasm32"))]
    replay_generation_drops: u64,
    /// Cached render bundles for maximal consecutive retained stretches in
    /// the fused segment pass (`CRANPOSE_RETAINED_BUNDLES` kill switch).
    #[cfg(not(target_arch = "wasm32"))]
    retained_bundle_cache: RetainedBundleCache,
}

/// Running totals for retained-slot patch uploads, the paint-bandwidth
/// instrument: recolors upload 16-byte paint records (plus gradient stop
/// spans), coalesced per slot between the lowest and highest patched
/// index, so `bytes` versus `ideal_bytes` (patched colors alone) is just
/// the untouched records inside each coalesced span.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct ReplayUploadStats {
    calls: u64,
    patched_calls: u64,
    patches: u64,
    slots: u64,
    records: u64,
    bytes: u64,
    ideal_bytes: u64,
    max_frame_bytes: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl ReplayUploadStats {
    /// One aggregate line roughly every few seconds: cheap enough to stay
    /// on unconditionally, which matters because the watch cannot take
    /// setprop-backed diag flags — its logcat is the only channel, and a
    /// measurement window must catch several lines. Counts every drain
    /// call (the drain runs several times per frame; only the first sees
    /// patches) so a target with zero paint traffic still reports an
    /// affirmative zero instead of silence, while the averages divide by
    /// PATCHED calls so they read as per-frame numbers.
    /// warn level: the platform loggers filter info on desktop.
    const REPORT_CALLS: u64 = 1024;

    fn note_frame(&mut self, patches: u64, slots: u64, records: u64, bytes: u64, ideal: u64) {
        self.calls += 1;
        if patches > 0 {
            self.patched_calls += 1;
            self.patches += patches;
            self.slots += slots;
            self.records += records;
            self.bytes += bytes;
            self.ideal_bytes += ideal;
            self.max_frame_bytes = self.max_frame_bytes.max(bytes);
        }
        if self.calls >= Self::REPORT_CALLS {
            let patched = self.patched_calls.max(1);
            log::warn!(
                "[replay-upload] {} patched of {} drains: avg {:.1} KB/frame (max {:.1} KB), \
                 color-only would be {:.1} KB/frame; avg {} patches over {} records in {} slots",
                self.patched_calls,
                self.calls,
                self.bytes as f64 / patched as f64 / 1024.0,
                self.max_frame_bytes as f64 / 1024.0,
                self.ideal_bytes as f64 / patched as f64 / 1024.0,
                self.patches / patched,
                self.records / patched,
                self.slots / patched,
            );
            *self = Self::default();
        }
    }
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

#[cfg(test)]
fn layer_raster_cache_candidate(
    layer: &LayerNode,
    root_scale: f32,
    has_backdrop_underlay: bool,
    allow_runtime_cache: bool,
) -> Option<(LayerRasterCacheKey, Rect)> {
    let mut layer_surface_requirements_cache = HashMap::new();
    let surface_requirements =
        layer_surface_requirements_cached(layer, &mut layer_surface_requirements_cache);
    let runtime_cache_is_safe = allow_runtime_cache
        && surface_requirements
            .surface_requirements
            .has_isolating_requirement()
        && !surface_requirements.contains_runtime_shader;
    let cache_is_allowed = layer.cache_policy == CachePolicy::Auto
        || (allow_runtime_cache && surface_requirements.has_renderer_forced_surface())
        || runtime_cache_is_safe;
    if !cache_is_allowed {
        return None;
    }
    if layer_uses_external_backdrop_input(layer, has_backdrop_underlay) {
        return None;
    }
    // Not just this layer's own effect: a shader anywhere below it makes the
    // whole subtree change every frame with nothing in any hash to say so.
    if surface_requirements.contains_runtime_shader {
        return None;
    }

    let logical_rect = estimate_layer_surface_rect(layer);
    let pixel_size = surface_target_size(logical_rect, root_scale, u32::MAX);
    Some((
        LayerRasterCacheKey::new(
            layer.node_id,
            layer.target_content_hash(),
            layer.effect_hash(),
            logical_rect,
            pixel_size,
            ScaleBucket::from_scale(root_scale),
        ),
        logical_rect,
    ))
}

impl GpuRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        adapter_backend: wgpu::Backend,
        text_fonts: SoftwareTextFontSet,
        renderer_epoch: u64,
        store_feed_generation: u64,
    ) -> Self {
        #[cfg(target_arch = "wasm32")]
        let _ = store_feed_generation;
        let shape_batch_limits = ShapeBatchLimits::for_device(&device);
        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        #[cfg(not(target_arch = "wasm32"))]
        let retained_glyph_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Retained Glyph Dynamic Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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

        // Read-only storage bindings where the device has them (so a whole
        // scene fits one batch); uniform arrays on WebGL-class devices, which
        // have no storage buffers in fragment shaders. The shape array is
        // visible to the vertex stage as well: the pipeline has no vertex
        // buffer and `vs_main` pulls quad corners from ShapeData. (Storage
        // mode is gated on `max_storage_buffers_per_shader_stage`, which GL
        // backends report as the minimum across stages, so a device that
        // cannot read storage from the vertex stage falls back to uniforms.)
        let mut shape_bind_group_layout_entries = vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: shape_batch_limits.data_binding_type(),
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: shape_batch_limits.data_binding_type(),
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // The similarity transform rides a dynamic offset so
            // retained draws sharing one captured batch can each
            // apply their own transform; ordinary batches pass
            // offset 0 into the identity buffer.
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<SimilarityTransform>() as u64,
                    ),
                },
                count: None,
            },
        ];
        // Retained-slot paint colors, read by the vertex stage under
        // `paint_select` (see `shape_shader_source`). Storage mode only:
        // the uniform-variant shader never declares the array, and
        // uniform-mode devices never host retained slots, so their layout
        // stays exactly the three-entry one the uniform pipeline expects.
        if shape_batch_limits.storage {
            shape_bind_group_layout_entries.push(wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            });
        }
        let shape_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Shape Bind Group Layout"),
                entries: &shape_bind_group_layout_entries,
            });

        let identity_similarity_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Identity Similarity Buffer"),
            size: std::mem::size_of::<SimilarityTransform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: true,
        });
        identity_similarity_buffer
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(bytemuck::bytes_of(&SimilarityTransform::IDENTITY));
        identity_similarity_buffer.unmap();

        // Fresh-batch bind groups need a resource at the paint binding even
        // though their draws leave `paint_select` at 0.0 and never use the
        // value; one minimal buffer (a single never-read vec4) serves every
        // batch. Uniform-mode layouts have no paint entry, so none exists.
        let dummy_paint_buffer = shape_batch_limits.storage.then(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Dummy Paint Buffer"),
                size: std::mem::size_of::<[f32; 4]>() as u64,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        });
        #[cfg(not(target_arch = "wasm32"))]
        let replay_slot_store = ReplaySlotStore::new(&device);

        let pipeline = LazyGpuResource::new("shape/src-over");
        let pipeline_dst_out = LazyGpuResource::new("shape/dst-out");
        #[cfg(not(target_arch = "wasm32"))]
        let mesh_pipeline = LazyGpuResource::new("shape/mesh");
        // The instanced-quad selection is LATCHED here, once per renderer:
        // cached retained bundles encode whichever pipelines this resolves
        // to, so a per-draw env read could let a bundle replay a selection
        // the direct path no longer makes. Storage mode only — the
        // uniform/WebGL path keeps `vs_main` and its plain draws untouched.
        #[cfg(not(target_arch = "wasm32"))]
        let instanced_quads =
            (shape_batch_limits.storage && instanced_quads_enabled()).then(|| {
                let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Instanced Quad Index Buffer"),
                    size: std::mem::size_of_val(&INSTANCED_QUAD_INDICES) as u64,
                    usage: wgpu::BufferUsages::INDEX,
                    mapped_at_creation: true,
                });
                index_buffer
                    .slice(..)
                    .get_mapped_range_mut()
                    .copy_from_slice(bytemuck::cast_slice(&INSTANCED_QUAD_INDICES));
                index_buffer.unmap();
                InstancedQuadPipelines {
                    pipeline: LazyGpuResource::new("shape/instanced-src-over"),
                    pipeline_dst_out: LazyGpuResource::new("shape/instanced-dst-out"),
                    index_buffer,
                }
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

        let image_pipeline = LazyGpuResource::new("image/src-over");
        let image_pipeline_dst_out = LazyGpuResource::new("image/dst-out");
        let glyph_atlas_pipeline = LazyGpuResource::new("glyph/shared");
        #[cfg(not(target_arch = "wasm32"))]
        let retained_glyph_atlas_pipeline = LazyGpuResource::new("glyph/retained");

        #[cfg(not(target_arch = "wasm32"))]
        let upload_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Frame Upload Buffer"),
            size: INITIAL_UPLOAD_BUFFER_BYTES,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        #[cfg(not(target_arch = "wasm32"))]
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Uniform Buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        #[cfg(not(target_arch = "wasm32"))]
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Uniform Bind Group"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        #[cfg(not(target_arch = "wasm32"))]
        let shape_buffers = ShapeBatchBuffers::new(
            &device,
            &shape_bind_group_layout,
            &identity_similarity_buffer,
            dummy_paint_buffer.as_ref(),
            shape_batch_limits,
        );

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

        #[cfg(not(target_arch = "wasm32"))]
        let image_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Image Vertex Buffer"),
            size: (std::mem::size_of::<Vertex>() * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        #[cfg(not(target_arch = "wasm32"))]
        let image_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Image Index Buffer"),
            size: (std::mem::size_of::<u32>() * 6) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        #[cfg(not(target_arch = "wasm32"))]
        let retained_glyph_uniform_stride = align_usize_to(
            std::mem::size_of::<Uniforms>(),
            (device.limits().min_uniform_buffer_offset_alignment as usize)
                .max(wgpu::COPY_BUFFER_ALIGNMENT as usize),
        ) as u64;
        #[cfg(not(target_arch = "wasm32"))]
        let retained_glyph_uniform_capacity = INITIAL_RETAINED_GLYPH_UNIFORM_SLOTS;
        #[cfg(not(target_arch = "wasm32"))]
        let retained_glyph_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Retained Glyph Uniform Buffer"),
            size: retained_glyph_uniform_stride * retained_glyph_uniform_capacity as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        #[cfg(not(target_arch = "wasm32"))]
        let retained_glyph_uniform_bind_group =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Retained Glyph Uniform Bind Group"),
                layout: &retained_glyph_uniform_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &retained_glyph_uniform_buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                    }),
                }],
            });

        let effect_renderer = EffectRenderer::new(&device, surface_format, adapter_backend);

        Self {
            device,
            queue,
            renderer_epoch,
            #[cfg(not(target_arch = "wasm32"))]
            store_feed_generation,
            surface_format,
            adapter_backend,
            shape_batch_limits,
            pipeline,
            pipeline_dst_out,
            #[cfg(not(target_arch = "wasm32"))]
            mesh_pipeline,
            #[cfg(not(target_arch = "wasm32"))]
            instanced_quads,
            uniform_bind_group_layout,
            shape_bind_group_layout,
            dummy_paint_buffer,
            identity_similarity_buffer,
            #[cfg(not(target_arch = "wasm32"))]
            replay_slots: replay_slot_store,
            image_pipeline,
            image_pipeline_dst_out,
            glyph_atlas_pipeline,
            #[cfg(not(target_arch = "wasm32"))]
            retained_glyph_atlas_pipeline,
            image_bind_group_layout,
            #[cfg(not(target_arch = "wasm32"))]
            retained_glyph_uniform_bind_group_layout,
            image_nearest_sampler,
            image_linear_sampler,
            text_fonts,
            #[cfg(not(target_arch = "wasm32"))]
            upload_buffer,
            #[cfg(not(target_arch = "wasm32"))]
            uniform_buffer,
            #[cfg(not(target_arch = "wasm32"))]
            uniform_bind_group,
            #[cfg(not(target_arch = "wasm32"))]
            shape_buffers,
            #[cfg(not(target_arch = "wasm32"))]
            image_vertex_buffer,
            #[cfg(not(target_arch = "wasm32"))]
            image_index_buffer,
            #[cfg(not(target_arch = "wasm32"))]
            retained_glyph_uniform_buffer,
            #[cfg(not(target_arch = "wasm32"))]
            retained_glyph_uniform_bind_group,
            #[cfg(not(target_arch = "wasm32"))]
            retained_glyph_uniform_stride,
            #[cfg(not(target_arch = "wasm32"))]
            retained_glyph_uniform_capacity,
            #[cfg(not(target_arch = "wasm32"))]
            retained_glyph_uniform_cursor: 0,
            #[cfg(target_arch = "wasm32")]
            wasm_uniform_batches: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            wasm_uniform_batch_cursor: 0,
            #[cfg(target_arch = "wasm32")]
            wasm_shape_batches: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            wasm_shape_batch_cursor: 0,
            #[cfg(target_arch = "wasm32")]
            wasm_image_batches: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            wasm_image_batch_cursor: 0,
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
            #[cfg(not(target_arch = "wasm32"))]
            text_glyph_gpu_run_cache: BoundedLruCache::with_capacity_at_least_one(
                MAX_TEXT_GLYPH_GPU_RUN_CACHE_ITEMS,
            ),
            text_glyph_mask_cache: SoftwareGlyphRasterCache::with_capacity_at_least_one(
                MAX_TEXT_GLYPH_MASK_CACHE_ITEMS,
            ),
            text_line_index_cache: TextLineIndexCache::new(MAX_TEXT_LINE_INDEX_CACHE_ITEMS),
            scratch_shape_data: Vec::new(),
            scratch_gradients: Vec::new(),
            scratch_image_vertices: Vec::new(),
            scratch_image_indices: Vec::new(),
            scratch_image_cmds: Vec::new(),
            scratch_glyph_cmds: Vec::new(),
            scratch_text_glyph_run: Vec::new(),
            scratch_text_glyph_placements: Vec::new(),
            scratch_text_glyph_quads: Vec::new(),
            scratch_segment_items: Vec::new(),
            scratch_effect_ranges: Vec::new(),
            scratch_layer_events: Vec::new(),
            staged_uploads: StagedBufferUploads::default(),
            frame_graph_executor: WgpuFrameGraphExecutor::new(),
            deferred_offscreen_releases: Vec::new(),
            effect_renderer,
            layer_surface_cache: LayerSurfaceCache::new(),
            observed_scene_range_cache_misses: BoundedLruCache::with_capacity_at_least_one(
                MAX_OBSERVED_SCENE_RANGE_CACHE_MISSES,
            ),
            shadow_surface_cache: BoundedLruCache::with_capacity_at_least_one(
                MAX_SHADOW_SURFACE_CACHE_ITEMS,
            ),
            shadow_surface_cache_bytes: 0,
            frame_stats: gpu_stats::FrameStats::default(),
            last_frame_stats: None,
            pending_frame_warmup_frames: 0,
            frame_count: 0,
            gpu_stats_enabled: gpu_stats_enabled(),
            warning_state: RendererWarningState::default(),
            #[cfg(not(target_arch = "wasm32"))]
            replay_upload_stats: ReplayUploadStats::default(),
            #[cfg(not(target_arch = "wasm32"))]
            replay_color_patches: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            color_patch_scratch: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            replay_ack_confirmations: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            replay_generation_drops: 0,
            #[cfg(not(target_arch = "wasm32"))]
            retained_bundle_cache: RetainedBundleCache::new(),
        }
    }

    fn shape_pipeline(&self, blend_mode: BlendMode) -> &wgpu::RenderPipeline {
        let resource = match blend_mode {
            BlendMode::DstOut => &self.pipeline_dst_out,
            _ => &self.pipeline,
        };
        resource.get_or_init(self.adapter_backend, || {
            create_shape_pipeline(
                &self.device,
                self.surface_format,
                &self.uniform_bind_group_layout,
                &self.shape_bind_group_layout,
                blend_mode,
                self.shape_batch_limits,
            )
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn mesh_pipeline(&self) -> &wgpu::RenderPipeline {
        self.mesh_pipeline.get_or_init(self.adapter_backend, || {
            create_mesh_shape_pipeline(
                &self.device,
                self.surface_format,
                &self.uniform_bind_group_layout,
                &self.shape_bind_group_layout,
                self.shape_batch_limits,
            )
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn instanced_pipeline<'a>(
        &'a self,
        instanced: &'a InstancedQuadPipelines,
        blend_mode: BlendMode,
    ) -> &'a wgpu::RenderPipeline {
        let resource = match blend_mode {
            BlendMode::DstOut => &instanced.pipeline_dst_out,
            _ => &instanced.pipeline,
        };
        resource.get_or_init(self.adapter_backend, || {
            create_instanced_shape_pipeline(
                &self.device,
                self.surface_format,
                &self.uniform_bind_group_layout,
                &self.shape_bind_group_layout,
                blend_mode,
                self.shape_batch_limits,
            )
        })
    }

    fn image_pipeline(&self, blend_mode: BlendMode) -> &wgpu::RenderPipeline {
        let resource = match blend_mode {
            BlendMode::DstOut => &self.image_pipeline_dst_out,
            _ => &self.image_pipeline,
        };
        resource.get_or_init(self.adapter_backend, || {
            create_image_pipeline(
                &self.device,
                self.surface_format,
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
                    self.surface_format,
                    &self.uniform_bind_group_layout,
                    &self.image_bind_group_layout,
                )
            })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn retained_glyph_atlas_pipeline(&self) -> &wgpu::RenderPipeline {
        self.retained_glyph_atlas_pipeline
            .get_or_init(self.adapter_backend, || {
                create_glyph_atlas_pipeline(
                    &self.device,
                    self.surface_format,
                    &self.retained_glyph_uniform_bind_group_layout,
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
        // Byte-bounded eviction on top of the count bound: never evict the
        // entry just inserted (this frame draws it).
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

    /// Acquire an offscreen target from the pool with stats tracking.
    /// Uses split borrows to avoid conflicting borrows on self.
    fn max_texture_dim(&self) -> u32 {
        self.effect_renderer.max_texture_dim()
    }

    fn acquire_offscreen(&mut self, width: u32, height: u32) -> OffscreenTarget {
        self.effect_renderer
            .acquire_offscreen(&self.device, width, height, Some(&self.frame_stats))
    }

    fn acquire_retained_surface(&mut self, width: u32, height: u32) -> OffscreenTarget {
        self.acquire_offscreen(width, height)
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
            self.surface_format,
        )
    }

    fn defer_offscreen_release(&mut self, target: OffscreenTarget) {
        self.deferred_offscreen_releases.push(target);
    }

    fn flush_deferred_offscreen_releases(&mut self) {
        for target in self.deferred_offscreen_releases.drain(..) {
            self.effect_renderer.release_offscreen(target);
        }
    }

    fn release_layer_surface_target(&mut self, target: LayerSurfaceTexture) {
        if let LayerSurfaceTexture::Owned(target) = target {
            self.defer_offscreen_release(target);
        }
    }

    fn cached_layer_surface(
        &mut self,
        key: &LayerRasterCacheKey,
    ) -> Option<(Rc<OffscreenTarget>, Rect)> {
        self.layer_surface_cache.get(key, &self.frame_stats)
    }

    fn admit_layer_surface_cache_miss(&mut self, key: &LayerRasterCacheKey) -> bool {
        admit_layer_surface_cache_miss_impl(key, &mut self.observed_scene_range_cache_misses)
    }

    fn insert_cached_layer_surface(
        &mut self,
        key: LayerRasterCacheKey,
        target: OffscreenTarget,
        logical_rect: Rect,
    ) -> Rc<OffscreenTarget> {
        self.layer_surface_cache
            .insert(key, target, logical_rect, &self.frame_stats)
    }

    fn cached_shadow_surface(
        &mut self,
        key: &ShadowSurfaceCacheKey,
    ) -> Option<Rc<OffscreenTarget>> {
        self.shadow_surface_cache
            .get(key)
            .map(|cached| cached.target.clone())
    }

    fn cached_shape_shadow_composite(
        &mut self,
        shadow: &ShadowDraw,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Option<CachedShadowComposite> {
        if shadow.blur_radius <= 0.0 || shadow.shapes.is_empty() || !shadow.texts.is_empty() {
            return None;
        }

        let plan = shape_shadow_surface_plan(
            &shadow.shapes,
            shadow.clip,
            shadow.blur_radius,
            width,
            height,
            root_scale,
            self.max_texture_dim(),
        )?;
        let key = shape_shadow_surface_cache_key(
            &shadow.shapes,
            plan.source_device_bounds,
            plan.pixel_radius,
            root_scale,
        )?;
        let cached = self.cached_shadow_surface(&key)?;
        let viewport_offset = [plan.source_device_bounds.x, plan.source_device_bounds.y];
        self.frame_stats.record_shadow_shape_cache_hit(
            plan.source_device_bounds.width,
            plan.source_device_bounds.height,
        );

        let clip_scissor = shadow
            .clip
            .and_then(|clip| scissor_rect_for_rect(clip, root_scale, width, height));
        let scissor = clip_scissor.or(plan.processing_scissor);
        let rounded_mask = inner_shadow_composite_mask(shadow, root_scale).map(|mut mask| {
            mask.rect[0] -= viewport_offset[0];
            mask.rect[1] -= viewport_offset[1];
            mask
        });
        let dest_viewport = Some((
            viewport_offset[0],
            viewport_offset[1],
            plan.source_device_bounds.width as f32,
            plan.source_device_bounds.height as f32,
        ));

        Some(CachedShadowComposite {
            source: cached,
            scissor,
            rounded_mask,
            dest_viewport,
        })
    }

    fn insert_cached_shadow_surface(
        &mut self,
        key: ShadowSurfaceCacheKey,
        target: OffscreenTarget,
    ) {
        let byte_size = offscreen_byte_size(target.width, target.height);
        while self.shadow_surface_cache_bytes + byte_size > MAX_SHADOW_SURFACE_CACHE_BYTES {
            let Some((_evicted_key, evicted_entry)) = self.shadow_surface_cache.pop_lru() else {
                break;
            };
            self.shadow_surface_cache_bytes = self
                .shadow_surface_cache_bytes
                .saturating_sub(evicted_entry.byte_size);
        }

        let cached = CachedShadowSurface {
            target: Rc::new(target),
            byte_size,
        };
        if let Some((_replaced_key, replaced_entry)) = self.shadow_surface_cache.push(key, cached) {
            self.shadow_surface_cache_bytes = self
                .shadow_surface_cache_bytes
                .saturating_sub(replaced_entry.byte_size);
        }
        self.shadow_surface_cache_bytes = self.shadow_surface_cache_bytes.saturating_add(byte_size);
    }

    fn supports_render_effect(&self, effect: &RenderEffect) -> bool {
        is_render_effect_supported(effect)
    }
}

struct RecordingSurfaceBackend<'renderer, 'recorder, C: FrameCommandRecorder> {
    renderer: &'renderer mut GpuRenderer,
    recorder: &'recorder mut C,
}

impl<C: FrameCommandRecorder> RecordingSurfaceBackend<'_, '_, C> {
    #[allow(clippy::too_many_arguments)]
    fn render_range_with_layer_events_to_target_recorded(
        &mut self,
        target: &OffscreenTarget,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        draw_ops: &[DrawOp],
        effect_layers: &[EffectLayer],
        backdrop_layers: &[BackdropLayer],
        z_start: usize,
        z_end: usize,
        excluded_effect_layer: Option<usize>,
        width: u32,
        height: u32,
        root_scale: f32,
        backdrop_underlay: Option<&OffscreenTarget>,
        initial_load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        if z_start >= z_end {
            if matches!(initial_load_op, wgpu::LoadOp::Clear(_)) {
                self.clear_target_view_with_load_op(&target.view, initial_load_op);
            }
            return Ok(());
        }

        let mut effect_z_ranges = std::mem::take(&mut self.renderer.scratch_effect_ranges);
        collect_effect_ranges(
            effect_layers,
            z_start,
            z_end,
            excluded_effect_layer,
            &mut effect_z_ranges,
        );
        let mut events = std::mem::take(&mut self.renderer.scratch_layer_events);
        collect_layer_events(
            effect_layers,
            backdrop_layers,
            z_start,
            z_end,
            excluded_effect_layer,
            &mut events,
        );

        let result = (|| -> Result<(), String> {
            let mut next_load_op = initial_load_op;
            let mut cursor_z = z_start;
            for event in &events {
                if event.z_index > cursor_z {
                    self.render_non_effect_segment(
                        &target.view,
                        shapes,
                        images,
                        texts,
                        shadow_draws,
                        // Windowed scenes never carry retained draws — see
                        // `build_scene_window`.
                        &[],
                        draw_ops,
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
                    self.clear_target_view_with_load_op(&target.view, next_load_op);
                    next_load_op = wgpu::LoadOp::Load;
                }

                match event.kind {
                    LayerEventKind::Backdrop(index) => {
                        let layer = &backdrop_layers[index];
                        let effective_backdrop_underlay = if backdrop_underlay.is_some()
                            && backdrop_underlay_is_covered_by_local_content(
                                shapes,
                                images,
                                shadow_draws,
                                draw_ops,
                                effect_layers,
                                backdrop_layers,
                                layer,
                            ) {
                            None
                        } else {
                            backdrop_underlay
                        };
                        execute_apply_backdrop_layer_to_target(
                            self,
                            target,
                            layer,
                            effective_backdrop_underlay,
                            width,
                            height,
                            root_scale,
                            None,
                        )?;
                    }
                    LayerEventKind::Effect(index) => {
                        let layer = &effect_layers[index];
                        if layer.z_start < cursor_z {
                            continue;
                        }
                        execute_render_effect_layer_to_target(
                            self,
                            target,
                            shapes,
                            images,
                            texts,
                            shadow_draws,
                            draw_ops,
                            effect_layers,
                            backdrop_layers,
                            index,
                            backdrop_underlay,
                            width,
                            height,
                            root_scale,
                        )?;
                        cursor_z = cursor_z.max(layer.z_end);
                    }
                }
            }

            if cursor_z < z_end {
                self.render_non_effect_segment(
                    &target.view,
                    shapes,
                    images,
                    texts,
                    shadow_draws,
                    &[],
                    draw_ops,
                    cursor_z,
                    z_end,
                    &effect_z_ranges,
                    width,
                    height,
                    root_scale,
                    next_load_op,
                )?;
            } else if matches!(next_load_op, wgpu::LoadOp::Clear(_)) {
                self.clear_target_view_with_load_op(&target.view, next_load_op);
            }

            Ok(())
        })();

        self.renderer.scratch_effect_ranges = effect_z_ranges;
        self.renderer.scratch_layer_events = events;
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn record_shader_composite(
        &mut self,
        source: &OffscreenTarget,
        shader: &RuntimeShader,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        sample_mode: CompositeSampleMode,
    ) {
        let device = self.renderer.device.clone();
        if let Some(viewport) = direct_shader_composite_viewport(
            alpha,
            blend_mode,
            dest_viewport,
            sample_mode,
            (source.width, source.height),
        ) {
            let shader_applied = self
                .renderer
                .effect_renderer
                .encode_shader_src_over_to_view(
                    self.recorder,
                    &device,
                    source,
                    dest_view,
                    shader,
                    effect_rect,
                    load_op,
                    scissor,
                    viewport,
                );
            if shader_applied {
                self.renderer
                    .effect_renderer
                    .debug_effects
                    .set(self.renderer.effect_renderer.debug_effects.get() + 1);
                self.recorder.record_pass();
                self.renderer.effect_renderer.record_composite_pass();
                return;
            }
        }
        let scratch_descriptor = self.renderer.transient_offscreen_descriptor(
            "Shader Effect Composite Scratch",
            source.width,
            source.height,
        );
        let scratch = self
            .recorder
            .acquire_transient_offscreen(&device, scratch_descriptor);
        let shader_applied = {
            self.renderer.effect_renderer.encode_shader(
                self.recorder,
                &device,
                source,
                &scratch.view,
                shader,
                effect_rect,
            )
        };
        let composite_source = if shader_applied {
            self.renderer
                .effect_renderer
                .debug_effects
                .set(self.renderer.effect_renderer.debug_effects.get() + 1);
            self.recorder.record_pass();
            &scratch
        } else {
            source
        };
        {
            self.renderer
                .effect_renderer
                .encode_composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
                    self.recorder,
                    &device,
                    composite_source,
                    dest_view,
                    alpha,
                    load_op,
                    scissor,
                    None,
                    supported_blend_mode(blend_mode),
                    dest_viewport,
                    sample_mode,
                );
        }
        self.recorder.record_pass();
        self.renderer.effect_renderer.record_composite_pass();
        self.recorder
            .release_transient_offscreen(scratch_descriptor, scratch);
    }

    #[allow(clippy::too_many_arguments)]
    fn record_shader_projective_composite(
        &mut self,
        source: &OffscreenTarget,
        shader: &RuntimeShader,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        source_size: (f32, f32),
        inverse_matrix: [[f32; 3]; 3],
        dest_bounds: [[f32; 2]; 4],
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
    ) {
        if projective_dest_bounds_rect(dest_bounds).is_none() {
            return;
        }
        let device = self.renderer.device.clone();
        let scratch_descriptor = self.renderer.transient_offscreen_descriptor(
            "Shader Projective Composite Scratch",
            source.width,
            source.height,
        );
        let scratch = self
            .recorder
            .acquire_transient_offscreen(&device, scratch_descriptor);
        let shader_applied = {
            self.renderer.effect_renderer.encode_shader(
                self.recorder,
                &device,
                source,
                &scratch.view,
                shader,
                effect_rect,
            )
        };
        let composite_source = if shader_applied {
            self.renderer
                .effect_renderer
                .debug_effects
                .set(self.renderer.effect_renderer.debug_effects.get() + 1);
            self.recorder.record_pass();
            &scratch
        } else {
            source
        };
        let composited = {
            self.renderer
                .effect_renderer
                .encode_composite_to_view_projective(
                    self.recorder,
                    &device,
                    composite_source,
                    dest_view,
                    viewport,
                    source_size,
                    inverse_matrix,
                    dest_bounds,
                    alpha,
                    load_op,
                    scissor,
                    supported_blend_mode(blend_mode),
                    sample_mode,
                )
        };
        if composited {
            self.recorder.record_pass();
            self.renderer.effect_renderer.record_composite_pass();
        }
        self.recorder
            .release_transient_offscreen(scratch_descriptor, scratch);
    }

    #[allow(clippy::too_many_arguments)]
    fn record_effect_with_direct_shader_tail_composite(
        &mut self,
        source: &OffscreenTarget,
        first_effect: &RenderEffect,
        shader: &RuntimeShader,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        dest_viewport: (f32, f32, f32, f32),
    ) -> Result<bool, String> {
        let device = self.renderer.device.clone();
        let intermediate_descriptor = self.renderer.transient_offscreen_descriptor(
            "Render Effect Direct Shader Tail Intermediate",
            source.width,
            source.height,
        );
        let intermediate = self
            .recorder
            .acquire_transient_offscreen(&device, intermediate_descriptor);
        let effect_scratch_targets = self
            .renderer
            .effect_renderer
            .acquire_recorded_effect_scratch_targets(
                self.recorder,
                &device,
                first_effect,
                source.width,
                source.height,
                self.renderer.surface_format,
            );
        let first_passes = {
            let mut effect_scratch_refs = effect_scratch_targets.refs();
            let pass_count = self.renderer.effect_renderer.encode_effect(
                self.recorder,
                &device,
                source,
                &intermediate.view,
                first_effect,
                effect_rect,
                &mut effect_scratch_refs,
            );
            match pass_count {
                Ok(pass_count) => effect_scratch_refs.assert_consumed().map(|()| pass_count),
                Err(error) => Err(error),
            }
        };
        let first_passes = match first_passes {
            Ok(pass_count) => pass_count,
            Err(error) => {
                effect_scratch_targets.release_into(self.recorder);
                self.recorder
                    .release_transient_offscreen(intermediate_descriptor, intermediate);
                return Err(error);
            }
        };
        let shader_applied = self
            .renderer
            .effect_renderer
            .encode_shader_src_over_to_view(
                self.recorder,
                &device,
                &intermediate,
                dest_view,
                shader,
                effect_rect,
                load_op,
                scissor,
                dest_viewport,
            );
        self.recorder
            .record_passes(first_passes.saturating_add(u32::from(shader_applied)));
        effect_scratch_targets.release_into(self.recorder);
        self.recorder
            .release_transient_offscreen(intermediate_descriptor, intermediate);
        if !shader_applied {
            return Ok(false);
        }
        self.renderer
            .effect_renderer
            .debug_effects
            .set(self.renderer.effect_renderer.debug_effects.get() + 1);
        self.renderer.effect_renderer.record_composite_pass();
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn record_effect_composite(
        &mut self,
        source: &OffscreenTarget,
        effect: &RenderEffect,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        sample_mode: CompositeSampleMode,
    ) -> Result<(), String> {
        if let (
            RenderEffect::Chain { first, second },
            Some(viewport),
            BlendMode::SrcOver,
            CompositeSampleMode::Linear,
        ) = (
            effect,
            dest_viewport,
            supported_blend_mode(blend_mode),
            sample_mode,
        ) {
            if let (
                RenderEffect::Blur {
                    radius_x,
                    radius_y,
                    edge_treatment,
                },
                RenderEffect::Shader { shader },
            ) = (first.as_ref(), second.as_ref())
            {
                if *radius_x > 0.0 || *radius_y > 0.0 {
                    let device = self.renderer.device.clone();
                    let scratch_descriptor = self.renderer.transient_offscreen_descriptor(
                        "Blur Rounded Mask Scratch",
                        source.width,
                        source.height,
                    );
                    let scratch = self
                        .recorder
                        .acquire_transient_offscreen(&device, scratch_descriptor);
                    let fused = self
                        .renderer
                        .effect_renderer
                        .encode_blur_then_rounded_mask_src_over_to_view(
                            self.recorder,
                            &device,
                            source,
                            &scratch,
                            dest_view,
                            *radius_x,
                            *radius_y,
                            *edge_treatment,
                            shader,
                            effect_rect,
                            load_op,
                            scissor,
                            viewport,
                        );
                    if fused {
                        self.recorder.record_passes(2);
                        self.renderer.effect_renderer.record_blur_pass();
                        self.renderer
                            .effect_renderer
                            .debug_effects
                            .set(self.renderer.effect_renderer.debug_effects.get() + 1);
                        self.renderer.effect_renderer.record_composite_pass();
                        self.recorder
                            .release_transient_offscreen(scratch_descriptor, scratch);
                        return Ok(());
                    }
                    self.recorder
                        .release_transient_offscreen(scratch_descriptor, scratch);
                }
            }
        }
        if let Some((first_effect, shader, viewport)) = direct_shader_tail_composite(
            effect,
            alpha,
            blend_mode,
            dest_viewport,
            sample_mode,
            (source.width, source.height),
        ) {
            if self.record_effect_with_direct_shader_tail_composite(
                source,
                first_effect,
                shader,
                effect_rect,
                dest_view,
                load_op,
                scissor,
                viewport,
            )? {
                return Ok(());
            }
        }
        let device = self.renderer.device.clone();
        let scratch_descriptor = self.renderer.transient_offscreen_descriptor(
            "Render Effect Composite Scratch",
            source.width,
            source.height,
        );
        let scratch = self
            .recorder
            .acquire_transient_offscreen(&device, scratch_descriptor);
        let effect_scratch_targets = self
            .renderer
            .effect_renderer
            .acquire_recorded_effect_scratch_targets(
                self.recorder,
                &device,
                effect,
                source.width,
                source.height,
                self.renderer.surface_format,
            );
        let effect_passes = {
            let mut effect_scratch_refs = effect_scratch_targets.refs();
            let pass_count = self.renderer.effect_renderer.encode_effect(
                self.recorder,
                &device,
                source,
                &scratch.view,
                effect,
                effect_rect,
                &mut effect_scratch_refs,
            )?;
            effect_scratch_refs.assert_consumed()?;
            Ok(pass_count)
        };
        let effect_passes = match effect_passes {
            Ok(pass_count) => pass_count,
            Err(error) => {
                effect_scratch_targets.release_into(self.recorder);
                self.recorder
                    .release_transient_offscreen(scratch_descriptor, scratch);
                return Err(error);
            }
        };
        {
            self.renderer
                .effect_renderer
                .encode_composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
                    self.recorder,
                    &device,
                    &scratch,
                    dest_view,
                    alpha,
                    load_op,
                    scissor,
                    None,
                    supported_blend_mode(blend_mode),
                    dest_viewport,
                    sample_mode,
                );
        }
        self.recorder.record_passes(effect_passes.saturating_add(1));
        self.renderer.effect_renderer.record_composite_pass();
        effect_scratch_targets.release_into(self.recorder);
        self.recorder
            .release_transient_offscreen(scratch_descriptor, scratch);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn record_effect_projective_composite(
        &mut self,
        source: &OffscreenTarget,
        effect: &RenderEffect,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        source_size: (f32, f32),
        inverse_matrix: [[f32; 3]; 3],
        dest_bounds: [[f32; 2]; 4],
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
    ) -> Result<(), String> {
        if projective_dest_bounds_rect(dest_bounds).is_none() {
            return Ok(());
        }
        let device = self.renderer.device.clone();
        let scratch_descriptor = self.renderer.transient_offscreen_descriptor(
            "Render Effect Projective Composite Scratch",
            source.width,
            source.height,
        );
        let scratch = self
            .recorder
            .acquire_transient_offscreen(&device, scratch_descriptor);
        let effect_scratch_targets = self
            .renderer
            .effect_renderer
            .acquire_recorded_effect_scratch_targets(
                self.recorder,
                &device,
                effect,
                source.width,
                source.height,
                self.renderer.surface_format,
            );
        let effect_passes = {
            let mut effect_scratch_refs = effect_scratch_targets.refs();
            let pass_count = self.renderer.effect_renderer.encode_effect(
                self.recorder,
                &device,
                source,
                &scratch.view,
                effect,
                effect_rect,
                &mut effect_scratch_refs,
            )?;
            effect_scratch_refs.assert_consumed()?;
            Ok(pass_count)
        };
        let effect_passes = match effect_passes {
            Ok(pass_count) => pass_count,
            Err(error) => {
                effect_scratch_targets.release_into(self.recorder);
                self.recorder
                    .release_transient_offscreen(scratch_descriptor, scratch);
                return Err(error);
            }
        };
        let composited = {
            self.renderer
                .effect_renderer
                .encode_composite_to_view_projective(
                    self.recorder,
                    &device,
                    &scratch,
                    dest_view,
                    viewport,
                    source_size,
                    inverse_matrix,
                    dest_bounds,
                    alpha,
                    load_op,
                    scissor,
                    supported_blend_mode(blend_mode),
                    sample_mode,
                )
        };
        if composited {
            self.recorder.record_passes(effect_passes.saturating_add(1));
            self.renderer.effect_renderer.record_composite_pass();
        } else {
            self.recorder.record_passes(effect_passes);
        }
        effect_scratch_targets.release_into(self.recorder);
        self.recorder
            .release_transient_offscreen(scratch_descriptor, scratch);
        Ok(())
    }
}

impl<C: FrameCommandRecorder> SurfaceExecutionBackend for RecordingSurfaceBackend<'_, '_, C> {
    fn max_texture_dim(&self) -> u32 {
        self.renderer.max_texture_dim()
    }

    fn acquire_retained_surface(&mut self, width: u32, height: u32) -> OffscreenTarget {
        self.renderer.acquire_retained_surface(width, height)
    }

    fn acquire_frame_surface(&mut self, width: u32, height: u32) -> OffscreenTarget {
        let descriptor =
            self.renderer
                .transient_offscreen_descriptor("Frame Surface", width, height);
        self.recorder
            .acquire_transient_offscreen(&self.renderer.device, descriptor)
    }

    fn release_frame_surface(&mut self, target: OffscreenTarget) {
        let descriptor = self.renderer.transient_offscreen_descriptor(
            "Frame Surface",
            target.width,
            target.height,
        );
        self.recorder
            .release_transient_offscreen(descriptor, target);
    }

    fn release_layer_surface_target(&mut self, target: LayerSurfaceTexture) {
        self.renderer.release_layer_surface_target(target);
    }

    fn cached_layer_surface(
        &mut self,
        key: &LayerRasterCacheKey,
    ) -> Option<(Rc<OffscreenTarget>, Rect)> {
        self.renderer.cached_layer_surface(key)
    }

    fn admit_layer_surface_cache_miss(&mut self, key: &LayerRasterCacheKey) -> bool {
        self.renderer.admit_layer_surface_cache_miss(key)
    }

    fn insert_cached_layer_surface(
        &mut self,
        key: LayerRasterCacheKey,
        target: OffscreenTarget,
        logical_rect: Rect,
    ) -> Rc<OffscreenTarget> {
        self.renderer
            .insert_cached_layer_surface(key, target, logical_rect)
    }

    fn clear_target_view_with_load_op(
        &mut self,
        target_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) {
        {
            let _clear = self
                .recorder
                .encoder()
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Layer Event Clear Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: load_op,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
        }
        self.recorder.record_pass();
    }

    #[allow(clippy::too_many_arguments)]
    fn render_non_effect_segment(
        &mut self,
        target_view: &wgpu::TextureView,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        retained_draws: &[RetainedDraw],
        draw_ops: &[DrawOp],
        z_start: usize,
        z_end: usize,
        effect_z_ranges: &[Range<usize>],
        width: u32,
        height: u32,
        root_scale: f32,
        initial_load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        self.render_non_effect_segment_with_composites(
            target_view,
            shapes,
            images,
            texts,
            shadow_draws,
            retained_draws,
            draw_ops,
            z_start,
            z_end,
            effect_z_ranges,
            &[],
            &[],
            width,
            height,
            root_scale,
            initial_load_op,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_non_effect_segment_with_composites(
        &mut self,
        target_view: &wgpu::TextureView,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        retained_draws: &[RetainedDraw],
        draw_ops: &[DrawOp],
        z_start: usize,
        z_end: usize,
        effect_z_ranges: &[Range<usize>],
        composites: &[(usize, CompositeBatchItem<'_>)],
        shader_composites: &[(usize, ShaderCompositeBatchItem<'_>)],
        width: u32,
        height: u32,
        root_scale: f32,
        initial_load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        let mut ordered_items = std::mem::take(&mut self.renderer.scratch_segment_items);
        collect_non_effect_segment_items(
            shapes,
            images,
            texts,
            shadow_draws,
            draw_ops,
            z_start,
            z_end,
            effect_z_ranges,
            width,
            height,
            root_scale,
            &mut ordered_items,
        );
        #[cfg(not(target_arch = "wasm32"))]
        let raw_shadow_items = ordered_items
            .iter()
            .filter(|(_, item)| matches!(item, SegmentDrawItem::Shadow(_)))
            .count();
        let culled_shadow_items = retain_renderable_shadow_items(
            &mut ordered_items,
            shadow_draws,
            width,
            height,
            root_scale,
            self.renderer.max_texture_dim(),
        );
        #[cfg(target_arch = "wasm32")]
        let _ = culled_shadow_items;
        let mut cached_shadow_composites: Vec<(usize, CachedShadowComposite)> = Vec::new();
        ordered_items.extend(
            composites
                .iter()
                .enumerate()
                .map(|(index, (z_index, _))| (*z_index, SegmentDrawItem::Composite(index))),
        );
        ordered_items.extend(
            shader_composites
                .iter()
                .enumerate()
                .map(|(index, (z_index, _))| (*z_index, SegmentDrawItem::ShaderComposite(index))),
        );
        for (z_index, item) in &mut ordered_items {
            let SegmentDrawItem::Shadow(shadow_index) = *item else {
                continue;
            };
            let Some(composite) = self.renderer.cached_shape_shadow_composite(
                &shadow_draws[shadow_index],
                width,
                height,
                root_scale,
            ) else {
                continue;
            };
            let composite_index = composites.len() + cached_shadow_composites.len();
            cached_shadow_composites.push((*z_index, composite));
            *item = SegmentDrawItem::Composite(composite_index);
        }
        let mut merged_composites = Vec::with_capacity(
            composites
                .len()
                .saturating_add(cached_shadow_composites.len()),
        );
        merged_composites.extend(composites.iter().copied());
        merged_composites.extend(
            cached_shadow_composites
                .iter()
                .map(|(z_index, composite)| (*z_index, composite.batch_item())),
        );
        // Z indices are unique — the scene hands every op its own `next_z` — so an
        // unstable sort cannot reorder anything a stable one wouldn't, and it skips
        // the stable sort's scratch allocation, paid here once per segment per frame.
        ordered_items.sort_unstable_by_key(|(z_index, _)| *z_index);
        #[cfg(not(target_arch = "wasm32"))]
        maybe_print_segment_diag(
            z_start..z_end,
            &ordered_items,
            shapes,
            images,
            SegmentDiagCounts {
                raw_shadow_items,
                culled_shadow_items,
                cached_shadow_composites: cached_shadow_composites.len(),
                composite_items: merged_composites.len(),
                shader_composite_items: shader_composites.len(),
            },
            self.renderer.shape_batch_limits,
        );
        let result = if ordered_items.is_empty() {
            Ok(SegmentCommandEncodeOutcome { first_batch: true })
        } else {
            self.renderer.encode_non_effect_segment_commands(
                self.recorder,
                target_view,
                &ordered_items,
                &merged_composites,
                shader_composites,
                shapes,
                images,
                texts,
                shadow_draws,
                retained_draws,
                initial_load_op,
                width,
                height,
                root_scale,
            )
        };
        self.renderer.scratch_segment_items = ordered_items;
        let outcome = result?;
        if outcome.first_batch && matches!(initial_load_op, wgpu::LoadOp::Clear(_)) {
            self.clear_target_view_with_load_op(target_view, initial_load_op);
        }
        Ok(())
    }

    fn render_range_with_layer_events_to_target(
        &mut self,
        target: &OffscreenTarget,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        draw_ops: &[DrawOp],
        effect_layers: &[EffectLayer],
        backdrop_layers: &[BackdropLayer],
        z_start: usize,
        z_end: usize,
        excluded_effect_layer: Option<usize>,
        width: u32,
        height: u32,
        root_scale: f32,
        backdrop_underlay: Option<&OffscreenTarget>,
        initial_load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        self.render_range_with_layer_events_to_target_recorded(
            target,
            shapes,
            images,
            texts,
            shadow_draws,
            draw_ops,
            effect_layers,
            backdrop_layers,
            z_start,
            z_end,
            excluded_effect_layer,
            width,
            height,
            root_scale,
            backdrop_underlay,
            initial_load_op,
        )
    }

    fn render_shadow_draw(
        &mut self,
        target_view: &wgpu::TextureView,
        shadow: &ShadowDraw,
        width: u32,
        height: u32,
        root_scale: f32,
    ) {
        self.renderer.encode_shadow_draw(
            self.recorder,
            target_view,
            shadow,
            width,
            height,
            root_scale,
        );
    }

    fn composite_to_view_projective(
        &mut self,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        source_size: (f32, f32),
        inverse_matrix: [[f32; 3]; 3],
        dest_bounds: [[f32; 2]; 4],
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
    ) {
        let device = self.renderer.device.clone();
        let composited = {
            self.renderer
                .effect_renderer
                .encode_composite_to_view_projective(
                    self.recorder,
                    &device,
                    source,
                    dest_view,
                    viewport,
                    source_size,
                    inverse_matrix,
                    dest_bounds,
                    alpha,
                    load_op,
                    scissor,
                    supported_blend_mode(blend_mode),
                    sample_mode,
                )
        };
        if composited {
            self.recorder.record_pass();
            self.renderer.effect_renderer.record_composite_pass();
        }
    }

    fn composite_projective_surfaces_to_view(
        &mut self,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        composites: &[ProjectiveSurfaceComposite<'_>],
    ) {
        let device = self.renderer.device.clone();
        let mut composite_count = 0_u32;
        for composite in composites
            .iter()
            .copied()
            .filter(|composite| projective_dest_bounds_rect(composite.dest_bounds).is_some())
        {
            let composited = {
                self.renderer
                    .effect_renderer
                    .encode_composite_to_view_projective(
                        self.recorder,
                        &device,
                        composite.source,
                        dest_view,
                        viewport,
                        composite.source_size,
                        composite.inverse_matrix,
                        composite.dest_bounds,
                        composite.alpha,
                        composite.load_op,
                        composite.scissor,
                        supported_blend_mode(composite.blend_mode),
                        composite.sample_mode,
                    )
            };
            if composited {
                composite_count = composite_count.saturating_add(1);
            }
        }
        if composite_count > 0 {
            self.recorder.record_passes(composite_count);
            self.renderer
                .effect_renderer
                .debug_composites
                .set(self.renderer.effect_renderer.debug_composites.get() + composite_count);
        }
    }

    fn composite_surface_batch_to_view(
        &mut self,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        load_op: wgpu::LoadOp<wgpu::Color>,
        composites: &[CompositeBatchItem<'_>],
    ) {
        if composites.is_empty() {
            return;
        }
        let device = self.renderer.device.clone();
        self.renderer
            .effect_renderer
            .encode_composite_batch_to_view_pass(
                self.recorder,
                &device,
                dest_view,
                viewport,
                load_op,
                composites,
            );
        self.recorder.record_pass();
        self.renderer.effect_renderer.record_composite_pass();
    }

    fn copy_texture_region_to_target(
        &mut self,
        source: &OffscreenTarget,
        source_origin: (u32, u32),
        target: &OffscreenTarget,
        size: (u32, u32),
    ) -> bool {
        let (width, height) = size;
        if width == 0 || height == 0 || width > target.width || height > target.height {
            return false;
        }
        let Some(source_right) = source_origin.0.checked_add(width) else {
            return false;
        };
        let Some(source_bottom) = source_origin.1.checked_add(height) else {
            return false;
        };
        if source_right > source.width || source_bottom > source.height {
            return false;
        }

        self.recorder.encoder().copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: source.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: source_origin.0,
                    y: source_origin.1,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: target.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        true
    }

    fn shader_composite_batch_to_view(
        &mut self,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        load_op: wgpu::LoadOp<wgpu::Color>,
        composites: &[ShaderCompositeBatchItem<'_>],
    ) -> bool {
        if composites.is_empty() {
            return true;
        }
        let device = self.renderer.device.clone();
        let encoded = self
            .renderer
            .effect_renderer
            .encode_shader_batch_src_over_to_view(
                self.recorder,
                &device,
                dest_view,
                viewport,
                load_op,
                composites,
            );
        if encoded {
            self.recorder.record_pass();
            self.renderer.effect_renderer.record_composite_pass();
            self.renderer
                .effect_renderer
                .debug_effects
                .set(self.renderer.effect_renderer.debug_effects.get() + composites.len() as u32);
        }
        encoded
    }

    fn composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
        &mut self,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        rounded_mask: Option<RoundedCompositeMask>,
        blend_mode: BlendMode,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        sample_mode: CompositeSampleMode,
    ) {
        let device = self.renderer.device.clone();
        {
            self.renderer
                .effect_renderer
                .encode_composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
                    self.recorder,
                    &device,
                    source,
                    dest_view,
                    alpha,
                    load_op,
                    scissor,
                    rounded_mask,
                    supported_blend_mode(blend_mode),
                    dest_viewport,
                    sample_mode,
                );
        }
        self.recorder.record_pass();
        self.renderer.effect_renderer.record_composite_pass();
    }

    fn apply_effect_and_composite_to_view(
        &mut self,
        source: &OffscreenTarget,
        effect: &RenderEffect,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        sample_mode: CompositeSampleMode,
    ) -> Result<(), String> {
        self.record_effect_composite(
            source,
            effect,
            effect_rect,
            dest_view,
            alpha,
            load_op,
            scissor,
            blend_mode,
            dest_viewport,
            sample_mode,
        )
    }

    fn apply_shader_and_composite_to_view(
        &mut self,
        source: &OffscreenTarget,
        shader: &RuntimeShader,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        sample_mode: CompositeSampleMode,
    ) {
        self.record_shader_composite(
            source,
            shader,
            effect_rect,
            dest_view,
            alpha,
            load_op,
            scissor,
            blend_mode,
            dest_viewport,
            sample_mode,
        );
    }

    fn apply_shader_and_composite_to_view_projective(
        &mut self,
        source: &OffscreenTarget,
        shader: &RuntimeShader,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        source_size: (f32, f32),
        inverse_matrix: [[f32; 3]; 3],
        dest_bounds: [[f32; 2]; 4],
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
    ) {
        self.record_shader_projective_composite(
            source,
            shader,
            effect_rect,
            dest_view,
            viewport,
            source_size,
            inverse_matrix,
            dest_bounds,
            alpha,
            load_op,
            scissor,
            blend_mode,
            sample_mode,
        );
    }

    fn apply_effect_and_composite_to_view_projective(
        &mut self,
        source: &OffscreenTarget,
        effect: &RenderEffect,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        source_size: (f32, f32),
        inverse_matrix: [[f32; 3]; 3],
        dest_bounds: [[f32; 2]; 4],
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
    ) -> Result<(), String> {
        self.record_effect_projective_composite(
            source,
            effect,
            effect_rect,
            dest_view,
            viewport,
            source_size,
            inverse_matrix,
            dest_bounds,
            alpha,
            load_op,
            scissor,
            blend_mode,
            sample_mode,
        )
    }

    fn is_render_effect_supported(&self, effect: &RenderEffect) -> bool {
        self.renderer.supports_render_effect(effect)
    }

    fn warn_unsupported_effect_once(&self) {
        self.renderer.warning_state.warn_unsupported_effect_once();
    }

    fn record_layer_cache_miss(&self, width: u32, height: u32) {
        self.renderer
            .frame_stats
            .record_layer_cache_miss(width, height);
    }

    fn record_isolated_layer_render(
        &self,
        width: u32,
        height: u32,
        node_id: Option<NodeId>,
        logical_rect: Rect,
        requirements: SurfaceRequirementSet,
    ) {
        self.renderer.frame_stats.record_isolated_layer_render(
            width,
            height,
            node_id,
            logical_rect,
            requirements.into(),
        );
    }
}

impl GpuRenderer {
    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        packet: FramePacket,
        surface_epoch: u64,
        returns: &mut RenderReturns,
    ) -> Result<(), String> {
        // Packet validity gate — BEFORE consume_replay_ops and any
        // encoding. A packet built against another renderer instance,
        // another surface configuration, or another viewport is cancelled
        // whole: its buffers travel back through `returns` for re-queue
        // and recycling, and nothing of it reaches the GPU.
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
        returns.frame_id = packet.frame_id;
        log::trace!("🎨 Rendering graph to {}x{}", width, height);
        let render_start = Instant::now();

        #[cfg(target_arch = "wasm32")]
        {
            self.wasm_uniform_batch_cursor = 0;
            self.wasm_shape_batch_cursor = 0;
            self.wasm_image_batch_cursor = 0;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.retained_glyph_uniform_cursor = 0;
        }

        // Producer-side text layout cache size, carried by the packet — the
        // present call tree holds no text layout state, and no layout runs
        // between packet build and the stats block below.
        let text_cache_len = packet.text_cache_len;
        let result = self.render_graph(view, packet, returns);
        let after_graph = Instant::now();
        self.flush_deferred_offscreen_releases();

        #[cfg(target_arch = "wasm32")]
        {
            const WASM_BATCH_POOL_MARGIN: usize = 4;
            self.wasm_uniform_batches.truncate(
                self.wasm_uniform_batch_cursor
                    .saturating_add(WASM_BATCH_POOL_MARGIN),
            );
            self.wasm_shape_batches.truncate(
                self.wasm_shape_batch_cursor
                    .saturating_add(WASM_BATCH_POOL_MARGIN),
            );
            self.wasm_image_batches.truncate(
                self.wasm_image_batch_cursor
                    .saturating_add(WASM_BATCH_POOL_MARGIN),
            );
        }
        self.staged_uploads
            .shrink_retained_capacity(RETAINED_STAGED_UPLOAD_BYTES, RETAINED_STAGED_UPLOAD_COPIES);

        self.layer_surface_cache.finish_frame(&self.frame_stats);
        #[cfg(not(target_arch = "wasm32"))]
        self.retained_bundle_cache.end_frame();

        self.frame_stats.offscreen_pool_size.set(
            self.effect_renderer
                .retained_offscreen_count()
                .saturating_add(self.frame_graph_executor.retained_texture_count())
                as u32,
        );
        self.frame_stats.offscreen_pool_bytes.set(
            (self.effect_renderer.retained_offscreen_bytes() as u64)
                .saturating_add(self.frame_graph_executor.retained_texture_bytes()),
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
        self.last_frame_stats = Some(snapshot);
        PRESENTED_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        update_frame_warmup_budget(&mut self.pending_frame_warmup_frames, &snapshot);
        self.frame_stats.maybe_print_snapshot(
            snapshot,
            &mut self.frame_count,
            self.gpu_stats_enabled,
        );
        if self.gpu_stats_enabled && self.frame_count.is_multiple_of(60) {
            gpu_stats::print_gpu_memory_report(&self.device, self.frame_count);
        }
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
            // Only a draw that actually ran may report `Presented`; an
            // errored draw leaves the default `NotRun`.
            returns.outcome = PresentOutcome::Presented;
        }
        result
    }

    /// Refuses a packet whole, before any encoding: every buffer it
    /// carries travels back through `returns` — the direct scene for the
    /// producer pool, the unconsumed replay plan for the planner to
    /// re-queue (its releases name still-live store slots; dropping them
    /// would leak pool ids forever). A cancel is a protocol outcome, not a
    /// draw error, so the render call returns `Ok(())`.
    fn cancel_packet(
        packet: FramePacket,
        reason: CancelReason,
        returns: &mut RenderReturns,
    ) -> Result<(), String> {
        let FramePacket {
            frame_id,
            viewport: _,
            renderer_epoch: _,
            surface_epoch: _,
            root_scale: _,
            root,
            overlay: _,
            replay,
            text_cache_len: _,
        } = packet;
        match root {
            PacketRoot::Direct(root) => {
                // Destructure: the scene buffers return to the producer
                // pool; the rest of the collected layer drops. A Direct
                // packet's replay plan came from the planner and must go
                // back to it unconsumed — a Surface packet only ever
                // carries the empty default plan, which has nothing to
                // reclaim.
                returns.scene = Some(root.scene);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    returns.cancelled_replay = Some(replay);
                }
            }
            PacketRoot::Surface(_) => {}
        }
        #[cfg(target_arch = "wasm32")]
        let _ = replay;
        returns.ack = None;
        returns.frame_id = frame_id;
        returns.outcome = PresentOutcome::Cancelled(reason);
        Ok(())
    }

    pub fn last_frame_stats(&self) -> Option<gpu_stats::FrameStatsSnapshot> {
        self.last_frame_stats
    }

    pub fn needs_frame_warmup(&self) -> bool {
        self.pending_frame_warmup_frames > 0
    }

    pub fn debug_cpu_allocation_stats(&self) -> DebugCpuAllocationStats {
        let layer_surface_cache_stats = self.layer_surface_cache.debug_stats();
        DebugCpuAllocationStats {
            scene_graph_node_count: 0,
            scene_graph_heap_bytes: 0,
            scene_hits_len: 0,
            scene_hits_cap: 0,
            scene_node_index_len: 0,
            scene_node_index_cap: 0,
            text_renderer_pool_len: self.text_image_cache.len(),
            text_renderer_pool_cap: self.text_image_cache.cap().get(),
            swash_image_cache_len: 0,
            swash_image_cache_cap: 0,
            swash_outline_cache_len: 0,
            swash_outline_cache_cap: 0,
            image_texture_cache_len: self.image_texture_cache.len(),
            image_texture_cache_cap: self.image_texture_cache.cap().get(),
            scratch_shape_data_cap: self.scratch_shape_data.capacity(),
            scratch_gradients_cap: self.scratch_gradients.capacity(),
            scratch_image_vertices_cap: self.scratch_image_vertices.capacity(),
            scratch_image_indices_cap: self.scratch_image_indices.capacity(),
            scratch_image_cmds_cap: self.scratch_image_cmds.capacity(),
            scratch_segment_items_cap: self.scratch_segment_items.capacity(),
            scratch_effect_ranges_cap: self.scratch_effect_ranges.capacity(),
            scratch_layer_events_cap: self.scratch_layer_events.capacity(),
            staged_upload_bytes_cap: self.staged_uploads.bytes.capacity(),
            staged_upload_copies_cap: self.staged_uploads.copies.capacity(),
            layer_surface_cache_len: layer_surface_cache_stats.entries_len,
            layer_surface_cache_cap: layer_surface_cache_stats.entries_cap,
            layer_surface_cache_identity_len: layer_surface_cache_stats.identity_len,
            layer_surface_cache_identity_cap: layer_surface_cache_stats.identity_cap,
            // The producer frontend owns the only lowering-memo pair since
            // step 6b; the present backend contributes nothing.
            layer_surface_rect_cache_len: 0,
            layer_surface_rect_cache_cap: 0,
            layer_surface_requirements_cache_len: 0,
            layer_surface_requirements_cache_cap: 0,
            layer_cache_seen_this_frame_len: layer_surface_cache_stats.seen_this_frame_len,
            layer_cache_seen_this_frame_cap: layer_surface_cache_stats.seen_this_frame_cap,
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

        let output_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Screenshot Output Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());

        self.render(&output_view, width, height, packet, surface_epoch, returns)?;

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

        self.convert_surface_pixels_to_rgba(&mut pixels)?;
        Ok(pixels)
    }

    fn render_graph(
        &mut self,
        surface_view: &wgpu::TextureView,
        packet: FramePacket,
        returns: &mut RenderReturns,
    ) -> Result<(), String> {
        let device = self.device.clone();
        let queue = self.queue.clone();
        let graph_start = Instant::now();

        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut executor = std::mem::take(&mut self.frame_graph_executor);
            let mut frame_graph = WgpuFrameGraph::new(Some("Renderer Frame Graph"));
            let surface = frame_graph.import_surface("renderer-surface");
            frame_graph.add_fallible_recorded_command_pass(
                Some("Renderer Frame Pass"),
                &[],
                &[surface],
                |frame_encoder| {
                    self.render_graph_recorded(surface_view, packet, returns, frame_encoder)
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
                    Ok(())
                }
                Err(crate::frame_graph::FrameGraphError::NoDeclaredPasses) => Ok(()),
                Err(error) => Err(error.to_string()),
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let mut executor = std::mem::take(&mut self.frame_graph_executor);
            let (result, execution) = {
                let mut frame_encoder =
                    executor.begin(&device, &queue, Some("Renderer Frame Encoder"));
                let initial_pass_count = frame_encoder.recorded_pass_count();
                let result =
                    self.render_graph_recorded(surface_view, packet, returns, &mut frame_encoder);
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
            if let Some(execution) = execution {
                self.frame_stats.record_command_stats(execution.stats);
            }
            result
        }
    }

    fn render_graph_recorded<C: FrameCommandRecorder>(
        &mut self,
        surface_view: &wgpu::TextureView,
        packet: FramePacket,
        returns: &mut RenderReturns,
        frame_encoder: &mut C,
    ) -> Result<(), String> {
        let recorded_start = Instant::now();

        // Present-side consumption of the packet's replay plan, adjacent to
        // packet consumption: the store honors the ops just before the
        // packet renders. Gated on a Direct root — a Surface packet never
        // touched the planner and carries the empty default plan
        // (generation 0), which the store must not consume: it would count
        // a false generation drop. The ack travels back through `returns`
        // and the producer applies it right after this render call —
        // equivalent to the in-store drain this replaces, because both
        // application points sit after this frame's graph build and before
        // the next collect, which is where the bypass gate and `feed_slots`
        // are read.
        #[cfg(not(target_arch = "wasm32"))]
        let mut packet = packet;
        #[cfg(not(target_arch = "wasm32"))]
        if let PacketRoot::Direct(root) = &packet.root {
            let ops = std::mem::take(&mut packet.replay);
            let (ack, recycled) =
                self.consume_replay_ops(ops, &root.scene.shapes, packet.root_scale);
            returns.ack = Some((ack, recycled));
        }

        let FramePacket {
            frame_id,
            viewport: (width, height),
            renderer_epoch: _,
            surface_epoch: _,
            root_scale,
            root,
            overlay,
            replay: _,
            text_cache_len: _,
        } = packet;

        let mut backend = RecordingSurfaceBackend {
            renderer: self,
            recorder: frame_encoder,
        };

        let surface_packet = match root {
            PacketRoot::Direct(root) => {
                let direct_render_start = Instant::now();
                let result = match execute_render_root_direct(
                    &mut backend,
                    surface_view,
                    *root,
                    width,
                    height,
                    root_scale,
                    wgpu::LoadOp::Clear(CLEAR_COLOR),
                ) {
                    // Return the packet's scene buffers to the producer pool
                    // in BOTH arms — for a heavy animated frame they are
                    // megabytes of Vec, and an errored draw must not leak
                    // them.
                    Ok(scene) => {
                        returns.scene = Some(scene);
                        Ok(())
                    }
                    Err((error, scene)) => {
                        returns.scene = Some(scene);
                        Err(error)
                    }
                };
                if result.is_ok() {
                    if let Some(overlay) = overlay {
                        Self::render_overlay_packet(
                            &mut backend,
                            surface_view,
                            overlay,
                            width,
                            height,
                            root_scale,
                        )?;
                    }
                }
                let after_direct_render = Instant::now();
                if let Some(total_ms) =
                    should_log_wgpu_render_stage(recorded_start, after_direct_render)
                {
                    log::warn!(
                        "[wgpu-render-stage:recorded-direct-root] frame={frame_id} total_ms={total_ms:.2} render_ms={:.2}",
                        instant_ms(direct_render_start, after_direct_render),
                    );
                }
                return result;
            }
            PacketRoot::Surface(surface_packet) => surface_packet,
        };
        let after_root_collect = Instant::now();

        let RootSurfacePacket {
            lowered,
            source,
            transform_to_parent,
            node_id,
            backdrop,
            graphics_layer,
            local_bounds,
            clip_rect,
            shadow_clip,
        } = *surface_packet;
        let mut lowered = lowered;
        lowered.source = source;

        // The root layer's visible area is always the viewport — content
        // outside the screen is invisible regardless of scroll offsets or
        // inflated scene bounds.  Pass the viewport rect as an explicit
        // surface rect to prevent offscreen inflation on constrained GPUs.
        let viewport_rect = Rect {
            x: 0.0,
            y: 0.0,
            width: width as f32 / root_scale,
            height: height as f32 / root_scale,
        };
        let root_surface = execute_render_layer_surface(
            &mut backend,
            &mut lowered,
            LayerSurfaceRequest {
                root_scale,
                backdrop_underlay: None,
                allow_runtime_cache: false,
                logical_rect_override: Some(viewport_rect),
                capture_clip_override: None,
                activates_nested_capture: false,
                translation_context: TranslationRenderContext::default(),
            },
        )?;
        let root_quad = transform_to_parent.map_rect(root_surface.logical_rect);
        let root_dest_quad = scaled_quad(root_quad, root_scale);

        let needs_root_composite_target =
            backdrop.is_some() || graphics_layer.shadow_elevation > 0.0;

        if needs_root_composite_target {
            let composite_target = backend.acquire_frame_surface(width, height);
            backend.clear_target_view_with_load_op(
                &composite_target.view,
                wgpu::LoadOp::Clear(CLEAR_COLOR),
            );

            if let Some(backdrop) = &backdrop {
                execute_apply_backdrop_layer_to_target(
                    &mut backend,
                    &composite_target,
                    &BackdropLayer {
                        node_id,
                        rect: quad_bounds(transform_to_parent.map_rect(local_bounds)),
                        clip: clip_rect.map(|clip| quad_bounds(transform_to_parent.map_rect(clip))),
                        snap_anchor: None,
                        effect: backdrop.clone(),
                        z_index: 0,
                    },
                    None,
                    width,
                    height,
                    root_scale,
                    None,
                )?;
            }

            let mut root_shadow_scene = CompositorScene::new();
            let root_shadow_clip =
                shadow_clip.map(|clip| quad_bounds(transform_to_parent.map_rect(clip)));
            push_layer_shadow(
                &mut root_shadow_scene,
                &graphics_layer,
                local_bounds,
                quad_bounds(transform_to_parent.map_rect(local_bounds)),
                root_shadow_clip,
            );
            for shadow in &root_shadow_scene.shadow_draws {
                backend.render_shadow_draw(
                    &composite_target.view,
                    shadow,
                    width,
                    height,
                    root_scale,
                );
            }

            let composite_dest_quad =
                snap_motion_stable_dest_quad(root_dest_quad, root_surface.sample_mode);
            execute_composite_surface_to_view(
                &mut backend,
                root_surface.target.target(),
                &composite_target.view,
                (width, height),
                composite_dest_quad,
                root_surface.composite_alpha,
                wgpu::LoadOp::Load,
                None,
                root_surface.blend_mode,
                root_surface.sample_mode,
            )?;
            backend.composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
                &composite_target,
                surface_view,
                1.0,
                wgpu::LoadOp::Clear(CLEAR_COLOR),
                None,
                None,
                BlendMode::SrcOver,
                None,
                CompositeSampleMode::Linear,
            );
            backend.release_frame_surface(composite_target);
        } else {
            let composite_dest_quad =
                snap_motion_stable_dest_quad(root_dest_quad, root_surface.sample_mode);
            execute_composite_surface_to_view(
                &mut backend,
                root_surface.target.target(),
                surface_view,
                (width, height),
                composite_dest_quad,
                root_surface.composite_alpha,
                wgpu::LoadOp::Clear(CLEAR_COLOR),
                None,
                root_surface.blend_mode,
                root_surface.sample_mode,
            )?;
        }
        backend.release_layer_surface_target(root_surface.target);
        if let Some(overlay) = overlay {
            Self::render_overlay_packet(
                &mut backend,
                surface_view,
                overlay,
                width,
                height,
                root_scale,
            )?;
        }
        let after_layer_render = Instant::now();
        if let Some(total_ms) = should_log_wgpu_render_stage(recorded_start, after_layer_render) {
            log::warn!(
                "[wgpu-render-stage:recorded-layer-root] total_ms={total_ms:.2} collect_ms={:.2} render_ms={:.2}",
                instant_ms(recorded_start, after_root_collect),
                instant_ms(after_root_collect, after_layer_render),
            );
        }
        Ok(())
    }

    /// Renders the producer-lowered dev overlay on top of the frame. The
    /// packet carries the collected overlay; the backend only validates
    /// that it stayed directly renderable and draws it.
    fn render_overlay_packet<C: FrameCommandRecorder>(
        backend: &mut RecordingSurfaceBackend<'_, '_, C>,
        surface_view: &wgpu::TextureView,
        overlay: CollectedLayer,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<(), String> {
        if !overlay.child_layers.is_empty()
            || !root_direct_scene_events_are_supported(&overlay.scene)
            || !direct_root_child_underlays_are_supported(&overlay)
        {
            return Err("dev overlay graph must stay directly renderable".to_string());
        }
        execute_render_root_direct(
            backend,
            surface_view,
            overlay,
            width,
            height,
            root_scale,
            wgpu::LoadOp::Load,
        )
        .map(|_overlay_scene| ())
        .map_err(|(error, _overlay_scene)| error)
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_non_effect_segment_commands<C: FrameCommandRecorder>(
        &mut self,
        frame_encoder: &mut C,
        target_view: &wgpu::TextureView,
        ordered_items: &[(usize, SegmentDrawItem)],
        composites: &[(usize, CompositeBatchItem<'_>)],
        shader_composites: &[(usize, ShaderCompositeBatchItem<'_>)],
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        retained_draws: &[RetainedDraw],
        initial_load_op: wgpu::LoadOp<wgpu::Color>,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<SegmentCommandEncodeOutcome, String> {
        let mut first_batch = true;
        for command in
            SegmentCommandIter::new(ordered_items, shapes, images, self.shape_batch_limits)
        {
            match command {
                SegmentRenderCommand::DrawChunk(chunk) => {
                    let load_op = if first_batch {
                        initial_load_op
                    } else {
                        wgpu::LoadOp::Load
                    };
                    let outcome = self.render_segment_draw_chunk(
                        frame_encoder,
                        target_view,
                        ordered_items,
                        composites,
                        shader_composites,
                        shapes,
                        images,
                        texts,
                        retained_draws,
                        chunk,
                        width,
                        height,
                        root_scale,
                        load_op,
                    )?;
                    if outcome.rendered_any {
                        frame_encoder.record_passes(outcome.pass_count);
                        first_batch = false;
                    }
                }
                SegmentRenderCommand::Shadow(index) => {
                    if first_batch && matches!(initial_load_op, wgpu::LoadOp::Clear(_)) {
                        {
                            let _clear = frame_encoder.encoder().begin_render_pass(
                                &wgpu::RenderPassDescriptor {
                                    label: Some("Shadow Pre-Clear"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: target_view,
                                        resolve_target: None,
                                        depth_slice: None,
                                        ops: wgpu::Operations {
                                            load: initial_load_op,
                                            store: wgpu::StoreOp::Store,
                                        },
                                    })],
                                    depth_stencil_attachment: None,
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                    multiview_mask: None,
                                },
                            );
                        }
                        frame_encoder.record_pass();
                        first_batch = false;
                    }
                    let pass_count_before = frame_encoder.recorded_pass_count();
                    self.encode_shadow_draw(
                        frame_encoder,
                        target_view,
                        &shadow_draws[index],
                        width,
                        height,
                        root_scale,
                    );
                    if frame_encoder.recorded_pass_count() > pass_count_before {
                        first_batch = false;
                    }
                }
            }
        }
        Ok(SegmentCommandEncodeOutcome { first_batch })
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    fn render_segment_draw_chunk_fused_native<C: FrameCommandRecorder>(
        &mut self,
        frame_encoder: &mut C,
        target_view: &wgpu::TextureView,
        ordered_items: &[(usize, SegmentDrawItem)],
        composites: &[(usize, CompositeBatchItem<'_>)],
        shader_composites: &[(usize, ShaderCompositeBatchItem<'_>)],
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        retained_draws: &[RetainedDraw],
        chunk: &SegmentDrawChunkPlan,
        width: u32,
        height: u32,
        root_scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<Option<SegmentRenderOutcome>, String> {
        let Some(partitions) = native_segment_fusion_partitions(
            ordered_items,
            shapes,
            chunk,
            self.shape_batch_limits,
        )?
        else {
            return Ok(None);
        };

        let mut rendered_any = false;
        let mut pass_count = 0_u32;
        let mut next_load_op = load_op;
        for partition in partitions {
            let outcome = self.render_segment_draw_chunk_fused_native_partition(
                frame_encoder,
                target_view,
                ordered_items,
                composites,
                shader_composites,
                shapes,
                images,
                texts,
                retained_draws,
                &partition.chunk,
                partition.budget,
                width,
                height,
                root_scale,
                next_load_op,
            )?;
            if outcome.rendered_any {
                rendered_any = true;
                pass_count = pass_count.saturating_add(outcome.pass_count);
                next_load_op = wgpu::LoadOp::Load;
            }
        }

        Ok(Some(SegmentRenderOutcome {
            rendered_any,
            pass_count,
        }))
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    fn render_segment_draw_chunk_fused_native_partition<C: FrameCommandRecorder>(
        &mut self,
        frame_encoder: &mut C,
        target_view: &wgpu::TextureView,
        ordered_items: &[(usize, SegmentDrawItem)],
        composites: &[(usize, CompositeBatchItem<'_>)],
        shader_composites: &[(usize, ShaderCompositeBatchItem<'_>)],
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        retained_draws: &[RetainedDraw],
        chunk: &SegmentDrawChunkPlan,
        budget: NativeSegmentFusionBudget,
        width: u32,
        height: u32,
        root_scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<SegmentRenderOutcome, String> {
        let partition_start = Instant::now();
        let mut staged_uploads = self.take_staged_uploads();
        staged_uploads.clear();
        let mut image_vertices = std::mem::take(&mut self.scratch_image_vertices);
        let mut image_indices = std::mem::take(&mut self.scratch_image_indices);
        let mut image_cmds = std::mem::take(&mut self.scratch_image_cmds);
        let mut glyph_cmds = std::mem::take(&mut self.scratch_glyph_cmds);

        image_vertices.clear();
        image_indices.clear();
        image_cmds.clear();
        glyph_cmds.clear();

        let result = (|| {
            let viewport = ViewportUniformParams {
                width,
                height,
                offset: [0.0, 0.0],
            };
            self.prewarm_offscreen_text_glyph_draws_in_chunk(
                ordered_items,
                texts,
                chunk,
                viewport,
                root_scale,
                &mut staged_uploads,
                &mut image_vertices,
                &mut image_indices,
                &mut glyph_cmds,
            )?;
            let mut shape_refs = Vec::with_capacity(budget.shape_count);
            for batch in chunk.iter() {
                let SegmentBatchPlan::Shape { start, end, .. } = batch else {
                    continue;
                };
                for (_, item) in &ordered_items[start..end] {
                    let SegmentDrawItem::Shape(shape_index) = item else {
                        return Err(format!(
                            "shape batch contains non-shape draw item: {item:?}"
                        ));
                    };
                    shape_refs.push(&shapes[*shape_index]);
                }
            }
            let after_shape_refs = Instant::now();

            let mut direct_shape_uploads = StagedBufferUploads::default();
            let mut shape_upload_base = 0u64;
            if !shape_refs.is_empty() {
                let Some((_, upload_base)) = self.prepare_shapes_batch_direct(
                    frame_encoder,
                    shape_refs.iter().copied(),
                    root_scale,
                    viewport,
                    &mut direct_shape_uploads,
                ) else {
                    return Err(
                        "native fused segment shape preparation produced no draw batch".to_string(),
                    );
                };
                shape_upload_base = upload_base;
            }
            let after_shape_prepare = Instant::now();

            let mut fused_batches = Vec::with_capacity(chunk.batches.len());
            let mut shape_cursor = 0_u32;
            let mut composite_cursor = 0usize;
            let mut shader_composite_cursor = 0usize;
            for batch in chunk.iter() {
                match batch {
                    SegmentBatchPlan::Shape {
                        start,
                        end,
                        blend_mode,
                    } => {
                        for (_, item) in &ordered_items[start..end] {
                            if !matches!(item, SegmentDrawItem::Shape(_)) {
                                return Err(format!(
                                    "shape batch contains non-shape draw item: {item:?}"
                                ));
                            }
                        }
                        let shape_count = end - start;
                        if shape_count > 0 {
                            fused_batches.push(FusedSegmentBatch::Shape {
                                batch: PreparedShapeBatch {
                                    vertex_start: shape_cursor * 6,
                                    vertex_count: shape_count as u32 * 6,
                                },
                                blend_mode,
                            });
                            shape_cursor += shape_count as u32;
                        }
                    }
                    SegmentBatchPlan::Image {
                        start,
                        end,
                        blend_mode,
                    } => {
                        let cmd_start = image_cmds.len();
                        for (_, item) in &ordered_items[start..end] {
                            let SegmentDrawItem::Image(image_index) = item else {
                                return Err(format!(
                                    "image batch contains non-image draw item: {item:?}"
                                ));
                            };
                            self.append_image_draw_cmd(
                                &images[*image_index],
                                viewport,
                                root_scale,
                                &mut image_vertices,
                                &mut image_indices,
                                &mut image_cmds,
                            )?;
                        }
                        let cmd_end = image_cmds.len();
                        if cmd_start < cmd_end {
                            fused_batches.push(FusedSegmentBatch::Image {
                                cmd_range: cmd_start..cmd_end,
                                blend_mode,
                            });
                        }
                    }
                    SegmentBatchPlan::Text { start, end } => {
                        let glyph_cmd_start = glyph_cmds.len();
                        let image_cmd_start = image_cmds.len();
                        let text_draws =
                            text_draws_for_ordered_range(ordered_items, texts, start, end)?;
                        if !self.append_text_glyph_draws(
                            text_draws,
                            viewport,
                            root_scale,
                            false,
                            &mut staged_uploads,
                            &mut image_vertices,
                            &mut image_indices,
                            &mut glyph_cmds,
                        )? {
                            let text_draws =
                                text_draws_for_ordered_range(ordered_items, texts, start, end)?;
                            self.append_text_image_draw_cmds(
                                text_draws,
                                viewport,
                                root_scale,
                                &mut image_vertices,
                                &mut image_indices,
                                &mut image_cmds,
                            )?;
                        }
                        let image_cmd_end = image_cmds.len();
                        let glyph_cmd_end = glyph_cmds.len();
                        if image_cmd_start < image_cmd_end || glyph_cmd_start < glyph_cmd_end {
                            fused_batches.push(FusedSegmentBatch::Text {
                                image_cmd_range: image_cmd_start..image_cmd_end,
                                glyph_cmd_range: glyph_cmd_start..glyph_cmd_end,
                            });
                        }
                    }
                    SegmentBatchPlan::Composite { start, end } => {
                        for (_, item) in &ordered_items[start..end] {
                            if !matches!(item, SegmentDrawItem::Composite(_)) {
                                return Err(format!(
                                    "composite batch contains non-composite draw item: {item:?}"
                                ));
                            }
                        }
                        let draw_count = end - start;
                        if draw_count > 0 {
                            let draw_start = composite_cursor;
                            composite_cursor += draw_count;
                            fused_batches.push(FusedSegmentBatch::Composite {
                                draw_range: draw_start..composite_cursor,
                            });
                        }
                    }
                    SegmentBatchPlan::ShaderComposite { start, end } => {
                        for (_, item) in &ordered_items[start..end] {
                            if !matches!(item, SegmentDrawItem::ShaderComposite(_)) {
                                return Err(format!(
                                    "shader composite batch contains non-shader-composite draw item: {item:?}"
                                ));
                            }
                        }
                        let draw_count = end - start;
                        if draw_count > 0 {
                            let draw_start = shader_composite_cursor;
                            shader_composite_cursor += draw_count;
                            fused_batches.push(FusedSegmentBatch::ShaderComposite {
                                draw_range: draw_start..shader_composite_cursor,
                            });
                        }
                    }
                    SegmentBatchPlan::Retained { start, end } => {
                        self.stage_replay_patches(&mut staged_uploads);
                        for (_, item) in &ordered_items[start..end] {
                            let SegmentDrawItem::Retained(index) = item else {
                                return Err(format!(
                                    "retained batch contains non-retained draw item: {item:?}"
                                ));
                            };
                            let retained = retained_draws.get(*index).ok_or_else(|| {
                                format!("retained draw index {index} out of bounds")
                            })?;
                            if (*index as u32) < MAX_REPLAY_SLOTS
                                && self.replay_slots.slots.contains_key(&retained.slot)
                            {
                                let transform = retained.transform.with_retained_paint();
                                staged_uploads.stage_at(
                                    UploadTarget::ReplayTransform,
                                    *index as u64 * REPLAY_TRANSFORM_STRIDE,
                                    bytemuck::bytes_of(&transform),
                                );
                            }
                        }
                        if end > start {
                            fused_batches.push(FusedSegmentBatch::Retained {
                                item_range: start..end,
                            });
                        }
                    }
                }
            }
            let after_batch_prepare = Instant::now();

            if !image_indices.is_empty() {
                self.stage_native_image_buffers(
                    &mut staged_uploads,
                    viewport,
                    &image_vertices,
                    &image_indices,
                );
            }

            let device = self.device.clone();
            let composite_items: Vec<_> = chunk
                .iter()
                .filter_map(|batch| match batch {
                    SegmentBatchPlan::Composite { start, end } => Some((start, end)),
                    _ => None,
                })
                .flat_map(|(start, end)| {
                    ordered_items[start..end].iter().filter_map(|(_, item)| {
                        let SegmentDrawItem::Composite(composite_index) = item else {
                            return None;
                        };
                        composites
                            .get(*composite_index)
                            .map(|(_, composite)| *composite)
                    })
                })
                .collect();
            let prepared_composites = self.effect_renderer.prepare_composite_batch_draws(
                frame_encoder,
                &device,
                load_op,
                &composite_items,
            );
            let shader_items: Vec<_> = chunk
                .iter()
                .filter_map(|batch| match batch {
                    SegmentBatchPlan::ShaderComposite { start, end } => Some((start, end)),
                    _ => None,
                })
                .flat_map(|(start, end)| {
                    ordered_items[start..end].iter().filter_map(|(_, item)| {
                        let SegmentDrawItem::ShaderComposite(composite_index) = item else {
                            return None;
                        };
                        shader_composites
                            .get(*composite_index)
                            .map(|(_, composite)| *composite)
                    })
                })
                .collect();
            let prepared_shaders = self
                .effect_renderer
                .prepare_shader_batch_draws(frame_encoder, &device, &shader_items)
                .ok_or_else(|| "shader composite batch preparation failed".to_string())?;
            if !shader_items.is_empty() {
                self.effect_renderer.record_composite_pass();
                self.effect_renderer
                    .debug_effects
                    .set(self.effect_renderer.debug_effects.get() + shader_items.len() as u32);
            }
            let after_composite_prepare = Instant::now();

            if fused_batches.is_empty() {
                return Ok(SegmentRenderOutcome {
                    rendered_any: false,
                    pass_count: 0,
                });
            }

            // The direct shape copies must be recorded before the staged
            // flush: its capacity check may replace `upload_buffer`, and the
            // shape payload was written into the buffer that existed at
            // prepare time. Recording first binds the copies to that buffer.
            self.flush_staged_uploads_at(
                frame_encoder.encoder(),
                &direct_shape_uploads,
                shape_upload_base,
            );
            let upload_offset =
                frame_encoder.allocate_staged_upload_bytes(staged_uploads.bytes.len() as u64);
            self.flush_staged_uploads_at(frame_encoder.encoder(), &staged_uploads, upload_offset);
            let after_upload = Instant::now();

            let use_retained_bundles = retained_bundles_enabled();
            let mut retained_encode_ms = 0.0_f64;
            {
                let mut render_pass =
                    frame_encoder
                        .encoder()
                        .begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("Fused Segment Draw Pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: target_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: load_op,
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });

                for batch in &fused_batches {
                    match batch {
                        FusedSegmentBatch::Shape { batch, blend_mode } => {
                            self.draw_prepared_shapes(
                                &mut render_pass,
                                *blend_mode,
                                *batch,
                                width,
                                height,
                            );
                        }
                        FusedSegmentBatch::Image {
                            cmd_range,
                            blend_mode,
                        } => {
                            self.draw_native_prepared_image_cmd_range(
                                &mut render_pass,
                                &image_cmds,
                                cmd_range.clone(),
                                *blend_mode,
                            )?;
                        }
                        FusedSegmentBatch::Text {
                            image_cmd_range,
                            glyph_cmd_range,
                        } => {
                            if !image_cmd_range.is_empty() {
                                self.draw_native_prepared_image_cmd_range(
                                    &mut render_pass,
                                    &image_cmds,
                                    image_cmd_range.clone(),
                                    BlendMode::SrcOver,
                                )?;
                                self.frame_stats.bump_text();
                            }
                            if !glyph_cmd_range.is_empty() {
                                self.draw_native_prepared_glyph_cmd_range(
                                    &mut render_pass,
                                    &glyph_cmds,
                                    glyph_cmd_range.clone(),
                                )?;
                            }
                        }
                        FusedSegmentBatch::Composite { draw_range } => {
                            for draw in
                                prepared_composites.get(draw_range.clone()).ok_or_else(|| {
                                    "composite draw range is outside the prepared command buffer"
                                        .to_string()
                                })?
                            {
                                self.effect_renderer.draw_prepared_composite(
                                    &mut render_pass,
                                    (width, height),
                                    draw,
                                );
                            }
                        }
                        FusedSegmentBatch::ShaderComposite { draw_range } => {
                            for draw in prepared_shaders.get(draw_range.clone()).ok_or_else(|| {
                                "shader composite draw range is outside the prepared command buffer"
                                    .to_string()
                            })? {
                                self.effect_renderer.draw_prepared_shader_src_over(
                                    &device,
                                    &mut render_pass,
                                    (width, height),
                                    draw,
                                );
                            }
                        }
                        FusedSegmentBatch::Retained { item_range } => {
                            // Each Retained arm is one MAXIMAL consecutive
                            // retained stretch — the planner groups adjacent
                            // retained items into a single batch — so caching
                            // per arm never flattens across the dynamic
                            // batches interleaved at their z positions.
                            let retained_start = Instant::now();
                            if use_retained_bundles {
                                self.draw_retained_stretch_bundled(
                                    &mut render_pass,
                                    ordered_items,
                                    retained_draws,
                                    item_range.clone(),
                                    width,
                                    height,
                                );
                            } else {
                                for (_, item) in &ordered_items[item_range.clone()] {
                                    if let SegmentDrawItem::Retained(index) = item {
                                        if let Some(retained) = retained_draws.get(*index) {
                                            self.draw_retained_batch(
                                                &mut render_pass,
                                                retained,
                                                *index,
                                                width,
                                                height,
                                            );
                                        }
                                    }
                                }
                            }
                            retained_encode_ms += instant_ms(retained_start, Instant::now());
                        }
                    }
                }
            }
            let after_pass = Instant::now();
            if let Some(total_ms) = should_log_wgpu_render_stage(partition_start, after_pass) {
                log::warn!(
                    "[wgpu-render-stage:fused-segment] total_ms={total_ms:.2} shape_refs_ms={:.2} shape_prepare_ms={:.2} batch_prepare_ms={:.2} composite_prepare_ms={:.2} upload_ms={:.2} pass_ms={:.2} retained_encode_ms={retained_encode_ms:.3} batches={} shapes={} image_cmds={} glyph_cmds={} staged_bytes={}",
                    instant_ms(partition_start, after_shape_refs),
                    instant_ms(after_shape_refs, after_shape_prepare),
                    instant_ms(after_shape_prepare, after_batch_prepare),
                    instant_ms(after_batch_prepare, after_composite_prepare),
                    instant_ms(after_composite_prepare, after_upload),
                    instant_ms(after_upload, after_pass),
                    fused_batches.len(),
                    budget.shape_count,
                    image_cmds.len(),
                    glyph_cmds.len(),
                    staged_uploads.bytes.len(),
                );
            }

            Ok(SegmentRenderOutcome {
                rendered_any: true,
                pass_count: 1,
            })
        })();

        self.scratch_image_vertices = image_vertices;
        self.scratch_image_indices = image_indices;
        self.scratch_image_cmds = image_cmds;
        self.scratch_glyph_cmds = glyph_cmds;
        self.restore_staged_uploads(staged_uploads);
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn render_segment_draw_chunk<C: FrameCommandRecorder>(
        &mut self,
        frame_encoder: &mut C,
        target_view: &wgpu::TextureView,
        ordered_items: &[(usize, SegmentDrawItem)],
        composites: &[(usize, CompositeBatchItem<'_>)],
        shader_composites: &[(usize, ShaderCompositeBatchItem<'_>)],
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        retained_draws: &[RetainedDraw],
        chunk: SegmentDrawChunkPlan,
        width: u32,
        height: u32,
        root_scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<SegmentRenderOutcome, String> {
        #[cfg(target_arch = "wasm32")]
        let _ = retained_draws;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(outcome) = self.render_segment_draw_chunk_fused_native(
            frame_encoder,
            target_view,
            ordered_items,
            composites,
            shader_composites,
            shapes,
            images,
            texts,
            retained_draws,
            &chunk,
            width,
            height,
            root_scale,
            load_op,
        )? {
            return Ok(outcome);
        }

        let mut staged_uploads = self.take_staged_uploads();
        let result = (|| {
            let mut rendered_any = false;
            let mut pass_count = 0_u32;
            let mut next_load_op = load_op;
            for batch in chunk.iter() {
                staged_uploads.clear();
                match batch {
                    SegmentBatchPlan::Shape {
                        start,
                        end,
                        blend_mode,
                    } => {
                        let slice = &ordered_items[start..end];
                        if slice.len() > self.shape_batch_limits.max_shapes_per_batch {
                            return Err(format!(
                                "shape batch contains {} shapes, exceeding the renderer limit of {}",
                                slice.len(),
                                self.shape_batch_limits.max_shapes_per_batch
                            ));
                        }
                        let viewport = ViewportUniformParams {
                            width,
                            height,
                            offset: [0.0, 0.0],
                        };
                        for (_, item) in slice {
                            if !matches!(item, SegmentDrawItem::Shape(_)) {
                                return Err(format!(
                                    "shape batch contains non-shape draw item: {item:?}"
                                ));
                            }
                        }
                        let Some(prepared) = self.prepare_shapes_batch(
                            slice.iter().filter_map(|(_, item)| match item {
                                SegmentDrawItem::Shape(shape_index) => Some(&shapes[*shape_index]),
                                _ => None,
                            }),
                            root_scale,
                            viewport,
                            &mut staged_uploads,
                        ) else {
                            continue;
                        };
                        let upload_offset = frame_encoder
                            .allocate_staged_upload_bytes(staged_uploads.bytes.len() as u64);
                        self.flush_staged_uploads_at(
                            frame_encoder.encoder(),
                            &staged_uploads,
                            upload_offset,
                        );
                        {
                            let mut render_pass = frame_encoder.encoder().begin_render_pass(
                                &wgpu::RenderPassDescriptor {
                                    label: Some("Segment Shape Pass"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: target_view,
                                        resolve_target: None,
                                        depth_slice: None,
                                        ops: wgpu::Operations {
                                            load: next_load_op,
                                            store: wgpu::StoreOp::Store,
                                        },
                                    })],
                                    depth_stencil_attachment: None,
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                    multiview_mask: None,
                                },
                            );
                            self.draw_prepared_shapes(
                                &mut render_pass,
                                blend_mode,
                                prepared,
                                width,
                                height,
                            );
                        }
                        pass_count = pass_count.saturating_add(1);
                        rendered_any = true;
                        next_load_op = wgpu::LoadOp::Load;
                    }
                    SegmentBatchPlan::Image {
                        start,
                        end,
                        blend_mode,
                    } => {
                        let viewport = ViewportUniformParams {
                            width,
                            height,
                            offset: [0.0, 0.0],
                        };
                        for (_, item) in &ordered_items[start..end] {
                            if !matches!(item, SegmentDrawItem::Image(_)) {
                                return Err(format!(
                                    "image batch contains non-image draw item: {item:?}"
                                ));
                            }
                        }
                        let prepared_images = self.prepare_image_draw_cmds(
                            ordered_items[start..end]
                                .iter()
                                .filter_map(|(_, item)| match item {
                                    SegmentDrawItem::Image(image_index) => {
                                        Some(&images[*image_index])
                                    }
                                    _ => None,
                                }),
                            viewport,
                            root_scale,
                            &mut staged_uploads,
                        )?;
                        if prepared_images.is_empty() {
                            self.scratch_image_cmds = prepared_images.into_cmds();
                            continue;
                        }
                        let upload_offset = frame_encoder
                            .allocate_staged_upload_bytes(staged_uploads.bytes.len() as u64);
                        self.flush_staged_uploads_at(
                            frame_encoder.encoder(),
                            &staged_uploads,
                            upload_offset,
                        );
                        let draw_result = {
                            let mut render_pass = frame_encoder.encoder().begin_render_pass(
                                &wgpu::RenderPassDescriptor {
                                    label: Some("Segment Image Pass"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: target_view,
                                        resolve_target: None,
                                        depth_slice: None,
                                        ops: wgpu::Operations {
                                            load: next_load_op,
                                            store: wgpu::StoreOp::Store,
                                        },
                                    })],
                                    depth_stencil_attachment: None,
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                    multiview_mask: None,
                                },
                            );
                            self.draw_prepared_images(
                                &mut render_pass,
                                &prepared_images,
                                blend_mode,
                            )
                        };
                        pass_count = pass_count.saturating_add(1);
                        self.scratch_image_cmds = prepared_images.into_cmds();
                        draw_result?;
                        rendered_any = true;
                        next_load_op = wgpu::LoadOp::Load;
                    }
                    SegmentBatchPlan::Text { start, end } => {
                        let viewport = ViewportUniformParams {
                            width,
                            height,
                            offset: [0.0, 0.0],
                        };
                        let text_draws =
                            text_draws_for_ordered_range(ordered_items, texts, start, end)?;
                        if let Some(prepared_glyphs) = self.prepare_text_glyph_draw_cmds(
                            text_draws,
                            viewport,
                            root_scale,
                            &mut staged_uploads,
                        )? {
                            if prepared_glyphs.is_empty() {
                                self.scratch_glyph_cmds = prepared_glyphs.into_cmds();
                                continue;
                            }
                            let upload_offset = frame_encoder
                                .allocate_staged_upload_bytes(staged_uploads.bytes.len() as u64);
                            self.flush_staged_uploads_at(
                                frame_encoder.encoder(),
                                &staged_uploads,
                                upload_offset,
                            );
                            {
                                let mut render_pass = frame_encoder.encoder().begin_render_pass(
                                    &wgpu::RenderPassDescriptor {
                                        label: Some("Segment Text Glyph Atlas Pass"),
                                        color_attachments: &[Some(
                                            wgpu::RenderPassColorAttachment {
                                                view: target_view,
                                                resolve_target: None,
                                                depth_slice: None,
                                                ops: wgpu::Operations {
                                                    load: next_load_op,
                                                    store: wgpu::StoreOp::Store,
                                                },
                                            },
                                        )],
                                        depth_stencil_attachment: None,
                                        timestamp_writes: None,
                                        occlusion_query_set: None,
                                        multiview_mask: None,
                                    },
                                );
                                self.draw_prepared_glyphs(&mut render_pass, &prepared_glyphs)?;
                            }
                            pass_count = pass_count.saturating_add(1);
                            self.scratch_glyph_cmds = prepared_glyphs.into_cmds();
                            rendered_any = true;
                            next_load_op = wgpu::LoadOp::Load;
                        } else {
                            let text_draws =
                                text_draws_for_ordered_range(ordered_items, texts, start, end)?;
                            let prepared_images = self.prepare_text_image_draw_cmds(
                                text_draws,
                                viewport,
                                root_scale,
                                &mut staged_uploads,
                            )?;
                            if prepared_images.is_empty() {
                                self.scratch_image_cmds = prepared_images.into_cmds();
                                continue;
                            }
                            let upload_offset = frame_encoder
                                .allocate_staged_upload_bytes(staged_uploads.bytes.len() as u64);
                            self.flush_staged_uploads_at(
                                frame_encoder.encoder(),
                                &staged_uploads,
                                upload_offset,
                            );
                            {
                                let mut render_pass = frame_encoder.encoder().begin_render_pass(
                                    &wgpu::RenderPassDescriptor {
                                        label: Some("Segment Text Pass"),
                                        color_attachments: &[Some(
                                            wgpu::RenderPassColorAttachment {
                                                view: target_view,
                                                resolve_target: None,
                                                depth_slice: None,
                                                ops: wgpu::Operations {
                                                    load: next_load_op,
                                                    store: wgpu::StoreOp::Store,
                                                },
                                            },
                                        )],
                                        depth_stencil_attachment: None,
                                        timestamp_writes: None,
                                        occlusion_query_set: None,
                                        multiview_mask: None,
                                    },
                                );
                                self.draw_prepared_images(
                                    &mut render_pass,
                                    &prepared_images,
                                    BlendMode::SrcOver,
                                )?;
                            }
                            self.frame_stats.bump_text();
                            pass_count = pass_count.saturating_add(1);
                            self.scratch_image_cmds = prepared_images.into_cmds();
                            rendered_any = true;
                            next_load_op = wgpu::LoadOp::Load;
                        }
                    }
                    SegmentBatchPlan::Composite { start, end } => {
                        let batch_items: Vec<_> = ordered_items[start..end]
                            .iter()
                            .map(|(_, item)| match item {
                                SegmentDrawItem::Composite(composite_index) => composites
                                    .get(*composite_index)
                                    .map(|(_, composite)| *composite)
                                    .ok_or_else(|| {
                                        "composite item index is outside the composite buffer"
                                            .to_string()
                                    }),
                                other => Err(format!(
                                    "composite batch contains non-composite draw item: {other:?}"
                                )),
                            })
                            .collect::<Result<_, _>>()?;
                        let device = self.device.clone();
                        self.effect_renderer.encode_composite_batch_to_view_pass(
                            frame_encoder,
                            &device,
                            target_view,
                            (width, height),
                            next_load_op,
                            &batch_items,
                        );
                        self.effect_renderer.record_composite_pass();
                        pass_count = pass_count.saturating_add(1);
                        rendered_any = true;
                        next_load_op = wgpu::LoadOp::Load;
                    }
                    SegmentBatchPlan::ShaderComposite { start, end } => {
                        let batch_items: Vec<_> = ordered_items[start..end]
                            .iter()
                            .map(|(_, item)| match item {
                                SegmentDrawItem::ShaderComposite(composite_index) => {
                                    shader_composites
                                        .get(*composite_index)
                                        .map(|(_, composite)| *composite)
                                        .ok_or_else(|| {
                                            "shader composite item index is outside the shader composite buffer"
                                                .to_string()
                                        })
                                }
                                other => Err(format!(
                                    "shader composite batch contains non-shader-composite draw item: {other:?}"
                                )),
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        let device = self.device.clone();
                        let encoded = self.effect_renderer.encode_shader_batch_src_over_to_view(
                            frame_encoder,
                            &device,
                            target_view,
                            (width, height),
                            next_load_op,
                            &batch_items,
                        );
                        if !encoded {
                            return Err("shader composite batch failed to encode".to_string());
                        }
                        self.effect_renderer.record_composite_pass();
                        self.effect_renderer.debug_effects.set(
                            self.effect_renderer.debug_effects.get() + batch_items.len() as u32,
                        );
                        pass_count = pass_count.saturating_add(1);
                        rendered_any = true;
                        next_load_op = wgpu::LoadOp::Load;
                    }
                    SegmentBatchPlan::Retained { start, end } => {
                        // Reached only when native fusion declined the chunk;
                        // retained batches exist on storage-mode native
                        // devices, where fusion always accepts, but the arm
                        // stays a real draw so that assumption is not load-
                        // bearing for correctness. Deliberately direct encode
                        // — retained bundle caching lives in the fused path
                        // only; this fallback stays the simple reference.
                        #[cfg(target_arch = "wasm32")]
                        {
                            let _ = (start, end);
                            return Err("retained shape batches are native-only".to_string());
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            self.stage_replay_patches(&mut staged_uploads);
                            for (_, item) in &ordered_items[start..end] {
                                let SegmentDrawItem::Retained(index) = item else {
                                    return Err(format!(
                                        "retained batch contains non-retained draw item: {item:?}"
                                    ));
                                };
                                let retained = retained_draws.get(*index).ok_or_else(|| {
                                    format!("retained draw index {index} out of bounds")
                                })?;
                                if (*index as u32) < MAX_REPLAY_SLOTS
                                    && self.replay_slots.slots.contains_key(&retained.slot)
                                {
                                    let transform = retained.transform.with_retained_paint();
                                    staged_uploads.stage_at(
                                        UploadTarget::ReplayTransform,
                                        *index as u64 * REPLAY_TRANSFORM_STRIDE,
                                        bytemuck::bytes_of(&transform),
                                    );
                                }
                            }
                            let upload_offset = frame_encoder
                                .allocate_staged_upload_bytes(staged_uploads.bytes.len() as u64);
                            self.flush_staged_uploads_at(
                                frame_encoder.encoder(),
                                &staged_uploads,
                                upload_offset,
                            );
                            {
                                let mut render_pass = frame_encoder.encoder().begin_render_pass(
                                    &wgpu::RenderPassDescriptor {
                                        label: Some("Segment Retained Pass"),
                                        color_attachments: &[Some(
                                            wgpu::RenderPassColorAttachment {
                                                view: target_view,
                                                resolve_target: None,
                                                depth_slice: None,
                                                ops: wgpu::Operations {
                                                    load: next_load_op,
                                                    store: wgpu::StoreOp::Store,
                                                },
                                            },
                                        )],
                                        depth_stencil_attachment: None,
                                        timestamp_writes: None,
                                        occlusion_query_set: None,
                                        multiview_mask: None,
                                    },
                                );
                                for (_, item) in &ordered_items[start..end] {
                                    if let SegmentDrawItem::Retained(index) = item {
                                        if let Some(retained) = retained_draws.get(*index) {
                                            self.draw_retained_batch(
                                                &mut render_pass,
                                                retained,
                                                *index,
                                                width,
                                                height,
                                            );
                                        }
                                    }
                                }
                            }
                            pass_count = pass_count.saturating_add(1);
                            rendered_any = true;
                            next_load_op = wgpu::LoadOp::Load;
                        }
                    }
                }
            }
            Ok(SegmentRenderOutcome {
                rendered_any,
                pass_count,
            })
        })();
        self.restore_staged_uploads(staged_uploads);
        result
    }

    fn viewport_uniforms(params: ViewportUniformParams) -> Uniforms {
        Uniforms {
            viewport: [params.width as f32, params.height as f32],
            viewport_offset: params.offset,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn stage_viewport_uniforms(
        &self,
        staged_uploads: &mut StagedBufferUploads,
        params: ViewportUniformParams,
    ) {
        let uniforms = Self::viewport_uniforms(params);
        staged_uploads.stage(UploadTarget::Uniform, bytemuck::bytes_of(&uniforms));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn stage_retained_glyph_viewport_uniforms(
        &mut self,
        staged_uploads: &mut StagedBufferUploads,
        params: ViewportUniformParams,
    ) -> usize {
        let slot = self.claim_retained_glyph_uniform_slot();
        let uniforms = Self::viewport_uniforms(params);
        staged_uploads.stage_at(
            UploadTarget::RetainedGlyphUniform,
            self.retained_glyph_uniform_offset(slot),
            bytemuck::bytes_of(&uniforms),
        );
        slot
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn claim_retained_glyph_uniform_slot(&mut self) -> usize {
        let slot = self.retained_glyph_uniform_cursor;
        self.retained_glyph_uniform_cursor = self.retained_glyph_uniform_cursor.saturating_add(1);
        self.ensure_retained_glyph_uniform_capacity(slot.saturating_add(1));
        slot
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn retained_glyph_uniform_offset(&self, slot: usize) -> u64 {
        self.retained_glyph_uniform_stride * slot as u64
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn retained_glyph_uniform_dynamic_offset(&self, slot: usize) -> Result<u32, String> {
        let offset = self.retained_glyph_uniform_offset(slot);
        u32::try_from(offset).map_err(|_| {
            "retained glyph uniform offset exceeded WGPU dynamic offset range".to_string()
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_retained_glyph_uniform_capacity(&mut self, required_slots: usize) {
        if required_slots <= self.retained_glyph_uniform_capacity {
            return;
        }
        let new_capacity = required_slots
            .next_power_of_two()
            .max(INITIAL_RETAINED_GLYPH_UNIFORM_SLOTS);
        self.retained_glyph_uniform_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Retained Glyph Uniform Buffer"),
            size: self.retained_glyph_uniform_stride * new_capacity as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.retained_glyph_uniform_bind_group =
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Retained Glyph Uniform Bind Group"),
                layout: &self.retained_glyph_uniform_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.retained_glyph_uniform_buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                    }),
                }],
            });
        self.retained_glyph_uniform_capacity = new_capacity;
    }

    #[cfg(target_arch = "wasm32")]
    fn prepare_wasm_viewport_uniforms(&mut self, params: ViewportUniformParams) -> usize {
        let slot = self.claim_wasm_uniform_batch();
        let uniforms = Self::viewport_uniforms(params);
        let bytes = bytemuck::bytes_of(&uniforms);
        let upload_stats = self.frame_graph_executor.upload_buffer(
            &self.queue,
            &self.wasm_uniform_batches[slot].buffer,
            0,
            bytes,
        );
        self.frame_stats.record_command_stats(upload_stats);
        slot
    }

    #[cfg(target_arch = "wasm32")]
    fn claim_wasm_uniform_batch(&mut self) -> usize {
        let slot = self.wasm_uniform_batch_cursor;
        self.wasm_uniform_batch_cursor += 1;
        while self.wasm_uniform_batches.len() <= slot {
            self.wasm_uniform_batches.push(UniformBatchBuffer::new(
                &self.device,
                &self.uniform_bind_group_layout,
            ));
        }
        slot
    }

    #[cfg(target_arch = "wasm32")]
    fn claim_wasm_shape_batch(&mut self) -> usize {
        let slot = self.wasm_shape_batch_cursor;
        self.wasm_shape_batch_cursor += 1;
        while self.wasm_shape_batches.len() <= slot {
            self.wasm_shape_batches.push(ShapeBatchBuffers::new(
                &self.device,
                &self.shape_bind_group_layout,
                &self.identity_similarity_buffer,
                self.dummy_paint_buffer.as_ref(),
                self.shape_batch_limits,
            ));
        }
        slot
    }

    #[cfg(target_arch = "wasm32")]
    fn claim_wasm_image_batch(&mut self) -> usize {
        let slot = self.wasm_image_batch_cursor;
        self.wasm_image_batch_cursor += 1;
        while self.wasm_image_batches.len() <= slot {
            self.wasm_image_batches
                .push(ImageBatchBuffers::new(&self.device));
        }
        slot
    }

    #[cfg(target_arch = "wasm32")]
    fn write_wasm_buffer(&self, buffer: &wgpu::Buffer, bytes: &[u8]) {
        let upload_stats = self
            .frame_graph_executor
            .upload_buffer(&self.queue, buffer, 0, bytes);
        self.frame_stats.record_command_stats(upload_stats);
    }

    fn take_staged_uploads(&mut self) -> StagedBufferUploads {
        let mut staged_uploads = std::mem::take(&mut self.staged_uploads);
        debug_assert!(
            staged_uploads.is_empty(),
            "renderer-owned staged uploads should be restored as empty scratch storage"
        );
        staged_uploads.clear();
        staged_uploads
    }

    fn restore_staged_uploads(&mut self, mut staged_uploads: StagedBufferUploads) {
        staged_uploads.clear();
        self.staged_uploads = staged_uploads;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_upload_buffer_capacity(&mut self, required_bytes: u64) {
        if required_bytes <= self.upload_buffer.size() {
            return;
        }

        let new_size = required_bytes
            .next_power_of_two()
            .max(INITIAL_UPLOAD_BUFFER_BYTES);
        self.upload_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Frame Upload Buffer"),
            size: new_size,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
    }

    fn flush_staged_uploads_at(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        staged_uploads: &StagedBufferUploads,
        upload_buffer_offset: u64,
    ) {
        if staged_uploads.is_empty() {
            return;
        }
        debug_assert_eq!(
            upload_buffer_offset % wgpu::COPY_BUFFER_ALIGNMENT,
            0,
            "upload-buffer base offset must satisfy copy alignment"
        );

        #[cfg(target_arch = "wasm32")]
        {
            let _ = upload_buffer_offset;
            let _ = encoder;
            debug_assert!(
                staged_uploads.is_empty(),
                "wasm draw uploads use retained per-batch resource slots"
            );
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.ensure_upload_buffer_capacity(
                upload_buffer_offset + staged_uploads.bytes.len() as u64,
            );
            let upload_stats = self.frame_graph_executor.upload_buffer(
                &self.queue,
                &self.upload_buffer,
                upload_buffer_offset,
                &staged_uploads.bytes,
            );
            self.frame_stats.record_command_stats(upload_stats);

            for copy in &staged_uploads.copies {
                let target_buffer = match copy.target {
                    UploadTarget::Uniform => &self.uniform_buffer,
                    UploadTarget::ShapeData => &self.shape_buffers.shape_buffer,
                    UploadTarget::ShapeGradient => &self.shape_buffers.gradient_buffer,
                    UploadTarget::ImageVertex => &self.image_vertex_buffer,
                    UploadTarget::ImageIndex => &self.image_index_buffer,
                    UploadTarget::RetainedGlyphUniform => &self.retained_glyph_uniform_buffer,
                    UploadTarget::ReplayTransform => &self.replay_slots.transform_buffer,
                    UploadTarget::ReplayPaintData(slot) => {
                        // A slot released between staging and flush has
                        // nothing left to patch.
                        let Some(entry) = self.replay_slots.slots.get(&slot) else {
                            continue;
                        };
                        &entry.paint_buffer
                    }
                };
                encoder.copy_buffer_to_buffer(
                    &self.upload_buffer,
                    upload_buffer_offset + copy.source_offset,
                    target_buffer,
                    copy.target_offset,
                    copy.size,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_shadow_draw<C: FrameCommandRecorder>(
        &mut self,
        frame_encoder: &mut C,
        target_view: &wgpu::TextureView,
        shadow: &ShadowDraw,
        width: u32,
        height: u32,
        root_scale: f32,
    ) {
        if shadow.shapes.is_empty() && shadow.texts.is_empty() {
            return;
        }

        let shape_bounds_opt = shadow
            .shapes
            .iter()
            .map(|(shape, _)| shape.rect)
            .reduce(|a, b| Rect {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
                width: (a.x + a.width).max(b.x + b.width) - a.x.min(b.x),
                height: (a.y + a.height).max(b.y + b.height) - a.y.min(b.y),
            });

        let text_bounds_opt = shadow
            .texts
            .iter()
            .map(|text| text.rect)
            .reduce(|a, b| Rect {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
                width: (a.x + a.width).max(b.x + b.width) - a.x.min(b.x),
                height: (a.y + a.height).max(b.y + b.height) - a.y.min(b.y),
            });

        let combined_bounds = match (shape_bounds_opt, text_bounds_opt) {
            (Some(s), Some(t)) => Some(Rect {
                x: s.x.min(t.x),
                y: s.y.min(t.y),
                width: (s.x + s.width).max(t.x + t.width) - s.x.min(t.x),
                height: (s.y + s.height).max(t.y + t.height) - s.y.min(t.y),
            }),
            (Some(s), None) => Some(s),
            (None, Some(t)) => Some(t),
            (None, None) => None,
        };

        let Some(shape_bounds) = combined_bounds else {
            return;
        };

        let blur_margin = blur_extent_margin(shadow.blur_radius);
        let source_blur_bounds = Rect {
            x: shape_bounds.x - blur_margin,
            y: shape_bounds.y - blur_margin,
            width: shape_bounds.width + blur_margin * 2.0,
            height: shape_bounds.height + blur_margin * 2.0,
        };
        let mut visible_blur_bounds = source_blur_bounds;
        if let Some(clip) = shadow.clip {
            let clip_expanded = Rect {
                x: clip.x - blur_margin,
                y: clip.y - blur_margin,
                width: clip.width + blur_margin * 2.0,
                height: clip.height + blur_margin * 2.0,
            };
            let Some(intersection) = visible_blur_bounds.intersect(clip_expanded) else {
                return;
            };
            visible_blur_bounds = intersection;
        }
        let processing_scissor =
            scissor_rect_for_rect(visible_blur_bounds, root_scale, width, height);
        if processing_scissor.is_none() {
            return;
        }

        // Zero blur: render shapes directly to target (fast path).
        if shadow.blur_radius <= 0.0 {
            for (shape, blend_mode) in &shadow.shapes {
                self.encode_shapes_pass(
                    frame_encoder,
                    target_view,
                    std::iter::once(shape),
                    *blend_mode,
                    width,
                    height,
                    root_scale,
                    wgpu::LoadOp::Load,
                    [0.0, 0.0],
                );
                frame_encoder.record_pass();
            }
            if !shadow.texts.is_empty() {
                let mut staged_uploads = self.take_staged_uploads();
                let viewport = ViewportUniformParams {
                    width,
                    height,
                    offset: [0.0, 0.0],
                };
                match self.prepare_text_image_draw_cmds(
                    shadow.texts.iter(),
                    viewport,
                    root_scale,
                    &mut staged_uploads,
                ) {
                    Ok(prepared_images) if !prepared_images.is_empty() => {
                        let upload_offset = frame_encoder
                            .allocate_staged_upload_bytes(staged_uploads.bytes.len() as u64);
                        self.flush_staged_uploads_at(
                            frame_encoder.encoder(),
                            &staged_uploads,
                            upload_offset,
                        );
                        let draw_result = {
                            let mut render_pass = frame_encoder.encoder().begin_render_pass(
                                &wgpu::RenderPassDescriptor {
                                    label: Some("Zero Blur Shadow Text Image Pass"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: target_view,
                                        resolve_target: None,
                                        depth_slice: None,
                                        ops: wgpu::Operations {
                                            load: wgpu::LoadOp::Load,
                                            store: wgpu::StoreOp::Store,
                                        },
                                    })],
                                    depth_stencil_attachment: None,
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                    multiview_mask: None,
                                },
                            );
                            self.draw_prepared_images(
                                &mut render_pass,
                                &prepared_images,
                                BlendMode::SrcOver,
                            )
                        };
                        self.scratch_image_cmds = prepared_images.into_cmds();
                        if let Err(e) = draw_result {
                            eprintln!("Failed to draw text for zero-blur shadow: {}", e);
                        } else {
                            self.frame_stats.bump_text();
                            frame_encoder.record_pass();
                        }
                    }
                    Ok(prepared_images) => {
                        self.scratch_image_cmds = prepared_images.into_cmds();
                    }
                    Err(e) => {
                        eprintln!("Failed to prepare text image for zero-blur shadow: {}", e);
                    }
                }
                self.restore_staged_uploads(staged_uploads);
            }
            return;
        }

        // Compute pixel-space bounds for the offscreen textures, clamped to viewport.
        let Some(device_bounds) =
            device_pixel_bounds_for_rect(visible_blur_bounds, width, height, root_scale)
        else {
            return;
        };
        let bounds_x = device_bounds.x;
        let bounds_y = device_bounds.y;
        let bounds_w = device_bounds.width;
        let bounds_h = device_bounds.height;
        let pixel_radius = shadow.blur_radius * root_scale;

        if shadow.texts.is_empty() && !shadow.shapes.is_empty() {
            if let Some(plan) = shape_shadow_surface_plan(
                &shadow.shapes,
                shadow.clip,
                shadow.blur_radius,
                width,
                height,
                root_scale,
                self.max_texture_dim(),
            ) {
                if self.encode_shape_only_blurred_shadow_draw(
                    frame_encoder,
                    target_view,
                    shadow,
                    plan.source_device_bounds,
                    plan.pixel_radius,
                    plan.processing_scissor,
                    width,
                    height,
                    root_scale,
                ) {
                    return;
                }
            }
        }

        if !shadow.texts.is_empty() {
            self.frame_stats.record_shadow_text_blur_fallback();
        }

        let device = self.device.clone();
        let source_descriptor =
            self.transient_offscreen_descriptor("Shadow Source", bounds_w, bounds_h);
        let source = frame_encoder.acquire_transient_offscreen(&device, source_descriptor);
        let viewport_offset = [bounds_x, bounds_y];
        let mut next_load_op = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let source_outcome = self.encode_shadow_shape_source_passes(
            frame_encoder,
            &source.view,
            &shadow.shapes,
            bounds_w,
            bounds_h,
            viewport_offset,
            root_scale,
            &mut next_load_op,
        );
        frame_encoder.record_passes(source_outcome.pass_count);
        let mut rendered_any = source_outcome.rendered_any;

        if !shadow.texts.is_empty() {
            let mut shifted_texts = shadow.texts.clone();
            for text in &mut shifted_texts {
                text.rect.x -= viewport_offset[0] / root_scale;
                text.rect.y -= viewport_offset[1] / root_scale;
                if let Some(clip) = text.clip.as_mut() {
                    clip.x -= viewport_offset[0] / root_scale;
                    clip.y -= viewport_offset[1] / root_scale;
                }
            }

            let mut staged_uploads = self.take_staged_uploads();
            let viewport = ViewportUniformParams {
                width: bounds_w,
                height: bounds_h,
                offset: [0.0, 0.0],
            };
            match self.prepare_text_image_draw_cmds(
                shifted_texts.iter(),
                viewport,
                root_scale,
                &mut staged_uploads,
            ) {
                Ok(prepared_images) if !prepared_images.is_empty() => {
                    let upload_offset = frame_encoder
                        .allocate_staged_upload_bytes(staged_uploads.bytes.len() as u64);
                    self.flush_staged_uploads_at(
                        frame_encoder.encoder(),
                        &staged_uploads,
                        upload_offset,
                    );
                    let draw_result = {
                        let mut render_pass = frame_encoder.encoder().begin_render_pass(
                            &wgpu::RenderPassDescriptor {
                                label: Some("Shadow Source Text Image Pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &source.view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: next_load_op,
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                                multiview_mask: None,
                            },
                        );
                        self.draw_prepared_images(
                            &mut render_pass,
                            &prepared_images,
                            BlendMode::SrcOver,
                        )
                    };
                    self.scratch_image_cmds = prepared_images.into_cmds();
                    if let Err(e) = draw_result {
                        eprintln!("Failed to draw text for shadow: {}", e);
                    } else {
                        self.frame_stats.bump_text();
                        frame_encoder.record_pass();
                        rendered_any = true;
                    }
                }
                Ok(prepared_images) => {
                    self.scratch_image_cmds = prepared_images.into_cmds();
                }
                Err(e) => {
                    eprintln!("Failed to prepare text image for shadow: {}", e);
                }
            }
            self.restore_staged_uploads(staged_uploads);
        }

        if !rendered_any {
            frame_encoder.release_transient_offscreen(source_descriptor, source);
            return;
        }

        let scratch_descriptor =
            self.transient_offscreen_descriptor("Shadow Blur Scratch", bounds_w, bounds_h);
        let scratch = frame_encoder.acquire_transient_offscreen(&device, scratch_descriptor);
        {
            self.effect_renderer.encode_blur_scissored_ping_pong_passes(
                frame_encoder,
                &device,
                &source,
                &scratch,
                &source.view,
                pixel_radius,
                pixel_radius,
                TileMode::Decal,
                None, // No scissor needed — the texture is already bounds-sized
            );
        }
        frame_encoder.record_passes(2);

        let clip_scissor = shadow
            .clip
            .and_then(|clip| scissor_rect_for_rect(clip, root_scale, width, height));
        let scissor = clip_scissor.or(processing_scissor);
        let rounded_mask = inner_shadow_composite_mask(shadow, root_scale).map(|mut mask| {
            // Adjust mask coordinates from viewport-space to texture-local space,
            // since the blit shader computes world_pos = uv * tex_size.
            mask.rect[0] -= viewport_offset[0];
            mask.rect[1] -= viewport_offset[1];
            mask
        });
        let dest_viewport = Some((
            viewport_offset[0],
            viewport_offset[1],
            bounds_w as f32,
            bounds_h as f32,
        ));
        {
            self.effect_renderer
                .encode_composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
                    frame_encoder,
                    &device,
                    &source,
                    target_view,
                    1.0,
                    wgpu::LoadOp::Load,
                    scissor,
                    rounded_mask,
                    BlendMode::SrcOver,
                    dest_viewport,
                    CompositeSampleMode::Linear,
                );
        }
        frame_encoder.record_pass();
        self.effect_renderer.record_blur_pass();
        self.effect_renderer.record_composite_pass();
        frame_encoder.release_transient_offscreen(scratch_descriptor, scratch);
        frame_encoder.release_transient_offscreen(source_descriptor, source);
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_shadow_shape_source_passes<C: FrameCommandRecorder>(
        &mut self,
        frame_encoder: &mut C,
        source_view: &wgpu::TextureView,
        shapes: &[(DrawShape, BlendMode)],
        width: u32,
        height: u32,
        viewport_offset: [f32; 2],
        root_scale: f32,
        next_load_op: &mut wgpu::LoadOp<wgpu::Color>,
    ) -> ShadowSourceRenderOutcome {
        if shapes.is_empty() {
            return ShadowSourceRenderOutcome {
                rendered_any: false,
                pass_count: 0,
            };
        }

        let mut staged_uploads = self.take_staged_uploads();
        let mut rendered_any = false;
        let mut pass_count = 0_u32;
        let mut start = 0usize;
        while start < shapes.len() {
            let blend_mode = supported_blend_mode(shapes[start].1);
            let mut end = start + 1;
            while end < shapes.len()
                && end - start < self.shape_batch_limits.max_shapes_per_batch
                && supported_blend_mode(shapes[end].1) == blend_mode
            {
                end += 1;
            }

            staged_uploads.clear();
            let viewport = ViewportUniformParams {
                width,
                height,
                offset: viewport_offset,
            };
            let Some(prepared_shape) = self.prepare_shapes_batch(
                shapes[start..end]
                    .iter()
                    .map(|(shape, _blend_mode)| shape)
                    .filter(|shape| shape_draw_is_visible_in_viewport(shape, viewport, root_scale)),
                root_scale,
                viewport,
                &mut staged_uploads,
            ) else {
                start = end;
                continue;
            };

            let upload_offset =
                frame_encoder.allocate_staged_upload_bytes(staged_uploads.bytes.len() as u64);
            self.flush_staged_uploads_at(frame_encoder.encoder(), &staged_uploads, upload_offset);

            {
                let mut render_pass =
                    frame_encoder
                        .encoder()
                        .begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("Shadow Source Shape Pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: source_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: *next_load_op,
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                self.draw_prepared_shapes(
                    &mut render_pass,
                    blend_mode,
                    prepared_shape,
                    width,
                    height,
                );
            }

            pass_count = pass_count.saturating_add(1);
            rendered_any = true;
            *next_load_op = wgpu::LoadOp::Load;
            start = end;
        }

        self.restore_staged_uploads(staged_uploads);
        ShadowSourceRenderOutcome {
            rendered_any,
            pass_count,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_shape_only_blurred_shadow_draw<C: FrameCommandRecorder>(
        &mut self,
        frame_encoder: &mut C,
        target_view: &wgpu::TextureView,
        shadow: &ShadowDraw,
        device_bounds: DevicePixelBounds,
        pixel_radius: f32,
        processing_scissor: Option<(u32, u32, u32, u32)>,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> bool {
        let bounds_w = device_bounds.width;
        let bounds_h = device_bounds.height;
        let viewport_offset = [device_bounds.x, device_bounds.y];
        let cache_key =
            shape_shadow_surface_cache_key(&shadow.shapes, device_bounds, pixel_radius, root_scale);

        if let Some(key) = cache_key {
            if let Some(cached) = self.cached_shadow_surface(&key) {
                self.frame_stats
                    .record_shadow_shape_cache_hit(bounds_w, bounds_h);
                let clip_scissor = shadow
                    .clip
                    .and_then(|clip| scissor_rect_for_rect(clip, root_scale, width, height));
                let scissor = clip_scissor.or(processing_scissor);
                let rounded_mask =
                    inner_shadow_composite_mask(shadow, root_scale).map(|mut mask| {
                        mask.rect[0] -= viewport_offset[0];
                        mask.rect[1] -= viewport_offset[1];
                        mask
                    });
                let dest_viewport = Some((
                    viewport_offset[0],
                    viewport_offset[1],
                    bounds_w as f32,
                    bounds_h as f32,
                ));
                {
                    self.effect_renderer
                        .encode_composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
                            frame_encoder,
                            &self.device,
                            &cached,
                            target_view,
                            1.0,
                            wgpu::LoadOp::Load,
                            scissor,
                            rounded_mask,
                            BlendMode::SrcOver,
                            dest_viewport,
                            CompositeSampleMode::Nearest,
                        );
                }
                frame_encoder.record_pass();
                self.effect_renderer.record_composite_pass();
                return true;
            }
            self.frame_stats
                .record_shadow_shape_cache_miss(bounds_w, bounds_h);
            self.frame_stats.maybe_print_shadow_shape_cache_miss(
                bounds_w,
                bounds_h,
                key.content_hash,
                pixel_radius,
                viewport_offset,
                shadow.shapes.len(),
                shadow.clip,
            );
        }

        let device = self.device.clone();
        let source_descriptor =
            self.transient_offscreen_descriptor("Shape Shadow Source", bounds_w, bounds_h);
        let source_is_cacheable = cache_key.is_some();
        let source = if source_is_cacheable {
            self.acquire_retained_surface(bounds_w, bounds_h)
        } else {
            frame_encoder.acquire_transient_offscreen(&device, source_descriptor)
        };
        let scratch_descriptor =
            self.transient_offscreen_descriptor("Shape Shadow Blur Scratch", bounds_w, bounds_h);
        let scratch = frame_encoder.acquire_transient_offscreen(&device, scratch_descriptor);
        let mut next_load_op = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let source_outcome = self.encode_shadow_shape_source_passes(
            frame_encoder,
            &source.view,
            &shadow.shapes,
            bounds_w,
            bounds_h,
            viewport_offset,
            root_scale,
            &mut next_load_op,
        );
        frame_encoder.record_passes(source_outcome.pass_count);

        if !source_outcome.rendered_any {
            frame_encoder.release_transient_offscreen(scratch_descriptor, scratch);
            if source_is_cacheable {
                self.defer_offscreen_release(source);
            } else {
                frame_encoder.release_transient_offscreen(source_descriptor, source);
            }
            return true;
        }

        {
            self.effect_renderer.encode_blur_scissored_ping_pong_passes(
                frame_encoder,
                &device,
                &source,
                &scratch,
                &source.view,
                pixel_radius,
                pixel_radius,
                TileMode::Decal,
                None,
            );
        }
        frame_encoder.record_passes(2);

        let clip_scissor = shadow
            .clip
            .and_then(|clip| scissor_rect_for_rect(clip, root_scale, width, height));
        let scissor = clip_scissor.or(processing_scissor);
        let rounded_mask = inner_shadow_composite_mask(shadow, root_scale).map(|mut mask| {
            mask.rect[0] -= viewport_offset[0];
            mask.rect[1] -= viewport_offset[1];
            mask
        });
        let dest_viewport = Some((
            viewport_offset[0],
            viewport_offset[1],
            bounds_w as f32,
            bounds_h as f32,
        ));
        {
            self.effect_renderer
                .encode_composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
                    frame_encoder,
                    &device,
                    &source,
                    target_view,
                    1.0,
                    wgpu::LoadOp::Load,
                    scissor,
                    rounded_mask,
                    BlendMode::SrcOver,
                    dest_viewport,
                    CompositeSampleMode::Nearest,
                );
        }
        frame_encoder.record_pass();

        self.effect_renderer.record_blur_pass();
        self.effect_renderer.record_composite_pass();
        frame_encoder.release_transient_offscreen(scratch_descriptor, scratch);
        if let Some(key) = cache_key {
            self.insert_cached_shadow_surface(key, source);
        } else {
            frame_encoder.release_transient_offscreen(source_descriptor, source);
        }
        true
    }

    fn prepare_shapes_batch<'a, I>(
        &mut self,
        layer_shapes: I,
        root_scale: f32,
        viewport: ViewportUniformParams,
        staged_uploads: &mut StagedBufferUploads,
    ) -> Option<PreparedShapeBatch>
    where
        I: Iterator<Item = &'a DrawShape>,
    {
        #[cfg(target_arch = "wasm32")]
        let _ = staged_uploads;

        // Build shape data for this subset. Callers hand in only shapes visible in
        // `viewport`: the segment paths culled at collect time, and the layer and
        // shadow-source paths filter at the call site. Re-checking here would run
        // the same quad math a second time on every shape of every frame.
        let shape_refs: Vec<&DrawShape> = layer_shapes
            .take(self.shape_batch_limits.max_shapes_per_batch)
            .collect();
        let shape_count = shape_refs.len();
        if shape_count == 0 {
            return None;
        }

        // Per-shape gradient spans as a prefix sum, so every output slot is
        // known before conversion starts and the shapes can convert in
        // parallel into disjoint sub-slices.
        let mut gradient_offsets: Vec<u32> = Vec::with_capacity(shape_count + 1);
        let mut total_gradient_stops = 0u32;
        gradient_offsets.push(0);
        for shape in &shape_refs {
            total_gradient_stops += shape_gradient_stop_count(shape) as u32;
            gradient_offsets.push(total_gradient_stops);
        }

        self.scratch_shape_data.clear();
        self.scratch_shape_data
            .resize(shape_count, ShapeData::zeroed());
        self.scratch_gradients.clear();
        self.scratch_gradients
            .resize(total_gradient_stops as usize, GradientStop::zeroed());

        convert_shapes_into_outputs(
            &shape_refs,
            &gradient_offsets,
            root_scale,
            &mut self.scratch_shape_data,
            &mut self.scratch_gradients,
        );

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.shape_buffers.ensure_capacity(
                &self.device,
                &self.shape_bind_group_layout,
                &self.identity_similarity_buffer,
                self.dummy_paint_buffer.as_ref(),
                shape_count,
                self.scratch_gradients.len().max(1),
            );
            self.stage_viewport_uniforms(staged_uploads, viewport);
            staged_uploads.stage(
                UploadTarget::ShapeData,
                bytemuck::cast_slice(&self.scratch_shape_data),
            );
            if !self.scratch_gradients.is_empty() {
                staged_uploads.stage(
                    UploadTarget::ShapeGradient,
                    bytemuck::cast_slice(&self.scratch_gradients),
                );
            }
        }

        #[cfg(target_arch = "wasm32")]
        let shape_slot = {
            let slot = self.claim_wasm_shape_batch();
            {
                let buffers = &mut self.wasm_shape_batches[slot];
                buffers.ensure_capacity(
                    &self.device,
                    &self.shape_bind_group_layout,
                    &self.identity_similarity_buffer,
                    self.dummy_paint_buffer.as_ref(),
                    shape_count,
                    self.scratch_gradients.len().max(1),
                );
            }
            let buffers = &self.wasm_shape_batches[slot];
            self.write_wasm_buffer(
                &buffers.shape_buffer,
                bytemuck::cast_slice(&self.scratch_shape_data),
            );
            if !self.scratch_gradients.is_empty() {
                self.write_wasm_buffer(
                    &buffers.gradient_buffer,
                    bytemuck::cast_slice(&self.scratch_gradients),
                );
            }
            slot
        };

        #[cfg(target_arch = "wasm32")]
        let uniform_slot = self.prepare_wasm_viewport_uniforms(viewport);

        Some(PreparedShapeBatch {
            vertex_start: 0,
            vertex_count: shape_count as u32 * 6,
            #[cfg(target_arch = "wasm32")]
            shape_slot,
            #[cfg(target_arch = "wasm32")]
            uniform_slot,
        })
    }

    /// Like [`Self::prepare_shapes_batch`], but converts shapes straight into
    /// mapped regions of the frame upload buffer instead of scratch vectors —
    /// one CPU pass over the data instead of three (convert, stage, upload).
    /// Returns the prepared batch and the upload-buffer base offset to pass
    /// to `flush_staged_uploads_at`; the GPU copies are recorded into
    /// `staged_uploads` while its byte blob stays empty.
    #[cfg(not(target_arch = "wasm32"))]
    fn prepare_shapes_batch_direct<'a, I, C: FrameCommandRecorder>(
        &mut self,
        frame_encoder: &mut C,
        layer_shapes: I,
        root_scale: f32,
        viewport: ViewportUniformParams,
        staged_uploads: &mut StagedBufferUploads,
    ) -> Option<(PreparedShapeBatch, u64)>
    where
        I: Iterator<Item = &'a DrawShape>,
    {
        let shape_refs: Vec<&DrawShape> = layer_shapes
            .take(self.shape_batch_limits.max_shapes_per_batch)
            .collect();
        let shape_count = shape_refs.len();
        if shape_count == 0 {
            return None;
        }

        let mut gradient_offsets: Vec<u32> = Vec::with_capacity(shape_count + 1);
        let mut total_gradient_stops = 0u32;
        gradient_offsets.push(0);
        for shape in &shape_refs {
            total_gradient_stops += shape_gradient_stop_count(shape) as u32;
            gradient_offsets.push(total_gradient_stops);
        }

        self.shape_buffers.ensure_capacity(
            &self.device,
            &self.shape_bind_group_layout,
            &self.identity_similarity_buffer,
            self.dummy_paint_buffer.as_ref(),
            shape_count,
            (total_gradient_stops as usize).max(1),
        );

        self.scratch_shape_data.clear();
        self.scratch_shape_data
            .resize(shape_count, ShapeData::zeroed());
        self.scratch_gradients.clear();
        self.scratch_gradients
            .resize(total_gradient_stops as usize, GradientStop::zeroed());
        convert_shapes_into_outputs(
            &shape_refs,
            &gradient_offsets,
            root_scale,
            &mut self.scratch_shape_data,
            &mut self.scratch_gradients,
        );

        // Region layout inside the frame upload buffer. Every element type is
        // f32/u32-based, so all lengths are multiples of
        // `COPY_BUFFER_ALIGNMENT` and back-to-back packing keeps each offset
        // copy-aligned. Writing each scratch slice straight into the upload
        // buffer skips the intermediate staged-bytes blob (one fewer CPU pass
        // over the batch payload).
        let uniform_len = std::mem::size_of::<Uniforms>() as u64;
        let shape_len = (shape_count * std::mem::size_of::<ShapeData>()) as u64;
        let gradient_len = total_gradient_stops as u64 * std::mem::size_of::<GradientStop>() as u64;
        let total_len = uniform_len + shape_len + gradient_len;
        let upload_base = frame_encoder.allocate_staged_upload_bytes(total_len);
        self.ensure_upload_buffer_capacity(upload_base + total_len);

        let shape_off = uniform_len;
        let gradient_off = shape_off + shape_len;

        let uniforms = Self::viewport_uniforms(viewport);
        let mut upload_stats = self.frame_graph_executor.upload_buffer(
            &self.queue,
            &self.upload_buffer,
            upload_base,
            bytemuck::bytes_of(&uniforms),
        );
        upload_stats.upload_bytes += self
            .frame_graph_executor
            .upload_buffer(
                &self.queue,
                &self.upload_buffer,
                upload_base + shape_off,
                bytemuck::cast_slice(&self.scratch_shape_data),
            )
            .upload_bytes;
        if !self.scratch_gradients.is_empty() {
            upload_stats.upload_bytes += self
                .frame_graph_executor
                .upload_buffer(
                    &self.queue,
                    &self.upload_buffer,
                    upload_base + gradient_off,
                    bytemuck::cast_slice(&self.scratch_gradients),
                )
                .upload_bytes;
        }
        self.frame_stats.record_command_stats(upload_stats);

        staged_uploads.record_upload_copy(UploadTarget::Uniform, 0, 0, uniform_len);
        staged_uploads.record_upload_copy(UploadTarget::ShapeData, shape_off, 0, shape_len);
        staged_uploads.record_upload_copy(
            UploadTarget::ShapeGradient,
            gradient_off,
            0,
            gradient_len,
        );

        Some((
            PreparedShapeBatch {
                vertex_start: 0,
                vertex_count: shape_count as u32 * 6,
            },
            upload_base,
        ))
    }

    /// Whether retained replay batches can exist on this device: they bind
    /// unsized buffers, so they ride the storage-buffer batch mode only.
    /// Always `false` on wasm, which has no retained replay path — the
    /// method exists on both arches so the packet producer has one
    /// architecture.
    pub(crate) fn replay_supported(&self) -> bool {
        // Deliberately not conditioned on free slot ids: an exhausted pool
        // only means new captures fail (handled per capture), while flipping
        // this bit would retire every live feed slot.
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.shape_batch_limits.storage
        }
    }

    /// Return the planner-drained ack confirmations buffer (capacity
    /// intact) to the store after the producer applied a frame's
    /// [`crate::frame_packet::ReplayAck`] — the ack channel's half of the
    /// P4b no-allocation contract, closed by the caller now that ack
    /// application lives producer-side. No-op on wasm.
    pub(crate) fn restore_replay_ack_confirmations(
        &mut self,
        confirmations: Vec<crate::frame_packet::ReplayConfirmation>,
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.replay_ack_confirmations = confirmations;
        }
        #[cfg(target_arch = "wasm32")]
        let _ = confirmations;
    }

    /// Present-side consumption of one frame's [`ReplayFrameOps`]: frees
    /// the plan's releases, then honors its capture requests against the
    /// scene they were recorded for, answering with a [`ReplayAck`] of
    /// (identity, gpu slot) confirmations plus the batch's emptied buffers
    /// for recycling. This is the store half of the split — it touches NO
    /// planner state: `feed_slots`, confirmation stamping, displaced-slot
    /// release, and age eviction all live in the planner
    /// (`take_frame_ops`/`apply_ack`).
    ///
    /// Ordering is what makes slot release safe: a slot the plan releases
    /// is never referenced by a retained op of the same frame (misses
    /// release before their op would have been pushed, and rebuild frames
    /// release at flush start), so freeing it here — before any encoding —
    /// cannot orphan a draw.
    #[cfg(not(target_arch = "wasm32"))]
    fn consume_replay_ops(
        &mut self,
        mut ops: crate::frame_packet::ReplayFrameOps,
        shapes: &[DrawShape],
        root_scale: f32,
    ) -> (
        crate::frame_packet::ReplayAck,
        crate::frame_packet::ReplayFrameOps,
    ) {
        if ops.generation < self.store_feed_generation {
            // Fail-closed: ops planned under an OLDER slot universe name
            // slots this store does not hold. Drop the batch whole —
            // captures unconfirmed self-heal (the planner never serves
            // them), and stale releases must not free live ids.
            // Synchronously impossible today; structural for the split.
            self.replay_generation_drops += 1;
            log::warn!(
                "[command-feed] dropping replay ops of generation {} against store \
                 generation {} ({} captures, {} patches, {} releases; lifetime drops {})",
                ops.generation,
                self.store_feed_generation,
                ops.captures.len(),
                ops.color_patches.len(),
                ops.releases.len(),
                self.replay_generation_drops,
            );
            ops.captures.clear();
            ops.color_patches.clear();
            ops.releases.clear();
            return (
                crate::frame_packet::ReplayAck {
                    generation: self.store_feed_generation,
                    confirmations: Vec::new(),
                },
                ops,
            );
        }
        if ops.generation > self.store_feed_generation {
            // Adopt forward: a producer-side bump (scale change,
            // `retire_feed`) delivers its whole retirement — the releases
            // for every retired slot — THROUGH this very batch, so a
            // higher generation is the new universe arriving, not a stale
            // one. The store follows the producer's authority; it never
            // reads the producer's thread-local.
            self.store_feed_generation = ops.generation;
        }
        let generation = ops.generation;
        // Queued releases free first, so their buffers are available before
        // this frame's captures ask.
        for slot in ops.releases.drain(..) {
            self.release_replay_slot(slot);
        }
        // `take` leaves `Vec::new()` behind (no allocation); the render
        // loop restores the vec after the planner drains the ack.
        let mut confirmations = std::mem::take(&mut self.replay_ack_confirmations);
        debug_assert!(confirmations.is_empty());
        for capture in ops.captures.drain(..) {
            if capture.frame != ops.frame {
                // Defensive: a capture that outlived its frame references
                // shape indices of a scene that never rendered; honoring it
                // against THIS frame's shapes would retain wrong content
                // under a confirmed identity. Categorically drop it. Should
                // never fire now that ops travel inside the frame's own
                // packet.
                log::warn!(
                    "[command-feed] dropping stale capture for slot {} of {:?} \
                     (queued frame {}, ops frame {})",
                    capture.key.1,
                    capture.key.0,
                    capture.frame,
                    ops.frame,
                );
                continue;
            }
            let end = capture.shape_start + capture.shape_count;
            let Some(slice) = shapes.get(capture.shape_start..end) else {
                continue;
            };
            let refs: Vec<&DrawShape> = slice.iter().collect();
            let Some(gpu_slot) = self.capture_replay_slot(&refs, root_scale) else {
                continue;
            };
            confirmations.push((capture.key, gpu_slot));
        }
        // Park the frame's recolor patches for the retained prepare arms
        // (`stage_replay_patches`); the vec swapped out is last frame's,
        // already drained empty, and returns to the producer with the ack.
        // The defensive clear only bites when no prepare arm ran last
        // frame (aborted render): those patches targeted a frame that
        // never encoded, and their spans re-queue fresh recolors each
        // served frame.
        self.replay_color_patches.clear();
        std::mem::swap(&mut self.replay_color_patches, &mut ops.color_patches);
        (
            crate::frame_packet::ReplayAck {
                generation,
                confirmations,
            },
            ops,
        )
    }

    /// Test/diagnostic view of the store's lifetime count of replay-ops
    /// batches dropped whole by the generation check — the consume gate's
    /// proof that Surface frames (default plans, generation 0) are never
    /// fed to the store.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn replay_generation_drops(&self) -> u64 {
        self.replay_generation_drops
    }

    /// Test hook for the message protocol: runs one planner→store→planner
    /// replay cycle outside a frame, with the batch stamped
    /// `store_feed_generation + generation_skew`, and returns how many
    /// captures the store confirmed. A skew that lands BELOW the store's
    /// generation manufactures the fail-closed drop; a skew above it
    /// exercises adopt-forward. Both are synchronously impossible through
    /// the public render path today.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn replay_ops_roundtrip_for_tests(&mut self, generation_skew: u64) -> usize {
        let generation = self.store_feed_generation.wrapping_add(generation_skew);
        let ops = crate::shape_replay::SHAPE_REPLAY
            .with(|state| state.borrow_mut().take_frame_ops(generation));
        let (ack, recycled) = self.consume_replay_ops(ops, &[], 1.0);
        let confirmed = ack.confirmations.len();
        self.replay_ack_confirmations = crate::shape_replay::SHAPE_REPLAY
            .with(|state| state.borrow_mut().apply_ack(ack, recycled));
        confirmed
    }

    /// Stages every queued replay recolor patch. Feed recolors are always
    /// solid, so every patch rewrites the shape's 16-byte record in the
    /// slot's paint buffer; the captured `ShapeData` itself is immutable, so
    /// a recolored frame uploads colors, not geometry. Runs in the retained
    /// prepare arms so the writes land in the same staged-upload flush that
    /// carries the frame's transforms; draining is idempotent across arms.
    #[cfg(not(target_arch = "wasm32"))]
    fn stage_replay_patches(&mut self, staged_uploads: &mut StagedBufferUploads) {
        // Capacity-retaining drain: swap the frame's parked patch buffer
        // (see `consume_replay_ops`) against the scratch arena instead of
        // `mem::take`, so both keep their high-water capacity across
        // frames. The scratch is cleared before every return, which
        // preserves drain idempotence across the retained prepare arms: a
        // later drain in the same frame swaps one empty-with-capacity
        // arena for another and stages nothing.
        std::mem::swap(
            &mut self.replay_color_patches,
            &mut self.color_patch_scratch,
        );
        let total_patches = self.color_patch_scratch.len();
        if total_patches == 0 {
            self.replay_upload_stats.note_frame(0, 0, 0, 0, 0);
            return;
        }

        // Patches land in the slot's CPU mirror and upload as one contiguous
        // span per slot. Uploading each patch individually would record one
        // copy command per patch, and MEGA's twinkle field recolors ~1.7k
        // dots a frame — that many commands stall a mobile GPU for longer
        // than the spans' untouched bytes ever cost.
        #[derive(Clone, Copy)]
        struct DirtySpan {
            paint_min: u32,
            paint_max: u32,
        }
        const CLEAN: DirtySpan = DirtySpan {
            paint_min: u32::MAX,
            paint_max: 0,
        };
        let mut dirty: std::collections::HashMap<
            u32,
            DirtySpan,
            cranpose_ui_graphics::FxBuildHasher,
        > = std::collections::HashMap::default();

        // One bare 16-byte write into the slot's paint mirror per patch.
        for patch in &self.color_patch_scratch {
            let Some(slot) = self.replay_slots.slots.get_mut(&patch.slot) else {
                continue;
            };
            let Some(paint) = slot.paint_mirror.get_mut(patch.shape_index as usize) else {
                continue;
            };
            *paint = patch.color;
            let span = dirty.entry(patch.slot).or_insert(CLEAN);
            span.paint_min = span.paint_min.min(patch.shape_index);
            span.paint_max = span.paint_max.max(patch.shape_index);
        }

        let mut uploaded_records = 0u64;
        let mut uploaded_bytes = 0u64;
        let slots_touched = dirty.len() as u64;
        for (slot_id, span) in dirty {
            let Some(slot) = self.replay_slots.slots.get(&slot_id) else {
                continue;
            };
            if span.paint_min <= span.paint_max {
                let range = span.paint_min as usize..span.paint_max as usize + 1;
                uploaded_records += range.len() as u64;
                uploaded_bytes += (range.len() * std::mem::size_of::<[f32; 4]>()) as u64;
                staged_uploads.stage_at(
                    UploadTarget::ReplayPaintData(slot_id),
                    range.start as u64 * std::mem::size_of::<[f32; 4]>() as u64,
                    bytemuck::cast_slice(&slot.paint_mirror[range]),
                );
            }
        }
        // A patched color is one 16-byte vec4; the staged bytes exceed this
        // only by the untouched records inside each coalesced span.
        let ideal_bytes = total_patches as u64 * 16;
        self.replay_upload_stats.note_frame(
            total_patches as u64,
            slots_touched,
            uploaded_records,
            uploaded_bytes,
            ideal_bytes,
        );
        if cranpose_core::env_flag!("CRANPOSE_COMMAND_REPLAY_DIAG") {
            log::warn!(
                "[replay-upload] frame: {} patches -> {} records / {:.1} KB staged \
                 across {} slots (color-only {:.1} KB)",
                total_patches,
                uploaded_records,
                uploaded_bytes as f64 / 1024.0,
                slots_touched,
                ideal_bytes as f64 / 1024.0,
            );
        }
        self.color_patch_scratch.clear();
    }

    /// Converts `shape_refs` once and retains the result on the GPU as a
    /// replay slot. Returns the slot id the scene's retained draws reference.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn capture_replay_slot(
        &mut self,
        shape_refs: &[&DrawShape],
        root_scale: f32,
    ) -> Option<u32> {
        if !self.shape_batch_limits.storage || shape_refs.is_empty() {
            return None;
        }
        let id = self.replay_slots.free_ids.pop()?;
        let shape_count = shape_refs.len();

        let mut gradient_offsets: Vec<u32> = Vec::with_capacity(shape_count + 1);
        let mut total_gradient_stops = 0u32;
        gradient_offsets.push(0);
        for shape in shape_refs {
            total_gradient_stops += shape_gradient_stop_count(shape) as u32;
            gradient_offsets.push(total_gradient_stops);
        }

        let mut shape_data = vec![ShapeData::zeroed(); shape_count];
        let mut gradients = vec![GradientStop::zeroed(); (total_gradient_stops as usize).max(1)];
        convert_shapes_into_outputs(
            shape_refs,
            &gradient_offsets,
            root_scale,
            &mut shape_data,
            &mut gradients,
        );

        let shape_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Replay Shape Buffer"),
            size: (std::mem::size_of::<ShapeData>() * shape_count) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        shape_buffer
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(bytemuck::cast_slice(&shape_data));
        shape_buffer.unmap();

        let gradient_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Replay Gradient Buffer"),
            size: (std::mem::size_of::<GradientStop>() * gradients.len()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        gradient_buffer
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(bytemuck::cast_slice(&gradients));
        gradient_buffer.unmap();

        let mesh = if arc_mesh_enabled() {
            match build_arc_mesh_vertices(&shape_data) {
                Some(build) => {
                    let cut = if build.quad_area > 0.0 {
                        (1.0 - build.mesh_area / build.quad_area) * 100.0
                    } else {
                        0.0
                    };
                    // Always-on warn: `log::info` is invisible on the desktop
                    // console, and captures are rare — one line per slot
                    // lifetime. The unique-vert/index counts against the
                    // six-per-shape quad baseline are the vertex-amplification
                    // instrument P1b exists for.
                    log::warn!(
                        "[arc-mesh] slot {id}: {} arcs meshed ({} segs), {} passthrough; \
                         {} unique verts / {} indices (quad path: {} verts); \
                         quad_px {:.0} -> mesh_px {:.0} (-{:.1}%)",
                        build.meshed_arcs,
                        build.meshed_segments,
                        build.passthrough,
                        build.vertices.len(),
                        build.indices.len(),
                        shape_count * 6,
                        build.quad_area,
                        build.mesh_area,
                        cut,
                    );
                    // A slot that meshed nothing gains nothing over the
                    // indexless quad path — skip the buffers.
                    (build.meshed_arcs > 0).then(|| {
                        let vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("Replay Mesh Vertex Buffer"),
                            size: (std::mem::size_of::<MeshVertex>() * build.vertices.len()) as u64,
                            usage: wgpu::BufferUsages::VERTEX,
                            mapped_at_creation: true,
                        });
                        vertex_buffer
                            .slice(..)
                            .get_mapped_range_mut()
                            .copy_from_slice(bytemuck::cast_slice(&build.vertices));
                        vertex_buffer.unmap();
                        let index_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("Replay Mesh Index Buffer"),
                            size: (std::mem::size_of::<u32>() * build.indices.len()) as u64,
                            usage: wgpu::BufferUsages::INDEX,
                            mapped_at_creation: true,
                        });
                        index_buffer
                            .slice(..)
                            .get_mapped_range_mut()
                            .copy_from_slice(bytemuck::cast_slice(&build.indices));
                        index_buffer.unmap();
                        ReplaySlotMesh {
                            vertex_buffer,
                            index_buffer,
                            index_prefix: build.index_prefix,
                        }
                    })
                }
                None => {
                    log::warn!(
                        "[arc-mesh] slot {id}: geometry byte budget overflowed for \
                         {shape_count} shapes; whole slot falls back to quad passthrough"
                    );
                    None
                }
            }
        } else {
            None
        };

        // Seed the mutable paint from the converted colors, so an unpatched
        // replay renders bit-identically to the capture frame.
        let paint: Vec<[f32; 4]> = shape_data.iter().map(|shape| shape.color).collect();
        let paint_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Replay Paint Buffer"),
            size: (std::mem::size_of::<[f32; 4]>() * shape_count) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        paint_buffer
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(bytemuck::cast_slice(&paint));
        paint_buffer.unmap();

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Replay Shape Bind Group"),
            layout: &self.shape_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: shape_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: gradient_buffer.as_entire_binding(),
                },
                // The transform slot is selected per draw via the dynamic
                // offset, so retained draws sharing this capture can each
                // move independently.
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.replay_slots.transform_buffer,
                        offset: 0,
                        size: Some(
                            std::num::NonZeroU64::new(
                                std::mem::size_of::<SimilarityTransform>() as u64
                            )
                            .expect("similarity transform is non-empty"),
                        ),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: paint_buffer.as_entire_binding(),
                },
            ],
        });

        let capture_epoch = self.replay_slots.next_capture_epoch;
        self.replay_slots.next_capture_epoch += 1;
        self.replay_slots.slots.insert(
            id,
            ReplaySlot {
                paint_buffer,
                bind_group,
                shape_count: shape_count as u32,
                paint_mirror: paint,
                mesh,
                capture_epoch,
            },
        );
        Some(id)
    }

    /// Frees a replay slot's GPU resources and returns its id to the pool.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn release_replay_slot(&mut self, id: u32) {
        if self.replay_slots.slots.remove(&id).is_some() {
            self.replay_slots.free_ids.push(id);
            // A cached bundle keeps references on the slot buffers it binds.
            // The epoch in each key already makes entries for this capture
            // unreachable — releases are rare (churn, retire_feed), so drop
            // the whole cache and free those references now rather than one
            // frame later through eviction.
            self.retained_bundle_cache.clear();
        }
    }

    /// Test/diagnostic view of the latched instanced-quad selection: `true`
    /// when this renderer's ordinary shape draws ride `vs_shape_instanced`.
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn instanced_quads_active(&self) -> bool {
        self.instanced_quads.is_some()
    }

    /// Test/diagnostic view of retained arc meshes: how many live replay
    /// slots hold a mesh, out of all live slots.
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn replay_slot_mesh_stats(&self) -> (usize, usize) {
        let meshed = self
            .replay_slots
            .slots
            .values()
            .filter(|slot| slot.mesh.is_some())
            .count();
        (meshed, self.replay_slots.slots.len())
    }

    /// Draws one retained replay batch — `retained`'s shape range of its
    /// slot's capture, under the transform staged for this draw's index (see
    /// the retained arms of the segment paths).
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_retained_batch(
        &self,
        render_pass: &mut wgpu::RenderPass<'_>,
        retained: &RetainedDraw,
        retained_index: usize,
        width: u32,
        height: u32,
    ) {
        let Some(slot) = self.replay_slots.slots.get(&retained.slot) else {
            return;
        };
        if retained_index as u32 >= MAX_REPLAY_SLOTS {
            return;
        }
        let first = retained.first_shape.min(slot.shape_count);
        let last = retained
            .first_shape
            .saturating_add(retained.shape_count)
            .min(slot.shape_count);
        if first >= last {
            return;
        }
        self.frame_stats.bump_shapes();
        self.frame_stats.add_draw_calls(1);
        render_pass.set_scissor_rect(0, 0, width, height);
        // A captured mesh replaces the six-per-shape quad expansion with the
        // slot's conservative arc mesh — same bind groups, same SrcOver
        // blend, one draw per op over the identical shape range, so z order
        // is untouched either way. Slots without a mesh draw through the
        // latched instanced-quad path when it exists (four vertex executions
        // per shape, shape index from the instance index), else the plain
        // six-vertex expansion.
        let mesh = slot.mesh.as_ref().map(|mesh| (mesh, self.mesh_pipeline()));
        match &mesh {
            Some((_, mesh_pipeline)) => render_pass.set_pipeline(mesh_pipeline),
            None => match &self.instanced_quads {
                Some(instanced) => {
                    render_pass.set_pipeline(self.instanced_pipeline(instanced, BlendMode::SrcOver))
                }
                None => render_pass.set_pipeline(self.shape_pipeline(BlendMode::SrcOver)),
            },
        }
        render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        render_pass.set_bind_group(
            1,
            &slot.bind_group,
            &[retained_index as u32 * REPLAY_TRANSFORM_STRIDE as u32],
        );
        match mesh {
            Some((mesh, _)) => {
                render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                render_pass
                    .set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(
                    mesh.index_prefix[first as usize]..mesh.index_prefix[last as usize],
                    0,
                    0..1,
                );
            }
            None => match &self.instanced_quads {
                Some(instanced) => {
                    render_pass.set_index_buffer(
                        instanced.index_buffer.slice(..),
                        wgpu::IndexFormat::Uint16,
                    );
                    render_pass.draw_indexed(0..6, 0, first..last);
                }
                None => render_pass.draw(first * 6..last * 6, 0..1),
            },
        }
    }

    /// Key of the retained stretch at `item_range`: one op key per resolved
    /// retained item, in draw order, carrying exactly the state that decides
    /// the commands [`Self::draw_retained_batch`] would encode for it —
    /// clamped range, dynamic-offset index, mesh-vs-quad pipeline choice,
    /// and the slot's capture epoch (`None` while the slot is absent, when
    /// the op draws nothing on the direct path too).
    #[cfg(not(target_arch = "wasm32"))]
    fn retained_bundle_key(
        &self,
        ordered_items: &[(usize, SegmentDrawItem)],
        retained_draws: &[RetainedDraw],
        item_range: Range<usize>,
    ) -> RetainedBundleKey {
        let mut ops = Vec::with_capacity(item_range.len());
        for (_, item) in &ordered_items[item_range] {
            let SegmentDrawItem::Retained(index) = item else {
                continue;
            };
            let Some(retained) = retained_draws.get(*index) else {
                continue;
            };
            let slot = self.replay_slots.slots.get(&retained.slot);
            let (first, last) = match slot {
                Some(slot) => (
                    retained.first_shape.min(slot.shape_count),
                    retained
                        .first_shape
                        .saturating_add(retained.shape_count)
                        .min(slot.shape_count),
                ),
                None => (
                    retained.first_shape,
                    retained.first_shape.saturating_add(retained.shape_count),
                ),
            };
            ops.push(RetainedBundleOpKey {
                slot: retained.slot,
                capture_epoch: slot.map(|slot| slot.capture_epoch),
                first,
                last,
                retained_index: *index as u32,
                has_mesh: slot.is_some_and(|slot| slot.mesh.is_some())
                    && self.shape_batch_limits.storage,
            });
        }
        RetainedBundleKey { ops }
    }

    /// Encodes `key`'s stretch into a render bundle: the IDENTICAL command
    /// sequence [`Self::draw_retained_batch`] issues on the pass, minus the
    /// scissor reset (bundles cannot set scissor; the caller sets the same
    /// full-target scissor on the pass before executing). Must only be
    /// called with a key built this frame, so every op with an epoch still
    /// resolves to its slot.
    #[cfg(not(target_arch = "wasm32"))]
    fn build_retained_bundle(&self, key: &RetainedBundleKey) -> wgpu::RenderBundle {
        let mut encoder =
            self.device
                .create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor {
                    label: Some("Retained Stretch Bundle"),
                    // Every fused-pass target — the swapchain, screenshot
                    // textures, pooled layer surfaces — is created with the
                    // renderer's one surface format.
                    color_formats: &[Some(self.surface_format)],
                    depth_stencil: None,
                    sample_count: 1,
                    multiview: None,
                });
        for op in &key.ops {
            if op.capture_epoch.is_none()
                || op.retained_index >= MAX_REPLAY_SLOTS
                || op.first >= op.last
            {
                continue;
            }
            let Some(slot) = self.replay_slots.slots.get(&op.slot) else {
                continue;
            };
            let mesh = slot.mesh.as_ref().map(|mesh| (mesh, self.mesh_pipeline()));
            match &mesh {
                Some((_, mesh_pipeline)) => encoder.set_pipeline(mesh_pipeline),
                None => match &self.instanced_quads {
                    Some(instanced) => {
                        encoder.set_pipeline(self.instanced_pipeline(instanced, BlendMode::SrcOver))
                    }
                    None => encoder.set_pipeline(self.shape_pipeline(BlendMode::SrcOver)),
                },
            }
            encoder.set_bind_group(0, &self.uniform_bind_group, &[]);
            encoder.set_bind_group(
                1,
                &slot.bind_group,
                &[op.retained_index * REPLAY_TRANSFORM_STRIDE as u32],
            );
            match mesh {
                Some((mesh, _)) => {
                    encoder.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    encoder
                        .set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    encoder.draw_indexed(
                        mesh.index_prefix[op.first as usize]..mesh.index_prefix[op.last as usize],
                        0,
                        0..1,
                    );
                }
                // The latched selection is a per-renderer constant, so it
                // needs no place in `RetainedBundleOpKey` — every cached
                // bundle in this renderer's lifetime encodes the same choice
                // the direct path makes.
                None => match &self.instanced_quads {
                    Some(instanced) => {
                        encoder.set_index_buffer(
                            instanced.index_buffer.slice(..),
                            wgpu::IndexFormat::Uint16,
                        );
                        encoder.draw_indexed(0..6, 0, op.first..op.last);
                    }
                    None => encoder.draw(op.first * 6..op.last * 6, 0..1),
                },
            }
        }
        encoder.finish(&wgpu::RenderBundleDescriptor {
            label: Some("Retained Stretch Bundle"),
        })
    }

    /// Draws one maximal consecutive retained stretch through the bundle
    /// cache: key the stretch, rebuild on any mismatch (recapture, reorder,
    /// range or count change, slot release), then execute the cached bundle.
    /// Replays byte-identical commands to the per-op direct path.
    /// `stage_replay_patches` and the per-frame transform staging stay in
    /// the prepare arms, untouched — bundles bind buffers whose contents are
    /// read at execution.
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_retained_stretch_bundled(
        &mut self,
        render_pass: &mut wgpu::RenderPass<'_>,
        ordered_items: &[(usize, SegmentDrawItem)],
        retained_draws: &[RetainedDraw],
        item_range: Range<usize>,
        width: u32,
        height: u32,
    ) {
        let key = self.retained_bundle_key(ordered_items, retained_draws, item_range);
        if !self.retained_bundle_cache.hit(&key) {
            let bundle = self.build_retained_bundle(&key);
            self.retained_bundle_cache.insert(key.clone(), bundle);
        }
        // Mirror the direct path's per-op stats for every op the bundle
        // draws, so bundling is invisible to the frame counters.
        for op in &key.ops {
            if op.capture_epoch.is_some()
                && op.retained_index < MAX_REPLAY_SLOTS
                && op.first < op.last
            {
                self.frame_stats.bump_shapes();
                self.frame_stats.add_draw_calls(1);
            }
        }
        // Bundles inherit the pass scissor: set the same full-target rect
        // the direct path sets before every retained draw. Executing the
        // bundle then resets pipeline/bind/vertex state, which is harmless —
        // every following fused arm re-binds its own.
        render_pass.set_scissor_rect(0, 0, width, height);
        if let Some(bundle) = self.retained_bundle_cache.get(&key) {
            render_pass.execute_bundles(std::iter::once(bundle));
        }
    }

    /// Test/diagnostic view of the retained bundle cache: lifetime
    /// (rebuilds, cached executes).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn retained_bundle_stats(&self) -> (u64, u64) {
        self.retained_bundle_cache.stats()
    }

    fn draw_prepared_shapes(
        &self,
        render_pass: &mut wgpu::RenderPass<'_>,
        blend_mode: BlendMode,
        batch: PreparedShapeBatch,
        width: u32,
        height: u32,
    ) {
        self.frame_stats.bump_shapes();
        self.frame_stats.add_draw_calls(1);
        render_pass.set_scissor_rect(0, 0, width, height);
        #[cfg(not(target_arch = "wasm32"))]
        let (uniform_bind_group, shape_buffers) = (&self.uniform_bind_group, &self.shape_buffers);
        #[cfg(target_arch = "wasm32")]
        let (uniform_bind_group, shape_buffers) = (
            &self.wasm_uniform_batches[batch.uniform_slot].bind_group,
            &self.wasm_shape_batches[batch.shape_slot],
        );
        // Latched instanced path (storage mode only): one instance per
        // shape, four vertices through the static quad index buffer —
        // identical triangles, identical bind groups, still one draw call.
        // The uniform/WebGL path never latches it and stays on `vs_main`.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(instanced) = &self.instanced_quads {
            assert!(
                batch.vertex_start.is_multiple_of(6) && batch.vertex_count.is_multiple_of(6),
                "shape batches are whole shapes: vertex range {}..+{} must be \
                 six-aligned to convert to an instance range",
                batch.vertex_start,
                batch.vertex_count,
            );
            render_pass.set_pipeline(self.instanced_pipeline(instanced, blend_mode));
            render_pass.set_bind_group(0, uniform_bind_group, &[]);
            // Dynamic offset 0: ordinary batches read the identity
            // similarity transform.
            render_pass.set_bind_group(1, &shape_buffers.bind_group, &[0]);
            let first_shape = batch.vertex_start / 6;
            let shape_count = batch.vertex_count / 6;
            render_pass
                .set_index_buffer(instanced.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..6, 0, first_shape..first_shape + shape_count);
            return;
        }
        render_pass.set_pipeline(self.shape_pipeline(blend_mode));
        render_pass.set_bind_group(0, uniform_bind_group, &[]);
        // Dynamic offset 0: ordinary batches read the identity similarity
        // transform.
        render_pass.set_bind_group(1, &shape_buffers.bind_group, &[0]);
        // Six unindexed vertices per shape; `vs_main` derives the corner from
        // `vertex_index` and pulls the quad out of `ShapeData`.
        render_pass.draw(
            batch.vertex_start..batch.vertex_start + batch.vertex_count,
            0..1,
        );
    }

    /// Stage shape buffer writes and record a shape render pass onto the
    /// provided encoder. The caller is responsible for submitting.
    #[allow(clippy::too_many_arguments)]
    fn encode_shapes_pass<'a, I, C: FrameCommandRecorder>(
        &mut self,
        frame_encoder: &mut C,
        target_view: &wgpu::TextureView,
        layer_shapes: I,
        blend_mode: BlendMode,
        width: u32,
        height: u32,
        root_scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        viewport_offset: [f32; 2],
    ) where
        I: Iterator<Item = &'a DrawShape>,
    {
        let mut staged_uploads = self.take_staged_uploads();
        let viewport = ViewportUniformParams {
            width,
            height,
            offset: viewport_offset,
        };
        let Some(batch) = self.prepare_shapes_batch(
            layer_shapes
                .filter(|shape| shape_draw_is_visible_in_viewport(shape, viewport, root_scale)),
            root_scale,
            viewport,
            &mut staged_uploads,
        ) else {
            self.restore_staged_uploads(staged_uploads);
            return;
        };
        let upload_offset =
            frame_encoder.allocate_staged_upload_bytes(staged_uploads.bytes.len() as u64);
        self.flush_staged_uploads_at(frame_encoder.encoder(), &staged_uploads, upload_offset);
        self.restore_staged_uploads(staged_uploads);
        let mut render_pass =
            frame_encoder
                .encoder()
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Shape Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: load_op,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
        self.draw_prepared_shapes(&mut render_pass, blend_mode, batch, width, height);
    }

    fn draw_prepared_images(
        &mut self,
        render_pass: &mut wgpu::RenderPass<'_>,
        batch: &PreparedImageBatch,
        blend_mode: BlendMode,
    ) -> Result<(), String> {
        if batch.cmds.is_empty() {
            return Ok(());
        }
        self.frame_stats.bump_images();
        self.frame_stats.add_draw_calls(batch.cmds.len() as u32);
        render_pass.set_pipeline(self.image_pipeline(blend_mode));
        #[cfg(not(target_arch = "wasm32"))]
        let (uniform_bind_group, vertex_buffer, index_buffer) = (
            &self.uniform_bind_group,
            &self.image_vertex_buffer,
            &self.image_index_buffer,
        );
        #[cfg(target_arch = "wasm32")]
        let (uniform_bind_group, vertex_buffer, index_buffer) = (
            &self.wasm_uniform_batches[batch.uniform_slot].bind_group,
            &self.wasm_image_batches[batch.image_slot].vertex_buffer,
            &self.wasm_image_batches[batch.image_slot].index_buffer,
        );
        render_pass.set_bind_group(0, uniform_bind_group, &[]);
        render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));

        for cmd in &batch.cmds {
            let (sx, sy, sw, sh) = cmd.scissor;
            render_pass.set_scissor_rect(sx, sy, sw, sh);

            let cached = self
                .image_texture_cache
                .get(&cmd.image_id)
                .ok_or_else(|| "image texture missing from cache".to_string())?;
            render_pass.set_bind_group(1, cached.bind_group(cmd.sampling), &[]);
            render_pass.draw_indexed(cmd.index_start..(cmd.index_start + 6), 0, 0..1);
        }
        Ok(())
    }

    fn draw_prepared_glyphs(
        &mut self,
        render_pass: &mut wgpu::RenderPass<'_>,
        batch: &PreparedGlyphBatch,
    ) -> Result<(), String> {
        if batch.cmds.is_empty() {
            return Ok(());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.draw_native_prepared_glyph_cmd_range(
                render_pass,
                &batch.cmds,
                0..batch.cmds.len(),
            )?;
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.frame_stats.bump_text();
            self.frame_stats.add_draw_calls(batch.cmds.len() as u32);
            render_pass.set_pipeline(self.glyph_atlas_pipeline());
            let (uniform_bind_group, vertex_buffer, index_buffer) = (
                &self.wasm_uniform_batches[batch.uniform_slot].bind_group,
                &self.wasm_image_batches[batch.image_slot].vertex_buffer,
                &self.wasm_image_batches[batch.image_slot].index_buffer,
            );
            render_pass.set_bind_group(0, uniform_bind_group, &[]);
            render_pass.set_bind_group(1, &self.text_glyph_atlas.bind_group, &[]);
            render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));

            for cmd in &batch.cmds {
                let (sx, sy, sw, sh) = cmd.scissor;
                render_pass.set_scissor_rect(sx, sy, sw, sh);
                let GlyphDrawSource::Shared {
                    index_start,
                    index_count,
                } = cmd.source;
                render_pass.draw_indexed(index_start..(index_start + index_count), 0, 0..1);
            }
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn draw_native_prepared_image_cmd_range(
        &mut self,
        render_pass: &mut wgpu::RenderPass<'_>,
        cmds: &[ImageDrawCmd],
        cmd_range: Range<usize>,
        blend_mode: BlendMode,
    ) -> Result<(), String> {
        let Some(cmds) = cmds.get(cmd_range) else {
            return Err("image command range is outside the prepared command buffer".to_string());
        };
        if cmds.is_empty() {
            return Ok(());
        }

        self.frame_stats.bump_images();
        self.frame_stats.add_draw_calls(cmds.len() as u32);
        render_pass.set_pipeline(self.image_pipeline(blend_mode));
        render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        render_pass.set_index_buffer(self.image_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.set_vertex_buffer(0, self.image_vertex_buffer.slice(..));

        for cmd in cmds {
            let (sx, sy, sw, sh) = cmd.scissor;
            render_pass.set_scissor_rect(sx, sy, sw, sh);

            let cached = self
                .image_texture_cache
                .get(&cmd.image_id)
                .ok_or_else(|| "image texture missing from cache".to_string())?;
            render_pass.set_bind_group(1, cached.bind_group(cmd.sampling), &[]);
            render_pass.draw_indexed(cmd.index_start..(cmd.index_start + 6), 0, 0..1);
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn draw_native_prepared_glyph_cmd_range(
        &mut self,
        render_pass: &mut wgpu::RenderPass<'_>,
        cmds: &[GlyphDrawCmd],
        cmd_range: Range<usize>,
    ) -> Result<(), String> {
        let Some(cmds) = cmds.get(cmd_range) else {
            return Err("glyph command range is outside the prepared command buffer".to_string());
        };
        if cmds.is_empty() {
            return Ok(());
        }

        self.frame_stats.bump_text();
        self.frame_stats.add_draw_calls(cmds.len() as u32);

        let mut shared_buffers_bound = false;
        let mut retained_pipeline_bound = false;
        for cmd in cmds {
            let (sx, sy, sw, sh) = cmd.scissor;
            render_pass.set_scissor_rect(sx, sy, sw, sh);
            match cmd.source {
                GlyphDrawSource::Shared {
                    index_start,
                    index_count,
                } => {
                    if retained_pipeline_bound || !shared_buffers_bound {
                        render_pass.set_pipeline(self.glyph_atlas_pipeline());
                        render_pass.set_bind_group(1, &self.text_glyph_atlas.bind_group, &[]);
                        retained_pipeline_bound = false;
                    }
                    if !shared_buffers_bound {
                        render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                        render_pass.set_index_buffer(
                            self.image_index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        render_pass.set_vertex_buffer(0, self.image_vertex_buffer.slice(..));
                        shared_buffers_bound = true;
                    }
                    render_pass.draw_indexed(index_start..(index_start + index_count), 0, 0..1);
                }
                GlyphDrawSource::Retained {
                    cache_key,
                    uniform_slot,
                } => {
                    shared_buffers_bound = false;
                    if !retained_pipeline_bound {
                        render_pass.set_pipeline(self.retained_glyph_atlas_pipeline());
                        render_pass.set_bind_group(1, &self.text_glyph_atlas.bind_group, &[]);
                        retained_pipeline_bound = true;
                    }
                    let cached = self
                        .text_glyph_gpu_run_cache
                        .peek(&cache_key)
                        .ok_or_else(|| "retained glyph buffer missing from cache".to_string())?;
                    let dynamic_offset =
                        self.retained_glyph_uniform_dynamic_offset(uniform_slot)?;
                    render_pass.set_bind_group(
                        0,
                        &self.retained_glyph_uniform_bind_group,
                        &[dynamic_offset],
                    );
                    render_pass
                        .set_index_buffer(cached.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.set_vertex_buffer(0, cached.vertex_buffer.slice(..));
                    render_pass.draw_indexed(0..cached.index_count, 0, 0..1);
                }
            }
        }
        Ok(())
    }

    fn append_image_draw_cmd(
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
        let Some(scissor) =
            scissor_rect_for_image(&adjusted_image, root_scale, viewport.width, viewport.height)
        else {
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

    #[cfg(not(target_arch = "wasm32"))]
    fn stage_native_image_buffers(
        &mut self,
        staged_uploads: &mut StagedBufferUploads,
        viewport: ViewportUniformParams,
        image_vertices: &[Vertex],
        image_indices: &[u32],
    ) {
        if image_indices.is_empty() {
            return;
        }

        self.stage_viewport_uniforms(staged_uploads, viewport);
        // Grow to a power of two, as the shape batch and frame upload buffers
        // do. Sizing these to the exact byte count instead means one more glyph
        // quad than the last frame destroys and recreates both buffers, and a
        // caption that grows a character at a time does it on every frame.
        let needed_bytes = std::mem::size_of_val(image_vertices) as u64;
        if needed_bytes > self.image_vertex_buffer.size() {
            self.image_vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Image Vertex Buffer"),
                size: needed_bytes.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let needed_index_bytes = std::mem::size_of_val(image_indices) as u64;
        if needed_index_bytes > self.image_index_buffer.size() {
            self.image_index_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Image Index Buffer"),
                size: needed_index_bytes.next_power_of_two(),
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        staged_uploads.stage(
            UploadTarget::ImageVertex,
            bytemuck::cast_slice(image_vertices),
        );
        staged_uploads.stage(
            UploadTarget::ImageIndex,
            bytemuck::cast_slice(image_indices),
        );
    }

    /// Prepare image vertices, indices, ensure caching, and write to GPU buffers.
    /// Returns the draw commands needed by `encode_images_pass`.
    fn prepare_image_draw_cmds<'a, I>(
        &mut self,
        layer_images: I,
        viewport: ViewportUniformParams,
        root_scale: f32,
        staged_uploads: &mut StagedBufferUploads,
    ) -> Result<PreparedImageBatch, String>
    where
        I: Iterator<Item = &'a ImageDraw>,
    {
        #[cfg(target_arch = "wasm32")]
        let _ = staged_uploads;

        let mut image_vertices = std::mem::take(&mut self.scratch_image_vertices);
        let mut image_indices = std::mem::take(&mut self.scratch_image_indices);
        let mut image_cmds = std::mem::take(&mut self.scratch_image_cmds);
        image_vertices.clear();
        image_indices.clear();
        image_cmds.clear();

        for image_draw in layer_images {
            self.append_image_draw_cmd(
                image_draw,
                viewport,
                root_scale,
                &mut image_vertices,
                &mut image_indices,
                &mut image_cmds,
            )?;
        }

        #[cfg(not(target_arch = "wasm32"))]
        if !image_cmds.is_empty() {
            self.stage_native_image_buffers(
                staged_uploads,
                viewport,
                &image_vertices,
                &image_indices,
            );
        }

        #[cfg(target_arch = "wasm32")]
        let image_slot = if image_cmds.is_empty() {
            0
        } else {
            let slot = self.claim_wasm_image_batch();
            {
                let buffers = &mut self.wasm_image_batches[slot];
                buffers.ensure_capacity(&self.device, image_vertices.len(), image_indices.len());
            }
            let buffers = &self.wasm_image_batches[slot];
            self.write_wasm_buffer(
                &buffers.vertex_buffer,
                bytemuck::cast_slice(&image_vertices),
            );
            self.write_wasm_buffer(&buffers.index_buffer, bytemuck::cast_slice(&image_indices));
            slot
        };

        #[cfg(target_arch = "wasm32")]
        let uniform_slot = if image_cmds.is_empty() {
            0
        } else {
            self.prepare_wasm_viewport_uniforms(viewport)
        };

        self.scratch_image_vertices = image_vertices;
        self.scratch_image_indices = image_indices;
        Ok(PreparedImageBatch {
            cmds: image_cmds,
            #[cfg(target_arch = "wasm32")]
            image_slot,
            #[cfg(target_arch = "wasm32")]
            uniform_slot,
        })
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
                // Read the size after the entry is in hand: the only path that
                // resizes the atlas is the overflow reset, which returns `Err`
                // above, so `entry` is always normalised against the atlas it
                // was placed in.
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

    #[cfg(not(target_arch = "wasm32"))]
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

    #[cfg(not(target_arch = "wasm32"))]
    fn retained_text_glyph_run_ready(&mut self, cache_key: TextGlyphRunCacheKey) -> bool {
        let atlas_generation = self.text_glyph_atlas.generation();
        self.text_glyph_gpu_run_cache
            .peek(&cache_key)
            .is_some_and(|cached| cached.atlas_generation == atlas_generation)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    fn emit_retained_text_glyph_run_if_ready(
        &mut self,
        cache_key: TextGlyphRunCacheKey,
        quads: &[CachedTextGlyphQuad],
        clip: Option<Rect>,
        viewport: ViewportUniformParams,
        source_raster_rect: Rect,
        scissor: (u32, u32, u32, u32),
        staged_uploads: &mut StagedBufferUploads,
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

        let uniform_slot = self.stage_retained_glyph_viewport_uniforms(
            staged_uploads,
            Self::retained_glyph_viewport(viewport, source_raster_rect),
        );
        glyph_cmds.push(GlyphDrawCmd::retained(cache_key, uniform_slot, scissor));
        true
    }

    #[cfg(not(target_arch = "wasm32"))]
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
        let vertex_upload =
            self.frame_graph_executor
                .upload_buffer(&self.queue, &vertex_buffer, 0, vertex_bytes);
        self.frame_stats.record_command_stats(vertex_upload);
        let index_upload =
            self.frame_graph_executor
                .upload_buffer(&self.queue, &index_buffer, 0, index_bytes);
        self.frame_stats.record_command_stats(index_upload);

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

    #[allow(clippy::too_many_arguments)]
    fn append_text_glyph_draws<'a, I>(
        &mut self,
        layer_texts: I,
        viewport: ViewportUniformParams,
        root_scale: f32,
        allow_offscreen_prewarm: bool,
        staged_uploads: &mut StagedBufferUploads,
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
        let initial_staged_bytes_len = staged_uploads.bytes.len();
        let initial_staged_copies_len = staged_uploads.copies.len();
        let mut collected_run = std::mem::take(&mut self.scratch_text_glyph_run);
        let mut collected_placements = std::mem::take(&mut self.scratch_text_glyph_placements);
        let mut generated_quads = std::mem::take(&mut self.scratch_text_glyph_quads);
        generated_quads.clear();
        let mut visited = 0usize;
        let mut emitted_glyphs = 0usize;
        let mut prewarmed_glyphs = 0usize;
        let mut run_hits = 0usize;
        let mut run_misses = 0usize;

        for text_draw in layer_texts {
            visited = visited.saturating_add(1);
            let Some((logical_rect, raster_rect, clip, text_scale, static_text_motion)) =
                self.text_raster_geometry(text_draw, root_scale)
            else {
                continue;
            };
            if !static_text_motion {
                image_vertices.truncate(initial_vertex_len);
                image_indices.truncate(initial_index_len);
                glyph_cmds.truncate(initial_cmd_len);
                staged_uploads.truncate(initial_staged_bytes_len, initial_staged_copies_len);
                self.scratch_text_glyph_run = collected_run;
                self.scratch_text_glyph_placements = collected_placements;
                self.scratch_text_glyph_quads = generated_quads;
                return Ok(false);
            }
            let is_visible =
                text_draw_is_visible_in_viewport(logical_rect, clip, viewport, root_scale);
            let draw_action = text_glyph_draw_action(
                is_visible,
                text_draw_should_prewarm_in_viewport(logical_rect, clip, viewport, root_scale),
                allow_offscreen_prewarm,
            );
            if draw_action == TextGlyphDrawAction::Skip {
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
            let mut miss_collect_ms = None;
            let mut miss_cached_glyphs = 0usize;
            let mut miss_new_glyphs = 0usize;
            let cached_glyph_run = if let Some(cached) = self.text_glyph_run_cache.get(&run_key) {
                run_hits = run_hits.saturating_add(1);
                if cached.atlas_generation == atlas_generation {
                    cached_quad_run = cached.quads.as_ref().map(Rc::clone);
                }
                Some(Rc::clone(&cached.glyphs))
            } else {
                run_misses = run_misses.saturating_add(1);
                collected_run.clear();
                let collect_start = Instant::now();
                let collect_result = collect_solid_text_atlas_run(
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
                miss_collect_ms = Some(instant_ms(collect_start, Instant::now()));
                if collect_result.is_none() {
                    if text_atlas_fallback_diag_enabled() {
                        let preview: String = source_draw.text.text.chars().take(96).collect();
                        log::warn!(
                            "[text-atlas-fallback] node={:?} visible={} prewarm={} spans={} links={} text_len={} preview={:?} span_style={:?} paragraph_style={:?}",
                            source_draw.node_id,
                            is_visible,
                            draw_action == TextGlyphDrawAction::PrewarmOffscreen,
                            source_draw.text.span_styles.len(),
                            source_draw.text.links.len(),
                            source_draw.text.text.len(),
                            preview,
                            source_draw.text_style.span_style,
                            source_draw.text_style.paragraph_style,
                        );
                    }
                    if draw_action == TextGlyphDrawAction::PrewarmOffscreen {
                        continue;
                    }
                    image_vertices.truncate(initial_vertex_len);
                    image_indices.truncate(initial_index_len);
                    glyph_cmds.truncate(initial_cmd_len);
                    staged_uploads.truncate(initial_staged_bytes_len, initial_staged_copies_len);
                    self.scratch_text_glyph_run = collected_run;
                    self.scratch_text_glyph_placements = collected_placements;
                    self.scratch_text_glyph_quads = generated_quads;
                    return Ok(false);
                }
                if text_glyph_run_diag_enabled() {
                    miss_cached_glyphs = collected_run
                        .iter()
                        .filter(|glyph| matches!(glyph, SoftwareGlyphAtlasRunGlyph::Cached(_)))
                        .count();
                    miss_new_glyphs = collected_run.len().saturating_sub(miss_cached_glyphs);
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

            if draw_action == TextGlyphDrawAction::PrewarmOffscreen {
                let prewarm_quads = if let Some(quad_run) = cached_quad_run {
                    quad_run
                } else {
                    let prepare_start = Instant::now();
                    match self.prepare_text_glyph_quads(
                        run_key,
                        atlas_generation,
                        cached_glyph_run.as_deref(),
                        &collected_run,
                        &mut generated_quads,
                    ) {
                        Ok(quads) => {
                            if let Some(collect_ms) = miss_collect_ms {
                                if text_glyph_run_diag_enabled() {
                                    log::warn!(
                                        "[text-glyph-run-diag] visible=false glyphs={} cached={} new={} collect_ms={:.2} prepare_ms={:.2}",
                                        quads.len(),
                                        miss_cached_glyphs,
                                        miss_new_glyphs,
                                        collect_ms,
                                        instant_ms(prepare_start, Instant::now()),
                                    );
                                }
                            }
                            quads
                        }
                        Err(_) => continue,
                    }
                };
                #[cfg(not(target_arch = "wasm32"))]
                if should_use_retained_text_glyph_run(prewarm_quads.len(), source_draw.clip) {
                    self.ensure_retained_text_glyph_run(run_key, prewarm_quads.as_ref());
                }
                prewarmed_glyphs = prewarmed_glyphs.saturating_add(prewarm_quads.len());
                continue;
            }

            let draw_rect = Rect {
                x: source_raster_rect.x / root_scale,
                y: source_raster_rect.y / root_scale,
                width: source_raster_rect.width / root_scale,
                height: source_raster_rect.height / root_scale,
            };
            let Some(scissor) = scissor_rect_for_layer(
                draw_rect,
                source_draw.clip,
                root_scale,
                viewport.width,
                viewport.height,
            ) else {
                continue;
            };

            #[cfg(not(target_arch = "wasm32"))]
            if let Some(quad_run) = cached_quad_run.as_ref() {
                if should_use_retained_text_glyph_run(quad_run.len(), source_draw.clip)
                    && self.emit_retained_text_glyph_run_if_ready(
                        run_key,
                        quad_run.as_ref(),
                        source_draw.clip,
                        viewport,
                        source_raster_rect,
                        scissor,
                        staged_uploads,
                        glyph_cmds,
                    )
                {
                    emitted_glyphs = emitted_glyphs.saturating_add(quad_run.len());
                    continue;
                }
            }

            let index_start = image_indices.len() as u32;
            if let Some(quad_run) = cached_quad_run {
                emitted_glyphs = emitted_glyphs.saturating_add(self.append_text_glyph_quad_run(
                    source_raster_rect,
                    quad_run.as_ref(),
                    source_draw.clip,
                    viewport,
                    root_scale,
                    image_vertices,
                    image_indices,
                    true,
                ));
            } else {
                let prepare_start = Instant::now();
                let Ok(quad_run) = self.prepare_text_glyph_quads(
                    run_key,
                    atlas_generation,
                    cached_glyph_run.as_deref(),
                    &collected_run,
                    &mut generated_quads,
                ) else {
                    image_vertices.truncate(initial_vertex_len);
                    image_indices.truncate(initial_index_len);
                    glyph_cmds.truncate(initial_cmd_len);
                    staged_uploads.truncate(initial_staged_bytes_len, initial_staged_copies_len);
                    self.scratch_text_glyph_run = collected_run;
                    self.scratch_text_glyph_placements = collected_placements;
                    self.scratch_text_glyph_quads = generated_quads;
                    return Ok(false);
                };
                if let Some(collect_ms) = miss_collect_ms {
                    if text_glyph_run_diag_enabled() {
                        log::warn!(
                            "[text-glyph-run-diag] visible=true glyphs={} cached={} new={} collect_ms={:.2} prepare_ms={:.2}",
                            quad_run.len(),
                            miss_cached_glyphs,
                            miss_new_glyphs,
                            collect_ms,
                            instant_ms(prepare_start, Instant::now()),
                        );
                    }
                }
                emitted_glyphs = emitted_glyphs.saturating_add(self.append_text_glyph_quad_run(
                    source_raster_rect,
                    quad_run.as_ref(),
                    source_draw.clip,
                    viewport,
                    root_scale,
                    image_vertices,
                    image_indices,
                    false,
                ));
            }
            let index_count = image_indices.len() as u32 - index_start;
            if index_count > 0 {
                glyph_cmds.push(GlyphDrawCmd::shared(index_start, index_count, scissor));
            }
        }

        self.scratch_text_glyph_run = collected_run;
        self.scratch_text_glyph_placements = collected_placements;
        self.scratch_text_glyph_quads = generated_quads;
        let append_end = Instant::now();
        if let Some(total_ms) = should_log_wgpu_render_stage(append_start, append_end) {
            log::warn!(
                "[wgpu-render-stage:text-glyph-atlas] total_ms={total_ms:.2} visited={} cmds={} glyphs={} prewarmed={} run_hits={} run_misses={}",
                visited,
                glyph_cmds.len().saturating_sub(initial_cmd_len),
                emitted_glyphs,
                prewarmed_glyphs,
                run_hits,
                run_misses,
            );
        }
        Ok(true)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn text_glyph_prewarm_decision(
        &self,
        text_draw: &TextDraw,
        viewport: ViewportUniformParams,
        root_scale: f32,
    ) -> TextGlyphPrewarmDecision {
        let Some((logical_rect, _, clip, _, static_text_motion)) =
            self.text_raster_geometry(text_draw, root_scale)
        else {
            return TextGlyphPrewarmDecision::MissingGeometry;
        };
        if !static_text_motion {
            return TextGlyphPrewarmDecision::DynamicMotion;
        }
        if text_draw_is_visible_in_viewport(logical_rect, clip, viewport, root_scale) {
            return TextGlyphPrewarmDecision::Visible;
        }
        if text_draw_should_prewarm_in_viewport(logical_rect, clip, viewport, root_scale) {
            TextGlyphPrewarmDecision::Candidate
        } else {
            TextGlyphPrewarmDecision::OutsidePrewarmWindow
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    fn prewarm_offscreen_text_glyph_draws_in_chunk(
        &mut self,
        ordered_items: &[(usize, SegmentDrawItem)],
        texts: &[TextDraw],
        chunk: &SegmentDrawChunkPlan,
        viewport: ViewportUniformParams,
        root_scale: f32,
        staged_uploads: &mut StagedBufferUploads,
        image_vertices: &mut Vec<Vertex>,
        image_indices: &mut Vec<u32>,
        glyph_cmds: &mut Vec<GlyphDrawCmd>,
    ) -> Result<(), String> {
        let prewarm_start = Instant::now();
        let diag_enabled = cranpose_core::env_flag!("CRANPOSE_TEXT_PREWARM_DIAG");
        let mut text_items = 0usize;
        let mut candidates = 0usize;
        let mut missing_geometry = 0usize;
        let mut dynamic_motion = 0usize;
        let mut visible = 0usize;
        let mut outside = 0usize;
        let mut already_prepared = 0usize;
        let mut admitted_candidates = 0usize;
        let mut skipped_unbounded = 0usize;
        let mut skipped_budget = 0usize;
        let initial_vertex_len = image_vertices.len();
        let initial_index_len = image_indices.len();
        let initial_cmd_len = glyph_cmds.len();
        let initial_staged_bytes_len = staged_uploads.bytes.len();
        let initial_staged_copies_len = staged_uploads.copies.len();
        'batches: for batch in chunk.iter() {
            let SegmentBatchPlan::Text { start, end } = batch else {
                continue;
            };
            for (_, item) in &ordered_items[start..end] {
                if offscreen_text_glyph_prewarm_budget_exhausted(prewarm_start, admitted_candidates)
                {
                    skipped_budget = skipped_budget.saturating_add(1);
                    break 'batches;
                }
                let SegmentDrawItem::Text(text_index) = item else {
                    return Err(format!(
                        "text prewarm batch contains non-text draw item: {item:?}"
                    ));
                };
                let Some(text_draw) = texts.get(*text_index) else {
                    continue;
                };
                text_items = text_items.saturating_add(1);
                match self.text_glyph_prewarm_decision(text_draw, viewport, root_scale) {
                    TextGlyphPrewarmDecision::Candidate => {}
                    TextGlyphPrewarmDecision::MissingGeometry => {
                        missing_geometry = missing_geometry.saturating_add(1);
                        continue;
                    }
                    TextGlyphPrewarmDecision::DynamicMotion => {
                        dynamic_motion = dynamic_motion.saturating_add(1);
                        continue;
                    }
                    TextGlyphPrewarmDecision::Visible => {
                        visible = visible.saturating_add(1);
                        continue;
                    }
                    TextGlyphPrewarmDecision::OutsidePrewarmWindow => {
                        outside = outside.saturating_add(1);
                        continue;
                    }
                }

                candidates = candidates.saturating_add(1);
                let Some((_, raster_rect, _, text_scale, static_text_motion)) =
                    self.text_raster_geometry(text_draw, root_scale)
                else {
                    missing_geometry = missing_geometry.saturating_add(1);
                    continue;
                };
                let raster_source = text_glyph_raster_source(text_draw, raster_rect);
                let source_draw = raster_source.draw.as_ref();
                let run_key = Self::text_glyph_run_cache_key(
                    source_draw,
                    raster_source.raster_rect,
                    text_scale,
                    static_text_motion,
                );
                let atlas_generation = self.text_glyph_atlas.generation();
                let cached_glyphs = if let Some(cached) = self.text_glyph_run_cache.peek(&run_key) {
                    if cached.atlas_generation == atlas_generation && cached.quads.is_some() {
                        already_prepared = already_prepared.saturating_add(1);
                        continue;
                    }
                    Some(cached.glyphs.len())
                } else {
                    None
                };
                if !offscreen_text_glyph_prewarm_work_is_bounded(
                    cached_glyphs,
                    source_draw.text.text.len(),
                ) {
                    skipped_unbounded = skipped_unbounded.saturating_add(1);
                    continue;
                }
                admitted_candidates = admitted_candidates.saturating_add(1);
                self.append_text_glyph_draws(
                    std::iter::once(text_draw),
                    viewport,
                    root_scale,
                    true,
                    staged_uploads,
                    image_vertices,
                    image_indices,
                    glyph_cmds,
                )?;
                image_vertices.truncate(initial_vertex_len);
                image_indices.truncate(initial_index_len);
                glyph_cmds.truncate(initial_cmd_len);
                staged_uploads.truncate(initial_staged_bytes_len, initial_staged_copies_len);
            }
        }

        if diag_enabled && text_items > 0 {
            log::warn!(
                "[text-glyph-prewarm-diag] texts={text_items} candidates={candidates} admitted={admitted_candidates} cached={already_prepared} skipped_unbounded={skipped_unbounded} skipped_budget={skipped_budget} visible={visible} outside={outside} dynamic={dynamic_motion} missing={missing_geometry}"
            );
        }
        if admitted_candidates > 0 {
            if let Some(total_ms) = should_log_wgpu_render_stage(prewarm_start, Instant::now()) {
                log::warn!(
                    "[wgpu-render-stage:text-glyph-prewarm] total_ms={total_ms:.2} candidates={candidates} admitted={admitted_candidates} cached={already_prepared} skipped_unbounded={skipped_unbounded} skipped_budget={skipped_budget}"
                );
            }
        }
        Ok(())
    }

    fn prepare_text_glyph_draw_cmds<'a, I>(
        &mut self,
        layer_texts: I,
        viewport: ViewportUniformParams,
        root_scale: f32,
        staged_uploads: &mut StagedBufferUploads,
    ) -> Result<Option<PreparedGlyphBatch>, String>
    where
        I: IntoIterator<Item = &'a TextDraw>,
    {
        #[cfg(target_arch = "wasm32")]
        let _ = staged_uploads;

        let mut image_vertices = std::mem::take(&mut self.scratch_image_vertices);
        let mut image_indices = std::mem::take(&mut self.scratch_image_indices);
        let mut glyph_cmds = std::mem::take(&mut self.scratch_glyph_cmds);
        image_vertices.clear();
        image_indices.clear();
        glyph_cmds.clear();

        if !self.append_text_glyph_draws(
            layer_texts,
            viewport,
            root_scale,
            false,
            staged_uploads,
            &mut image_vertices,
            &mut image_indices,
            &mut glyph_cmds,
        )? {
            self.scratch_image_vertices = image_vertices;
            self.scratch_image_indices = image_indices;
            self.scratch_glyph_cmds = glyph_cmds;
            return Ok(None);
        }

        #[cfg(not(target_arch = "wasm32"))]
        if !image_indices.is_empty() {
            self.stage_native_image_buffers(
                staged_uploads,
                viewport,
                &image_vertices,
                &image_indices,
            );
        }

        #[cfg(target_arch = "wasm32")]
        let image_slot = if glyph_cmds.is_empty() {
            0
        } else {
            let slot = self.claim_wasm_image_batch();
            {
                let buffers = &mut self.wasm_image_batches[slot];
                buffers.ensure_capacity(&self.device, image_vertices.len(), image_indices.len());
            }
            let buffers = &self.wasm_image_batches[slot];
            self.write_wasm_buffer(
                &buffers.vertex_buffer,
                bytemuck::cast_slice(&image_vertices),
            );
            self.write_wasm_buffer(&buffers.index_buffer, bytemuck::cast_slice(&image_indices));
            slot
        };

        #[cfg(target_arch = "wasm32")]
        let uniform_slot = if glyph_cmds.is_empty() {
            0
        } else {
            self.prepare_wasm_viewport_uniforms(viewport)
        };

        self.scratch_image_vertices = image_vertices;
        self.scratch_image_indices = image_indices;
        Ok(Some(PreparedGlyphBatch {
            cmds: glyph_cmds,
            #[cfg(target_arch = "wasm32")]
            image_slot,
            #[cfg(target_arch = "wasm32")]
            uniform_slot,
        }))
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

        let Some(scissor) = scissor_rect_for_layer(
            scissor_rect,
            clip,
            root_scale,
            viewport.width,
            viewport.height,
        ) else {
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
    fn append_text_image_draw_cmds<'a, I>(
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

    fn prepare_text_image_draw_cmds<'a, I>(
        &mut self,
        layer_texts: I,
        viewport: ViewportUniformParams,
        root_scale: f32,
        staged_uploads: &mut StagedBufferUploads,
    ) -> Result<PreparedImageBatch, String>
    where
        I: Iterator<Item = &'a TextDraw>,
    {
        #[cfg(target_arch = "wasm32")]
        let _ = staged_uploads;

        let mut image_vertices = std::mem::take(&mut self.scratch_image_vertices);
        let mut image_indices = std::mem::take(&mut self.scratch_image_indices);
        let mut image_cmds = std::mem::take(&mut self.scratch_image_cmds);
        image_vertices.clear();
        image_indices.clear();
        image_cmds.clear();

        self.append_text_image_draw_cmds(
            layer_texts,
            viewport,
            root_scale,
            &mut image_vertices,
            &mut image_indices,
            &mut image_cmds,
        )?;

        #[cfg(not(target_arch = "wasm32"))]
        if !image_cmds.is_empty() {
            self.stage_native_image_buffers(
                staged_uploads,
                viewport,
                &image_vertices,
                &image_indices,
            );
        }

        #[cfg(target_arch = "wasm32")]
        let image_slot = if image_cmds.is_empty() {
            0
        } else {
            let slot = self.claim_wasm_image_batch();
            {
                let buffers = &mut self.wasm_image_batches[slot];
                buffers.ensure_capacity(&self.device, image_vertices.len(), image_indices.len());
            }
            let buffers = &self.wasm_image_batches[slot];
            self.write_wasm_buffer(
                &buffers.vertex_buffer,
                bytemuck::cast_slice(&image_vertices),
            );
            self.write_wasm_buffer(&buffers.index_buffer, bytemuck::cast_slice(&image_indices));
            slot
        };

        #[cfg(target_arch = "wasm32")]
        let uniform_slot = if image_cmds.is_empty() {
            0
        } else {
            self.prepare_wasm_viewport_uniforms(viewport)
        };

        self.scratch_image_vertices = image_vertices;
        self.scratch_image_indices = image_indices;
        Ok(PreparedImageBatch {
            cmds: image_cmds,
            #[cfg(target_arch = "wasm32")]
            image_slot,
            #[cfg(target_arch = "wasm32")]
            uniform_slot,
        })
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

#[cfg(test)]
fn clipped_text_raster_source<'a>(
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
    let line_starts = line_start_offsets(text_draw.text.text.as_str());
    clipped_text_raster_source_with_line_starts(
        text_draw,
        logical_rect,
        raster_rect,
        clip,
        root_scale,
        &line_starts,
    )
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

#[cfg(not(target_arch = "wasm32"))]
fn align_usize_to(value: usize, alignment: usize) -> usize {
    debug_assert!(alignment > 0);
    value.div_ceil(alignment) * alignment
}

impl GpuRenderer {
    fn convert_surface_pixels_to_rgba(&self, pixels: &mut [u8]) -> Result<(), String> {
        match self.surface_format {
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => Ok(()),
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                for pixel in pixels.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
                Ok(())
            }
            format => Err(format!(
                "Screenshot readback unsupported for texture format: {format:?}"
            )),
        }
    }
}

fn is_in_effect_range(z_index: usize, effect_z_ranges: &[Range<usize>]) -> bool {
    effect_z_ranges.iter().any(|range| range.contains(&z_index))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegmentDrawItem {
    Shape(usize),
    Image(usize),
    Text(usize),
    Shadow(usize),
    Composite(usize),
    ShaderComposite(usize),
    Retained(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegmentBatchPlan {
    Shape {
        start: usize,
        end: usize,
        blend_mode: BlendMode,
    },
    Image {
        start: usize,
        end: usize,
        blend_mode: BlendMode,
    },
    Text {
        start: usize,
        end: usize,
    },
    Composite {
        start: usize,
        end: usize,
    },
    ShaderComposite {
        start: usize,
        end: usize,
    },
    /// Retained replay batches: each item is one bind + draw of GPU slots
    /// captured on an earlier frame, so they never merge and cost no budget.
    Retained {
        start: usize,
        end: usize,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SegmentDrawChunkPlan {
    batches: Vec<SegmentBatchPlan>,
}

struct SegmentRenderOutcome {
    rendered_any: bool,
    pass_count: u32,
}

struct SegmentCommandEncodeOutcome {
    first_batch: bool,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextGlyphPrewarmDecision {
    Candidate,
    MissingGeometry,
    DynamicMotion,
    Visible,
    OutsidePrewarmWindow,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NativeSegmentFusionBudget {
    shape_count: usize,
    gradient_stop_count: usize,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeSegmentFusionPartition {
    chunk: SegmentDrawChunkPlan,
    budget: NativeSegmentFusionBudget,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq)]
enum FusedSegmentBatch {
    Shape {
        batch: PreparedShapeBatch,
        blend_mode: BlendMode,
    },
    Image {
        cmd_range: Range<usize>,
        blend_mode: BlendMode,
    },
    Text {
        image_cmd_range: Range<usize>,
        glyph_cmd_range: Range<usize>,
    },
    Composite {
        draw_range: Range<usize>,
    },
    ShaderComposite {
        draw_range: Range<usize>,
    },
    Retained {
        item_range: Range<usize>,
    },
}

struct ShadowSourceRenderOutcome {
    rendered_any: bool,
    pass_count: u32,
}

impl SegmentDrawChunkPlan {
    fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    fn push(&mut self, batch: SegmentBatchPlan) {
        self.batches.push(batch);
    }

    fn iter(&self) -> impl Iterator<Item = SegmentBatchPlan> + '_ {
        self.batches.iter().copied()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SegmentRenderCommand {
    DrawChunk(SegmentDrawChunkPlan),
    Shadow(usize),
}

struct SegmentCommandIter<'a> {
    ordered_items: &'a [(usize, SegmentDrawItem)],
    shapes: &'a [DrawShape],
    images: &'a [ImageDraw],
    cursor: usize,
    batch_limits: ShapeBatchLimits,
}

impl<'a> SegmentCommandIter<'a> {
    fn new(
        ordered_items: &'a [(usize, SegmentDrawItem)],
        shapes: &'a [DrawShape],
        images: &'a [ImageDraw],
        batch_limits: ShapeBatchLimits,
    ) -> Self {
        Self {
            ordered_items,
            shapes,
            images,
            cursor: 0,
            batch_limits,
        }
    }
}

impl Iterator for SegmentCommandIter<'_> {
    type Item = SegmentRenderCommand;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor >= self.ordered_items.len() {
            return None;
        }

        if let SegmentDrawItem::Shadow(index) = self.ordered_items[self.cursor].1 {
            self.cursor += 1;
            return Some(SegmentRenderCommand::Shadow(index));
        }

        let mut chunk = SegmentDrawChunkPlan::default();
        while self.cursor < self.ordered_items.len() {
            if let SegmentDrawItem::Shadow(index) = self.ordered_items[self.cursor].1 {
                if chunk.is_empty() {
                    self.cursor += 1;
                    return Some(SegmentRenderCommand::Shadow(index));
                }
                break;
            }

            let Some((batch, next_cursor)) = segment_batch_plan_at_cursor(
                self.ordered_items,
                self.shapes,
                self.images,
                self.cursor,
                self.batch_limits,
            ) else {
                break;
            };
            chunk.push(batch);
            self.cursor = next_cursor;
        }

        Some(SegmentRenderCommand::DrawChunk(chunk))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreparedShapeBatch {
    /// First vertex and vertex count for the unindexed shape draw; always
    /// multiples of 6 so `vs_main`'s `vertex_index / 6` lands on whole shapes.
    vertex_start: u32,
    vertex_count: u32,
    #[cfg(target_arch = "wasm32")]
    shape_slot: usize,
    #[cfg(target_arch = "wasm32")]
    uniform_slot: usize,
}

struct PreparedImageBatch {
    cmds: Vec<ImageDrawCmd>,
    #[cfg(target_arch = "wasm32")]
    image_slot: usize,
    #[cfg(target_arch = "wasm32")]
    uniform_slot: usize,
}

impl PreparedImageBatch {
    fn is_empty(&self) -> bool {
        self.cmds.is_empty()
    }

    fn into_cmds(self) -> Vec<ImageDrawCmd> {
        self.cmds
    }
}

struct PreparedGlyphBatch {
    cmds: Vec<GlyphDrawCmd>,
    #[cfg(target_arch = "wasm32")]
    image_slot: usize,
    #[cfg(target_arch = "wasm32")]
    uniform_slot: usize,
}

impl PreparedGlyphBatch {
    fn is_empty(&self) -> bool {
        self.cmds.is_empty()
    }

    fn into_cmds(self) -> Vec<GlyphDrawCmd> {
        self.cmds
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn gradient_stop_count_for_shape(shape: &DrawShape) -> usize {
    match &shape.brush {
        Brush::Solid(_) => 0,
        Brush::LinearGradient { colors, .. }
        | Brush::RadialGradient { colors, .. }
        | Brush::SweepGradient { colors, .. } => colors.len(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_segment_fusion_budget(
    ordered_items: &[(usize, SegmentDrawItem)],
    shapes: &[DrawShape],
    chunk: &SegmentDrawChunkPlan,
    batch_limits: ShapeBatchLimits,
) -> Result<Option<NativeSegmentFusionBudget>, String> {
    let mut shape_count = 0usize;
    let mut gradient_stop_count = 0usize;

    for batch in chunk.iter() {
        let SegmentBatchPlan::Shape { start, end, .. } = batch else {
            continue;
        };
        for (_, item) in &ordered_items[start..end] {
            let SegmentDrawItem::Shape(shape_index) = item else {
                return Err(format!(
                    "shape batch contains non-shape draw item: {item:?}"
                ));
            };
            let shape = &shapes[*shape_index];
            shape_count = shape_count.saturating_add(1);
            gradient_stop_count =
                gradient_stop_count.saturating_add(gradient_stop_count_for_shape(shape));
        }
    }

    if shape_count > batch_limits.max_shapes_per_batch
        || gradient_stop_count > batch_limits.max_gradient_stops
    {
        return Ok(None);
    }

    Ok(Some(NativeSegmentFusionBudget {
        shape_count,
        gradient_stop_count,
    }))
}

#[cfg(not(target_arch = "wasm32"))]
fn push_native_segment_fusion_partition(
    partitions: &mut Vec<NativeSegmentFusionPartition>,
    current: &mut SegmentDrawChunkPlan,
    current_budget: &mut NativeSegmentFusionBudget,
) {
    if current.is_empty() {
        return;
    }

    partitions.push(NativeSegmentFusionPartition {
        chunk: std::mem::take(current),
        budget: *current_budget,
    });
    *current_budget = NativeSegmentFusionBudget {
        shape_count: 0,
        gradient_stop_count: 0,
    };
}

#[cfg(not(target_arch = "wasm32"))]
fn native_segment_fusion_partitions(
    ordered_items: &[(usize, SegmentDrawItem)],
    shapes: &[DrawShape],
    chunk: &SegmentDrawChunkPlan,
    batch_limits: ShapeBatchLimits,
) -> Result<Option<Vec<NativeSegmentFusionPartition>>, String> {
    if let Some(budget) = native_segment_fusion_budget(ordered_items, shapes, chunk, batch_limits)?
    {
        return Ok(Some(vec![NativeSegmentFusionPartition {
            chunk: chunk.clone(),
            budget,
        }]));
    }

    let mut partitions = Vec::new();
    let mut current = SegmentDrawChunkPlan::default();
    let mut current_budget = NativeSegmentFusionBudget {
        shape_count: 0,
        gradient_stop_count: 0,
    };

    for batch in chunk.iter() {
        let SegmentBatchPlan::Shape {
            start,
            end,
            blend_mode,
        } = batch
        else {
            current.push(batch);
            continue;
        };

        let mut run_start = start;
        for (item_cursor, (_, item)) in ordered_items.iter().enumerate().take(end).skip(start) {
            let SegmentDrawItem::Shape(shape_index) = *item else {
                return Err(format!(
                    "shape batch contains non-shape draw item: {:?}",
                    item
                ));
            };
            let gradient_stop_count = gradient_stop_count_for_shape(&shapes[shape_index]);
            if gradient_stop_count > batch_limits.max_gradient_stops {
                return Ok(None);
            }

            let fits_shape_count =
                current_budget.shape_count.saturating_add(1) <= batch_limits.max_shapes_per_batch;
            let fits_gradient_count = current_budget
                .gradient_stop_count
                .saturating_add(gradient_stop_count)
                <= batch_limits.max_gradient_stops;
            if !fits_shape_count || !fits_gradient_count {
                if run_start < item_cursor {
                    current.push(SegmentBatchPlan::Shape {
                        start: run_start,
                        end: item_cursor,
                        blend_mode,
                    });
                }
                push_native_segment_fusion_partition(
                    &mut partitions,
                    &mut current,
                    &mut current_budget,
                );
                run_start = item_cursor;
            }

            current_budget.shape_count = current_budget.shape_count.saturating_add(1);
            current_budget.gradient_stop_count = current_budget
                .gradient_stop_count
                .saturating_add(gradient_stop_count);
        }

        if run_start < end {
            current.push(SegmentBatchPlan::Shape {
                start: run_start,
                end,
                blend_mode,
            });
        }
    }

    push_native_segment_fusion_partition(&mut partitions, &mut current, &mut current_budget);
    Ok(Some(partitions))
}

fn segment_batch_plan_at_cursor(
    ordered_items: &[(usize, SegmentDrawItem)],
    shapes: &[DrawShape],
    images: &[ImageDraw],
    start: usize,
    batch_limits: ShapeBatchLimits,
) -> Option<(SegmentBatchPlan, usize)> {
    match ordered_items[start].1 {
        SegmentDrawItem::Shape(index) => {
            let blend_mode = supported_blend_mode(shapes[index].blend_mode);
            let mut end = start + 1;
            let shape_limit = (start + batch_limits.max_shapes_per_batch).min(ordered_items.len());
            while end < shape_limit {
                match ordered_items[end].1 {
                    SegmentDrawItem::Shape(next_index)
                        if supported_blend_mode(shapes[next_index].blend_mode) == blend_mode =>
                    {
                        end += 1;
                    }
                    _ => break,
                }
            }
            Some((
                SegmentBatchPlan::Shape {
                    start,
                    end,
                    blend_mode,
                },
                end,
            ))
        }
        SegmentDrawItem::Image(index) => {
            let blend_mode = supported_blend_mode(images[index].blend_mode);
            let mut end = start + 1;
            while end < ordered_items.len() {
                match ordered_items[end].1 {
                    SegmentDrawItem::Image(next_index)
                        if supported_blend_mode(images[next_index].blend_mode) == blend_mode =>
                    {
                        end += 1;
                    }
                    _ => break,
                }
            }
            Some((
                SegmentBatchPlan::Image {
                    start,
                    end,
                    blend_mode,
                },
                end,
            ))
        }
        SegmentDrawItem::Text(_) => {
            let mut end = start + 1;
            while end < ordered_items.len() {
                if matches!(ordered_items[end].1, SegmentDrawItem::Text(_)) {
                    end += 1;
                } else {
                    break;
                }
            }
            Some((SegmentBatchPlan::Text { start, end }, end))
        }
        SegmentDrawItem::Composite(_) => {
            let mut end = start + 1;
            while end < ordered_items.len() {
                if matches!(ordered_items[end].1, SegmentDrawItem::Composite(_)) {
                    end += 1;
                } else {
                    break;
                }
            }
            Some((SegmentBatchPlan::Composite { start, end }, end))
        }
        SegmentDrawItem::ShaderComposite(_) => {
            let mut end = start + 1;
            while end < ordered_items.len() {
                if matches!(ordered_items[end].1, SegmentDrawItem::ShaderComposite(_)) {
                    end += 1;
                } else {
                    break;
                }
            }
            Some((SegmentBatchPlan::ShaderComposite { start, end }, end))
        }
        SegmentDrawItem::Retained(_) => {
            let mut end = start + 1;
            while end < ordered_items.len() {
                if matches!(ordered_items[end].1, SegmentDrawItem::Retained(_)) {
                    end += 1;
                } else {
                    break;
                }
            }
            Some((SegmentBatchPlan::Retained { start, end }, end))
        }
        SegmentDrawItem::Shadow(_) => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_non_effect_segment_items(
    shapes: &[DrawShape],
    _images: &[ImageDraw],
    _texts: &[TextDraw],
    _shadow_draws: &[ShadowDraw],
    draw_ops: &[DrawOp],
    z_start: usize,
    z_end: usize,
    effect_z_ranges: &[Range<usize>],
    width: u32,
    height: u32,
    root_scale: f32,
    scratch: &mut Vec<(usize, SegmentDrawItem)>,
) {
    scratch.clear();
    let viewport = ViewportUniformParams {
        width,
        height,
        offset: [0.0, 0.0],
    };

    scratch.extend(draw_ops.iter().filter_map(|op| {
        if op.z_index < z_start
            || op.z_index >= z_end
            || is_in_effect_range(op.z_index, effect_z_ranges)
        {
            return None;
        }
        let item = match op.kind {
            DrawOpKind::Shape(index) => {
                let shape = shapes.get(index)?;
                if !shape_draw_is_visible_in_viewport(shape, viewport, root_scale) {
                    return None;
                }
                SegmentDrawItem::Shape(index)
            }
            DrawOpKind::Image(index) => SegmentDrawItem::Image(index),
            DrawOpKind::Text(index) => SegmentDrawItem::Text(index),
            DrawOpKind::Shadow(index) => SegmentDrawItem::Shadow(index),
            DrawOpKind::Retained(index) => SegmentDrawItem::Retained(index),
        };
        Some((op.z_index, item))
    }));
}

fn retain_renderable_shadow_items(
    ordered_items: &mut Vec<(usize, SegmentDrawItem)>,
    shadow_draws: &[ShadowDraw],
    width: u32,
    height: u32,
    root_scale: f32,
    max_texture_dim: u32,
) -> usize {
    let original_len = ordered_items.len();
    ordered_items.retain(|(_, item)| match item {
        SegmentDrawItem::Shadow(index) => shadow_draws.get(*index).is_some_and(|shadow| {
            shadow_draw_may_render(shadow, width, height, root_scale, max_texture_dim)
        }),
        _ => true,
    });
    original_len.saturating_sub(ordered_items.len())
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
struct SegmentDiagCounts {
    raw_shadow_items: usize,
    culled_shadow_items: usize,
    cached_shadow_composites: usize,
    composite_items: usize,
    shader_composite_items: usize,
}

#[cfg(not(target_arch = "wasm32"))]
fn maybe_print_segment_diag(
    z_range: Range<usize>,
    ordered_items: &[(usize, SegmentDrawItem)],
    shapes: &[DrawShape],
    images: &[ImageDraw],
    counts: SegmentDiagCounts,
    batch_limits: ShapeBatchLimits,
) {
    if !cranpose_core::env_flag!("CRANPOSE_SEGMENT_DIAG") {
        return;
    }
    let line = SEGMENT_DIAG_LINES.fetch_add(1, Ordering::Relaxed);
    if line >= 64 {
        return;
    }

    let remaining_shadow_items = ordered_items
        .iter()
        .filter(|(_, item)| matches!(item, SegmentDrawItem::Shadow(_)))
        .count();
    let commands: Vec<_> =
        SegmentCommandIter::new(ordered_items, shapes, images, batch_limits).collect();
    let draw_chunks = commands
        .iter()
        .filter(|command| matches!(command, SegmentRenderCommand::DrawChunk(_)))
        .count();
    let shadow_commands = commands
        .iter()
        .filter(|command| matches!(command, SegmentRenderCommand::Shadow(_)))
        .count();
    let mut native_partitions = 0usize;
    let mut native_unfused_chunks = 0usize;
    for command in &commands {
        let SegmentRenderCommand::DrawChunk(chunk) = command else {
            continue;
        };
        match native_segment_fusion_partitions(ordered_items, shapes, chunk, batch_limits) {
            Ok(Some(partitions)) => native_partitions += partitions.len(),
            Ok(None) | Err(_) => native_unfused_chunks += 1,
        }
    }

    eprintln!(
        "[segment-diag #{line}] z={}..{} items={} raw_shadows={} culled_shadows={} cached_shadows={} remaining_shadows={} composites={} shader_composites={} draw_chunks={} shadow_commands={} native_partitions={} native_unfused_chunks={}",
        z_range.start,
        z_range.end,
        ordered_items.len(),
        counts.raw_shadow_items,
        counts.culled_shadow_items,
        counts.cached_shadow_composites,
        remaining_shadow_items,
        counts.composite_items,
        counts.shader_composite_items,
        draw_chunks,
        shadow_commands,
        native_partitions,
        native_unfused_chunks,
    );
}

pub(crate) fn has_backdrop_layer_in_range(
    backdrop_layers: &[BackdropLayer],
    z_start: usize,
    z_end: usize,
) -> bool {
    backdrop_layers
        .iter()
        .any(|layer| layer.z_index >= z_start && layer.z_index < z_end)
}

pub(crate) fn scissor_rect_for_rect(
    rect: Rect,
    root_scale: f32,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let mut left = canonicalize_device_coordinate(rect.x * root_scale);
    let mut top = canonicalize_device_coordinate(rect.y * root_scale);
    let mut right = canonicalize_device_coordinate((rect.x + rect.width) * root_scale);
    let mut bottom = canonicalize_device_coordinate((rect.y + rect.height) * root_scale);

    left = left.max(0.0).min(width as f32).floor();
    top = top.max(0.0).min(height as f32).floor();
    right = right.max(0.0).min(width as f32).ceil();
    bottom = bottom.max(0.0).min(height as f32).ceil();

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
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let clipped_rect = match clip {
        Some(clip_rect) => rect.intersect(clip_rect)?,
        None => rect,
    };

    scissor_rect_for_rect(clipped_rect, root_scale, width, height)
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

/// Normalises an atlas entry against `atlas_size`, the side length of the
/// texture the entry was placed in. The atlas grows on overflow, so the size
/// has to be read from the live atlas rather than a constant — a UV computed
/// against the wrong size samples the wrong glyph.
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
    for pixel in image.pixels().chunks_exact(4) {
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
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    scissor_rect_for_layer(image.rect, image.clip, root_scale, width, height)
}

fn inner_shadow_composite_mask(
    shadow: &ShadowDraw,
    root_scale: f32,
) -> Option<RoundedCompositeMask> {
    if !shadow
        .shapes
        .iter()
        .any(|(_, mode)| *mode == BlendMode::DstOut)
    {
        return None;
    }
    let (fill, _) = shadow.shapes.first()?;
    let rect = fill.local_rect;
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }

    let radii = fill.shape.map_or([0.0; 4], |rounded| {
        let resolved = rounded.resolve(rect.width, rect.height);
        [
            resolved.top_left * root_scale,
            resolved.top_right * root_scale,
            resolved.bottom_left * root_scale,
            resolved.bottom_right * root_scale,
        ]
    });

    Some(RoundedCompositeMask {
        rect: [
            rect.x * root_scale,
            rect.y * root_scale,
            rect.width * root_scale,
            rect.height * root_scale,
        ],
        radii,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalized_scene::visible_draw_rect;
    use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope, LazyListState};
    use cranpose_render_common::graph::{DrawPrimitiveNode, IsolationReasons, TextPrimitiveNode};
    use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
    use cranpose_render_common::scene_builder::build_graph_from_applier;
    use cranpose_ui::text::{
        AnnotatedString, BaselineShift, RangeStyle, Shadow, SpanStyle, TextDecoration,
        TextDrawStyle, TextGeometricTransform, TextMotion, TextUnit,
    };
    use cranpose_ui::{
        LayoutEngine, LazyColumn, LazyColumnSpec, Modifier, Size, Text, TextLayoutOptions,
        TextStyle,
    };
    use cranpose_ui_graphics::{
        Brush, Color, CornerRadii, DrawPrimitive, Rect, RenderEffect, RoundedCornerShape,
        RuntimeShader,
    };

    fn chunk(batches: &[SegmentBatchPlan]) -> SegmentDrawChunkPlan {
        let mut chunk = SegmentDrawChunkPlan::default();
        for batch in batches {
            chunk.push(*batch);
        }
        chunk
    }

    fn with_test_app_context<R>(block: impl FnOnce() -> R) -> R {
        let app_context = cranpose_ui::AppContext::new();
        app_context.enter(block)
    }

    fn assert_snap_anchor_close(actual: Option<SnapAnchor>, expected_origin: Point, message: &str) {
        let Some(actual) = actual else {
            panic!("{message}: missing snap anchor");
        };
        let expected = SnapAnchor::rigid(expected_origin);
        assert_eq!(
            actual.device_pixel_step, expected.device_pixel_step,
            "{message}: device pixel step changed"
        );
        assert!(
            (actual.origin.x - expected.origin.x).abs() <= 1e-4
                && (actual.origin.y - expected.origin.y).abs() <= 1e-4,
            "{message}: expected origin {:?}, got {:?}",
            expected.origin,
            actual.origin
        );
    }

    fn effect_layer(z_start: usize, z_end: usize) -> EffectLayer {
        EffectLayer {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            clip: None,
            snap_anchor: None,
            effect: Some(RenderEffect::blur(4.0)),
            blend_mode: BlendMode::SrcOver,
            composite_alpha: 1.0,
            z_start,
            z_end,
            requirements: SurfaceRequirementSet::default().with(SurfaceRequirement::RenderEffect),
        }
    }

    #[test]
    fn direct_shader_composite_accepts_box4_when_viewport_preserves_source_pixels() {
        assert_eq!(
            direct_shader_composite_viewport(
                1.0,
                BlendMode::SrcOver,
                Some((12.0, 18.0, 64.0, 32.0)),
                CompositeSampleMode::Box4,
                (64, 32),
            ),
            Some((12.0, 18.0, 64.0, 32.0))
        );
    }

    #[test]
    fn direct_shader_composite_rejects_box4_when_viewport_resamples_source() {
        assert_eq!(
            direct_shader_composite_viewport(
                1.0,
                BlendMode::SrcOver,
                Some((12.0, 18.0, 64.5, 32.0)),
                CompositeSampleMode::Box4,
                (64, 32),
            ),
            None
        );
        assert_eq!(
            direct_shader_composite_viewport(
                1.0,
                BlendMode::SrcOver,
                Some((12.25, 18.0, 64.0, 32.0)),
                CompositeSampleMode::Box4,
                (64, 32),
            ),
            None
        );
    }

    fn test_text_draw(rect: Rect, text_motion: TextMotion) -> TextDraw {
        let mut text_style = TextStyle::default();
        text_style.paragraph_style.text_motion = Some(text_motion);
        TextDraw {
            node_id: 42,
            rect,
            snap_anchor: None,
            translated_content_context: false,
            text: Arc::new(AnnotatedString::new("stable markdown row".to_string()).render_string()),
            color: Color::WHITE,
            text_style,
            font_size: 14.0,
            scale: 1.0,
            layout_options: TextLayoutOptions::default(),
            z_index: 0,
            clip: None,
        }
    }

    #[test]
    fn static_text_image_cache_key_ignores_absolute_scroll_position() {
        let base = test_text_draw(
            Rect {
                x: 12.25,
                y: 40.75,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Static,
        );
        let scrolled = test_text_draw(
            Rect {
                x: 12.75,
                y: -318.5,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Static,
        );

        let base_key = GpuRenderer::text_image_cache_key(&base, base.rect, 1.0, true);
        let scrolled_key = GpuRenderer::text_image_cache_key(&scrolled, scrolled.rect, 1.0, true);

        assert_eq!(
            base_key, scrolled_key,
            "scrolling static text must reuse the same raster cache entry"
        );
    }

    #[test]
    fn static_text_glyph_run_cache_key_ignores_absolute_scroll_position() {
        let base = test_text_draw(
            Rect {
                x: 12.25,
                y: 40.75,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Static,
        );
        let scrolled = test_text_draw(
            Rect {
                x: 12.75,
                y: -318.5,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Static,
        );

        let base_key = GpuRenderer::text_glyph_run_cache_key(&base, base.rect, 1.0, true);
        let scrolled_key =
            GpuRenderer::text_glyph_run_cache_key(&scrolled, scrolled.rect, 1.0, true);

        assert_eq!(
            base_key, scrolled_key,
            "scrolling static text must reuse the same retained glyph run"
        );
    }

    #[test]
    fn static_multiline_text_glyph_source_keeps_full_text_when_image_source_slices() {
        let rect = Rect {
            x: 8.0,
            y: 100.0,
            width: 240.0,
            height: 1_000.0,
        };
        let mut draw = test_text_draw(rect, TextMotion::Static);
        let lines = (0..100)
            .map(|line| format!("line-{line:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        draw.text = Arc::new(AnnotatedString::from(lines).render_string());

        let raster_rect = Rect {
            x: 16.0,
            y: 200.0,
            width: 480.0,
            height: 2_000.0,
        };
        let clipped = clipped_text_raster_source(
            &draw,
            rect,
            raster_rect,
            Some(Rect {
                x: 0.0,
                y: 610.0,
                width: 800.0,
                height: 40.0,
            }),
            2.0,
            true,
        );
        let glyph = text_glyph_raster_source(&draw, raster_rect);

        assert!(
            matches!(clipped.draw, Cow::Owned(_)),
            "the image source should still slice large clipped multiline text"
        );
        assert!(
            matches!(glyph.draw, Cow::Borrowed(_)),
            "the glyph source must keep a stable full-text run key while scrolling"
        );

        let clipped_key = GpuRenderer::text_glyph_run_cache_key(
            clipped.draw.as_ref(),
            clipped.raster_rect,
            2.0,
            true,
        );
        let glyph_key = GpuRenderer::text_glyph_run_cache_key(
            glyph.draw.as_ref(),
            glyph.raster_rect,
            2.0,
            true,
        );

        assert_ne!(
            clipped_key, glyph_key,
            "image slicing must not force glyph rendering onto per-scroll line-window cache keys"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn retained_glyph_viewport_offsets_relative_vertices_by_source_origin() {
        let viewport = ViewportUniformParams {
            width: 800,
            height: 600,
            offset: [10.0, 20.0],
        };
        let source = Rect {
            x: 40.0,
            y: 90.0,
            width: 120.0,
            height: 48.0,
        };

        let retained = GpuRenderer::retained_glyph_viewport(viewport, source);

        assert_eq!(retained.width, viewport.width);
        assert_eq!(retained.height, viewport.height);
        assert_eq!(retained.offset, [-30.0, -70.0]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tiny_text_glyph_runs_stay_in_shared_uploads() {
        assert!(
            !should_use_retained_text_glyph_run(8, None),
            "tiny labels must stay in the shared fused batch"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn line_sized_text_glyph_runs_stay_in_shared_uploads() {
        assert!(
            !should_use_retained_text_glyph_run(64, None),
            "Markdown scroll frames contain many line-sized text runs; retaining each one creates per-run buffer binds instead of one shared glyph batch"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn large_clipped_text_glyph_runs_stay_in_shared_uploads() {
        assert!(
            !should_use_retained_text_glyph_run(
                MIN_RETAINED_TEXT_GLYPH_QUADS.saturating_mul(2),
                Some(Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 100.0,
                }),
            ),
            "clipped lazy-list text must not draw a full retained run outside the viewport"
        );
    }

    #[test]
    fn normal_text_glyph_draw_skips_offscreen_prewarm_candidates() {
        assert_eq!(
            text_glyph_draw_action(false, true, false),
            TextGlyphDrawAction::Skip,
            "normal draw traversal must not prepare offscreen text"
        );
    }

    #[test]
    fn bounded_text_glyph_prewarm_admits_offscreen_candidates() {
        assert_eq!(
            text_glyph_draw_action(false, true, true),
            TextGlyphDrawAction::PrewarmOffscreen,
            "only the bounded prewarm path may prepare offscreen text"
        );
    }

    #[test]
    fn visible_text_glyph_draws_are_always_admitted() {
        assert_eq!(
            text_glyph_draw_action(true, false, false),
            TextGlyphDrawAction::DrawVisible
        );
        assert_eq!(
            text_glyph_draw_action(true, true, true),
            TextGlyphDrawAction::DrawVisible
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_text_prewarm_skips_large_uncached_text_runs() {
        assert!(
            !offscreen_text_glyph_prewarm_work_is_bounded(
                None,
                MAX_OFFSCREEN_TEXT_GLYPH_PREWARM_UNCACHED_CHARS + 1,
            ),
            "offscreen prewarm must not collect large uncached text runs in an input frame"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_text_prewarm_admits_small_uncached_text_runs() {
        assert!(
            offscreen_text_glyph_prewarm_work_is_bounded(
                None,
                MAX_OFFSCREEN_TEXT_GLYPH_PREWARM_UNCACHED_CHARS,
            ),
            "small labels can be warmed without risking a frame-budget spike"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_text_prewarm_skips_large_cached_runs_without_quads() {
        assert!(
            !offscreen_text_glyph_prewarm_work_is_bounded(
                Some(MAX_OFFSCREEN_TEXT_GLYPH_PREWARM_CACHED_GLYPHS + 1),
                0,
            ),
            "cached glyph placements can still be too large to prepare during input frames"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_text_prewarm_stops_after_candidate_budget() {
        assert!(
            offscreen_text_glyph_prewarm_budget_exhausted(
                Instant::now(),
                MAX_OFFSCREEN_TEXT_GLYPH_PREWARM_CANDIDATES,
            ),
            "prewarm must be bounded by candidate count even when each candidate is cheap"
        );
    }

    #[test]
    fn clipped_cached_glyph_quads_are_filtered_to_viewport() {
        fn quad(y: i32) -> CachedTextGlyphQuad {
            CachedTextGlyphQuad {
                x: 8,
                y,
                width: 20,
                height: 10,
                color: (1.0, 1.0, 1.0, 1.0),
                uv: ImageUvRect {
                    min: [0.0, 0.0],
                    max: [1.0, 1.0],
                    sample_bounds: [0.0, 0.0, 1.0, 1.0],
                },
            }
        }

        let source = Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 400.0,
        };
        let clip = Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 80.0,
        });
        let viewport = ViewportUniformParams {
            width: 320,
            height: 80,
            offset: [0.0, 0.0],
        };

        assert!(cached_text_glyph_quad_is_visible_in_viewport(
            source,
            &quad(40),
            clip,
            viewport,
            1.0,
        ));
        assert!(
            !cached_text_glyph_quad_is_visible_in_viewport(source, &quad(140), clip, viewport, 1.0,),
            "glyphs outside the effective clip should not enter the frame command stream"
        );
    }

    #[test]
    fn small_scene_range_cache_miss_observes_first_render() {
        let key = LayerRasterCacheKey::scene_range(
            0xCACE,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
            },
            (120, 80),
            ScaleBucket::from_scale(1.0),
        );

        assert!(
            !first_cache_miss_admission(&key),
            "a small scene-range miss should render directly first instead of materializing a tiny one-frame retained target"
        );
        assert!(
            repeated_cache_miss_admission(&key),
            "a repeated small scene-range miss is stable enough to materialize into the retained cache"
        );
    }

    #[test]
    fn large_scene_range_cache_miss_requires_repeated_stable_key() {
        let key = LayerRasterCacheKey::scene_range(
            0xCACE,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1200.0,
                height: 900.0,
            },
            (1200, 900),
            ScaleBucket::from_scale(1.0),
        );

        assert!(
            !first_cache_miss_admission(&key),
            "a large first scene-range miss should render directly instead of materializing a multi-MB one-frame cache entry"
        );
        assert!(
            repeated_cache_miss_admission(&key),
            "a repeated scene-range miss is stable enough to materialize into the retained cache"
        );
    }

    #[test]
    fn renderer_warmup_frame_is_requested_for_cache_miss_stats_only() {
        let stats = gpu_stats::FrameStats::default();
        let mut snapshot = stats.snapshot();
        assert!(
            !frame_stats_need_warmup_frame(&snapshot),
            "a clean frame must not keep a static scene redrawing"
        );

        snapshot.layer_cache_misses = 1;
        assert!(frame_stats_need_warmup_frame(&snapshot));
        snapshot.layer_cache_misses = 0;

        snapshot.shadow_shape_cache_misses = 1;
        assert!(frame_stats_need_warmup_frame(&snapshot));
        snapshot.shadow_shape_cache_misses = 0;

        snapshot.text_image_cache_misses = 1;
        assert!(frame_stats_need_warmup_frame(&snapshot));
        snapshot.text_image_cache_misses = 0;

        snapshot.text_glyph_atlas_misses = 1;
        assert!(frame_stats_need_warmup_frame(&snapshot));
    }

    #[test]
    fn renderer_warmup_budget_is_consumed_by_a_repeated_cache_miss() {
        let stats = gpu_stats::FrameStats::default();
        let mut snapshot = stats.snapshot();
        snapshot.layer_cache_misses = 1;
        let mut pending_frames = 0;

        update_frame_warmup_budget(&mut pending_frames, &snapshot);
        assert_eq!(pending_frames, CACHE_MISS_WARMUP_FRAMES);

        update_frame_warmup_budget(&mut pending_frames, &snapshot);
        assert_eq!(
            pending_frames, 0,
            "a cache miss during the warmup frame must not replenish its budget"
        );
    }

    #[test]
    fn non_scene_layer_surface_cache_miss_admits_first_render() {
        let key = LayerRasterCacheKey::new(
            Some(77),
            0xC0FFEE,
            0,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
            },
            (120, 80),
            ScaleBucket::from_scale(1.0),
        );

        assert!(
            first_cache_miss_admission(&key),
            "ordinary retained layer surfaces should still cache on first miss"
        );
    }

    #[test]
    fn text_image_cache_key_is_content_addressed_not_node_addressed() {
        let first = test_text_draw(
            Rect {
                x: 12.25,
                y: 40.75,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Static,
        );
        let mut second = first.clone();
        second.node_id = first.node_id + 1;

        let first_key = GpuRenderer::text_image_cache_key(&first, first.rect, 1.0, true);
        let second_key = GpuRenderer::text_image_cache_key(&second, second.rect, 1.0, true);

        assert_eq!(
            first_key, second_key,
            "text raster cache keys must be based on rendered pixels, not node identity"
        );
    }

    #[test]
    fn animated_text_image_cache_key_keeps_fractional_phase_only() {
        let base = test_text_draw(
            Rect {
                x: 12.25,
                y: 40.75,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Animated,
        );
        let integer_translated = test_text_draw(
            Rect {
                x: 44.25,
                y: 88.75,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Animated,
        );
        let phase_shifted = test_text_draw(
            Rect {
                x: 44.5,
                y: 88.75,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Animated,
        );

        let base_key = GpuRenderer::text_image_cache_key(&base, base.rect, 1.0, false);
        let translated_key = GpuRenderer::text_image_cache_key(
            &integer_translated,
            integer_translated.rect,
            1.0,
            false,
        );
        let phase_shifted_key =
            GpuRenderer::text_image_cache_key(&phase_shifted, phase_shifted.rect, 1.0, false);

        assert_eq!(
            base_key, translated_key,
            "integer translation should not invalidate animated text raster cache entries"
        );
        assert_ne!(
            base_key, phase_shifted_key,
            "fractional phase affects animated text rasterization and must stay in the key"
        );
    }

    #[test]
    fn animated_translated_text_raster_geometry_applies_snap_anchor() {
        let mut base = test_text_draw(
            Rect {
                x: 14.25,
                y: 16.50,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Animated,
        );
        base.snap_anchor = Some(SnapAnchor::rigid(Point::new(14.25, 16.50)));

        let mut scrolled = test_text_draw(
            Rect {
                x: 14.25,
                y: 15.80,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Animated,
        );
        scrolled.snap_anchor = Some(SnapAnchor::rigid(Point::new(14.25, 15.80)));

        let (base_logical, base_raster, _, _, base_static) =
            text_raster_geometry_for_draw(&base, 1.0).expect("base text geometry");
        let (scrolled_logical, scrolled_raster, _, _, scrolled_static) =
            text_raster_geometry_for_draw(&scrolled, 1.0).expect("scrolled text geometry");

        assert!(!base_static);
        assert!(!scrolled_static);
        assert!((base_logical.x - 14.0).abs() < f32::EPSILON);
        assert!((base_logical.y - 17.0).abs() < f32::EPSILON);
        assert!((scrolled_logical.x - 14.0).abs() < f32::EPSILON);
        assert!((scrolled_logical.y - 16.0).abs() < f32::EPSILON);
        assert_eq!(base_raster.x.fract(), 0.0);
        assert_eq!(base_raster.y.fract(), 0.0);
        assert_eq!(scrolled_raster.x.fract(), 0.0);
        assert_eq!(scrolled_raster.y.fract(), 0.0);

        let base_key = GpuRenderer::text_image_cache_key(&base, base_raster, 1.0, false);
        let scrolled_key =
            GpuRenderer::text_image_cache_key(&scrolled, scrolled_raster, 1.0, false);
        assert_eq!(
            base_key, scrolled_key,
            "translated animated text should keep a stable raster phase while scrolling"
        );
    }

    #[test]
    fn translated_static_text_moves_one_device_pixel_at_half_pixel_phase() {
        let root_scale = 1.25;
        let mut base = test_text_draw(
            Rect {
                x: 14.0,
                y: 276.0,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Static,
        );
        base.snap_anchor = Some(SnapAnchor::rigid(Point::new(0.0, 127.600_006)));

        let mut scrolled = test_text_draw(
            Rect {
                x: 14.0,
                y: 275.2,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Static,
        );
        scrolled.snap_anchor = Some(SnapAnchor::rigid(Point::new(0.0, 126.799_99)));

        let (_, base_raster, _, _, _) =
            text_raster_geometry_for_draw(&base, root_scale).expect("base text geometry");
        let (_, scrolled_raster, _, _, _) =
            text_raster_geometry_for_draw(&scrolled, root_scale).expect("scrolled text geometry");

        assert_eq!(
            base_raster.y - scrolled_raster.y,
            1.0,
            "one physical pixel of rigid scrolling must move static text by one raster pixel"
        );
    }

    #[test]
    fn translated_text_snap_does_not_move_its_fixed_ancestor_clip() {
        let root_scale = 1.25;
        let fixed_clip = Rect {
            x: 8.0,
            y: 20.0,
            width: 300.0,
            height: 680.0,
        };
        let mut draw = test_text_draw(
            Rect {
                x: 14.0,
                y: 276.0,
                width: 220.0,
                height: 24.0,
            },
            TextMotion::Static,
        );
        draw.snap_anchor = Some(SnapAnchor::rigid(Point::new(0.0, 127.4)));
        draw.clip = Some(fixed_clip);

        let (_, _, clip, _, _) =
            text_raster_geometry_for_draw(&draw, root_scale).expect("clipped text geometry");

        assert_eq!(
            clip,
            Some(fixed_clip),
            "content pixel snapping must not translate a fixed ancestor clip"
        );
    }

    #[test]
    fn clipped_static_multiline_text_raster_source_limits_visible_line_window() {
        let rect = Rect {
            x: 8.0,
            y: 100.0,
            width: 240.0,
            height: 1_000.0,
        };
        let mut draw = test_text_draw(rect, TextMotion::Static);
        let lines = (0..100)
            .map(|line| format!("line-{line:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        draw.text = Arc::new(AnnotatedString::from(lines).render_string());

        let raster_rect = Rect {
            x: 16.0,
            y: 200.0,
            width: 480.0,
            height: 2_000.0,
        };
        let source = clipped_text_raster_source(
            &draw,
            rect,
            raster_rect,
            Some(Rect {
                x: 0.0,
                y: 610.0,
                width: 800.0,
                height: 40.0,
            }),
            2.0,
            true,
        );

        let Cow::Owned(sliced_draw) = source.draw else {
            panic!("clipped static multiline text should rasterize only the visible line window");
        };
        let sliced_text = sliced_draw.text.text.as_str();
        assert!(sliced_text.contains("line-050"));
        assert!(sliced_text.contains("line-055"));
        assert!(!sliced_text.contains("line-000"));
        assert!(!sliced_text.contains("line-099"));
        assert_eq!(source.raster_rect.x, raster_rect.x);
        assert!(source.raster_rect.y > raster_rect.y);
        assert!(source.raster_rect.height < raster_rect.height);
    }

    #[test]
    fn clipped_static_multiline_text_raster_source_slices_short_multiline_text() {
        let rect = Rect {
            x: 8.0,
            y: 100.0,
            width: 240.0,
            height: 320.0,
        };
        let mut draw = test_text_draw(rect, TextMotion::Static);
        let lines = (0..24)
            .map(|line| format!("code-line-{line:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        draw.text = Arc::new(AnnotatedString::from(lines).render_string());

        let raster_rect = Rect {
            x: 16.0,
            y: 200.0,
            width: 480.0,
            height: 640.0,
        };
        let source = clipped_text_raster_source(
            &draw,
            rect,
            raster_rect,
            Some(Rect {
                x: 0.0,
                y: 190.0,
                width: 800.0,
                height: 120.0,
            }),
            2.0,
            true,
        );

        let Cow::Owned(sliced_draw) = source.draw else {
            panic!("clipped multiline text should rasterize only the visible line window");
        };
        assert!(sliced_draw.text.text.as_str().contains("code-line-06"));
        assert!(!sliced_draw.text.text.as_str().contains("code-line-00"));
        assert!(!sliced_draw.text.text.as_str().contains("code-line-23"));
        assert_eq!(source.raster_rect.x, raster_rect.x);
        assert!(source.raster_rect.y > raster_rect.y);
        assert!(source.raster_rect.height < raster_rect.height);
    }

    #[test]
    fn text_line_index_cache_reuses_retained_index_for_same_text_instance() {
        let mut cache = TextLineIndexCache::new(4);
        let text = Arc::new(AnnotatedString::from("a\nb\nc").render_string());

        let first = cache.line_starts(&text);
        let second = cache.line_starts(&text);

        assert_eq!(first.as_ref(), &[0, 2, 4]);
        assert!(
            Rc::ptr_eq(&first, &second),
            "retained text should not rebuild its line index on every clipped frame"
        );
    }

    #[test]
    fn text_line_index_cache_is_retained_text_instance_local() {
        let mut cache = TextLineIndexCache::new(4);
        let first_text = Arc::new(AnnotatedString::from("a\nb\nc").render_string());
        let second_text = Arc::new(AnnotatedString::from("a\nb\nc").render_string());

        let first = cache.line_starts(&first_text);
        let second = cache.line_starts(&second_text);

        assert_eq!(first.as_ref(), second.as_ref());
        assert!(
            !Rc::ptr_eq(&first, &second),
            "line index lookup should not hash large text contents to find unrelated retained nodes"
        );
    }

    #[test]
    fn device_pixel_bounds_for_rect_snaps_origin_and_extents() {
        let bounds = device_pixel_bounds_for_rect(
            Rect {
                x: 10.25,
                y: 14.6,
                width: 20.1,
                height: 9.2,
            },
            200,
            120,
            2.0,
        )
        .expect("rect should intersect the viewport");

        assert_eq!(
            bounds,
            DevicePixelBounds {
                x: 20.0,
                y: 29.0,
                width: 41,
                height: 19,
            }
        );
    }

    #[test]
    fn visible_layer_rect_intersects_clip_and_viewport() {
        let visible = visible_layer_rect(
            Rect {
                x: -10.0,
                y: 5.0,
                width: 80.0,
                height: 40.0,
            },
            Some(Rect {
                x: 4.0,
                y: 8.0,
                width: 20.0,
                height: 50.0,
            }),
            2.0,
            60,
            40,
        )
        .expect("visible rect");

        assert_eq!(
            visible,
            Rect {
                x: 4.0,
                y: 8.0,
                width: 20.0,
                height: 12.0,
            }
        );
    }

    #[test]
    fn clamp_effect_surface_scale_caps_large_surfaces_but_keeps_base_scale() {
        let clamped = clamp_effect_surface_scale(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1200.0,
                height: 900.0,
            },
            1.0,
            8.0,
            16_384,
        );

        assert!(
            clamped < 8.0,
            "large translated effect layers must be capped to avoid OOM, got {clamped}"
        );
        assert!(
            clamped >= 1.0,
            "effect surfaces must not fall below destination resolution, got {clamped}"
        );
    }

    #[test]
    fn clamp_effect_surface_scale_keeps_decorated_text_capture_scale() {
        let clamped = clamp_effect_surface_scale(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 446.0,
                height: 44.0,
            },
            1.0,
            9.0,
            16_384,
        );

        assert_eq!(
            clamped, 9.0,
            "decorated text motion-stable captures must keep full scale"
        );
    }

    fn backdrop_layer(z_index: usize) -> BackdropLayer {
        BackdropLayer {
            node_id: Some(700 + z_index),
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            clip: None,
            snap_anchor: None,
            effect: RenderEffect::blur(2.0),
            z_index,
        }
    }

    fn test_shape(z_index: usize, blend_mode: BlendMode) -> DrawShape {
        DrawShape {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            local_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            quad: [[0.0, 0.0], [8.0, 0.0], [0.0, 8.0], [8.0, 8.0]],
            snap_anchor: None,
            brush: Brush::solid(Color::BLACK),
            shape: None,
            stroke: None,
            arc: None,
            z_index,
            clip: None,
            blend_mode,
            motion_context_animated: false,
        }
    }

    #[test]
    fn shape_shadow_content_hash_ignores_viewport_translation() {
        fn translate_shape(shape: &DrawShape, dx: f32, dy: f32) -> DrawShape {
            let mut translated = shape.clone();
            translated.rect.x += dx;
            translated.rect.y += dy;
            translated.local_rect.x += dx;
            translated.local_rect.y += dy;
            for point in &mut translated.quad {
                point[0] += dx;
                point[1] += dy;
            }
            translated.snap_anchor = translated.snap_anchor.map(|anchor| {
                SnapAnchor::rigid(Point::new(anchor.origin.x + dx, anchor.origin.y + dy))
            });
            translated.clip = translated.clip.map(|mut clip| {
                clip.x += dx;
                clip.y += dy;
                clip
            });
            translated
        }

        let mut first = test_shape(1, BlendMode::SrcOver);
        first.rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 80.0,
            height: 40.0,
        };
        first.local_rect = first.rect;
        first.quad = [[10.0, 20.0], [90.0, 20.0], [10.0, 60.0], [90.0, 60.0]];
        first.snap_anchor = Some(SnapAnchor::rigid(Point::new(7.0, 11.0)));
        first.shape = Some(RoundedCornerShape::uniform(8.0));
        first.clip = Some(Rect {
            x: 8.0,
            y: 18.0,
            width: 86.0,
            height: 44.0,
        });
        let mut cutout = test_shape(2, BlendMode::DstOut);
        cutout.rect = Rect {
            x: 18.0,
            y: 26.0,
            width: 62.0,
            height: 22.0,
        };
        cutout.local_rect = cutout.rect;
        cutout.quad = [[18.0, 26.0], [80.0, 26.0], [18.0, 48.0], [80.0, 48.0]];
        cutout.shape = Some(RoundedCornerShape::uniform(4.0));

        let dx = 37.0;
        let dy = -11.5;
        let translated = translate_shape(&first, dx, dy);
        let translated_cutout = translate_shape(&cutout, dx, dy);

        let root_scale = 1.25;
        let first_shapes = vec![
            (first.clone(), BlendMode::SrcOver),
            (cutout, BlendMode::DstOut),
        ];
        let translated_shapes = vec![
            (translated.clone(), BlendMode::SrcOver),
            (translated_cutout, BlendMode::DstOut),
        ];

        let first_hash = shape_shadow_content_hash(&first_shapes, root_scale);
        let translated_hash = shape_shadow_content_hash(&translated_shapes, root_scale);

        assert_eq!(first_hash, translated_hash);

        let mut changed_shapes = translated_shapes;
        changed_shapes[0].0.rect.width += 1.0;
        let changed_hash = shape_shadow_content_hash(&changed_shapes, root_scale);

        assert_ne!(first_hash, changed_hash);
    }

    #[test]
    fn shape_shadow_content_hash_is_stable_under_fractional_scale_scroll() {
        // Regression: scrolling a shadowed panel on a fractional-scale display
        // (e.g. Xft.dpi 130 → scale ≈ 1.354) must not re-render the shadow blur
        // every frame. The production cache key derives its viewport offset from
        // FLOORED device-pixel bounds, so the residual subpixel phase used to leak
        // into the content hash and miss the cache on every scroll step.
        fn shadow_shapes_at(y: f32) -> Vec<(DrawShape, BlendMode)> {
            let mut shape = test_shape(1, BlendMode::SrcOver);
            shape.rect = Rect {
                x: 24.0,
                y,
                width: 180.0,
                height: 90.0,
            };
            shape.local_rect = shape.rect;
            shape.quad = crate::rect_to_quad(shape.rect);
            shape.shape = Some(RoundedCornerShape::uniform(14.0));
            vec![(shape, BlendMode::SrcOver)]
        }

        let root_scale = 130.0f32 / 96.0;
        let blur_radius = 18.0f32;
        let pixel_radius = blur_radius * root_scale;

        let key_at = |y: f32| {
            let shapes = shadow_shapes_at(y);
            let plan =
                shape_shadow_surface_plan(&shapes, None, blur_radius, 1600, 1600, root_scale, 8192)
                    .expect("surface plan");
            shape_shadow_surface_cache_key(
                &shapes,
                plan.source_device_bounds,
                pixel_radius,
                root_scale,
            )
            .expect("cache key")
        };

        // Wheel scroll translates the panel by whole logical pixels; the device
        // subpixel phase changes on every step at fractional scale. The whole
        // cache key (content hash AND surface pixel size) must stay stable, or
        // every scroll frame re-renders the shadow blur.
        let base = key_at(640.0);
        for step in 1..=12 {
            let scrolled = key_at(640.0 - step as f32 * 4.0);
            assert_eq!(
                base, scrolled,
                "scrolled shadow cache key must stay stable at fractional scale (step {step})"
            );
        }
    }

    #[test]
    fn shape_shadow_cache_key_uses_unclipped_source_bounds_for_scrolled_clip() {
        fn translated_card_shadow(y: f32) -> Vec<(DrawShape, BlendMode)> {
            let mut shape = test_shape(1, BlendMode::SrcOver);
            shape.rect = Rect {
                x: 24.0,
                y,
                width: 280.0,
                height: 120.0,
            };
            shape.local_rect = shape.rect;
            shape.quad = [[24.0, y], [304.0, y], [24.0, y + 120.0], [304.0, y + 120.0]];
            shape.shape = Some(RoundedCornerShape::uniform(18.0));
            vec![(shape, BlendMode::SrcOver)]
        }

        let root_scale = 1.0;
        let blur_radius = 18.0;
        let viewport_clip = Rect {
            x: 0.0,
            y: 96.0,
            width: 360.0,
            height: 720.0,
        };
        let key_for = |y: f32| {
            let shapes = translated_card_shadow(y);
            let plan = shape_shadow_surface_plan(
                &shapes,
                Some(viewport_clip),
                blur_radius,
                360,
                900,
                root_scale,
                4096,
            )
            .expect("surface plan");
            shape_shadow_surface_cache_key(
                &shapes,
                plan.source_device_bounds,
                plan.pixel_radius,
                root_scale,
            )
            .expect("cache key")
        };

        // The card scrolls under a fixed viewport clip; the visible portion
        // changes but the cache key must stay anchored to the unclipped source.
        assert_eq!(key_for(740.0), key_for(756.0));
    }

    #[test]
    fn shape_visibility_uses_nonzero_viewport_offset_for_cropped_offscreen() {
        let mut shape = test_shape(1, BlendMode::SrcOver);
        shape.rect = Rect {
            x: 24.0,
            y: 740.0,
            width: 280.0,
            height: 120.0,
        };
        shape.local_rect = shape.rect;
        shape.quad = [[24.0, 740.0], [304.0, 740.0], [24.0, 860.0], [304.0, 860.0]];
        let viewport = ViewportUniformParams {
            width: 316,
            height: 228,
            offset: [6.0, 686.0],
        };

        assert!(shape_draw_is_visible_in_viewport(&shape, viewport, 1.0));
    }

    #[test]
    fn text_prewarm_uses_nonzero_viewport_offset_for_cropped_offscreen() {
        let viewport = ViewportUniformParams {
            width: 316,
            height: 228,
            offset: [6.0, 686.0],
        };
        let text_rect = Rect {
            x: 24.0,
            y: 740.0,
            width: 280.0,
            height: 40.0,
        };

        assert!(text_draw_is_visible_in_viewport(
            text_rect, None, viewport, 1.0
        ));
        assert!(text_draw_should_prewarm_in_viewport(
            text_rect, None, viewport, 1.0
        ));
    }

    fn test_shadow_draw(shapes: Vec<(DrawShape, BlendMode)>) -> ShadowDraw {
        ShadowDraw {
            shapes,
            texts: vec![],
            blur_radius: 8.0,
            clip: None,
            z_index: 0,
        }
    }

    fn test_image(z_index: usize, blend_mode: BlendMode) -> ImageDraw {
        ImageDraw {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            local_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            quad: [[0.0, 0.0], [8.0, 0.0], [0.0, 8.0], [8.0, 8.0]],
            snap_anchor: None,
            image: ImageBitmap::from_rgba8(1, 1, vec![255, 255, 255, 255]).expect("image"),
            alpha: 1.0,
            color_filter: None,
            sampling: ImageSampling::Nearest,
            z_index,
            clip: None,
            blend_mode,
            src_rect: None,
            motion_context_animated: false,
        }
    }

    #[test]
    fn image_sampler_descriptors_match_requested_sampling() {
        let nearest = image_sampler_descriptor(ImageSampling::Nearest);
        assert_eq!(nearest.mag_filter, wgpu::FilterMode::Nearest);
        assert_eq!(nearest.min_filter, wgpu::FilterMode::Nearest);

        let linear = image_sampler_descriptor(ImageSampling::Linear);
        assert_eq!(linear.mag_filter, wgpu::FilterMode::Linear);
        assert_eq!(linear.min_filter, wgpu::FilterMode::Linear);
    }

    #[test]
    fn image_uv_rect_clamps_source_rect_to_texel_centers() {
        let image = ImageBitmap::from_rgba8(24, 16, vec![0; 24 * 16 * 4]).expect("image");
        let uv = image_uv_rect(
            &image,
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
            }),
        )
        .expect("uv rect");

        assert_eq!(uv.min, [0.0, 0.0]);
        assert_eq!(uv.max, [16.0 / 24.0, 1.0]);
        assert_eq!(
            uv.sample_bounds,
            [0.5 / 24.0, 0.5 / 16.0, 15.5 / 24.0, 15.5 / 16.0]
        );
    }

    #[test]
    fn image_uv_rect_keeps_full_image_unclamped() {
        let image = ImageBitmap::from_rgba8(2, 2, vec![0; 16]).expect("image");
        let uv = image_uv_rect(&image, None).expect("uv rect");

        assert_eq!(uv.min, [0.0, 0.0]);
        assert_eq!(uv.max, [1.0, 1.0]);
        assert_eq!(uv.sample_bounds, [0.0, 0.0, 1.0, 1.0]);
    }

    fn test_text(z_index: usize) -> TextDraw {
        TextDraw {
            node_id: 0,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            snap_anchor: None,
            translated_content_context: false,
            text: Arc::new(cranpose_ui::text::AnnotatedString::from("t").render_string()),
            color: Color::WHITE,
            text_style: cranpose_ui::TextStyle::default(),
            font_size: 12.0,
            scale: 1.0,
            layout_options: cranpose_ui::TextLayoutOptions::default(),
            z_index,
            clip: None,
        }
    }

    #[test]
    fn text_draw_visibility_rejects_text_outside_clip_before_rasterization() {
        let viewport = ViewportUniformParams {
            width: 320,
            height: 240,
            offset: [0.0, 0.0],
        };
        let text_rect = Rect {
            x: 0.0,
            y: 260.0,
            width: 200.0,
            height: 40.0,
        };
        let clip = Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 200.0,
        });

        assert!(
            !text_draw_is_visible_in_viewport(text_rect, clip, viewport, 1.0),
            "lazy-list beyond-bound text outside the clip must not be rasterized"
        );
    }

    #[test]
    fn text_draw_prewarm_accepts_clipped_text_near_viewport() {
        let viewport = ViewportUniformParams {
            width: 320,
            height: 240,
            offset: [0.0, 0.0],
        };
        let text_rect = Rect {
            x: 0.0,
            y: 260.0,
            width: 200.0,
            height: 40.0,
        };
        let clip = Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 200.0,
        });

        assert!(!text_draw_is_visible_in_viewport(
            text_rect, clip, viewport, 1.0
        ));
        assert!(text_draw_should_prewarm_in_viewport(
            text_rect, clip, viewport, 1.0
        ));
    }

    #[test]
    fn text_draw_prewarm_rejects_far_clipped_text() {
        let viewport = ViewportUniformParams {
            width: 320,
            height: 240,
            offset: [0.0, 0.0],
        };
        let text_rect = Rect {
            x: 0.0,
            y: 1600.0,
            width: 200.0,
            height: 40.0,
        };
        let clip = Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 200.0,
        });

        assert!(!text_draw_should_prewarm_in_viewport(
            text_rect, clip, viewport, 1.0
        ));
    }

    #[test]
    fn text_draw_visibility_rejects_unclipped_text_outside_viewport() {
        let viewport = ViewportUniformParams {
            width: 320,
            height: 240,
            offset: [0.0, 0.0],
        };
        let text_rect = Rect {
            x: 0.0,
            y: 241.0,
            width: 200.0,
            height: 40.0,
        };

        assert!(
            !text_draw_is_visible_in_viewport(text_rect, None, viewport, 1.0),
            "unclipped text outside the target viewport must not be rasterized"
        );
    }

    #[test]
    fn text_draw_visibility_keeps_partially_visible_text() {
        let viewport = ViewportUniformParams {
            width: 320,
            height: 240,
            offset: [0.0, 0.0],
        };
        let text_rect = Rect {
            x: 0.0,
            y: 220.0,
            width: 200.0,
            height: 40.0,
        };

        assert!(text_draw_is_visible_in_viewport(
            text_rect, None, viewport, 1.0
        ));
    }

    fn test_draw_ops(
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadows: &[ShadowDraw],
    ) -> Vec<DrawOp> {
        let mut ops = Vec::new();
        ops.extend(shapes.iter().enumerate().map(|(index, shape)| DrawOp {
            z_index: shape.z_index,
            kind: DrawOpKind::Shape(index),
        }));
        ops.extend(images.iter().enumerate().map(|(index, image)| DrawOp {
            z_index: image.z_index,
            kind: DrawOpKind::Image(index),
        }));
        ops.extend(texts.iter().enumerate().map(|(index, text)| DrawOp {
            z_index: text.z_index,
            kind: DrawOpKind::Text(index),
        }));
        ops.extend(shadows.iter().enumerate().map(|(index, shadow)| DrawOp {
            z_index: shadow.z_index,
            kind: DrawOpKind::Shadow(index),
        }));
        ops.sort_by_key(|op| op.z_index);
        ops
    }

    fn test_layer(local_bounds: Rect, children: Vec<RenderNode>) -> LayerNode {
        crate::test_support::layer_node(
            local_bounds,
            ProjectiveTransform::identity(),
            GraphicsLayer::default(),
            children,
        )
    }

    fn cacheable_layer(
        node_id: cranpose_core::NodeId,
        local_bounds: Rect,
        children: Vec<RenderNode>,
    ) -> LayerNode {
        let mut layer = test_layer(local_bounds, children);
        layer.node_id = Some(node_id);
        layer.cache_policy = cranpose_render_common::graph::CachePolicy::Auto;
        layer.recompute_raster_cache_hashes();
        layer
    }

    fn text_layer_with_style(text: AnnotatedString, text_style: TextStyle) -> LayerNode {
        test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                    node_id: 1,
                    rect: Rect {
                        x: 2.0,
                        y: 3.0,
                        width: 48.0,
                        height: 18.0,
                    },
                    text: std::rc::Rc::new(text),
                    text_style,
                    font_size: 14.0,
                    layout_options: TextLayoutOptions::default(),
                    clip: None,
                })),
            })],
        )
    }

    fn snapped_text_leaf(animated: bool, translated_content_context: bool) -> LayerNode {
        LayerNode {
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
            content_offset: Point::default(),
            scene_children_origin: cranpose_ui_graphics::Point::default(),
            scene_children_layer_translation: cranpose_ui_graphics::Point::default(),
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
                            stroke: None,
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
                        text: std::rc::Rc::new(AnnotatedString::from("48 px")),
                        text_style: TextStyle::default(),
                        font_size: 14.0,
                        layout_options: TextLayoutOptions::default(),
                        clip: None,
                    })),
                }),
            ],
        }
    }

    fn snapped_text_leaf_root(animated: bool, translated_content_context: bool) -> LayerNode {
        let text_leaf = snapped_text_leaf(animated, translated_content_context);
        test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 96.0,
                height: 64.0,
            },
            vec![RenderNode::Layer(Box::new(text_leaf))],
        )
    }

    fn translated_content_local_surface_root() -> LayerNode {
        let mut effectful_text = text_layer_with_style(
            AnnotatedString::from("shadow"),
            TextStyle::from_span_style(SpanStyle {
                shadow: Some(Shadow {
                    color: Color::BLACK,
                    offset: Point::new(1.0, 2.0),
                    blur_radius: 3.0,
                }),
                ..SpanStyle::default()
            }),
        );
        effectful_text.translated_content_context = true;

        let translated_content = LayerNode {
            node_id: Some(78),
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 96.0,
                height: 64.0,
            },
            transform_to_parent: ProjectiveTransform::translation(14.25, 16.5),
            motion_context_animated: false,
            translated_content_context: true,
            translated_content_offset: Point::default(),
            content_offset: Point::default(),
            scene_children_origin: cranpose_ui_graphics::Point::default(),
            scene_children_layer_translation: cranpose_ui_graphics::Point::default(),
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: vec![RenderNode::Layer(Box::new(effectful_text))],
        };

        test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 120.0,
            },
            vec![RenderNode::Layer(Box::new(translated_content))],
        )
    }

    #[test]
    fn scissor_rect_for_layer_intersects_with_clip() {
        let rect = Rect {
            x: 10.0,
            y: 10.0,
            width: 30.0,
            height: 20.0,
        };
        let clip = Rect {
            x: 20.0,
            y: 15.0,
            width: 100.0,
            height: 100.0,
        };

        let scissor = scissor_rect_for_layer(rect, Some(clip), 1.0, 200, 200);
        assert_eq!(scissor, Some((20, 15, 20, 15)));
    }

    #[test]
    fn visible_draw_rect_no_clip_returns_original() {
        let rect = Rect {
            x: 100.0,
            y: 200.0,
            width: 300.0,
            height: 400.0,
        };
        assert_eq!(visible_draw_rect(rect, None), Some(rect));
    }

    #[test]
    fn visible_draw_rect_with_clip_intersects() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 2000.0,
            height: 5000.0,
        };
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let visible = visible_draw_rect(rect, Some(clip)).expect("should have visible area");
        assert_eq!(visible.width, 800.0);
        assert_eq!(visible.height, 600.0);
    }

    #[test]
    fn visible_draw_rect_fully_clipped_returns_none() {
        let rect = Rect {
            x: 1000.0,
            y: 1000.0,
            width: 200.0,
            height: 200.0,
        };
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        assert!(visible_draw_rect(rect, Some(clip)).is_none());
    }

    #[test]
    fn scene_bounds_respects_clip_on_shapes() {
        let mut scene = CompositorScene::new();
        // Shape inside viewport — visible
        scene.shapes.push(DrawShape {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                width: 100.0,
                height: 50.0,
            },
            clip: Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            }),
            ..test_shape(0, BlendMode::SrcOver)
        });
        // Shape far outside viewport — clipped away entirely
        scene.shapes.push(DrawShape {
            rect: Rect {
                x: 0.0,
                y: 3000.0,
                width: 100.0,
                height: 50.0,
            },
            clip: Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            }),
            ..test_shape(1, BlendMode::SrcOver)
        });
        let bounds = scene_bounds(&scene).expect("should have bounds");
        // Bounds should only cover the first shape's visible area,
        // NOT extend to y=3050 from the clipped second shape.
        assert!(bounds.y + bounds.height <= 600.0);
    }

    #[test]
    fn scene_bounds_scroll_content_clipped_to_viewport() {
        // Simulates a scroll container: many items with large y offsets,
        // all clipped to a viewport-sized clip rect.
        let mut scene = CompositorScene::new();
        let viewport_clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        for i in 0..20 {
            scene.shapes.push(DrawShape {
                rect: Rect {
                    x: 0.0,
                    y: i as f32 * 300.0,
                    width: 800.0,
                    height: 200.0,
                },
                clip: Some(viewport_clip),
                ..test_shape(i, BlendMode::SrcOver)
            });
        }
        let bounds = scene_bounds(&scene).expect("should have bounds");
        // All shapes are clipped to viewport — bounds should be viewport-sized,
        // NOT 20*300 = 6000 dp tall.
        assert_eq!(bounds.x, 0.0);
        assert_eq!(bounds.y, 0.0);
        assert!(bounds.width <= 800.0);
        assert!(bounds.height <= 600.0);
    }

    #[test]
    fn scene_bounds_stable_across_scroll_offsets() {
        // Simulates horizontal scroll at different offsets —
        // bounds should be identical regardless of scroll position.
        let viewport_clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 50.0,
        };
        let compute_bounds_at_offset = |scroll_x: f32| {
            let mut scene = CompositorScene::new();
            for i in 0..10 {
                scene.shapes.push(DrawShape {
                    rect: Rect {
                        x: i as f32 * 100.0 - scroll_x,
                        y: 0.0,
                        width: 80.0,
                        height: 40.0,
                    },
                    clip: Some(viewport_clip),
                    ..test_shape(i, BlendMode::SrcOver)
                });
            }
            scene_bounds(&scene).expect("bounds")
        };
        let bounds_at_0 = compute_bounds_at_offset(0.0);
        let bounds_at_300 = compute_bounds_at_offset(300.0);
        let bounds_at_600 = compute_bounds_at_offset(600.0);
        // Width should be stable (clipped to viewport) regardless of scroll offset
        assert!(
            (bounds_at_0.width - bounds_at_300.width).abs() < 1.0,
            "bounds width changed with scroll: {} vs {}",
            bounds_at_0.width,
            bounds_at_300.width
        );
        assert!(
            (bounds_at_0.width - bounds_at_600.width).abs() < 1.0,
            "bounds width changed with scroll: {} vs {}",
            bounds_at_0.width,
            bounds_at_600.width
        );
    }

    #[test]
    fn collect_effect_ranges_respects_excluded_effect() {
        let layers = vec![effect_layer(10, 40), effect_layer(20, 30)];
        let mut ranges = Vec::new();
        collect_effect_ranges(&layers, 10, 40, Some(0), &mut ranges);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], 20..30);
    }

    #[test]
    fn collect_layer_events_includes_nested_when_parent_excluded() {
        let effects = vec![effect_layer(10, 40), effect_layer(20, 30)];
        let backdrops = vec![backdrop_layer(25)];
        let mut events = Vec::new();
        collect_layer_events(&effects, &backdrops, 10, 40, Some(0), &mut events);
        assert_eq!(events.len(), 2);

        match events[0].kind {
            LayerEventKind::Effect(index) => assert_eq!(index, 1),
            LayerEventKind::Backdrop(_) => panic!("expected nested effect as first event"),
        }
        match events[1].kind {
            LayerEventKind::Backdrop(index) => assert_eq!(index, 0),
            LayerEventKind::Effect(_) => panic!("expected backdrop as second event"),
        }
    }

    fn pure_text_leaf(animated: bool, translated_content_context: bool) -> LayerNode {
        LayerNode {
            node_id: Some(177),
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 96.0,
                height: 32.0,
            },
            transform_to_parent: ProjectiveTransform::translation(11.4, 23.6),
            motion_context_animated: animated,
            translated_content_context,
            translated_content_offset: Point::default(),
            content_offset: Point::default(),
            scene_children_origin: cranpose_ui_graphics::Point::default(),
            scene_children_layer_translation: cranpose_ui_graphics::Point::default(),
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
                node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                    node_id: 177,
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 96.0,
                        height: 24.0,
                    },
                    clip: None,
                    text: std::rc::Rc::new(AnnotatedString::from("Pure text")),
                    text_style: TextStyle::default(),
                    font_size: 14.0,
                    layout_options: TextLayoutOptions::default(),
                })),
            })],
        }
    }

    fn pure_text_leaf_root(animated: bool, translated_content_context: bool) -> LayerNode {
        let text_leaf = pure_text_leaf(animated, translated_content_context);
        test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 96.0,
            },
            vec![RenderNode::Layer(Box::new(text_leaf))],
        )
    }

    #[test]
    fn collect_layer_events_sorts_backdrop_before_effect_at_same_z() {
        let effects = vec![effect_layer(10, 20)];
        let backdrops = vec![backdrop_layer(10)];
        let mut events = Vec::new();
        collect_layer_events(&effects, &backdrops, 0, 30, None, &mut events);
        assert_eq!(events.len(), 2);

        match events[0].kind {
            LayerEventKind::Backdrop(_) => {}
            LayerEventKind::Effect(_) => panic!("expected backdrop to run before effect"),
        }
        match events[1].kind {
            LayerEventKind::Effect(_) => {}
            LayerEventKind::Backdrop(_) => panic!("expected effect as second event"),
        }
    }

    #[test]
    fn collect_layer_events_prefers_outer_effect_when_same_start_z() {
        // Child emitted before parent (matching scene collection order where a
        // parent effect is recorded after recursively processing children).
        let effects = vec![effect_layer(10, 20), effect_layer(10, 40)];
        let mut events = Vec::new();
        collect_layer_events(&effects, &[], 0, 50, None, &mut events);

        assert_eq!(events.len(), 2);
        match events[0].kind {
            LayerEventKind::Effect(index) => assert_eq!(index, 1),
            LayerEventKind::Backdrop(_) => panic!("expected outer effect first"),
        }
        match events[1].kind {
            LayerEventKind::Effect(index) => assert_eq!(index, 0),
            LayerEventKind::Backdrop(_) => panic!("expected child effect second"),
        }
    }

    #[test]
    fn collect_layer_events_prefers_later_effect_when_ranges_match() {
        let effects = vec![effect_layer(10, 20), effect_layer(10, 20)];
        let mut events = Vec::new();
        collect_layer_events(&effects, &[], 0, 30, None, &mut events);

        assert_eq!(events.len(), 2);
        match events[0].kind {
            LayerEventKind::Effect(index) => assert_eq!(index, 1),
            LayerEventKind::Backdrop(_) => panic!("expected later effect first"),
        }
        match events[1].kind {
            LayerEventKind::Effect(index) => assert_eq!(index, 0),
            LayerEventKind::Backdrop(_) => panic!("expected earlier effect second"),
        }
    }

    #[test]
    fn has_backdrop_layer_in_range_detects_nested_layers() {
        let backdrops = vec![backdrop_layer(5), backdrop_layer(15), backdrop_layer(25)];
        assert!(has_backdrop_layer_in_range(&backdrops, 10, 20));
        assert!(has_backdrop_layer_in_range(&backdrops, 0, 6));
        assert!(!has_backdrop_layer_in_range(&backdrops, 20, 25));
    }

    #[test]
    fn layer_contains_descendant_backdrop_ignores_self_backdrop() {
        let mut self_backdrop = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            vec![],
        );
        self_backdrop.graphics_layer.backdrop_effect = Some(RenderEffect::blur(2.0));
        assert!(!layer_contains_descendant_backdrop(&self_backdrop));

        let mut child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            vec![],
        );
        child.graphics_layer.backdrop_effect = Some(RenderEffect::blur(2.0));

        let parent = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );
        assert!(layer_contains_descendant_backdrop(&parent));
    }

    fn child_layer_composite(
        layer: &LayerNode,
        z_index: usize,
        rect: Rect,
        needs_nested_underlay: bool,
    ) -> crate::normalized_scene::ChildLayerComposite {
        let mut requirements_cache = cranpose_core::collections::map::HashMap::new();
        let surface_requirements =
            crate::surface_plan::layer_surface_requirements_cached(layer, &mut requirements_cache);
        crate::normalized_scene::ChildLayerComposite {
            z_index,
            logical_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: rect.width,
                height: rect.height,
            },
            dest_quad: rect_to_quad(rect),
            snap_anchor: None,
            composite_snap_origin: None,
            backdrop_rect: rect,
            visual_clip: None,
            surface_clip: None,
            shadow_draws: Vec::new(),
            needs_nested_underlay,
            node_id: layer.node_id,
            backdrop: layer.backdrop().cloned(),
            has_effect: layer.effect().is_some(),
            effect_contains_runtime_shader: layer
                .effect()
                .is_some_and(|effect| effect.contains_runtime_shader()),
            target_content_hash: layer.target_content_hash(),
            effect_hash: layer.effect_hash(),
            motion_source_content_hash: Some(layer.motion_source_content_hash()),
            contains_descendant_backdrop: layer_contains_descendant_backdrop(layer),
            cache_policy: layer.cache_policy,
            surface_requirements,
            rounded_clip: crate::surface_executor::backend::LayerSurfaceRoundedClip::from_layer(
                layer,
            ),
            isolation: cranpose_render_common::layer_composition::effective_layer_isolation(
                &layer.graphics_layer,
            ),
            translated_content_context: layer.translated_content_context,
            own_translated_content_axes: crate::surface_plan::translated_content_axes_for_layer(
                layer,
            ),
            clip_rect: layer.clip_rect(),
            local_bounds: layer.local_bounds,
            surface_scale: crate::surface_plan::layer_surface_scale(layer),
            source: crate::normalized_scene::LoweredChildSource::default(),
        }
    }

    #[test]
    fn root_direct_preflight_allows_first_translated_child_underlay() {
        let child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 280.0,
            },
            vec![],
        );
        let collected = CollectedLayer {
            scene: CompositorScene::new(),
            child_layers: vec![child_layer_composite(
                &child,
                3,
                Rect {
                    x: 48.0,
                    y: 96.0,
                    width: 400.0,
                    height: 280.0,
                },
                true,
            )],
        };

        assert!(direct_root_child_underlays_are_supported(&collected));
    }

    #[test]
    fn root_direct_preflight_allows_axis_aligned_prior_child_underlay() {
        let first = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: 40.0,
            },
            vec![],
        );
        let backdrop_child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 280.0,
            },
            vec![],
        );
        let collected = CollectedLayer {
            scene: CompositorScene::new(),
            child_layers: vec![
                child_layer_composite(
                    &first,
                    1,
                    Rect {
                        x: 8.0,
                        y: 16.0,
                        width: 80.0,
                        height: 40.0,
                    },
                    false,
                ),
                child_layer_composite(
                    &backdrop_child,
                    4,
                    Rect {
                        x: 48.0,
                        y: 96.0,
                        width: 400.0,
                        height: 280.0,
                    },
                    true,
                ),
            ],
        };

        assert!(direct_root_child_underlays_are_supported(&collected));
    }

    #[test]
    fn root_direct_preflight_rejects_effectful_prior_child_underlay() {
        let mut first = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: 40.0,
            },
            vec![],
        );
        first.graphics_layer.render_effect = Some(RenderEffect::blur(2.0));
        let backdrop_child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 280.0,
            },
            vec![],
        );
        let collected = CollectedLayer {
            scene: CompositorScene::new(),
            child_layers: vec![
                child_layer_composite(
                    &first,
                    1,
                    Rect {
                        x: 64.0,
                        y: 112.0,
                        width: 80.0,
                        height: 40.0,
                    },
                    false,
                ),
                child_layer_composite(
                    &backdrop_child,
                    4,
                    Rect {
                        x: 48.0,
                        y: 96.0,
                        width: 400.0,
                        height: 280.0,
                    },
                    true,
                ),
            ],
        };

        assert!(!direct_root_child_underlays_are_supported(&collected));
    }

    #[test]
    fn root_direct_preflight_ignores_non_overlapping_effectful_prior_child_underlay() {
        let mut first = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: 40.0,
            },
            vec![],
        );
        first.graphics_layer.render_effect = Some(RenderEffect::blur(2.0));
        let backdrop_child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 280.0,
            },
            vec![],
        );
        let collected = CollectedLayer {
            scene: CompositorScene::new(),
            child_layers: vec![
                child_layer_composite(
                    &first,
                    1,
                    Rect {
                        x: 8.0,
                        y: 16.0,
                        width: 80.0,
                        height: 40.0,
                    },
                    false,
                ),
                child_layer_composite(
                    &backdrop_child,
                    4,
                    Rect {
                        x: 48.0,
                        y: 96.0,
                        width: 400.0,
                        height: 280.0,
                    },
                    true,
                ),
            ],
        };

        assert!(direct_root_child_underlays_are_supported(&collected));
    }

    #[test]
    fn root_direct_preflight_rejects_underlay_that_would_replay_prior_scene_effects() {
        let backdrop_child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 280.0,
            },
            vec![],
        );
        let mut scene = CompositorScene::new();
        scene.next_z = 1;
        scene.push_effect_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 120.0,
            },
            None,
            Some(RenderEffect::blur(2.0)),
            BlendMode::SrcOver,
            1.0,
            0,
            1,
        );
        let collected = CollectedLayer {
            scene,
            child_layers: vec![child_layer_composite(
                &backdrop_child,
                4,
                Rect {
                    x: 48.0,
                    y: 96.0,
                    width: 400.0,
                    height: 280.0,
                },
                true,
            )],
        };

        assert!(!direct_root_child_underlays_are_supported(&collected));
    }

    #[test]
    fn root_direct_eligibility_does_not_reject_descendant_backdrop() {
        let mut backdrop = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0,
            },
            vec![],
        );
        backdrop.graphics_layer.backdrop_effect = Some(RenderEffect::blur(4.0));
        let child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 96.0,
            },
            vec![RenderNode::Layer(Box::new(backdrop))],
        );
        let root = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 240.0,
                height: 160.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );
        let mut cache = HashMap::new();

        assert!(root_can_render_directly_cached(&root, &mut cache));
    }

    #[test]
    fn root_direct_scene_events_allow_root_local_effects() {
        let mut scene = CompositorScene::new();
        scene.effect_layers.push(EffectLayer {
            rect: Rect {
                x: 20.0,
                y: 30.0,
                width: 120.0,
                height: 80.0,
            },
            clip: None,
            snap_anchor: None,
            effect: Some(RenderEffect::blur(6.0)),
            blend_mode: BlendMode::SrcOver,
            composite_alpha: 1.0,
            z_start: 0,
            z_end: 1,
            requirements: SurfaceRequirementSet::default().with(SurfaceRequirement::RenderEffect),
        });

        assert!(root_direct_scene_events_are_supported(&scene));
    }

    #[test]
    fn root_direct_scene_events_reject_root_local_backdrops() {
        let mut scene = CompositorScene::new();
        scene.backdrop_layers.push(BackdropLayer {
            node_id: Some(99),
            rect: Rect {
                x: 20.0,
                y: 30.0,
                width: 120.0,
                height: 80.0,
            },
            clip: None,
            snap_anchor: None,
            effect: RenderEffect::blur(6.0),
            z_index: 1,
        });

        assert!(!root_direct_scene_events_are_supported(&scene));
    }

    #[test]
    fn estimate_layer_surface_rect_includes_transformed_child_bounds() {
        let mut child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 6.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 10.0,
                            height: 6.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                        stroke: None,
                    },
                    clip: None,
                }),
            })],
        );
        child.transform_to_parent = ProjectiveTransform::translation(18.0, 7.0);

        let parent = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 4.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );

        assert_eq!(
            estimate_layer_surface_rect(&parent),
            Rect {
                x: 18.0,
                y: 7.0,
                width: 10.0,
                height: 6.0,
            }
        );
    }

    #[test]
    fn estimate_layer_surface_rect_clips_translated_clip_layers_without_hidden_leading_content() {
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 72.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                        rect: Rect {
                            x: 24.0,
                            y: 0.0,
                            width: 200.0,
                            height: 480.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                        stroke: None,
                    },
                    clip: None,
                }),
            })],
        );
        layer.translated_content_context = true;
        layer.motion_context_animated = true;
        layer.clip_to_bounds = true;

        assert_eq!(
            estimate_layer_surface_rect(&layer),
            Rect {
                x: 24.0,
                y: 0.0,
                width: 96.0,
                height: 72.0,
            }
        );
    }

    #[test]
    fn estimate_layer_surface_rect_clips_active_horizontal_scroll_content() {
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 72.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                        rect: Rect {
                            x: -24.0,
                            y: 0.0,
                            width: 200.0,
                            height: 480.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                        stroke: None,
                    },
                    clip: None,
                }),
            })],
        );
        layer.translated_content_context = true;
        layer.motion_context_animated = true;
        layer.clip_to_bounds = true;

        assert_eq!(
            estimate_layer_surface_rect(&layer),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 72.0,
            }
        );
    }

    #[test]
    fn estimate_layer_surface_rect_clips_active_vertical_scroll_content() {
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 72.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                        rect: Rect {
                            x: 0.0,
                            y: -24.0,
                            width: 120.0,
                            height: 200.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                        stroke: None,
                    },
                    clip: None,
                }),
            })],
        );
        layer.translated_content_context = true;
        layer.motion_context_animated = true;
        layer.clip_to_bounds = true;

        assert_eq!(
            estimate_layer_surface_rect(&layer),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 72.0,
            }
        );
    }

    #[test]
    fn estimate_layer_surface_rect_keeps_shallow_scroll_capture_origin_stable() {
        fn shallow_scroll_surface_rect(content_y: f32) -> Rect {
            let mut layer = test_layer(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 120.0,
                    height: 72.0,
                },
                vec![RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(DrawPrimitiveNode {
                        primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                            rect: Rect {
                                x: 0.0,
                                y: content_y,
                                width: 120.0,
                                height: 200.0,
                            },
                            brush: Brush::solid(Color::WHITE),
                            stroke: None,
                        },
                        clip: None,
                    }),
                })],
            );
            layer.translated_content_context = true;
            layer.motion_context_animated = true;
            layer.clip_to_bounds = true;
            estimate_layer_surface_rect(&layer)
        }

        assert_eq!(
            shallow_scroll_surface_rect(-24.0),
            shallow_scroll_surface_rect(-25.0),
            "shallow scroll capture bounds must not move the offscreen surface origin on adjacent scroll positions"
        );
    }

    #[test]
    fn estimate_layer_surface_rect_clips_active_xy_scroll_content() {
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 72.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                        rect: Rect {
                            x: -16.0,
                            y: -24.0,
                            width: 180.0,
                            height: 240.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                        stroke: None,
                    },
                    clip: None,
                }),
            })],
        );
        layer.translated_content_context = true;
        layer.motion_context_animated = true;
        layer.clip_to_bounds = true;

        assert_eq!(
            estimate_layer_surface_rect(&layer),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 72.0,
            }
        );
    }

    #[test]
    fn estimate_layer_surface_rect_clips_deep_hidden_active_scroll_content() {
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 72.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                        rect: Rect {
                            x: 0.0,
                            y: -1200.0,
                            width: 120.0,
                            height: 1400.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                        stroke: None,
                    },
                    clip: None,
                }),
            })],
        );
        layer.translated_content_context = true;
        layer.motion_context_animated = true;
        layer.clip_to_bounds = true;

        assert_eq!(
            estimate_layer_surface_rect(&layer),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 72.0,
            }
        );
    }

    #[test]
    fn estimate_layer_surface_rect_keeps_deep_scroll_capture_origin_stable() {
        fn deep_scroll_surface_rect(content_y: f32) -> Rect {
            let mut layer = test_layer(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 120.0,
                    height: 72.0,
                },
                vec![RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(DrawPrimitiveNode {
                        primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                            rect: Rect {
                                x: 0.0,
                                y: content_y,
                                width: 120.0,
                                height: 1400.0,
                            },
                            brush: Brush::solid(Color::WHITE),
                            stroke: None,
                        },
                        clip: None,
                    }),
                })],
            );
            layer.translated_content_context = true;
            layer.motion_context_animated = true;
            layer.clip_to_bounds = true;
            estimate_layer_surface_rect(&layer)
        }

        assert_eq!(
            deep_scroll_surface_rect(-1200.0),
            deep_scroll_surface_rect(-1201.0),
            "deep scroll capture bounds must not re-phase the offscreen surface origin on adjacent scroll positions"
        );
    }

    #[test]
    fn motion_stable_capture_bounds_bounds_shadows_for_clipped_effect_layer() {
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 72.0,
            },
            vec![],
        );
        layer.clip_to_bounds = true;
        layer.graphics_layer.clip = true;
        layer.graphics_layer.render_effect = Some(RenderEffect::blur(2.0));

        let mut shadow_shape = test_shape(0, BlendMode::SrcOver);
        shadow_shape.rect = Rect {
            x: -24.0,
            y: -1200.0,
            width: 180.0,
            height: 1400.0,
        };
        let mut scene = CompositorScene::new();
        scene
            .shadow_draws
            .push(test_shadow_draw(vec![(shadow_shape, BlendMode::SrcOver)]));

        let requirements = SurfaceRequirementSet::default()
            .with(SurfaceRequirement::RenderEffect)
            .with(SurfaceRequirement::MotionStableCapture);

        assert_eq!(
            motion_stable_capture_bounds(
                &layer,
                &scene,
                &[],
                requirements,
                TranslatedContentAxes::default(),
                None,
            ),
            Some(Rect {
                x: -360.0,
                y: -216.0,
                width: 480.0,
                height: 288.0,
            })
        );
    }

    #[test]
    fn vertical_motion_stable_capture_uses_viewport_cross_axis_bounds() {
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            },
            vec![],
        );
        layer.clip_to_bounds = true;
        layer.graphics_layer.clip = true;

        let mut shape = test_shape(0, BlendMode::SrcOver);
        shape.rect = Rect {
            x: 60.0,
            y: -80.0,
            width: 80.0,
            height: 220.0,
        };
        let mut scene = CompositorScene::new();
        scene.shapes.push(shape);

        let requirements =
            SurfaceRequirementSet::default().with(SurfaceRequirement::MotionStableCapture);

        assert_eq!(
            motion_stable_capture_bounds(
                &layer,
                &scene,
                &[],
                requirements,
                TranslatedContentAxes { x: false, y: true },
                None,
            ),
            Some(Rect {
                x: -96.0,
                y: -64.0,
                width: 296.0,
                height: 164.0,
            })
        );
    }

    #[test]
    fn vertical_motion_stable_capture_uses_external_surface_clip() {
        let layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            },
            vec![],
        );

        let mut shape = test_shape(0, BlendMode::SrcOver);
        shape.rect = Rect {
            x: 60.0,
            y: -80.0,
            width: 80.0,
            height: 220.0,
        };
        let mut scene = CompositorScene::new();
        scene.shapes.push(shape);

        let requirements =
            SurfaceRequirementSet::default().with(SurfaceRequirement::MotionStableCapture);

        assert_eq!(
            motion_stable_capture_bounds(
                &layer,
                &scene,
                &[],
                requirements,
                TranslatedContentAxes { x: false, y: true },
                Some(Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 100.0,
                }),
            ),
            Some(Rect {
                x: -96.0,
                y: -64.0,
                width: 296.0,
                height: 164.0,
            })
        );
    }

    #[test]
    fn estimate_layer_surface_rect_expands_for_child_layer_shadow() {
        let mut child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 12.0,
                height: 8.0,
            },
            vec![],
        );
        child.transform_to_parent = ProjectiveTransform::translation(20.0, 9.0);
        child.graphics_layer.shadow_elevation = 6.0;

        let parent = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 4.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );

        let rect = estimate_layer_surface_rect(&parent);
        assert!(rect.x < 20.0);
        assert!(rect.y < 9.0);
        assert!(rect.width > 12.0);
        assert!(rect.height > 8.0);
    }

    #[test]
    fn estimate_layer_surface_rect_respects_local_bounds_for_effect_layers() {
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 28.0,
                height: 28.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                        rect: Rect {
                            x: 10.0,
                            y: 10.0,
                            width: 10.0,
                            height: 10.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                        stroke: None,
                    },
                    clip: None,
                }),
            })],
        );
        layer.graphics_layer.render_effect = Some(RenderEffect::blur(12.0));

        assert_eq!(
            estimate_layer_surface_rect(&layer),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 28.0,
                height: 28.0,
            }
        );
    }

    #[test]
    fn layer_raster_cache_candidate_ignores_parent_transform() {
        let primitive = PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                    rect: Rect {
                        x: 2.0,
                        y: 3.0,
                        width: 6.0,
                        height: 4.0,
                    },
                    brush: Brush::solid(Color::BLACK),
                    stroke: None,
                },
                clip: None,
            }),
        };
        let base = cacheable_layer(
            41,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            vec![RenderNode::Primitive(primitive.clone())],
        );
        let mut moved = base.clone();
        moved.transform_to_parent = ProjectiveTransform::translation(32.0, 18.0);

        assert_eq!(
            layer_raster_cache_candidate(&base, 1.25, false, false),
            layer_raster_cache_candidate(&moved, 1.25, false, false)
        );
    }

    #[test]
    fn layer_raster_cache_candidate_changes_for_translated_content_offset() {
        let primitive = PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                    rect: Rect {
                        x: 2.0,
                        y: 3.0,
                        width: 6.0,
                        height: 4.0,
                    },
                    brush: Brush::solid(Color::BLACK),
                    stroke: None,
                },
                clip: None,
            }),
        };
        let mut base = cacheable_layer(
            42,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            vec![RenderNode::Primitive(primitive)],
        );
        base.translated_content_context = true;
        base.translated_content_offset = Point::new(0.0, -8.0);
        base.recompute_raster_cache_hashes();

        let mut moved = base.clone();
        moved.translated_content_offset = Point::new(0.0, -16.0);
        moved.recompute_raster_cache_hashes();

        assert_ne!(
            layer_raster_cache_candidate(&base, 1.25, false, false),
            layer_raster_cache_candidate(&moved, 1.25, false, false),
            "full-surface layer cache candidates must not alias different scroll offsets"
        );
    }

    #[test]
    fn layer_raster_cache_candidate_changes_for_child_transform() {
        let mut child = cacheable_layer(
            8,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 12.0,
                height: 10.0,
            },
            vec![],
        );
        child.transform_to_parent = ProjectiveTransform::translation(4.0, 6.0);
        let base = cacheable_layer(
            7,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            vec![RenderNode::Layer(Box::new(child.clone()))],
        );
        let mut moved_child = child;
        moved_child.transform_to_parent = ProjectiveTransform::translation(9.0, 6.0);
        let moved = cacheable_layer(
            7,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            vec![RenderNode::Layer(Box::new(moved_child))],
        );

        assert_ne!(
            layer_raster_cache_candidate(&base, 1.0, false, false),
            layer_raster_cache_candidate(&moved, 1.0, false, false)
        );
    }

    #[test]
    fn layer_raster_cache_candidate_rejects_external_backdrop_dependency() {
        let mut child = cacheable_layer(
            12,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            vec![],
        );
        child.graphics_layer.backdrop_effect = Some(RenderEffect::blur(2.0));
        let parent = cacheable_layer(
            11,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );

        assert!(layer_raster_cache_candidate(&parent, 1.0, false, false).is_some());
        assert!(layer_raster_cache_candidate(&parent, 1.0, true, false).is_none());
    }

    #[test]
    fn layer_raster_cache_candidate_does_not_force_translation_only_text_surfaces() {
        let text = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id: 77,
                rect: Rect {
                    x: 2.0,
                    y: 3.0,
                    width: 48.0,
                    height: 18.0,
                },
                text: std::rc::Rc::new(AnnotatedString::from("runtime cache")),
                text_style: TextStyle::default(),
                font_size: 14.0,
                layout_options: TextLayoutOptions::default(),
                clip: None,
            })),
        });
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            vec![text],
        );
        layer.node_id = Some(77);
        layer.recompute_raster_cache_hashes();

        assert!(
            layer_raster_cache_candidate(&layer, 1.0, false, false).is_none(),
            "root path should not isolate plain translation-only text layers"
        );
        assert!(
            layer_raster_cache_candidate(&layer, 1.0, false, true).is_none(),
            "child path should also render plain translation-only text layers directly"
        );
    }

    #[test]
    fn layer_raster_cache_candidate_allows_stable_runtime_child_effect_surfaces() {
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::Rect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 64.0,
                            height: 32.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                        stroke: None,
                    },
                    clip: None,
                }),
            })],
        );
        layer.node_id = Some(78);
        layer.graphics_layer.render_effect = Some(RenderEffect::blur(4.0));
        layer.recompute_raster_cache_hashes();

        assert!(
            layer_raster_cache_candidate(&layer, 1.0, false, false).is_none(),
            "root direct path should not force-cache ordinary stable effects"
        );
        assert!(
            layer_raster_cache_candidate(&layer, 1.0, false, true).is_some(),
            "child surface rendering should retain stable non-runtime effects"
        );
    }

    #[test]
    fn layer_raster_cache_candidate_rejects_runtime_shader_child_effect_surfaces() {
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            vec![],
        );
        layer.node_id = Some(79);
        layer.graphics_layer.render_effect = Some(RenderEffect::runtime_shader(
            RuntimeShader::new("runtime shader"),
        ));
        layer.recompute_raster_cache_hashes();

        assert!(
            layer_raster_cache_candidate(&layer, 1.0, false, true).is_none(),
            "runtime shaders must not fill the retained layer cache with per-frame uniform variants"
        );
    }

    #[test]
    fn layer_surface_requirements_keep_plain_text_on_direct_path() {
        let layer = text_layer_with_style(AnnotatedString::from("plain"), TextStyle::default());

        let requirements = layer_surface_requirements(&layer);

        assert_eq!(requirements.direct_translation, Some(Point::default()));
        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::PixelStableComposite));
        assert!(!requirements
            .surface_requirements
            .has_isolating_requirement());
    }

    #[test]
    fn layer_surface_requirements_keep_translated_plain_text_leaf_on_direct_path() {
        let layer = pure_text_leaf(false, true);

        let requirements = layer_surface_requirements(&layer);

        assert_eq!(
            requirements.direct_translation,
            Some(Point::new(11.4, 23.6))
        );
        assert!(
            requirements
                .surface_requirements
                .contains(SurfaceRequirement::PixelStableComposite)
                && !requirements
                    .surface_requirements
                    .has_isolating_requirement(),
            "translated plain text should stay on the direct path and isolate only the glyph draw"
        );
    }

    #[test]
    fn layer_surface_requirements_keep_translated_text_leaf_with_background_on_direct_path() {
        let layer = snapped_text_leaf(false, true);

        let requirements = layer_surface_requirements(&layer);

        assert_eq!(
            requirements.direct_translation,
            Some(Point::new(14.25, 16.5))
        );
        assert!(
            requirements
                .surface_requirements
                .contains(SurfaceRequirement::PixelStableComposite)
                && !requirements
                    .surface_requirements
                    .has_isolating_requirement(),
            "translated text with direct sibling decoration/background should keep the layer direct"
        );
    }

    #[test]
    fn translated_plain_text_uses_bounded_snap_surface() {
        let root = pure_text_leaf_root(true, true);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.child_layers.len(), 1);
        assert!(collected.scene.texts.is_empty());
        assert!(collected.scene.effect_layers.is_empty());
        assert_snap_anchor_close(
            collected.child_layers[0].snap_anchor,
            Point::new(11.4, 23.6),
            "translated plain text's bounded local surface should composite at the content-origin snap phase",
        );
    }

    /// Not a correctness test: a local timing harness for the shape-run
    /// collect path. Run manually with
    /// `cargo test --release -p cranpose-render-wgpu -- --ignored collect_timing --nocapture`.
    #[test]
    #[ignore]
    fn shape_run_collect_timing_harness() {
        use cranpose_render_common::graph::DrawPrimitiveNode;
        use cranpose_render_common::layer_composition::local_content_layer_for;
        use cranpose_ui_graphics::Stroke;

        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 1080.0,
            height: 2244.0,
        };
        let graphics_layer = GraphicsLayer::default();

        // A MEGA-BOSS-shaped workload: thousands of consecutive arcs, most
        // solid, some gradient, one text-free layer.
        let mut nodes: Vec<DrawPrimitiveNode> = Vec::new();
        for i in 0..3000u32 {
            let f = i as f32;
            let brush = if i % 8 == 0 {
                Brush::linear_gradient(vec![Color::WHITE, Color::BLACK])
            } else {
                Brush::Solid(Color(0.5, 0.2, 0.8, 1.0))
            };
            let center = Point::new(540.0 + (f % 400.0), 1122.0 + (f % 350.0));
            let radius = 8.0 + (i % 23) as f32;
            let half = radius + 4.0;
            nodes.push(DrawPrimitiveNode {
                primitive: DrawPrimitive::Arc {
                    rect: Rect {
                        x: center.x - half,
                        y: center.y - half,
                        width: half * 2.0,
                        height: half * 2.0,
                    },
                    brush,
                    center,
                    radius,
                    start_angle: f * 0.07,
                    sweep_angle: 0.5 + (i % 5) as f32,
                    stroke: (i % 3 != 0).then(|| Stroke::new(4.0)),
                    inner_radius: if i % 3 == 0 { radius * 0.6 } else { 0.0 },
                },
                clip: None,
            });
        }

        let children: Vec<RenderNode> = nodes
            .iter()
            .map(|node| {
                RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(node.clone()),
                })
            })
            .collect();
        let layer = crate::test_support::layer_node(
            bounds,
            ProjectiveTransform::identity(),
            graphics_layer,
            children,
        );

        const ITERS: usize = 300;

        // Reference: the pre-run per-primitive path.
        let local_layer = local_content_layer_for(&layer.graphics_layer);
        let start = Instant::now();
        let mut sink_shapes = 0usize;
        for _ in 0..ITERS {
            let mut scene = CompositorScene::new();
            for node in &nodes {
                crate::pipeline::push_draw_primitive(
                    &node.primitive,
                    bounds,
                    &local_layer,
                    None,
                    &mut scene,
                    None,
                    false,
                );
            }
            sink_shapes = scene.shapes.len();
        }
        let serial = start.elapsed();

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let start = Instant::now();
        let mut run_shapes = 0usize;
        for _ in 0..ITERS {
            let collected = collect_layer_contents(
                &layer,
                None,
                None,
                &mut rect_cache,
                &mut requirements_cache,
            );
            run_shapes = collected.scene.shapes.len();
        }
        let run = start.elapsed();

        println!(
            "per-primitive: {:?}/iter ({sink_shapes} shapes)  shape-run: {:?}/iter ({run_shapes} shapes)",
            serial / ITERS as u32,
            run / ITERS as u32,
        );
    }

    /// Shared body for the serial and forced-parallel equivalence tests:
    fn assert_shape_run_collect_matches_per_primitive_emission() {
        use cranpose_render_common::graph::DrawPrimitiveNode;
        use cranpose_render_common::layer_composition::local_content_layer_for;
        use cranpose_render_common::primitive_emit::{resolve_primitive_clip, PrimitiveClipSpace};
        use cranpose_ui_graphics::{CornerRadii, Stroke};

        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 800.0,
        };
        // Rotation keeps rigid snapping off, so both paths agree on
        // `snap_anchor: None` without replicating the anchor computation here.
        let graphics_layer = GraphicsLayer {
            scale: 1.25,
            translation_x: 3.5,
            translation_y: -2.0,
            alpha: 0.9,
            rotation_z: 0.35,
            ..GraphicsLayer::default()
        };

        let mut nodes: Vec<DrawPrimitiveNode> = Vec::new();
        for i in 0..600u32 {
            let f = i as f32;
            let brush = if i % 11 == 0 {
                Brush::linear_gradient(vec![Color::WHITE, Color::BLACK])
            } else {
                Brush::Solid(Color(0.1 + (i % 7) as f32 * 0.1, 0.5, 0.9, 1.0))
            };
            let stroke = (i % 5 == 0).then(|| Stroke::new(1.0 + (i % 3) as f32));
            let primitive = match i % 3 {
                0 => DrawPrimitive::Rect {
                    rect: Rect {
                        x: f % 37.0,
                        y: f % 53.0,
                        width: 8.0 + f % 9.0,
                        height: 6.0 + f % 5.0,
                    },
                    brush,
                    stroke,
                },
                1 => DrawPrimitive::RoundRect {
                    rect: Rect {
                        x: f % 41.0,
                        y: f % 43.0,
                        width: 12.0,
                        height: 10.0,
                    },
                    brush,
                    radii: CornerRadii::uniform(2.0 + (i % 4) as f32),
                    stroke,
                },
                _ => {
                    let center = Point::new(60.0 + f % 71.0, 60.0 + f % 67.0);
                    let radius = 5.0 + (i % 13) as f32;
                    // One degenerate sweep proves dropped draws stay dropped.
                    let sweep_angle = if i == 302 { 0.0 } else { 0.4 + (i % 6) as f32 };
                    let half = radius + 4.0;
                    DrawPrimitive::Arc {
                        rect: Rect {
                            x: center.x - half,
                            y: center.y - half,
                            width: half * 2.0,
                            height: half * 2.0,
                        },
                        brush,
                        center,
                        radius,
                        start_angle: f * 0.11,
                        sweep_angle,
                        stroke: (i % 2 == 0).then(|| Stroke::new(3.0)),
                        inner_radius: if i % 4 == 2 { radius * 0.5 } else { 0.0 },
                    }
                }
            };
            let primitive = if i == 300 {
                // A nested blend disqualifies the run view and forces a
                // mid-run flush through the serial path, splitting 600 draws
                // into two runs that are both long enough to fan out.
                DrawPrimitive::Blend {
                    primitive: Box::new(DrawPrimitive::Blend {
                        primitive: Box::new(primitive),
                        blend_mode: BlendMode::SrcOver,
                    }),
                    blend_mode: BlendMode::DstOut,
                }
            } else if i % 7 == 3 {
                DrawPrimitive::Blend {
                    primitive: Box::new(primitive),
                    blend_mode: BlendMode::DstOut,
                }
            } else {
                primitive
            };
            let clip = (i % 31 == 7).then_some(Rect {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 30.0,
            });
            nodes.push(DrawPrimitiveNode { primitive, clip });
        }

        let children: Vec<RenderNode> = nodes
            .iter()
            .map(|node| {
                RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(node.clone()),
                })
            })
            .collect();
        let layer = crate::test_support::layer_node(
            bounds,
            ProjectiveTransform::identity(),
            graphics_layer,
            children,
        );

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected =
            collect_layer_contents(&layer, None, None, &mut rect_cache, &mut requirements_cache);

        // The reference scene: every primitive through the per-primitive
        // emission path, exactly as the pre-run collect loop ran it.
        let local_layer = local_content_layer_for(&layer.graphics_layer);
        let mut expected = CompositorScene::new();
        for node in &nodes {
            let clip = resolve_primitive_clip(
                node.clip,
                bounds,
                &local_layer,
                None,
                PrimitiveClipSpace::Local,
            );
            if node.clip.is_some() && clip.is_none() {
                continue;
            }
            crate::pipeline::push_draw_primitive(
                &node.primitive,
                bounds,
                &local_layer,
                clip,
                &mut expected,
                None,
                false,
            );
        }

        assert!(
            collected.scene.shapes.len() >= 590,
            "the runs should engage the parallel branch: got {} shapes",
            collected.scene.shapes.len()
        );
        assert_eq!(collected.scene.shapes.len(), expected.shapes.len());
        assert_eq!(collected.scene.draw_ops, expected.draw_ops);
        assert_eq!(collected.scene.next_z, expected.next_z);
        assert!(
            collected
                .scene
                .shapes
                .iter()
                .all(|s| s.snap_anchor.is_none()),
            "a rotated layer must not rigid-snap; the reference scene assumes it"
        );
        for (index, (got, want)) in collected
            .scene
            .shapes
            .iter()
            .zip(&expected.shapes)
            .enumerate()
        {
            assert_eq!(got.rect, want.rect, "shape {index} rect");
            assert_eq!(got.local_rect, want.local_rect, "shape {index} local_rect");
            assert_eq!(got.quad, want.quad, "shape {index} quad");
            assert_eq!(got.snap_anchor, want.snap_anchor, "shape {index} snap");
            assert_eq!(got.brush, want.brush, "shape {index} brush");
            assert_eq!(got.shape, want.shape, "shape {index} shape");
            assert_eq!(got.stroke, want.stroke, "shape {index} stroke");
            assert_eq!(got.arc, want.arc, "shape {index} arc");
            assert_eq!(got.z_index, want.z_index, "shape {index} z");
            assert_eq!(got.clip, want.clip, "shape {index} clip");
            assert_eq!(got.blend_mode, want.blend_mode, "shape {index} blend");
            assert_eq!(
                got.motion_context_animated, want.motion_context_animated,
                "shape {index} motion flag"
            );
        }
    }

    /// The run collector must emit exactly what per-primitive emission does,
    /// on BOTH flush paths: the serial drain and the scoped-thread fan-out
    /// (forced via the tuning override, since a test-sized scene would never
    /// cross the size gate on its own).
    #[test]
    fn shape_run_collect_matches_per_primitive_emission_exactly() {
        assert_shape_run_collect_matches_per_primitive_emission();
        crate::normalized_scene::force_shape_run_parallel_for_tests(true);
        let outcome =
            std::panic::catch_unwind(assert_shape_run_collect_matches_per_primitive_emission);
        crate::normalized_scene::force_shape_run_parallel_for_tests(false);
        if let Err(payload) = outcome {
            std::panic::resume_unwind(payload);
        }
    }

    #[test]
    fn non_translated_text_local_surface_keeps_linear_composite_resolve() {
        let layer = text_layer_with_style(
            AnnotatedString::from("gradient"),
            TextStyle::from_span_style(SpanStyle {
                brush: Some(Brush::linear_gradient(vec![Color::WHITE, Color::BLACK])),
                ..SpanStyle::default()
            }),
        );
        let requirements = layer_surface_requirements(&layer);

        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::TextMaterialMask));
        assert_eq!(
            composite_sample_mode_for_requirements(false, false, requirements),
            CompositeSampleMode::Linear
        );
    }

    #[test]
    fn inherited_translated_text_local_surface_uses_box4_layer_surface() {
        let layer = text_layer_with_style(
            AnnotatedString::from("shadow"),
            TextStyle::from_span_style(SpanStyle {
                shadow: Some(Shadow {
                    color: Color::BLACK,
                    offset: Point::new(1.0, 2.0),
                    blur_radius: 3.0,
                }),
                ..SpanStyle::default()
            }),
        );
        let requirements = layer_surface_requirements(&layer);

        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::TextMaterialMask));
        assert_eq!(
            composite_sample_mode_for_requirements(true, false, requirements),
            CompositeSampleMode::Box4
        );
        assert_eq!(
            layer_surface_target_scale(
                true,
                false,
                requirements,
                1.25,
                layer_surface_scale(&layer)
            ),
            SurfaceRequirementSet::default()
                .with(SurfaceRequirement::TextMaterialMask)
                .with(SurfaceRequirement::MotionStableCapture)
                .target_scale(1.25, 1.0)
        );
    }

    #[test]
    fn translated_text_local_surface_inside_capture_keeps_parent_scale() {
        let layer = text_layer_with_style(
            AnnotatedString::from("shadow"),
            TextStyle::from_span_style(SpanStyle {
                shadow: Some(Shadow {
                    color: Color::BLACK,
                    offset: Point::new(1.0, 2.0),
                    blur_radius: 3.0,
                }),
                ..SpanStyle::default()
            }),
        );
        let requirements = layer_surface_requirements(&layer);

        assert_eq!(
            composite_sample_mode_for_requirements(true, true, requirements),
            CompositeSampleMode::Linear
        );
        assert_eq!(
            layer_surface_target_scale(true, true, requirements, 10.0, layer_surface_scale(&layer)),
            SurfaceRequirementSet::default()
                .with(SurfaceRequirement::TextMaterialMask)
                .target_scale(10.0, 1.0)
        );
    }

    #[test]
    fn layer_surface_requirements_use_local_surface_for_gradient_and_stroke_text() {
        let cases = [
            (
                "draw_style",
                AnnotatedString::from("draw_style"),
                TextStyle::from_span_style(SpanStyle {
                    draw_style: Some(TextDrawStyle::Stroke { width: 2.0 }),
                    ..SpanStyle::default()
                }),
            ),
            (
                "gradient_brush",
                AnnotatedString::from("gradient"),
                TextStyle::from_span_style(SpanStyle {
                    brush: Some(Brush::linear_gradient(vec![Color::WHITE, Color::BLACK])),
                    ..SpanStyle::default()
                }),
            ),
        ];

        for (label, text, text_style) in cases {
            let layer = text_layer_with_style(text, text_style);
            let requirements = layer_surface_requirements(&layer);
            assert!(
                requirements
                    .surface_requirements
                    .contains(SurfaceRequirement::TextMaterialMask),
                "{label} text should use a bounded local surface: {requirements:?}"
            );
        }
    }

    #[test]
    fn layer_surface_requirements_use_local_surface_for_complex_text_effects() {
        let cases = [
            (
                "shadow",
                AnnotatedString::from("shadow"),
                TextStyle::from_span_style(SpanStyle {
                    shadow: Some(Shadow {
                        color: Color::BLACK,
                        offset: Point::new(1.0, 2.0),
                        blur_radius: 3.0,
                    }),
                    ..SpanStyle::default()
                }),
            ),
            (
                "background",
                AnnotatedString::from("background"),
                TextStyle::from_span_style(SpanStyle {
                    background: Some(Color::BLACK),
                    ..SpanStyle::default()
                }),
            ),
            (
                "baseline_shift",
                AnnotatedString::from("baseline_shift"),
                TextStyle::from_span_style(SpanStyle {
                    baseline_shift: Some(BaselineShift::SUPERSCRIPT),
                    ..SpanStyle::default()
                }),
            ),
            (
                "geometric_transform",
                AnnotatedString::from("geometric_transform"),
                TextStyle::from_span_style(SpanStyle {
                    text_geometric_transform: Some(TextGeometricTransform {
                        scale_x: 1.2,
                        skew_x: 0.15,
                    }),
                    ..SpanStyle::default()
                }),
            ),
            (
                "letter_spacing",
                AnnotatedString::from("letter_spacing"),
                TextStyle::from_span_style(SpanStyle {
                    letter_spacing: TextUnit::Em(0.2),
                    ..SpanStyle::default()
                }),
            ),
        ];

        for (label, text, text_style) in cases {
            let layer = text_layer_with_style(text, text_style);
            let requirements = layer_surface_requirements(&layer);
            assert!(
                requirements
                    .surface_requirements
                    .contains(SurfaceRequirement::TextMaterialMask),
                "{label} text should use a bounded local surface: {requirements:?}"
            );
            assert_eq!(
                requirements.direct_translation,
                Some(Point::default()),
                "{label} text should still classify as a direct translation"
            );
        }
    }

    #[test]
    fn layer_surface_requirements_color_only_span_styles_use_direct_path() {
        let layer = text_layer_with_style(
            AnnotatedString {
                text: "styled".to_string(),
                span_styles: vec![RangeStyle {
                    item: SpanStyle {
                        color: Some(Color::BLACK),
                        ..SpanStyle::default()
                    },
                    range: 0..3,
                }],
                ..AnnotatedString::default()
            },
            TextStyle::default(),
        );
        let requirements = layer_surface_requirements(&layer);
        assert!(
            !requirements
                .surface_requirements
                .contains(SurfaceRequirement::TextMaterialMask),
            "color-only span styles should render directly via software text raster colors"
        );
    }

    #[test]
    fn layer_surface_requirements_keep_decoration_only_text_on_direct_path() {
        let layer = text_layer_with_style(
            AnnotatedString::from("decoration"),
            TextStyle::from_span_style(SpanStyle {
                text_decoration: Some(TextDecoration::UNDERLINE),
                ..SpanStyle::default()
            }),
        );

        let requirements = layer_surface_requirements(&layer);

        assert_eq!(requirements.direct_translation, Some(Point::default()));
        assert!(
            requirements
                .surface_requirements
                .contains(SurfaceRequirement::PixelStableComposite)
                && !requirements
                    .surface_requirements
                    .has_isolating_requirement(),
            "decoration-only text should not force an isolating layer surface: {requirements:?}"
        );
    }

    #[test]
    fn direct_text_leaf_snaps_modifier_background_and_text_with_one_anchor() {
        let root = snapped_text_leaf_root(false, false);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.scene.shapes.len(), 1);
        assert_eq!(collected.scene.images.len(), 1);
        assert_eq!(collected.scene.texts.len(), 1);
        let expected_anchor = Some(SnapAnchor::rigid(Point::new(14.25, 16.5)));
        assert_eq!(collected.scene.shapes[0].snap_anchor, expected_anchor);
        assert_eq!(collected.scene.images[0].snap_anchor, expected_anchor);
        assert_eq!(collected.scene.texts[0].snap_anchor, expected_anchor);
    }

    #[test]
    fn animated_translated_content_text_leaf_uses_bounded_content_snap() {
        let root = snapped_text_leaf_root(true, true);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.child_layers.len(), 1);
        assert!(collected.scene.shapes.is_empty());
        assert!(collected.scene.images.is_empty());
        assert!(collected.scene.texts.is_empty());
        assert!(collected.scene.effect_layers.is_empty());
        let expected_anchor = Some(SnapAnchor::rigid(Point::new(14.25, 16.5)));
        assert_eq!(
            collected.child_layers[0].snap_anchor, expected_anchor,
            "active translated leaf surface should keep the content-origin snap phase"
        );
    }

    #[test]
    fn translated_content_assigns_motion_anchor_to_rotated_child_surface() {
        let mut child = snapped_text_leaf(false, false);
        child.graphics_layer.rotation_z = 5.0;
        child.transform_to_parent =
            cranpose_render_common::layer_transform::layer_transform_to_parent(
                child.local_bounds,
                Point::new(108.0, 3.0),
                &child.graphics_layer,
            );
        child.recompute_raster_cache_hashes();
        let mut root = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 320.0,
                height: 180.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );
        root.translated_content_context = true;
        root.translated_content_offset = Point::new(0.0, -80.8);
        root.recompute_raster_cache_hashes();
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.child_layers.len(), 1);
        assert!(
            collected.child_layers[0].snap_anchor.is_some(),
            "a projective child still translates rigidly with its scrolling parent"
        );
    }

    #[test]
    fn rested_translated_content_context_text_leaf_snaps_for_crisp_scroll_rest() {
        let root = snapped_text_leaf_root(false, true);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.child_layers.len(), 0);
        assert_eq!(collected.scene.shapes.len(), 1);
        assert_eq!(collected.scene.images.len(), 1);
        assert_eq!(collected.scene.texts.len(), 1);
        assert_eq!(collected.scene.effect_layers.len(), 0);
        let expected_anchor = Some(SnapAnchor::rigid(Point::new(14.25, 16.5)));
        assert_eq!(
            collected.scene.shapes[0].snap_anchor, expected_anchor,
            "rested scroll content should snap back to device pixels"
        );
        assert_eq!(
            collected.scene.images[0].snap_anchor, expected_anchor,
            "rested scroll images should snap back to device pixels"
        );
        assert_eq!(
            collected.scene.texts[0].snap_anchor, expected_anchor,
            "rested scroll text should snap back to device pixels"
        );
    }

    #[test]
    fn complex_text_uses_local_surface() {
        let root = translated_content_local_surface_root();
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert!(
            !collected.child_layers.is_empty(),
            "translated-content effectful text should render through a bounded local surface"
        );
        assert!(collected.scene.texts.is_empty());
        assert!(collected.scene.shadow_draws.is_empty());
    }

    #[test]
    fn translated_content_surface_composite_uses_scroll_content_snap_anchor() {
        let mut root = translated_content_local_surface_root();
        let scroll_offset = Point::new(0.0, -18.5);
        let Some(RenderNode::Layer(translated_content)) = root.children.get_mut(0) else {
            panic!("expected translated content layer");
        };
        translated_content.translated_content_offset = scroll_offset;
        let Some(RenderNode::Layer(effectful_text)) = translated_content.children.get_mut(0) else {
            panic!("expected effectful text layer");
        };
        effectful_text.transform_to_parent =
            effectful_text
                .transform_to_parent
                .then(ProjectiveTransform::translation(
                    scroll_offset.x,
                    scroll_offset.y,
                ));

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.child_layers.len(), 1);
        assert_eq!(
            collected.child_layers[0].snap_anchor,
            Some(SnapAnchor::rigid(Point::new(14.25, -2.0))),
            "isolated scrolled descendants must composite with the same content-origin snap phase"
        );
    }

    #[test]
    fn animated_translated_content_surface_composite_uses_scroll_content_snap_anchor() {
        let mut root = translated_content_local_surface_root();
        let scroll_offset = Point::new(0.0, -18.5);
        let Some(RenderNode::Layer(translated_content)) = root.children.get_mut(0) else {
            panic!("expected translated content layer");
        };
        translated_content.motion_context_animated = true;
        translated_content.translated_content_offset = scroll_offset;
        let Some(RenderNode::Layer(effectful_text)) = translated_content.children.get_mut(0) else {
            panic!("expected effectful text layer");
        };
        effectful_text.transform_to_parent =
            effectful_text
                .transform_to_parent
                .then(ProjectiveTransform::translation(
                    scroll_offset.x,
                    scroll_offset.y,
                ));

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.child_layers.len(), 1);
        assert_eq!(
            collected.child_layers[0].snap_anchor,
            Some(SnapAnchor::rigid(Point::new(14.25, 16.5))),
            "animated translated content should composite the stable local surface at the viewport-origin snap phase"
        );
    }

    #[test]
    fn translated_text_material_effect_layer_uses_scroll_content_snap_anchor() {
        let mut layer = text_layer_with_style(
            AnnotatedString::from("gradient"),
            TextStyle::from_span_style(SpanStyle {
                brush: Some(Brush::linear_gradient(vec![Color::WHITE, Color::BLACK])),
                ..SpanStyle::default()
            }),
        );
        layer.translated_content_context = true;
        layer.translated_content_offset = Point::new(0.0, -18.5);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&layer, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.scene.effect_layers.len(), 1);
        assert_eq!(
            composite_sample_mode_for_effect_layer(&collected.scene.effect_layers[0]),
            CompositeSampleMode::Box4
        );
        assert_eq!(
            collected.scene.effect_layers[0].snap_anchor,
            Some(SnapAnchor::rigid(Point::new(0.0, -18.5))),
            "text material surfaces must composite with the scroll content-origin snap phase"
        );
    }

    #[test]
    fn translated_layer_surface_capture_does_not_restart_local_picture_for_shadow_text() {
        let mut layer = text_layer_with_style(
            AnnotatedString::from("shadow"),
            TextStyle::from_span_style(SpanStyle {
                shadow: Some(Shadow {
                    color: Color::BLACK,
                    offset: Point::new(1.0, 2.0),
                    blur_radius: 3.0,
                }),
                ..SpanStyle::default()
            }),
        );
        layer.translated_content_context = true;
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected = collect_layer_contents_with_translation_context(
            &layer,
            None,
            None,
            TranslationRenderContext {
                inherited_content_translation: false,
                surface_capture_active: true,
                local_picture_capture_active: true,
                ..TranslationRenderContext::default()
            },
            &mut rect_cache,
            &mut requirements_cache,
        );

        assert!(
            collected.scene.effect_layers.is_empty(),
            "a translated layer surface already provides the stable local capture"
        );
        assert_eq!(collected.scene.shadow_draws.len(), 1);
        assert_eq!(collected.scene.texts.len(), 1);
        assert!(
            !collected.scene.texts[0].translated_content_context,
            "text inside an active motion-stable capture must raster in capture-local coordinates"
        );
    }

    #[test]
    fn translated_layer_surface_capture_keeps_only_material_effect_layers() {
        let mut layer = text_layer_with_style(
            AnnotatedString::from("gradient"),
            TextStyle::from_span_style(SpanStyle {
                brush: Some(Brush::linear_gradient(vec![Color::WHITE, Color::BLACK])),
                ..SpanStyle::default()
            }),
        );
        layer.translated_content_context = true;
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected = collect_layer_contents_with_translation_context(
            &layer,
            None,
            None,
            TranslationRenderContext {
                inherited_content_translation: false,
                surface_capture_active: true,
                local_picture_capture_active: true,
                ..TranslationRenderContext::default()
            },
            &mut rect_cache,
            &mut requirements_cache,
        );

        assert_eq!(collected.scene.effect_layers.len(), 1);
        assert!(
            collected.scene.effect_layers[0]
                .requirements
                .contains(SurfaceRequirement::MotionStableCapture),
            "translated text materials still need motion-stable resolve semantics inside a stable capture"
        );
        assert_eq!(
            composite_sample_mode_for_effect_layer(&collected.scene.effect_layers[0]),
            CompositeSampleMode::Box4
        );
        assert_eq!(
            effect_layer_target_scale(&collected.scene.effect_layers[0], 10.0),
            10.0
        );
        assert!(collected.scene.effect_layers[0].effect.is_some());
    }

    #[test]
    fn translated_viewport_surface_does_not_add_plain_local_picture_capture() {
        let mut layer = text_layer_with_style(
            AnnotatedString::from("shadow"),
            TextStyle::from_span_style(SpanStyle {
                shadow: Some(Shadow {
                    color: Color::BLACK,
                    offset: Point::new(1.0, 2.0),
                    blur_radius: 3.0,
                }),
                ..SpanStyle::default()
            }),
        );
        layer.translated_content_context = true;
        layer.motion_context_animated = true;
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected = collect_layer_contents_with_translation_context(
            &layer,
            None,
            None,
            TranslationRenderContext {
                surface_capture_active: true,
                ..TranslationRenderContext::default()
            },
            &mut rect_cache,
            &mut requirements_cache,
        );

        assert_eq!(
            collected.scene.effect_layers.len(),
            0,
            "plain translated content inside a viewport surface should not be captured again"
        );
        assert_eq!(collected.scene.shadow_draws.len(), 1);
        assert_eq!(collected.scene.texts.len(), 1);
    }

    #[test]
    fn static_pure_text_leaf_snaps_without_sibling_draw_primitives() {
        let root = pure_text_leaf_root(false, false);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.scene.texts.len(), 1);
        assert!(
            collected.scene.texts[0].snap_anchor.is_some(),
            "idle pure text leaves should participate in rigid snap anchoring"
        );
    }

    #[test]
    fn animated_pure_text_leaf_stays_unsnapped() {
        let root = pure_text_leaf_root(true, false);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.scene.texts.len(), 1);
        assert_eq!(collected.scene.texts[0].snap_anchor, None);
    }

    #[test]
    fn animated_translated_pure_text_uses_bounded_content_snap() {
        let root = pure_text_leaf_root(true, true);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.child_layers.len(), 1);
        assert!(collected.scene.texts.is_empty());
        assert!(collected.scene.effect_layers.is_empty());
        assert_snap_anchor_close(
            collected.child_layers[0].snap_anchor,
            Point::new(11.4, 23.6),
            "animated translated pure text should use the bounded content snap phase",
        );
    }

    #[test]
    fn rested_translated_pure_text_leaf_snaps_for_crisp_scroll_rest() {
        let root = pure_text_leaf_root(false, true);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.child_layers.len(), 0);
        assert_eq!(collected.scene.texts.len(), 1);
        assert_eq!(collected.scene.effect_layers.len(), 0);
        assert_snap_anchor_close(
            collected.scene.texts[0].snap_anchor,
            Point::new(11.4, 23.6),
            "rested translated text should snap to device pixels",
        );
    }

    #[test]
    fn static_gpu_effect_text_leaf_stays_unsnapped() {
        let root = text_layer_with_style(
            AnnotatedString::from("Gradient"),
            TextStyle::from_span_style(SpanStyle {
                brush: Some(Brush::linear_gradient(vec![
                    Color(0.2, 0.8, 1.0, 1.0),
                    Color(1.0, 0.7, 0.4, 1.0),
                ])),
                draw_style: Some(TextDrawStyle::Stroke { width: 2.5 }),
                ..SpanStyle::default()
            }),
        );
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.scene.texts.len(), 1);
        assert_eq!(
            collected.scene.texts[0].snap_anchor, None,
            "gpu text-effect leaves must not take the rigid text snap path"
        );
        assert_eq!(
            collected.scene.effect_layers.len(),
            1,
            "gradient stroke text should still emit a runtime shader effect layer"
        );
    }

    #[test]
    fn layer_surface_requirements_keep_shape_plus_direct_child_on_direct_path() {
        let mut child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 20.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::Rect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 40.0,
                            height: 20.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                        stroke: None,
                    },
                    clip: None,
                }),
            })],
        );
        child.transform_to_parent = ProjectiveTransform::translation(8.0, 6.0);

        let layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            vec![
                RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(DrawPrimitiveNode {
                        primitive: DrawPrimitive::Rect {
                            rect: Rect {
                                x: 0.0,
                                y: 0.0,
                                width: 64.0,
                                height: 32.0,
                            },
                            brush: Brush::solid(Color::BLACK),
                            stroke: None,
                        },
                        clip: None,
                    }),
                }),
                RenderNode::Layer(Box::new(child)),
            ],
        );

        let requirements = layer_surface_requirements(&layer);

        assert_eq!(requirements.direct_translation, Some(Point::default()));
        assert!(!requirements
            .surface_requirements
            .contains(SurfaceRequirement::MixedDirectContent));
        assert!(!requirements
            .surface_requirements
            .has_isolating_requirement());
    }

    #[test]
    fn collect_layer_contents_translates_direct_text_rects_into_parent_space() {
        let mut child = text_layer_with_style(
            AnnotatedString::from("direct"),
            TextStyle::from_span_style(SpanStyle {
                text_decoration: Some(TextDecoration::UNDERLINE),
                ..SpanStyle::default()
            }),
        );
        child.transform_to_parent = ProjectiveTransform::translation(9.0, 7.0);

        let parent = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected = with_test_app_context(|| {
            collect_layer_contents(
                &parent,
                None,
                None,
                &mut rect_cache,
                &mut requirements_cache,
            )
        });

        assert!(
            collected.child_layers.is_empty(),
            "decoration-only text child should collapse directly into the parent scene"
        );
        assert_eq!(collected.scene.texts.len(), 1, "expected one text draw");
        let text = &collected.scene.texts[0];
        assert!(
            text.rect.x >= 9.0 && text.rect.y >= 7.0,
            "collapsed text rect should be translated into parent space, got {:?}",
            text.rect
        );
        assert!(
            collected
                .scene
                .shapes
                .iter()
                .any(|shape| shape.rect.y >= 7.0),
            "collapsed underline geometry should also be translated into parent space"
        );
    }

    #[test]
    fn normalized_scene_keeps_lazy_after_bound_text_for_prewarm() {
        use std::cell::RefCell;

        fn collect_graph_text_labels(layer: &LayerNode, labels: &mut Vec<String>) {
            for child in &layer.children {
                match child {
                    RenderNode::Primitive(PrimitiveEntry {
                        node: PrimitiveNode::Text(text),
                        ..
                    }) => labels.push(text.text.text.clone()),
                    RenderNode::Layer(child_layer) => {
                        collect_graph_text_labels(child_layer, labels)
                    }
                    RenderNode::Primitive(_) | RenderNode::DrawRun(_) => {}
                }
            }
        }

        let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let state_holder_for_comp = state_holder.clone();
        let mut composition = cranpose_ui::run_test_composition(move || {
            let list_state = remember_lazy_list_state();
            *state_holder_for_comp.borrow_mut() = Some(list_state);
            let mut spec = LazyColumnSpec::new()
                .vertical_arrangement(cranpose_ui::LinearArrangement::SpacedBy(6.0));
            spec.beyond_bounds_item_count = 0;
            LazyColumn(Modifier::empty().height(96.0), list_state, spec, |scope| {
                scope.items(
                    12,
                    None::<fn(usize) -> u64>,
                    None::<fn(usize) -> u64>,
                    |index| {
                        Text(
                            format!("WarmRow {index}"),
                            Modifier::empty().height(32.0),
                            TextStyle::default(),
                        );
                    },
                );
            });
        });

        let list_state = (*state_holder.borrow()).expect("lazy list state should be captured");
        list_state.scroll_to_item(4, 0.0);

        let root = composition.root().expect("lazy column root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let _ = applier
            .compute_layout(
                root,
                Size {
                    width: 240.0,
                    height: 240.0,
                },
            )
            .expect("lazy column layout");
        let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("lazy column graph");
        applier.clear_runtime_handle();
        let mut graph_labels = Vec::new();
        collect_graph_text_labels(&graph.root, &mut graph_labels);

        let visible_indices: Vec<_> = list_state
            .layout_info()
            .visible_items_info
            .iter()
            .map(|item| item.index)
            .collect();
        assert_eq!(
            visible_indices,
            vec![4, 5, 6],
            "test setup expects exactly three viewport-visible rows"
        );

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected = with_test_app_context(|| {
            collect_layer_contents(
                &graph.root,
                None,
                None,
                &mut rect_cache,
                &mut requirements_cache,
            )
        });
        let root_text_labels: Vec<_> = collected
            .scene
            .texts
            .iter()
            .map(|text| text.text.text.clone())
            .collect();
        let child_layer_count = collected.child_layers.len();
        let warm_text = collected
            .scene
            .texts
            .iter()
            .find(|text| text.text.text == "WarmRow 7")
            .unwrap_or_else(|| {
                panic!(
                    "after-bound lazy text should reach WGPU scene collection; graph_texts={graph_labels:?} root_texts={root_text_labels:?} child_layers={child_layer_count}"
                )
            });

        assert!(
            warm_text.rect.y >= 96.0,
            "after-bound text should be below the viewport, got {:?}",
            warm_text.rect
        );
        assert_eq!(
            visible_draw_rect(warm_text.rect, warm_text.clip),
            None,
            "after-bound text should remain clipped away for drawing while staying available for glyph prewarm"
        );
        assert!(
            text_draw_should_prewarm_in_viewport(
                warm_text.rect,
                warm_text.clip,
                ViewportUniformParams {
                    width: 240,
                    height: 96,
                    offset: [0.0, 0.0],
                },
                1.0,
            ),
            "after-bound text inside the warm window must be selected by WGPU prewarm"
        );
    }

    #[test]
    fn direct_translation_accepts_nearly_identity_axis_scale_noise() {
        let local_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 393.3,
            height: 16.8,
        };
        let quad = [
            [10.0, 78.399_994],
            [403.3, 78.399_994],
            [10.0, 95.2],
            [403.3, 95.2],
        ];
        let transform = ProjectiveTransform::from_rect_to_quad(local_bounds, quad);

        assert_eq!(
            direct_translation(transform),
            Some(Point::new(10.0, 78.399_994)),
        );
    }

    #[test]
    fn layer_surface_requirements_keep_shape_plus_isolating_child_as_mixed_content() {
        let mut child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 24.0,
                height: 18.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::Rect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 24.0,
                            height: 18.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                        stroke: None,
                    },
                    clip: None,
                }),
            })],
        );
        child.transform_to_parent = ProjectiveTransform::translation(8.0, 6.0);
        child.graphics_layer.render_effect = Some(RenderEffect::blur(2.0));

        let layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            vec![
                RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(DrawPrimitiveNode {
                        primitive: DrawPrimitive::Rect {
                            rect: Rect {
                                x: 0.0,
                                y: 0.0,
                                width: 64.0,
                                height: 32.0,
                            },
                            brush: Brush::solid(Color::BLACK),
                            stroke: None,
                        },
                        clip: None,
                    }),
                }),
                RenderNode::Layer(Box::new(child)),
            ],
        );

        let requirements = layer_surface_requirements(&layer);

        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::MixedDirectContent));
        assert!(!requirements
            .surface_requirements
            .has_isolating_requirement());
    }

    #[test]
    fn build_scene_window_filters_and_translates_items() {
        let mut shape = test_shape(6, BlendMode::SrcOver);
        shape.rect.x = 12.0;
        shape.rect.y = 25.0;
        shape.local_rect.x = 12.0;
        shape.local_rect.y = 25.0;
        shape.quad = [[12.0, 25.0], [20.0, 25.0], [12.0, 33.0], [20.0, 33.0]];
        shape.clip = Some(Rect {
            x: 11.0,
            y: 24.0,
            width: 10.0,
            height: 10.0,
        });

        let mut image = test_image(8, BlendMode::SrcOver);
        image.rect.x = 18.0;
        image.rect.y = 27.0;
        image.local_rect.x = 18.0;
        image.local_rect.y = 27.0;
        image.quad = [[18.0, 27.0], [26.0, 27.0], [18.0, 35.0], [26.0, 35.0]];

        let mut text = test_text(9);
        text.rect.x = 16.0;
        text.rect.y = 29.0;
        text.clip = Some(Rect {
            x: 15.0,
            y: 28.0,
            width: 9.0,
            height: 6.0,
        });

        let mut shadow_shape = test_shape(7, BlendMode::SrcOver);
        shadow_shape.rect.x = 14.0;
        shadow_shape.rect.y = 26.0;
        shadow_shape.local_rect.x = 14.0;
        shadow_shape.local_rect.y = 26.0;
        shadow_shape.quad = [[14.0, 26.0], [22.0, 26.0], [14.0, 34.0], [22.0, 34.0]];
        let mut shadow = test_shadow_draw(vec![(shadow_shape, BlendMode::SrcOver)]);
        shadow.z_index = 7;

        let mut nested_effect = effect_layer(6, 10);
        nested_effect.rect.x = 13.0;
        nested_effect.rect.y = 24.0;
        nested_effect.clip = Some(Rect {
            x: 15.0,
            y: 25.0,
            width: 4.0,
            height: 5.0,
        });

        let mut nested_backdrop = backdrop_layer(8);
        nested_backdrop.rect.x = 17.0;
        nested_backdrop.rect.y = 26.0;
        nested_backdrop.clip = Some(Rect {
            x: 18.0,
            y: 27.0,
            width: 3.0,
            height: 4.0,
        });

        let window = build_scene_window(
            SceneWindowSource {
                shapes: &[test_shape(4, BlendMode::SrcOver), shape],
                images: &[image],
                texts: &[text],
                shadow_draws: &[shadow],
                draw_ops: &[],
                effect_layers: &[effect_layer(2, 4), nested_effect.clone()],
                backdrop_layers: &[backdrop_layer(4), nested_backdrop.clone()],
            },
            5,
            10,
            Rect {
                x: 10.0,
                y: 20.0,
                width: 20.0,
                height: 20.0,
            },
        );

        assert_eq!(window.shapes.len(), 1);
        assert_eq!(
            window.shapes[0].rect,
            Rect {
                x: 2.0,
                y: 5.0,
                width: 8.0,
                height: 8.0,
            }
        );
        assert_eq!(
            window.shapes[0].clip,
            Some(Rect {
                x: 1.0,
                y: 4.0,
                width: 10.0,
                height: 10.0,
            })
        );
        assert_eq!(window.images.len(), 1);
        assert_eq!(window.images[0].rect.x, 8.0);
        assert_eq!(window.images[0].rect.y, 7.0);
        assert_eq!(window.texts.len(), 1);
        assert_eq!(window.texts[0].rect.x, 6.0);
        assert_eq!(window.texts[0].rect.y, 9.0);
        assert_eq!(
            window.texts[0].clip,
            Some(Rect {
                x: 5.0,
                y: 8.0,
                width: 9.0,
                height: 6.0,
            })
        );
        assert_eq!(window.shadow_draws.len(), 1);
        assert_eq!(window.shadow_draws[0].shapes[0].0.rect.x, 4.0);
        assert_eq!(window.shadow_draws[0].shapes[0].0.rect.y, 6.0);
        assert_eq!(window.effect_layers.len(), 1);
        assert_eq!(
            window.effect_layers[0].rect,
            Rect {
                x: 3.0,
                y: 4.0,
                width: 10.0,
                height: 10.0,
            }
        );
        assert_eq!(
            window.effect_layers[0].clip,
            Some(Rect {
                x: 5.0,
                y: 5.0,
                width: 4.0,
                height: 5.0,
            })
        );
        assert_eq!(window.backdrop_layers.len(), 1);
        assert_eq!(
            window.backdrop_layers[0].rect,
            Rect {
                x: 7.0,
                y: 6.0,
                width: 10.0,
                height: 10.0,
            }
        );
        assert_eq!(
            window.backdrop_layers[0].clip,
            Some(Rect {
                x: 8.0,
                y: 7.0,
                width: 3.0,
                height: 4.0,
            })
        );
    }

    #[test]
    fn filtered_effect_layer_index_counts_only_window_members() {
        let effects = vec![
            effect_layer(0, 2),
            effect_layer(5, 12),
            effect_layer(6, 10),
            effect_layer(14, 20),
        ];

        assert_eq!(filtered_effect_layer_index(&effects, 1, 5, 12), Some(0));
        assert_eq!(filtered_effect_layer_index(&effects, 2, 5, 12), Some(1));
        assert_eq!(filtered_effect_layer_index(&effects, 3, 5, 12), None);
    }

    #[test]
    fn blend_mode_support_matrix_is_explicit() {
        assert!(is_blend_mode_supported(BlendMode::SrcOver));
        assert!(is_blend_mode_supported(BlendMode::DstOut));
        assert!(!is_blend_mode_supported(BlendMode::Clear));
        assert!(!is_blend_mode_supported(BlendMode::Multiply));
    }

    #[test]
    fn collect_non_effect_segment_items_preserves_global_z_order() {
        let shapes = vec![
            test_shape(3, BlendMode::SrcOver),
            test_shape(1, BlendMode::DstOut),
        ];
        let images = vec![test_image(2, BlendMode::SrcOver)];
        let texts = vec![test_text(0)];
        let shadows: Vec<ShadowDraw> = Vec::new();
        let draw_ops = test_draw_ops(&shapes, &images, &texts, &shadows);

        let mut scratch = Vec::new();
        collect_non_effect_segment_items(
            &shapes,
            &images,
            &texts,
            &shadows,
            &draw_ops,
            0,
            4,
            &[],
            100,
            100,
            1.0,
            &mut scratch,
        );
        let items: Vec<_> = scratch.iter().map(|(_, item)| *item).collect();
        assert_eq!(
            items,
            vec![
                SegmentDrawItem::Text(0),
                SegmentDrawItem::Shape(1),
                SegmentDrawItem::Image(0),
                SegmentDrawItem::Shape(0),
            ]
        );
    }

    #[test]
    fn collect_non_effect_segment_items_filters_effect_ranges() {
        let shapes = vec![
            test_shape(1, BlendMode::SrcOver),
            test_shape(3, BlendMode::DstOut),
        ];
        let images = vec![test_image(2, BlendMode::SrcOver)];
        let texts = vec![test_text(4)];
        let shadows: Vec<ShadowDraw> = Vec::new();
        let draw_ops = test_draw_ops(&shapes, &images, &texts, &shadows);
        let effect_ranges = [std::ops::Range { start: 2, end: 4 }];

        let mut scratch = Vec::new();
        collect_non_effect_segment_items(
            &shapes,
            &images,
            &texts,
            &shadows,
            &draw_ops,
            0,
            5,
            &effect_ranges,
            100,
            100,
            1.0,
            &mut scratch,
        );
        let items: Vec<_> = scratch.iter().map(|(_, item)| *item).collect();
        assert_eq!(
            items,
            vec![SegmentDrawItem::Shape(0), SegmentDrawItem::Text(0)]
        );
    }

    #[test]
    fn collect_non_effect_segment_items_culls_offscreen_shapes_but_keeps_text_prewarm() {
        let mut shape = test_shape(0, BlendMode::SrcOver);
        shape.rect.y = 160.0;
        shape.local_rect.y = 160.0;
        shape.quad = [[0.0, 160.0], [8.0, 160.0], [0.0, 168.0], [8.0, 168.0]];

        let shapes = vec![shape];
        let images = Vec::new();
        let mut text = test_text(1);
        text.rect.y = 160.0;
        let texts = vec![text];
        let shadows: Vec<ShadowDraw> = Vec::new();
        let draw_ops = test_draw_ops(&shapes, &images, &texts, &shadows);

        let mut scratch = Vec::new();
        collect_non_effect_segment_items(
            &shapes,
            &images,
            &texts,
            &shadows,
            &draw_ops,
            0,
            2,
            &[],
            100,
            100,
            1.0,
            &mut scratch,
        );

        let items: Vec<_> = scratch.iter().map(|(_, item)| *item).collect();
        assert_eq!(items, vec![SegmentDrawItem::Text(0)]);
    }

    #[test]
    fn segment_command_iter_merges_non_conflicting_batches_into_one_chunk() {
        let ordered_items = vec![
            (0, SegmentDrawItem::Shape(0)),
            (1, SegmentDrawItem::Image(0)),
            (2, SegmentDrawItem::Text(0)),
        ];
        let shapes = vec![test_shape(0, BlendMode::SrcOver)];
        let images = vec![test_image(1, BlendMode::DstOut)];

        let commands: Vec<_> = SegmentCommandIter::new(
            &ordered_items,
            &shapes,
            &images,
            ShapeBatchLimits::desktop(),
        )
        .collect();

        assert_eq!(
            commands,
            vec![SegmentRenderCommand::DrawChunk(chunk(&[
                SegmentBatchPlan::Shape {
                    start: 0,
                    end: 1,
                    blend_mode: BlendMode::SrcOver,
                },
                SegmentBatchPlan::Image {
                    start: 1,
                    end: 2,
                    blend_mode: BlendMode::DstOut,
                },
                SegmentBatchPlan::Text { start: 2, end: 3 },
            ]))]
        );
    }

    #[test]
    fn segment_command_iter_keeps_layer_composites_in_ordered_draw_chunk() {
        let ordered_items = vec![
            (0, SegmentDrawItem::Shape(0)),
            (1, SegmentDrawItem::Composite(0)),
            (2, SegmentDrawItem::Image(0)),
            (3, SegmentDrawItem::Composite(1)),
            (4, SegmentDrawItem::Text(0)),
        ];
        let shapes = vec![test_shape(0, BlendMode::SrcOver)];
        let images = vec![test_image(2, BlendMode::SrcOver)];

        let commands: Vec<_> = SegmentCommandIter::new(
            &ordered_items,
            &shapes,
            &images,
            ShapeBatchLimits::desktop(),
        )
        .collect();

        assert_eq!(
            commands,
            vec![SegmentRenderCommand::DrawChunk(chunk(&[
                SegmentBatchPlan::Shape {
                    start: 0,
                    end: 1,
                    blend_mode: BlendMode::SrcOver,
                },
                SegmentBatchPlan::Composite { start: 1, end: 2 },
                SegmentBatchPlan::Image {
                    start: 2,
                    end: 3,
                    blend_mode: BlendMode::SrcOver,
                },
                SegmentBatchPlan::Composite { start: 3, end: 4 },
                SegmentBatchPlan::Text { start: 4, end: 5 },
            ]))]
        );
    }

    #[test]
    fn retain_renderable_shadow_items_culls_invisible_shadow_boundaries() {
        let shapes = vec![test_shape(0, BlendMode::SrcOver)];
        let images = vec![test_image(2, BlendMode::SrcOver)];
        let mut shadow_shape = test_shape(1, BlendMode::SrcOver);
        shadow_shape.rect = Rect {
            x: 500.0,
            y: 500.0,
            width: 12.0,
            height: 12.0,
        };
        let shadow_draws = vec![ShadowDraw {
            shapes: vec![(shadow_shape, BlendMode::SrcOver)],
            texts: Vec::new(),
            blur_radius: 8.0,
            clip: None,
            z_index: 1,
        }];
        let mut ordered_items = vec![
            (0, SegmentDrawItem::Shape(0)),
            (1, SegmentDrawItem::Shadow(0)),
            (2, SegmentDrawItem::Image(0)),
        ];

        let culled =
            retain_renderable_shadow_items(&mut ordered_items, &shadow_draws, 100, 100, 1.0, 4096);
        let commands: Vec<_> = SegmentCommandIter::new(
            &ordered_items,
            &shapes,
            &images,
            ShapeBatchLimits::desktop(),
        )
        .collect();

        assert_eq!(culled, 1);
        assert_eq!(
            commands,
            vec![SegmentRenderCommand::DrawChunk(chunk(&[
                SegmentBatchPlan::Shape {
                    start: 0,
                    end: 1,
                    blend_mode: BlendMode::SrcOver,
                },
                SegmentBatchPlan::Image {
                    start: 1,
                    end: 2,
                    blend_mode: BlendMode::SrcOver,
                },
            ]))]
        );
    }

    #[test]
    fn retain_renderable_shadow_items_keeps_visible_shadow_boundaries() {
        let mut shadow_shape = test_shape(1, BlendMode::SrcOver);
        shadow_shape.rect = Rect {
            x: 20.0,
            y: 20.0,
            width: 12.0,
            height: 12.0,
        };
        let shadow_draws = vec![ShadowDraw {
            shapes: vec![(shadow_shape, BlendMode::SrcOver)],
            texts: Vec::new(),
            blur_radius: 8.0,
            clip: None,
            z_index: 1,
        }];
        let mut ordered_items = vec![(1, SegmentDrawItem::Shadow(0))];

        let culled =
            retain_renderable_shadow_items(&mut ordered_items, &shadow_draws, 100, 100, 1.0, 4096);

        assert_eq!(culled, 0);
        assert_eq!(ordered_items, vec![(1, SegmentDrawItem::Shadow(0))]);
    }

    #[test]
    fn shape_data_layout_matches_the_wgsl_mirror() {
        // 10 x vec4-sized slots. The uniform address space requires a 16-byte
        // multiple, and `shape.wgsl`'s array length literal is derived from
        // this size — if it drifts, batches silently overrun the binding.
        assert_eq!(std::mem::size_of::<ShapeData>(), 160);
        assert_eq!(std::mem::size_of::<ShapeData>() % 16, 0);
        assert_eq!(std::mem::size_of::<GradientStop>(), 32);
    }

    #[test]
    fn shape_flags_pack_kind_cap_and_join_without_collision() {
        assert_eq!(
            pack_shape_flags(SHAPE_KIND_FILL, StrokeCap::Butt, StrokeJoin::Miter),
            0.0
        );
        assert_eq!(
            pack_shape_flags(SHAPE_KIND_STROKE, StrokeCap::Butt, StrokeJoin::Miter),
            1.0
        );
        assert_eq!(
            pack_shape_flags(SHAPE_KIND_ARC, StrokeCap::Butt, StrokeJoin::Miter),
            2.0
        );
        // cap in bits 2-3, join in bits 4-5
        assert_eq!(
            pack_shape_flags(SHAPE_KIND_ARC, StrokeCap::Round, StrokeJoin::Miter),
            2.0 + 4.0
        );
        assert_eq!(
            pack_shape_flags(SHAPE_KIND_ARC, StrokeCap::Square, StrokeJoin::Miter),
            2.0 + 8.0
        );
        assert_eq!(
            pack_shape_flags(SHAPE_KIND_STROKE, StrokeCap::Butt, StrokeJoin::Round),
            1.0 + 16.0
        );
        assert_eq!(
            pack_shape_flags(SHAPE_KIND_STROKE, StrokeCap::Butt, StrokeJoin::Bevel),
            1.0 + 32.0
        );
        // Every combination must round-trip through f32 exactly.
        for kind in [SHAPE_KIND_FILL, SHAPE_KIND_STROKE, SHAPE_KIND_ARC] {
            for cap in [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square] {
                for join in [StrokeJoin::Miter, StrokeJoin::Round, StrokeJoin::Bevel] {
                    let packed = pack_shape_flags(kind, cap, join);
                    let bits = packed as u32;
                    assert_eq!(bits & 3, kind);
                    assert_eq!((bits >> 2) & 3, stroke_cap_code(cap));
                    assert_eq!((bits >> 4) & 3, stroke_join_code(join));
                    assert_eq!(packed, bits as f32, "flags must be exact in f32");
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn mesh_vertex_layout_matches_the_wgsl_input() {
        // {pos: vec2<f32>, uv: vec2<f32>, shape_idx: u32} = 20 bytes, no
        // padding — the vertex buffer layout stride relies on it.
        assert_eq!(std::mem::size_of::<MeshVertex>(), 20);
    }

    /// f32 port of `sdf_arc_band` (shape.wgsl), operation for operation: the
    /// same ra/rb derivation and clamp, the same mirror trick (`abs` on the
    /// rotated x), the same cap branches.
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    fn sdf_arc_band_reference(
        p: [f32; 2],
        center: [f32; 2],
        inner: f32,
        outer: f32,
        mid_sin_cos: [f32; 2],
        half_sin_cos: [f32; 2],
        cap: u32,
    ) -> f32 {
        let ra = (outer + inner) * 0.5;
        let rb = ((outer - inner) * 0.5).max(0.0);
        let sm = mid_sin_cos[0];
        let cm = mid_sin_cos[1];
        let d = [p[0] - center[0], p[1] - center[1]];
        let mut q = [-sm * d[0] + cm * d[1], cm * d[0] + sm * d[1]];
        q[0] = q[0].abs();
        let sc = half_sin_cos;
        let mut dist = if sc[1] * q[0] > sc[0] * q[1] {
            let dx = q[0] - sc[0] * ra;
            let dy = q[1] - sc[1] * ra;
            (dx * dx + dy * dy).sqrt() - rb
        } else {
            ((q[0] * q[0] + q[1] * q[1]).sqrt() - ra).abs() - rb
        };
        let plane = sc[1] * q[0] - sc[0] * q[1];
        // STROKE_CAP_BUTT = 0, STROKE_CAP_SQUARE = 2, as in the shader.
        if cap == 0 {
            dist = dist.max(plane);
        } else if cap == 2 {
            dist = dist.max(plane - rb);
        }
        dist
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn point_in_triangle(p: [f64; 2], tri: &[[f64; 2]; 3]) -> bool {
        let side = |a: [f64; 2], b: [f64; 2]| {
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
        };
        let d0 = side(tri[0], tri[1]);
        let d1 = side(tri[1], tri[2]);
        let d2 = side(tri[2], tri[0]);
        let has_neg = d0 < 0.0 || d1 < 0.0 || d2 < 0.0;
        let has_pos = d0 > 0.0 || d1 > 0.0 || d2 > 0.0;
        !(has_neg && has_pos)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn converted_arc_shape(arc: cranpose_ui_graphics::ArcGeometry, root_scale: f32) -> ShapeData {
        let bounds = arc.bounds();
        let mut shape = test_shape(0, BlendMode::SrcOver);
        shape.rect = bounds;
        shape.local_rect = bounds;
        shape.quad = [
            [bounds.x, bounds.y],
            [bounds.x + bounds.width, bounds.y],
            [bounds.x, bounds.y + bounds.height],
            [bounds.x + bounds.width, bounds.y + bounds.height],
        ];
        shape.arc = Some(arc);
        let mut converted = ShapeData::zeroed();
        convert_shape_into_slots(&shape, root_scale, 0, &mut converted, &mut []);
        converted
    }

    /// The containment invariant, checked directly: every point of the
    /// capture box whose (exactly ported) SDF keeps it must lie inside the
    /// emitted triangle set. Thin/thick, tiny/huge, full rings, near-zero
    /// and near-TAU sweeps, all caps, `Ri == 0` discs and pie wedges.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn arc_mesh_contains_every_band_pixel() {
        use cranpose_ui_graphics::ArcGeometry;
        let tau = cranpose_ui_graphics::TAU;
        let center = Point::new(250.0, 250.0);
        let cases: &[(f32, f32, f32, f32, StrokeCap)] = &[
            // full ring, thin band
            (90.0, 100.0, 0.0, tau, StrokeCap::Round),
            // sweep > TAU normalizes to a closed ring
            (80.0, 100.0, 1.0, 10.0, StrokeCap::Butt),
            // full disc: Ri == 0
            (0.0, 40.0, 0.0, tau, StrokeCap::Round),
            // thick partial arc, every cap
            (30.0, 80.0, 0.7, 2.5, StrokeCap::Butt),
            (30.0, 80.0, 0.7, 2.5, StrokeCap::Round),
            (30.0, 80.0, 0.7, 2.5, StrokeCap::Square),
            // thin, axis-crossing sweep
            (99.0, 101.0, 3.0, 4.0, StrokeCap::Round),
            // tiny
            (0.6, 2.0, 0.3, 1.2, StrokeCap::Butt),
            // huge radius, thin band
            (1900.0, 1904.0, 0.1, 0.35, StrokeCap::Square),
            // near-zero sweep
            (40.0, 60.0, 5.0, 1e-3, StrokeCap::Round),
            // sweep near TAU: the cap pads wrap the range closed
            (40.0, 60.0, 0.2, tau - 1e-3, StrokeCap::Butt),
            // rb_m >= ra: the cap disc wraps the center (pie wedge)
            (0.0, 3.0, 1.0, 2.0, StrokeCap::Round),
            // filled annular sector (butt radial ends)
            (20.0, 60.0, 4.5, 1.9, StrokeCap::Butt),
        ];
        for (case, &(inner, outer, start, sweep, cap)) in cases.iter().enumerate() {
            // 2.75 is deliberately non-dyadic: quad corners and rect then
            // disagree by an ulp, which the axis-aligned gate must tolerate
            // (an equality-with-rect gate silently failed every arc on the
            // Huawei at scale 2.75).
            for root_scale in [1.0f32, 2.0, 2.75] {
                let arc = ArcGeometry::new(center, inner, outer, start, sweep, cap);
                assert!(!arc.is_degenerate(), "case {case} must be drawable");
                let converted = converted_arc_shape(arc, root_scale);
                let band = arc_mesh_band(&converted)
                    .unwrap_or_else(|| panic!("case {case} must qualify for meshing"));
                let mut vertices = Vec::new();
                let mut indices = Vec::new();
                let segments =
                    emit_arc_band_mesh(&converted, 0, &band, &mut vertices, &mut indices)
                        .unwrap_or_else(|| panic!("case {case} must produce a mesh"));
                assert!(segments >= ARC_MESH_MIN_SEGMENTS);
                // The rasterized set is the indexed walk: triangles are index
                // triples into the shared vertex list.
                let position = |index: u32| {
                    let p = vertices[index as usize].position;
                    [p[0] as f64, p[1] as f64]
                };
                let triangles: Vec<[[f64; 2]; 3]> = indices
                    .chunks_exact(3)
                    .map(|tri| [position(tri[0]), position(tri[1]), position(tri[2])])
                    .collect();

                // Sample the QUAD box, not `rect`: quad expansion rasterizes the
                // quad, the mesh clips to the quad, and at non-dyadic root
                // scales the two boxes differ by an ulp.
                let [qx, qy, ..] = converted.quad01;
                let [_, _, qr, qb] = converted.quad23;
                let (rw, rh) = (qr - qx, qb - qy);
                let cap_bits = (converted.stroke_params[1].max(0.0) as u32 >> 2) & 3;
                let step = (rw.max(rh) / 400.0).clamp(0.25, 2.0);
                let mut band_points = 0usize;
                let mut y = qy;
                while y <= qb {
                    let mut x = qx;
                    while x <= qr {
                        let dist = sdf_arc_band_reference(
                            [x, y],
                            [converted.arc_params[0], converted.arc_params[1]],
                            converted.stroke_params[3],
                            converted.stroke_params[2],
                            [converted.radii[0], converted.radii[1]],
                            [converted.radii[2], converted.radii[3]],
                            cap_bits,
                        );
                        if dist <= 0.5 {
                            band_points += 1;
                            let p = [x as f64, y as f64];
                            assert!(
                                triangles.iter().any(|tri| point_in_triangle(p, tri)),
                                "case {case} scale {root_scale}: band point ({x}, {y}) \
                                 dist {dist} escapes the mesh"
                            );
                        }
                        x += step;
                    }
                    y += step;
                }
                assert!(
                    band_points > 0,
                    "case {case} scale {root_scale}: the sampling grid never hit the band"
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn arc_mesh_passthrough_replicates_the_quad_expansion() {
        let shape = test_shape(0, BlendMode::SrcOver);
        let mut converted = ShapeData::zeroed();
        convert_shape_into_slots(&shape, 1.0, 0, &mut converted, &mut []);
        let build =
            build_arc_mesh_vertices(std::slice::from_ref(&converted)).expect("within budget");
        assert_eq!(build.meshed_arcs, 0);
        assert_eq!(build.passthrough, 1);
        // Four shared corner vertices, six indices — amplification-free.
        assert_eq!(build.vertices.len(), 4);
        assert_eq!(build.index_prefix, vec![0, 6]);
        assert_eq!(build.indices, vec![0, 1, 2, 2, 1, 3]);
        let corners = [
            ([converted.quad01[0], converted.quad01[1]], [0.0f32, 0.0]),
            ([converted.quad01[2], converted.quad01[3]], [1.0, 0.0]),
            ([converted.quad23[0], converted.quad23[1]], [0.0, 1.0]),
            ([converted.quad23[2], converted.quad23[3]], [1.0, 1.0]),
        ];
        for (vertex, corner) in build.vertices.iter().zip(corners) {
            assert_eq!(vertex.position, corner.0);
            assert_eq!(vertex.uv, corner.1);
            assert_eq!(vertex.shape_idx, 0);
        }
        // The indexed walk expands to vs_main's slot order: triangles
        // (0, 1, 2) and (2, 1, 3).
        for (index, corner) in build.indices.iter().zip([0usize, 1, 2, 2, 1, 3]) {
            assert_eq!(build.vertices[*index as usize].position, corners[corner].0);
            assert_eq!(build.vertices[*index as usize].uv, corners[corner].1);
        }
    }

    /// The indexed-topology contract for arcs whose trapezoids survive
    /// clipping whole: every band boundary contributes exactly one (inner,
    /// outer) vertex pair, both adjacent trapezoids reference it through the
    /// index list, and a closed ring's last segment wraps around to boundary
    /// zero's pair — one seam vertex pair instead of bitwise-equal copies.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn arc_mesh_indices_share_boundary_vertices_and_wrap_closed_rings() {
        use cranpose_ui_graphics::ArcGeometry;
        let tau = cranpose_ui_graphics::TAU;
        // (sweep, expected boundary count relation): a closed ring wraps
        // (boundaries == segments), an open arc does not (segments + 1).
        for (sweep, closed) in [(tau, true), (1.9f32, false)] {
            let arc = ArcGeometry::new(
                Point::new(250.0, 250.0),
                80.0,
                100.0,
                0.7,
                sweep,
                StrokeCap::Round,
            );
            let mut converted = converted_arc_shape(arc, 1.0);
            // Inflate the quad box (and rect, for uv) far beyond the dilated
            // band so NO trapezoid is clipped: every segment must take the
            // shared-boundary path.
            converted.rect = [0.0, 0.0, 500.0, 500.0];
            converted.quad01 = [0.0, 0.0, 500.0, 0.0];
            converted.quad23 = [0.0, 500.0, 500.0, 500.0];
            let band = arc_mesh_band(&converted).expect("arc must qualify");
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            let segments = emit_arc_band_mesh(&converted, 0, &band, &mut vertices, &mut indices)
                .expect("arc must mesh");
            let boundary_count = if closed { segments } else { segments + 1 };
            assert_eq!(
                vertices.len(),
                2 * boundary_count,
                "closed={closed}: every boundary owns exactly one (inner, outer) pair"
            );
            assert_eq!(indices.len(), 6 * segments);
            // Emission order is boundary order: boundary j's pair is
            // (2j, 2j + 1). Each segment must reference its own boundary and
            // its successor's — modulo the count exactly when closed.
            for j in 0..segments {
                let jb = (j + 1) % boundary_count;
                let (in_a, out_a) = (2 * j as u32, 2 * j as u32 + 1);
                let (in_b, out_b) = (2 * jb as u32, 2 * jb as u32 + 1);
                assert_eq!(
                    indices[6 * j..6 * j + 6],
                    [in_a, out_a, out_b, in_a, out_b, in_b],
                    "closed={closed}: segment {j} must share its boundary pairs"
                );
            }
            if closed {
                // The wrap made concrete: the final segment indexes boundary
                // zero's vertices.
                assert_eq!(indices[6 * segments - 1], 0);
            }
            // Inner vertices ride the dilated inner radius, outer vertices
            // the pushed-out chord radius — sanity that pairs are ordered
            // (inner, outer).
            for pair in vertices.chunks_exact(2) {
                let radius = |v: &MeshVertex| {
                    let dx = v.position[0] - 250.0;
                    let dy = v.position[1] - 250.0;
                    (dx * dx + dy * dy).sqrt()
                };
                assert!(radius(&pair[0]) < radius(&pair[1]));
            }
        }
    }

    /// The private-vertex arm of the indexed topology: under the real
    /// tight-AABB quad the pushed-out chord vertices near the box edges get
    /// clipped, and those trapezoids must fan over vertices of their own —
    /// appended after the shared block, carrying clip-plane coordinates —
    /// while untouched diagonal trapezoids still share boundary pairs.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn arc_mesh_clipped_segments_fan_over_private_vertices() {
        use cranpose_ui_graphics::ArcGeometry;
        let arc = ArcGeometry::new(
            Point::new(250.0, 250.0),
            80.0,
            100.0,
            0.0,
            cranpose_ui_graphics::TAU,
            StrokeCap::Round,
        );
        let converted = converted_arc_shape(arc, 1.0);
        let band = arc_mesh_band(&converted).expect("ring must qualify");
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        emit_arc_band_mesh(&converted, 0, &band, &mut vertices, &mut indices)
            .expect("ring must mesh");
        // Sharing must actually happen: a shared boundary vertex is used by
        // both of its trapezoids' fans (at least three triangle references).
        let mut uses = vec![0usize; vertices.len()];
        for &index in &indices {
            uses[index as usize] += 1;
        }
        assert!(
            uses.iter().any(|&count| count >= 3),
            "some boundary vertices must be shared across trapezoids"
        );
        // Clipping must actually happen, and clipped polygons index private
        // vertices lying bitwise ON the quad box (the clipper writes the
        // bound coordinate exactly; boundary vertices never touch the box —
        // inner ones sit strictly inside, pushed-out outer ones strictly
        // outside near the extremes, where they are clipped).
        let [left, top, ..] = converted.quad01;
        let [.., right, bottom] = converted.quad23;
        let clipped: Vec<&MeshVertex> = vertices
            .iter()
            .filter(|vertex| {
                let [x, y] = vertex.position;
                x == left || x == right || y == top || y == bottom
            })
            .collect();
        assert!(
            !clipped.is_empty(),
            "the tight box must clip the pushed-out chord vertices"
        );
        // Fewer unique vertices than the non-indexed emitter's
        // three-per-triangle — the amplification this change removes.
        assert!(
            vertices.len() < indices.len(),
            "{} unique vertices should undercut {} triangle corners",
            vertices.len(),
            indices.len()
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn arc_mesh_budget_overflow_falls_back_to_whole_slot_passthrough() {
        use cranpose_ui_graphics::ArcGeometry;
        // 100 large full rings mesh at the 64-segment ceiling (well over
        // 4 KB of vertices + indices each), far past the byte budget
        // max(100 * ~960 B, ~80 KB) — the builder must refuse the whole
        // slot rather than truncate.
        let arc = ArcGeometry::new(
            Point::new(2000.0, 2000.0),
            1690.0,
            1710.0,
            0.0,
            cranpose_ui_graphics::TAU,
            StrokeCap::Round,
        );
        let converted = converted_arc_shape(arc, 1.0);
        let shapes = vec![converted; 100];
        assert!(build_arc_mesh_vertices(&shapes).is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn shape_batch_limits_follow_uniform_binding_size() {
        // With a 160-byte ShapeData, even a desktop-class 64 KiB binding can no
        // longer hold the full compile-time cap: 65536 / 160 = 409 < 768.
        let desktop_shapes = 65536 / std::mem::size_of::<ShapeData>();
        assert_eq!(desktop_shapes, 409);
        assert_eq!(
            ShapeBatchLimits::desktop(),
            ShapeBatchLimits {
                max_shapes_per_batch: desktop_shapes.min(MAX_SHAPES_PER_BATCH),
                max_gradient_stops: MAX_GRADIENT_STOPS,
                storage: false,
            }
        );

        // The 16 KiB downlevel/GLES minimum must shrink batches to fit:
        // 16384 / 160-byte ShapeData = 102 shapes, 16384 / 32-byte stop = 512.
        let downlevel = ShapeBatchLimits::for_uniform_binding_size(16384);
        assert_eq!(downlevel.max_shapes_per_batch, 16384 / 160);
        assert_eq!(downlevel.max_shapes_per_batch, 102);
        assert_eq!(downlevel.max_gradient_stops, 512.min(MAX_GRADIENT_STOPS));
        assert!(downlevel.max_shapes_per_batch * std::mem::size_of::<ShapeData>() <= 16384);
        assert!(downlevel.max_gradient_stops * std::mem::size_of::<GradientStop>() <= 16384);

        // Degenerate limits must not produce zero-sized buffers.
        let tiny = ShapeBatchLimits::for_uniform_binding_size(1);
        assert_eq!(tiny.max_shapes_per_batch, 1);
        assert_eq!(tiny.max_gradient_stops, 1);
    }

    #[test]
    fn storage_shape_batch_limits_uncap_the_batch_and_start_small() {
        // A typical 128 MiB storage binding hits the compile-time ceilings,
        // not the device limit: one batch holds the whole scene.
        let storage = ShapeBatchLimits::for_storage_binding_size(128 << 20);
        assert!(storage.storage);
        assert_eq!(storage.max_shapes_per_batch, MAX_SHAPES_PER_STORAGE_BATCH);
        assert_eq!(
            storage.max_gradient_stops,
            MAX_GRADIENT_STOPS_PER_STORAGE_BATCH
        );

        // The buffers must not be allocated at the multi-megabyte ceiling up
        // front; they start small and grow on demand.
        assert_eq!(
            storage.initial_shape_capacity(),
            INITIAL_STORAGE_BATCH_CAPACITY
        );
        assert_eq!(
            storage.initial_gradient_capacity(),
            INITIAL_STORAGE_BATCH_CAPACITY
        );
        assert_eq!(
            storage.data_binding_type(),
            wgpu::BufferBindingType::Storage { read_only: true }
        );
        assert!(storage
            .data_buffer_usage()
            .contains(wgpu::BufferUsages::STORAGE));

        // Uniform mode keeps its start-at-the-cap invariant: a uniform
        // binding smaller than the shader's fixed array fails validation.
        let uniform = ShapeBatchLimits::desktop();
        assert_eq!(
            uniform.initial_shape_capacity(),
            uniform.max_shapes_per_batch
        );
        assert_eq!(
            uniform.initial_gradient_capacity(),
            uniform.max_gradient_stops
        );
        assert_eq!(
            uniform.data_binding_type(),
            wgpu::BufferBindingType::Uniform
        );
        assert!(uniform
            .data_buffer_usage()
            .contains(wgpu::BufferUsages::UNIFORM));
    }

    #[test]
    fn storage_shape_shader_swaps_the_arrays_to_runtime_sized_storage() {
        let source = shape_shader_source(ShapeBatchLimits::for_storage_binding_size(128 << 20));
        assert!(
            source.contains("var<storage, read> shape_data: array<ShapeData>;"),
            "storage-mode shader must declare a runtime-sized shape array"
        );
        assert!(
            source.contains("var<storage, read> gradient_stops: array<GradientStop>;"),
            "storage-mode shader must declare a runtime-sized gradient array"
        );
        assert!(
            !source.contains("var<uniform> shape_data"),
            "the uniform shape declaration must be fully replaced"
        );
        assert!(
            !source.contains("var<uniform> gradient_stops"),
            "the uniform gradient declaration must be fully replaced"
        );
        assert!(
            source.contains("var<storage, read> paint: array<vec4<f32>>;"),
            "storage-mode shader must declare the retained paint array"
        );
        assert!(
            source.contains("select(shape.color, paint[shape_idx], similarity.paint_select > 0.5)"),
            "storage-mode shader must read paint under the paint_select flag"
        );
        assert!(
            source.contains("fn vs_mesh("),
            "the storage rewrite must leave the retained-mesh vertex entry intact"
        );
        assert!(
            source.contains("fn vs_shape_instanced("),
            "the storage rewrite must leave the instanced-quad vertex entry intact"
        );
        assert_eq!(
            source
                .matches("select(shape.color, paint[shape_idx], similarity.paint_select > 0.5)")
                .count(),
            3,
            "vs_main, vs_shape_instanced and vs_mesh must all read paint under \
             the paint_select flag (meshless retained draws ride the instanced \
             entry when the selection is latched on)"
        );

        // The storage variant is what native devices actually compile; it
        // must be valid WGSL, not just textually plausible.
        let module = naga::front::wgsl::parse_str(&source)
            .expect("storage-mode shape shader must parse as WGSL");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("storage-mode shape shader must validate for WebGPU");
    }

    #[test]
    fn uniform_shape_shader_keeps_the_in_record_color_and_no_paint_binding() {
        // The base text serves WebGL-class uniform devices, which can bind
        // no storage buffers: the paint array and its select must exist only
        // in the storage-mode rewrite.
        for source in [
            Cow::Borrowed(shaders::SHADER),
            shape_shader_source(ShapeBatchLimits::desktop()),
        ] {
            assert!(
                !source.contains("paint: array"),
                "the uniform variant must not declare a paint array"
            );
            assert!(
                source.contains("output.color = shape.color;"),
                "the uniform variant must read the color from ShapeData \
                 (this literal is also what `shape_shader_source` rewrites)"
            );
            assert!(
                source.contains("paint_select: f32"),
                "SimilarityTransform must name the flag field in both \
                 variants; the Rust mirror is Pod and uploads raw bytes"
            );
        }
    }

    #[test]
    fn shipped_shape_shader_array_length_fits_the_downlevel_uniform_floor() {
        // The wasm build uses `shaders::SHADER` verbatim, so its declared array
        // length is simultaneously the wasm batch cap and the WebGL binding
        // size. It must fit the 16 KiB floor exactly.
        assert!(
            shaders::SHADER.contains("array<ShapeData, 102>"),
            "shape.wgsl array length must stay in sync with \
             `shape_shader_source`'s replace string and MAX_SHAPES_PER_BATCH"
        );
        assert!(102 * std::mem::size_of::<ShapeData>() <= 16384);
        assert!(103 * std::mem::size_of::<ShapeData>() > 16384);
    }

    #[test]
    fn glyph_atlas_doubles_on_overflow_and_stops_at_the_device_ceiling() {
        // Every overflow buys one doubling, so an app that needs the old fixed
        // 4096 atlas reaches it in three resets and then stays there.
        assert_eq!(
            next_glyph_atlas_size(TEXT_GLYPH_ATLAS_MIN_SIZE, TEXT_GLYPH_ATLAS_MAX_SIZE),
            1024
        );
        assert_eq!(
            next_glyph_atlas_size(2048, TEXT_GLYPH_ATLAS_MAX_SIZE),
            TEXT_GLYPH_ATLAS_MAX_SIZE
        );
        assert_eq!(
            next_glyph_atlas_size(TEXT_GLYPH_ATLAS_MAX_SIZE, TEXT_GLYPH_ATLAS_MAX_SIZE),
            TEXT_GLYPH_ATLAS_MAX_SIZE
        );

        // A device that only grants `downlevel_defaults()`'s 2048 caps the
        // growth there rather than failing to create the texture.
        assert_eq!(next_glyph_atlas_size(1024, 2048), 2048);
        assert_eq!(next_glyph_atlas_size(2048, 2048), 2048);

        // Never zero and never wrapping, whatever the ceiling turns out to be.
        assert_eq!(next_glyph_atlas_size(u32::MAX, 4096), 4096);
        assert_eq!(next_glyph_atlas_size(0, 0), 1);
    }

    #[test]
    fn glyph_atlas_uv_rect_normalizes_against_the_atlas_it_was_placed_in() {
        // The atlas grows, so a UV is only meaningful together with the size of
        // the texture the entry came from. Reading the size off a constant is
        // what would make a grown atlas sample the wrong glyph.
        let entry = GlyphAtlasEntry {
            x: 128,
            y: 256,
            width: 16,
            height: 32,
        };

        let small = glyph_atlas_uv_rect(entry, 512);
        let large = glyph_atlas_uv_rect(entry, 4096);

        assert_eq!(small.min, [128.0 / 512.0, 256.0 / 512.0]);
        assert_eq!(large.min, [128.0 / 4096.0, 256.0 / 4096.0]);
        assert_eq!(small.max, [144.0 / 512.0, 288.0 / 512.0]);
        assert_eq!(large.max, [144.0 / 4096.0, 288.0 / 4096.0]);
    }

    #[test]
    fn native_shape_shader_source_uses_native_batch_limits() {
        let limits = ShapeBatchLimits::desktop();
        let source = shape_shader_source(limits);

        assert!(source.contains(&format!(
            "array<ShapeData, {}>",
            limits.max_shapes_per_batch
        )));
        assert!(source.contains(&format!(
            "array<GradientStop, {}>",
            limits.max_gradient_stops
        )));
        // Sanity: the substitution actually fired rather than silently leaving
        // the downlevel literal in place.
        assert!(!source.contains("array<ShapeData, 146>"));
    }

    #[test]
    fn stroked_and_arc_shapes_batch_together_with_fills() {
        // Strokes and arcs ride the same pipeline, the same ShapeData array and
        // the same blend state as fills, so a run of mixed shapes must stay a
        // single batch. If they ever split the batch, a polar UI built from
        // hundreds of arcs would pay a draw call per arc — precisely the cost
        // this primitive exists to remove.
        let fill = test_shape(0, BlendMode::SrcOver);
        let mut stroked = test_shape(1, BlendMode::SrcOver);
        stroked.stroke = Some(
            cranpose_ui_graphics::Stroke::new(3.0)
                .with_cap(StrokeCap::Round)
                .with_join(StrokeJoin::Bevel),
        );
        let mut arc = test_shape(2, BlendMode::SrcOver);
        arc.arc = Some(cranpose_ui_graphics::ArcGeometry::new(
            Point::new(4.0, 4.0),
            2.0,
            4.0,
            0.0,
            1.0,
            StrokeCap::Round,
        ));
        let trailing_fill = test_shape(3, BlendMode::SrcOver);

        assert!(!fill.has_stroke_or_arc());
        assert!(stroked.has_stroke_or_arc());
        assert!(arc.has_stroke_or_arc());
        assert!(!trailing_fill.has_stroke_or_arc());

        let shapes = vec![fill, stroked, arc, trailing_fill];
        let ordered_items: Vec<_> = (0..shapes.len())
            .map(|index| (index, SegmentDrawItem::Shape(index)))
            .collect();
        let images = Vec::new();

        let commands: Vec<_> = SegmentCommandIter::new(
            &ordered_items,
            &shapes,
            &images,
            ShapeBatchLimits::desktop(),
        )
        .collect();

        assert_eq!(
            commands,
            vec![SegmentRenderCommand::DrawChunk(chunk(&[
                SegmentBatchPlan::Shape {
                    start: 0,
                    end: 4,
                    blend_mode: BlendMode::SrcOver,
                }
            ]))],
            "mixed fill/stroke/arc runs must stay one batch"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_segment_fusion_budget_allows_small_interleaved_chunks() {
        let ordered_items = vec![
            (0, SegmentDrawItem::Shape(0)),
            (1, SegmentDrawItem::Image(0)),
            (2, SegmentDrawItem::Text(0)),
            (3, SegmentDrawItem::Shape(1)),
        ];
        let shapes = vec![
            test_shape(0, BlendMode::SrcOver),
            test_shape(3, BlendMode::DstOut),
        ];
        let segment = chunk(&[
            SegmentBatchPlan::Shape {
                start: 0,
                end: 1,
                blend_mode: BlendMode::SrcOver,
            },
            SegmentBatchPlan::Image {
                start: 1,
                end: 2,
                blend_mode: BlendMode::SrcOver,
            },
            SegmentBatchPlan::Text { start: 2, end: 3 },
            SegmentBatchPlan::Shape {
                start: 3,
                end: 4,
                blend_mode: BlendMode::DstOut,
            },
        ]);

        let budget = native_segment_fusion_budget(
            &ordered_items,
            &shapes,
            &segment,
            ShapeBatchLimits::desktop(),
        )
        .expect("budget should be valid")
        .expect("chunk should fit native fusion budget");

        assert_eq!(
            budget,
            NativeSegmentFusionBudget {
                shape_count: 2,
                gradient_stop_count: 0,
            }
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_segment_fusion_budget_rejects_shape_uniform_overflow() {
        let ordered_items: Vec<_> = (0..=MAX_SHAPES_PER_BATCH)
            .map(|index| (index, SegmentDrawItem::Shape(index)))
            .collect();
        let shapes: Vec<_> = (0..=MAX_SHAPES_PER_BATCH)
            .map(|index| test_shape(index, BlendMode::SrcOver))
            .collect();
        let segment = chunk(&[
            SegmentBatchPlan::Shape {
                start: 0,
                end: MAX_SHAPES_PER_BATCH,
                blend_mode: BlendMode::SrcOver,
            },
            SegmentBatchPlan::Shape {
                start: MAX_SHAPES_PER_BATCH,
                end: MAX_SHAPES_PER_BATCH + 1,
                blend_mode: BlendMode::SrcOver,
            },
        ]);

        let budget = native_segment_fusion_budget(
            &ordered_items,
            &shapes,
            &segment,
            ShapeBatchLimits::desktop(),
        )
        .expect("valid plan");

        assert_eq!(budget, None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_segment_fusion_budget_rejects_gradient_uniform_overflow() {
        let ordered_items = vec![(0, SegmentDrawItem::Shape(0))];
        let mut shape = test_shape(0, BlendMode::SrcOver);
        shape.brush = Brush::linear_gradient(vec![Color::BLACK; MAX_GRADIENT_STOPS + 1]);
        let shapes = vec![shape];
        let segment = chunk(&[SegmentBatchPlan::Shape {
            start: 0,
            end: 1,
            blend_mode: BlendMode::SrcOver,
        }]);

        let budget = native_segment_fusion_budget(
            &ordered_items,
            &shapes,
            &segment,
            ShapeBatchLimits::desktop(),
        )
        .expect("valid plan");

        assert_eq!(budget, None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_segment_fusion_partitions_shape_uniform_overflow() {
        // The uniform batch cap is derived from the device binding size and
        // the 112-byte ShapeData, not from the compile-time ceiling.
        let desktop_batch_cap = ShapeBatchLimits::desktop().max_shapes_per_batch;
        let ordered_items: Vec<_> = (0..=desktop_batch_cap)
            .map(|index| (index, SegmentDrawItem::Shape(index)))
            .collect();
        let shapes: Vec<_> = (0..=desktop_batch_cap)
            .map(|index| test_shape(index, BlendMode::SrcOver))
            .collect();
        let segment = chunk(&[
            SegmentBatchPlan::Shape {
                start: 0,
                end: desktop_batch_cap,
                blend_mode: BlendMode::SrcOver,
            },
            SegmentBatchPlan::Shape {
                start: desktop_batch_cap,
                end: desktop_batch_cap + 1,
                blend_mode: BlendMode::SrcOver,
            },
        ]);

        let partitions = native_segment_fusion_partitions(
            &ordered_items,
            &shapes,
            &segment,
            ShapeBatchLimits::desktop(),
        )
        .expect("valid plan")
        .expect("overflowing segment should be partitionable");

        assert_eq!(partitions.len(), 2);
        assert_eq!(
            partitions[0],
            NativeSegmentFusionPartition {
                chunk: chunk(&[SegmentBatchPlan::Shape {
                    start: 0,
                    end: desktop_batch_cap,
                    blend_mode: BlendMode::SrcOver,
                }]),
                budget: NativeSegmentFusionBudget {
                    shape_count: desktop_batch_cap,
                    gradient_stop_count: 0,
                },
            }
        );
        assert_eq!(
            partitions[1],
            NativeSegmentFusionPartition {
                chunk: chunk(&[SegmentBatchPlan::Shape {
                    start: desktop_batch_cap,
                    end: desktop_batch_cap + 1,
                    blend_mode: BlendMode::SrcOver,
                }]),
                budget: NativeSegmentFusionBudget {
                    shape_count: 1,
                    gradient_stop_count: 0,
                },
            }
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_segment_fusion_partitions_gradient_uniform_overflow() {
        const STOPS_PER_SHAPE: usize = MAX_GRADIENT_STOPS / 2;
        let ordered_items = vec![
            (0, SegmentDrawItem::Shape(0)),
            (1, SegmentDrawItem::Shape(1)),
            (2, SegmentDrawItem::Shape(2)),
        ];
        let mut shapes = Vec::new();
        for index in 0..3 {
            let mut shape = test_shape(index, BlendMode::SrcOver);
            shape.brush = Brush::linear_gradient(vec![Color::BLACK; STOPS_PER_SHAPE]);
            shapes.push(shape);
        }
        let segment = chunk(&[SegmentBatchPlan::Shape {
            start: 0,
            end: 3,
            blend_mode: BlendMode::SrcOver,
        }]);

        let partitions = native_segment_fusion_partitions(
            &ordered_items,
            &shapes,
            &segment,
            ShapeBatchLimits::desktop(),
        )
        .expect("valid plan")
        .expect("overflowing gradient segment should be partitionable");

        assert_eq!(partitions.len(), 2);
        assert_eq!(
            partitions[0],
            NativeSegmentFusionPartition {
                chunk: chunk(&[SegmentBatchPlan::Shape {
                    start: 0,
                    end: 2,
                    blend_mode: BlendMode::SrcOver,
                }]),
                budget: NativeSegmentFusionBudget {
                    shape_count: 2,
                    gradient_stop_count: MAX_GRADIENT_STOPS,
                },
            }
        );
        assert_eq!(
            partitions[1],
            NativeSegmentFusionPartition {
                chunk: chunk(&[SegmentBatchPlan::Shape {
                    start: 2,
                    end: 3,
                    blend_mode: BlendMode::SrcOver,
                }]),
                budget: NativeSegmentFusionBudget {
                    shape_count: 1,
                    gradient_stop_count: STOPS_PER_SHAPE,
                },
            }
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_segment_fusion_accepts_layer_composite_chunks() {
        let ordered_items = vec![
            (0, SegmentDrawItem::Shape(0)),
            (1, SegmentDrawItem::Composite(0)),
            (2, SegmentDrawItem::ShaderComposite(0)),
            (3, SegmentDrawItem::Shape(1)),
        ];
        let shapes = vec![
            test_shape(0, BlendMode::SrcOver),
            test_shape(1, BlendMode::SrcOver),
        ];
        let segment = chunk(&[
            SegmentBatchPlan::Shape {
                start: 0,
                end: 1,
                blend_mode: BlendMode::SrcOver,
            },
            SegmentBatchPlan::Composite { start: 1, end: 2 },
            SegmentBatchPlan::ShaderComposite { start: 2, end: 3 },
            SegmentBatchPlan::Shape {
                start: 3,
                end: 4,
                blend_mode: BlendMode::SrcOver,
            },
        ]);

        let partitions = native_segment_fusion_partitions(
            &ordered_items,
            &shapes,
            &segment,
            ShapeBatchLimits::desktop(),
        )
        .expect("valid plan")
        .expect("composites are drawable inside the native fused pass");

        assert_eq!(
            partitions,
            vec![NativeSegmentFusionPartition {
                chunk: segment,
                budget: NativeSegmentFusionBudget {
                    shape_count: 2,
                    gradient_stop_count: 0,
                },
            }],
            "layer composites and shader composites must preserve order without forcing separate render passes"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_segment_fusion_partitions_preserve_non_shape_order_at_budget_boundary() {
        // The uniform batch cap is derived from the device binding size and
        // the 112-byte ShapeData, not from the compile-time ceiling.
        let desktop_batch_cap = ShapeBatchLimits::desktop().max_shapes_per_batch;
        let ordered_items: Vec<_> = (0..desktop_batch_cap)
            .map(|index| (index, SegmentDrawItem::Shape(index)))
            .chain([
                (desktop_batch_cap, SegmentDrawItem::Image(0)),
                (
                    desktop_batch_cap + 1,
                    SegmentDrawItem::Shape(desktop_batch_cap),
                ),
            ])
            .collect();
        let shapes: Vec<_> = (0..=desktop_batch_cap)
            .map(|index| test_shape(index, BlendMode::SrcOver))
            .collect();
        let segment = chunk(&[
            SegmentBatchPlan::Shape {
                start: 0,
                end: desktop_batch_cap,
                blend_mode: BlendMode::SrcOver,
            },
            SegmentBatchPlan::Image {
                start: desktop_batch_cap,
                end: desktop_batch_cap + 1,
                blend_mode: BlendMode::SrcOver,
            },
            SegmentBatchPlan::Shape {
                start: desktop_batch_cap + 1,
                end: desktop_batch_cap + 2,
                blend_mode: BlendMode::SrcOver,
            },
        ]);

        let partitions = native_segment_fusion_partitions(
            &ordered_items,
            &shapes,
            &segment,
            ShapeBatchLimits::desktop(),
        )
        .expect("valid plan")
        .expect("overflowing segment should be partitionable");

        assert_eq!(partitions.len(), 2);
        assert_eq!(
            partitions[0].chunk,
            chunk(&[
                SegmentBatchPlan::Shape {
                    start: 0,
                    end: desktop_batch_cap,
                    blend_mode: BlendMode::SrcOver,
                },
                SegmentBatchPlan::Image {
                    start: desktop_batch_cap,
                    end: desktop_batch_cap + 1,
                    blend_mode: BlendMode::SrcOver,
                },
            ])
        );
        assert_eq!(
            partitions[1].chunk,
            chunk(&[SegmentBatchPlan::Shape {
                start: desktop_batch_cap + 1,
                end: desktop_batch_cap + 2,
                blend_mode: BlendMode::SrcOver,
            }])
        );
    }

    #[test]
    fn segment_command_iter_keeps_repeated_batch_kinds_in_one_chunk() {
        let ordered_items = vec![
            (0, SegmentDrawItem::Shape(0)),
            (1, SegmentDrawItem::Image(0)),
            (2, SegmentDrawItem::Shape(1)),
        ];
        let shapes = vec![
            test_shape(0, BlendMode::SrcOver),
            test_shape(2, BlendMode::DstOut),
        ];
        let images = vec![test_image(1, BlendMode::SrcOver)];

        let commands: Vec<_> = SegmentCommandIter::new(
            &ordered_items,
            &shapes,
            &images,
            ShapeBatchLimits::desktop(),
        )
        .collect();

        assert_eq!(
            commands,
            vec![SegmentRenderCommand::DrawChunk(chunk(&[
                SegmentBatchPlan::Shape {
                    start: 0,
                    end: 1,
                    blend_mode: BlendMode::SrcOver,
                },
                SegmentBatchPlan::Image {
                    start: 1,
                    end: 2,
                    blend_mode: BlendMode::SrcOver,
                },
                SegmentBatchPlan::Shape {
                    start: 2,
                    end: 3,
                    blend_mode: BlendMode::DstOut,
                },
            ]))]
        );
    }

    #[test]
    fn segment_command_iter_splits_contiguous_shape_runs_at_uniform_batch_limit() {
        // The uniform batch cap is derived from the device binding size and
        // the 112-byte ShapeData, not from the compile-time ceiling.
        let desktop_batch_cap = ShapeBatchLimits::desktop().max_shapes_per_batch;
        let ordered_items: Vec<_> = (0..=desktop_batch_cap)
            .map(|index| (index, SegmentDrawItem::Shape(index)))
            .collect();
        let shapes: Vec<_> = (0..=desktop_batch_cap)
            .map(|index| test_shape(index, BlendMode::SrcOver))
            .collect();
        let images = Vec::new();

        let commands: Vec<_> = SegmentCommandIter::new(
            &ordered_items,
            &shapes,
            &images,
            ShapeBatchLimits::desktop(),
        )
        .collect();

        assert_eq!(
            commands,
            vec![SegmentRenderCommand::DrawChunk(chunk(&[
                SegmentBatchPlan::Shape {
                    start: 0,
                    end: desktop_batch_cap,
                    blend_mode: BlendMode::SrcOver,
                },
                SegmentBatchPlan::Shape {
                    start: desktop_batch_cap,
                    end: desktop_batch_cap + 1,
                    blend_mode: BlendMode::SrcOver,
                },
            ]))]
        );
    }

    #[test]
    fn segment_command_iter_keeps_shadows_as_explicit_boundaries() {
        let ordered_items = vec![
            (0, SegmentDrawItem::Shape(0)),
            (1, SegmentDrawItem::Shadow(0)),
            (2, SegmentDrawItem::Image(0)),
            (3, SegmentDrawItem::Text(0)),
        ];
        let shapes = vec![test_shape(0, BlendMode::SrcOver)];
        let images = vec![test_image(2, BlendMode::SrcOver)];

        let commands: Vec<_> = SegmentCommandIter::new(
            &ordered_items,
            &shapes,
            &images,
            ShapeBatchLimits::desktop(),
        )
        .collect();

        assert_eq!(
            commands,
            vec![
                SegmentRenderCommand::DrawChunk(chunk(&[SegmentBatchPlan::Shape {
                    start: 0,
                    end: 1,
                    blend_mode: BlendMode::SrcOver,
                }])),
                SegmentRenderCommand::Shadow(0),
                SegmentRenderCommand::DrawChunk(chunk(&[
                    SegmentBatchPlan::Image {
                        start: 2,
                        end: 3,
                        blend_mode: BlendMode::SrcOver,
                    },
                    SegmentBatchPlan::Text { start: 3, end: 4 },
                ])),
            ]
        );
    }

    #[test]
    fn staged_buffer_uploads_align_new_copies_to_copy_buffer_alignment() {
        let mut uploads = StagedBufferUploads::default();
        uploads.bytes.extend_from_slice(&[1, 2]);

        uploads.stage(UploadTarget::ImageIndex, &[3, 4, 5, 6]);

        assert_eq!(uploads.bytes, vec![1, 2, 0, 0, 3, 4, 5, 6]);
        assert_eq!(
            uploads.copies,
            vec![PendingBufferCopy {
                source_offset: 4,
                target_offset: 0,
                size: 4,
                target: UploadTarget::ImageIndex,
            }]
        );
    }

    #[test]
    fn staged_buffer_uploads_ignore_empty_payloads() {
        let mut uploads = StagedBufferUploads::default();

        uploads.stage(UploadTarget::Uniform, &[]);

        assert!(uploads.is_empty());
        assert!(uploads.bytes.is_empty());
    }

    #[test]
    fn staged_buffer_uploads_return_exact_payload_slice_for_copy() {
        let mut uploads = StagedBufferUploads::default();
        uploads.stage(UploadTarget::Uniform, &[1, 2, 3, 4]);
        uploads.stage(UploadTarget::ImageIndex, &[5, 6, 7, 8]);

        assert_eq!(uploads.payload_for_copy(uploads.copies[0]), &[1, 2, 3, 4]);
        assert_eq!(uploads.payload_for_copy(uploads.copies[1]), &[5, 6, 7, 8]);
    }

    #[test]
    fn staged_buffer_uploads_record_destination_offsets() {
        let mut uploads = StagedBufferUploads::default();

        uploads.stage_at(UploadTarget::ImageIndex, 256, &[1, 2, 3, 4]);

        assert_eq!(uploads.copies[0].target_offset, 256);
        assert_eq!(uploads.payload_for_copy(uploads.copies[0]), &[1, 2, 3, 4]);
    }

    #[test]
    fn staged_buffer_uploads_truncate_restores_previous_state() {
        let mut uploads = StagedBufferUploads::default();
        uploads.stage(UploadTarget::Uniform, &[1, 2, 3, 4]);
        let bytes_len = uploads.bytes.len();
        let copies_len = uploads.copies.len();
        uploads.stage(UploadTarget::ImageIndex, &[5, 6, 7, 8]);

        uploads.truncate(bytes_len, copies_len);

        assert_eq!(uploads.bytes, vec![1, 2, 3, 4]);
        assert_eq!(uploads.copies.len(), 1);
    }

    #[test]
    fn inner_shadow_composite_mask_uses_fill_shape_and_scale() {
        let mut fill = test_shape(0, BlendMode::SrcOver);
        fill.local_rect = Rect {
            x: 10.0,
            y: 12.0,
            width: 40.0,
            height: 20.0,
        };
        fill.shape = Some(RoundedCornerShape::uniform(6.0));

        let cutout = test_shape(1, BlendMode::DstOut);
        let shadow = test_shadow_draw(vec![
            (fill, BlendMode::SrcOver),
            (cutout, BlendMode::DstOut),
        ]);

        let mask = inner_shadow_composite_mask(&shadow, 1.5).expect("inner mask expected");
        assert_eq!(mask.rect, [15.0, 18.0, 60.0, 30.0]);
        assert_eq!(mask.radii, [9.0, 9.0, 9.0, 9.0]);
    }

    #[test]
    fn inner_shadow_composite_mask_is_none_without_dst_out() {
        let fill = test_shape(0, BlendMode::SrcOver);
        let shadow = test_shadow_draw(vec![(fill, BlendMode::SrcOver)]);
        assert!(inner_shadow_composite_mask(&shadow, 1.0).is_none());
    }

    #[test]
    fn render_effect_support_matrix_covers_all_variants() {
        let blur = RenderEffect::blur(4.0);
        let offset = RenderEffect::offset(2.0, 3.0);
        let shader = RenderEffect::runtime_shader(cranpose_ui_graphics::RuntimeShader::new(
            r#"
            @group(0) @binding(0) var input_texture: texture_2d<f32>;
            @group(0) @binding(1) var input_sampler: sampler;
            @group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;
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
            @fragment
            fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
                return textureSample(input_texture, input_sampler, input.uv);
            }
            "#,
        ));
        let chain = blur.clone().then(offset.clone());

        assert!(is_render_effect_supported(&blur));
        assert!(is_render_effect_supported(&offset));
        assert!(is_render_effect_supported(&shader));
        assert!(is_render_effect_supported(&chain));
    }

    #[test]
    fn clip_to_bounds_propagates_visual_clip_to_all_descendant_shapes() {
        // Simulates: root → clip_to_bounds container → child with shapes above/below clip
        // All shapes inside the clip_to_bounds container must have a clip set.
        let container_local_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 500.0,
        };
        // Container is placed at y=50 in parent space via transform_to_parent
        let container_clip_in_parent = Rect {
            x: 0.0,
            y: 50.0,
            width: 800.0,
            height: 500.0,
        };

        // Shape that extends above the clip boundary (scroll content scrolled up)
        let shape_above = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 10.0,
                        y: -30.0,
                        width: 100.0,
                        height: 40.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
                clip: None,
            }),
        });

        // Shape within the clip boundary
        let shape_inside = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 10.0,
                        y: 100.0,
                        width: 100.0,
                        height: 40.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
                clip: None,
            }),
        });

        // Shape below the clip boundary (scroll content below viewport)
        let shape_below = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 10.0,
                        y: 600.0,
                        width: 100.0,
                        height: 40.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
                clip: None,
            }),
        });

        // Content child layer (represents scroll content, translated up by scroll offset)
        let mut content_layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 1000.0,
            },
            vec![shape_above, shape_inside, shape_below],
        );
        content_layer.transform_to_parent = ProjectiveTransform::translation(0.0, -30.0);
        content_layer.translated_content_context = true;

        // Clip container (e.g. TabContent with clip_to_bounds)
        let mut clip_container = test_layer(
            container_local_bounds,
            vec![RenderNode::Layer(Box::new(content_layer))],
        );
        clip_container.clip_to_bounds = true;
        clip_container.transform_to_parent = ProjectiveTransform::translation(0.0, 50.0);

        // Root
        let root = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            vec![RenderNode::Layer(Box::new(clip_container))],
        );

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(
            collected.scene.shapes.len(),
            3,
            "all three shapes should be flattened into the scene"
        );

        for (i, shape) in collected.scene.shapes.iter().enumerate() {
            assert!(
                shape.clip.is_some(),
                "shape {} at rect {:?} must have a clip from clip_to_bounds container, but clip is None",
                i,
                shape.rect
            );
            let clip = shape.clip.unwrap();
            assert_eq!(
                clip, container_clip_in_parent,
                "shape {} clip should match the clip_to_bounds container bounds in parent space",
                i
            );
        }
    }

    #[test]
    fn clip_to_bounds_culls_child_layers_outside_boundary() {
        // Reproduces the out-of-clip rendering bug: a child layer with
        // graphics_layer.clip=true (e.g. from rounded_surface()) positioned
        // entirely below the parent's clip_to_bounds boundary must be culled.
        // Before the fix, resolve_clip returned None for non-overlapping rects,
        // which downstream code interpreted as "no clipping" instead of "fully clipped",
        // causing invisible content to render everywhere.

        let clip_container_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 500.0,
        };

        let shape_in_card = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 300.0,
                        height: 80.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
                clip: None,
            }),
        });

        // Card layer with graphics_layer.clip=true, positioned BELOW the clip boundary
        let mut card_outside = crate::test_support::layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 80.0,
            },
            ProjectiveTransform::identity(),
            GraphicsLayer {
                clip: true,
                ..GraphicsLayer::default()
            },
            vec![shape_in_card.clone()],
        );
        card_outside.transform_to_parent = ProjectiveTransform::translation(10.0, 600.0);

        // Card layer with graphics_layer.clip=true, positioned INSIDE the clip boundary
        let mut card_inside = crate::test_support::layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 80.0,
            },
            ProjectiveTransform::identity(),
            GraphicsLayer {
                clip: true,
                ..GraphicsLayer::default()
            },
            vec![shape_in_card],
        );
        card_inside.transform_to_parent = ProjectiveTransform::translation(10.0, 100.0);

        // Content layer holding both cards
        let content = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 1000.0,
            },
            vec![
                RenderNode::Layer(Box::new(card_inside)),
                RenderNode::Layer(Box::new(card_outside)),
            ],
        );

        // Clip container
        let mut clip_container = test_layer(
            clip_container_bounds,
            vec![RenderNode::Layer(Box::new(content))],
        );
        clip_container.clip_to_bounds = true;

        // Root
        let root = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            vec![RenderNode::Layer(Box::new(clip_container))],
        );

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(
            collected.scene.shapes.len(),
            1,
            "only the card inside the clip boundary should produce shapes; \
             the card outside must be culled entirely"
        );

        let shape = &collected.scene.shapes[0];
        assert!(
            shape.clip.is_some(),
            "the visible card's shape must have a clip from clip_to_bounds"
        );
    }

    #[test]
    fn flattened_layer_shadow_z_index_is_below_content() {
        // Shadow must render behind content. When a child layer with shadow_elevation
        // is flattened (no isolation), its shadow z-index must be lower than any
        // content z-index so shadow draws render first.
        let shape = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
                clip: None,
            }),
        });

        let child_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        let child = crate::test_support::layer_node(
            child_bounds,
            ProjectiveTransform::translation(50.0, 50.0),
            GraphicsLayer {
                shadow_elevation: 20.0,
                ..GraphicsLayer::default()
            },
            vec![shape],
        );

        let root = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert!(
            !collected.scene.shadow_draws.is_empty(),
            "shadow_elevation > 0 must produce shadow draws"
        );
        let max_shadow_z = collected
            .scene
            .shadow_draws
            .iter()
            .map(|s| s.z_index)
            .max()
            .unwrap();
        let min_content_z = collected
            .scene
            .shapes
            .iter()
            .map(|s| s.z_index)
            .min()
            .unwrap();
        assert!(
            max_shadow_z < min_content_z,
            "shadow z-index ({}) must be less than content z-index ({}); \
             shadows must render behind their content",
            max_shadow_z,
            min_content_z
        );
    }

    /// One retained bundle op key with the fields the invalidation tests
    /// vary; the rest stay representative constants.
    #[cfg(not(target_arch = "wasm32"))]
    fn bundle_op(slot: u32, epoch: Option<u64>, first: u32, last: u32) -> RetainedBundleOpKey {
        RetainedBundleOpKey {
            slot,
            capture_epoch: epoch,
            first,
            last,
            retained_index: slot,
            has_mesh: false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn bundle_key(ops: &[RetainedBundleOpKey]) -> RetainedBundleKey {
        RetainedBundleKey { ops: ops.to_vec() }
    }

    /// The same stretch on consecutive frames reuses its bundle: one
    /// rebuild, then cached executes.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn retained_bundle_cache_reuses_stable_keys() {
        let mut cache: RetainedBundleCacheImpl<u32> = RetainedBundleCacheImpl::new();
        let ops = [bundle_op(3, Some(7), 0, 40), bundle_op(5, Some(9), 4, 12)];
        let key = bundle_key(&ops);

        assert!(!cache.hit(&key), "empty cache must miss");
        cache.insert(key.clone(), 111);
        assert_eq!(cache.get(&key), Some(&111));
        cache.end_frame();

        for _ in 0..3 {
            assert!(cache.hit(&bundle_key(&ops)), "stable key must stay cached");
            cache.end_frame();
        }
        assert_eq!(cache.stats(), (1, 3), "one rebuild, three cached executes");
    }

    /// Recapture (epoch bump), span reorder, count change, range change and
    /// slot release each change the key, so a stale bundle can never satisfy
    /// the lookup.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn retained_bundle_cache_invalidates_on_any_op_change() {
        let ops = [bundle_op(3, Some(7), 0, 40), bundle_op(5, Some(9), 4, 12)];
        let variants: [Vec<RetainedBundleOpKey>; 5] = [
            // Recaptured slot 3: same id, bumped epoch.
            vec![bundle_op(3, Some(8), 0, 40), bundle_op(5, Some(9), 4, 12)],
            // Reordered stretch.
            vec![bundle_op(5, Some(9), 4, 12), bundle_op(3, Some(7), 0, 40)],
            // Op count changed.
            vec![bundle_op(3, Some(7), 0, 40)],
            // Draw range changed.
            vec![bundle_op(3, Some(7), 0, 41), bundle_op(5, Some(9), 4, 12)],
            // Slot 5 released: epoch gone.
            vec![bundle_op(3, Some(7), 0, 40), bundle_op(5, None, 4, 12)],
        ];
        for changed in variants {
            let mut cache: RetainedBundleCacheImpl<u32> = RetainedBundleCacheImpl::new();
            cache.insert(bundle_key(&ops), 111);
            cache.end_frame();
            assert!(
                !cache.hit(&RetainedBundleKey {
                    ops: changed.clone()
                }),
                "changed key {changed:?} must not reuse the stale bundle"
            );
        }
    }

    /// Entries a frame does not use are evicted at its end — bundles pin
    /// slot buffers, so unused ones must not accumulate — and `clear` (the
    /// slot-release path) empties the cache outright.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn retained_bundle_cache_evicts_unused_entries() {
        let mut cache: RetainedBundleCacheImpl<u32> = RetainedBundleCacheImpl::new();
        let stale = bundle_key(&[bundle_op(1, Some(1), 0, 6)]);
        let live = bundle_key(&[bundle_op(2, Some(2), 0, 6)]);
        cache.insert(stale.clone(), 1);
        cache.insert(live.clone(), 2);
        cache.end_frame();

        assert!(cache.hit(&live));
        cache.end_frame();

        assert!(
            !cache.hit(&stale),
            "entry unused for a frame must have been evicted"
        );
        assert!(cache.hit(&live), "used entry must survive eviction");

        cache.clear();
        assert!(!cache.hit(&live), "clear must drop every entry");
    }
}
