use std::{
    cell::Cell,
    hash::{Hash, Hasher},
    rc::Rc,
    sync::{Arc, mpsc},
    time::Duration,
};

use bytemuck::{Pod, Zeroable};
use cranpose_core::collections::{bounded_lru::BoundedLruCache, map::HashMap};
use cranpose_render_common::{
    geometry::blur_reach, graph::quad_bounds, software_text_raster::SoftwareTextFontSet,
};
use cranpose_ui_graphics::{
    BlendMode, FRAGMENT_KIND_ARC, ImageSampling, Point, RecordLane, Rect, RenderHash, TileMode,
};
use smallvec::SmallVec;
use web_time::Instant;

use crate::{
    DebugCpuAllocationStats,
    ablation::Ablation,
    arc_trig_fill::ArcTrigFill,
    collect::LayerScene,
    draw_pass::{
        PassSegment, PassTarget, ResolvedComposite, ResolvedCompositeKind, RunCalls, SegmentLayout,
        SourceContent, TrackedPass,
    },
    effect_renderer::{BlurDest, BlurSpec, CompositeSampleMode, EffectRenderer},
    fixed_pipeline::{FixedPipeline, FixedPipelineJob},
    frame::{
        ClearedImages, Damage, DeviceRect, FrameExecutor, FrameRecording, FrameWindow,
        ImageContent, KeptWindow, PresentedImage, Repaint, StageSideScratch, Summaries,
        SwapchainHistory, Window, WindowDamage,
    },
    frame_graph::{
        BufferUpload, FrameCommandRecorder, FrameCommandStats, FrameTextureDescriptor,
        FrameUploadAllocators, TextureRegionCopy, UniformUpload, UploadAllocatorId,
        UploadAllocatorSpec, UploadMode, WgpuFrameGraph, WgpuFrameGraphExecutor,
    },
    frame_packet::{CancelReason, FramePacket, FrameSceneStorage, PresentOutcome, RenderReturns},
    geometry::{
        DevicePixelBounds, SegmentTransform, anchored_device_rect, canonicalize_device_coordinate,
        snap_delta_for_anchor, snapped_anchor_device_origin, translate_quad,
        translation_stable_anchored_device_pixel_bounds,
    },
    glyph_run_arena::GlyphRunArena,
    gpu_stats::{self, gpu_stats_enabled},
    layer_cache::LayerCache,
    lazy_resource::Pending,
    offscreen::{COMPOSITION_BYTES_PER_PIXEL, OffscreenTarget},
    output_conversion::OutputConverter,
    pipeline_compiler::{CompileLane, CompilerSend, PipelineCompilation, PipelineCompiler},
    rrect_shadow::{SHADOW_QUAD_CORNERS, ShadowInstance},
    run_store::{ArenaBinding, PlacementData, RunBufferMode, RunDrawCall, RunStore, StoredTables},
    scene::{ImageDraw, RunDraw, ShadowDraw, SnapAnchor, TextDraw},
    shaders,
    shape_pipelines::{PipelineDemand, ShapePipelineFactory, ShapePipelines},
    shared_shader::SharedShader,
};
const MAX_TRANSPARENT_SOURCES: usize = 16;

pub(crate) const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: cranpose_render_common::FRAME_CLEAR_COLOR[0] as f64,
    g: cranpose_render_common::FRAME_CLEAR_COLOR[1] as f64,
    b: cranpose_render_common::FRAME_CLEAR_COLOR[2] as f64,
    a: cranpose_render_common::FRAME_CLEAR_COLOR[3] as f64,
};
const MAX_TEXTURE_CACHE_ITEMS: usize = 256;

const DEFAULT_WGPU_RENDER_STAGE_TELEMETRY_THRESHOLD_MS: f64 = 4.0;

fn wgpu_render_stage_telemetry_threshold_ms() -> Option<f64> {
    let diagnostics = &cranpose_core::DIAGNOSTICS;
    diagnostics
        .wgpu_render_stage_telemetry_ms
        .threshold_ms()
        .or_else(|| {
            diagnostics
                .wgpu_render_stage_telemetry
                .is_set()
                .then_some(DEFAULT_WGPU_RENDER_STAGE_TELEMETRY_THRESHOLD_MS)
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

type DeviceRect4 = (f32, f32, f32, f32);
/// A rect of whole target pixels: x, y, width, height.
pub(crate) type TargetRect = (u32, u32, u32, u32);

/// The target pixels a clipped shape variant keeps of `placement`'s records
/// under `viewport`, as a scissor that stands in for its clip test: every
/// pixel whose centre, moved by the viewport's offset, lies inside the
/// device clip, inclusive of its edges, as the shader compares them. `None`
/// without a clip or under a turn, where the variant tests the clip itself.
/// An empty clip keeps an empty scissor.
pub(crate) fn clip_scissor(
    placement: &crate::scene::Placement,
    root_scale: f32,
    viewport: ViewportUniformParams,
) -> Option<TargetRect> {
    if !viewport.transform.is_identity() {
        return None;
    }
    let [x, y, width, height] = crate::run_store::device_clip(placement, root_scale)?;
    let columns = kept_pixels(x, x + width, viewport.offset[0], viewport.width);
    let rows = kept_pixels(y, y + height, viewport.offset[1], viewport.height);
    Some((
        columns.start,
        rows.start,
        columns.end - columns.start,
        rows.end - rows.start,
    ))
}

/// The pixels along one axis whose centre, moved by `offset`, lies within
/// `low..=high`, within `0..extent`: stepped with the shader's own float
/// sums so the edges fall where its comparisons put them.
fn kept_pixels(low: f32, high: f32, offset: f32, extent: u32) -> std::ops::Range<u32> {
    let centre = |pixel: u32| pixel as f32 + 0.5 + offset;
    let guess = |edge: f32| (edge - offset - 0.5).ceil().clamp(0.0, extent as f32) as u32;
    let mut start = guess(low);
    while start > 0 && centre(start - 1) >= low {
        start -= 1;
    }
    while start < extent && centre(start) < low {
        start += 1;
    }
    let mut end = guess(high).max(start);
    while end > start && centre(end - 1) > high {
        end -= 1;
    }
    while end < extent && centre(end) <= high {
        end += 1;
    }
    start..end
}

/// The pixels both scissors keep, `None` when they share none.
pub(crate) fn intersect_target_rects(a: TargetRect, b: TargetRect) -> Option<TargetRect> {
    let left = a.0.max(b.0);
    let top = a.1.max(b.1);
    let right = (a.0 + a.2).min(b.0 + b.2);
    let bottom = (a.1 + a.3).min(b.1 + b.3);
    (right > left && bottom > top).then(|| (left, top, right - left, bottom - top))
}

/// Whether `inner` lies within `outer`.
pub(crate) fn target_rect_within(inner: TargetRect, outer: TargetRect) -> bool {
    inner.0 >= outer.0
        && inner.1 >= outer.1
        && inner.0 + inner.2 <= outer.0 + outer.2
        && inner.1 + inner.3 <= outer.1 + outer.3
}

/// The scissor that keeps a draw under `scissor`, or under none, within
/// `slot`: empty when they share no pixel.
pub(crate) fn confined_scissor(scissor: Option<TargetRect>, slot: TargetRect) -> TargetRect {
    scissor
        .map_or(Some(slot), |scissor| intersect_target_rects(scissor, slot))
        .unwrap_or((slot.0, slot.1, 0, 0))
}

/// A draw's scissor cut down to the pixels its pass segment may touch;
/// `None` when nothing of it remains.
pub(crate) fn bounded_scissor(
    scissor: TargetRect,
    bound: Option<TargetRect>,
) -> Option<TargetRect> {
    match bound {
        Some(bound) => intersect_target_rects(scissor, bound),
        None => (scissor.2 > 0 && scissor.3 > 0).then_some(scissor),
    }
}

fn intersect_device_rects(a: DeviceRect4, b: DeviceRect4) -> Option<DeviceRect4> {
    let left = a.0.max(b.0);
    let top = a.1.max(b.1);
    let right = (a.0 + a.2).min(b.0 + b.2);
    let bottom = (a.1 + a.3).min(b.1 + b.3);
    (right > left && bottom > top).then_some((left, top, right - left, bottom - top))
}

pub(crate) fn anchored_rect_to_device(
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

pub(crate) fn blend_state_for_mode(mode: BlendMode) -> wgpu::BlendState {
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

/// Hashes what a text draws apart from where and how large its rect is.
fn hash_text_draw_for_cache<H: Hasher>(
    text_draw: &TextDraw,
    raster_rect: Rect,
    text_scale: f32,
    state: &mut H,
) {
    text_draw.text.render_hash().hash(state);
    text_draw.style_hash.hash(state);
    text_draw.color.render_hash().hash(state);
    hash_text_gradient_phase_for_cache(text_draw, raster_rect, state);
    text_draw.font_size.to_bits().hash(state);
    text_scale.to_bits().hash(state);
    text_draw.layout_options.hash(state);
}

pub(crate) fn hash_text_gradient_phase_for_cache<H: Hasher>(
    text_draw: &TextDraw,
    raster_rect: Rect,
    state: &mut H,
) {
    if text_draw.text.gradient() {
        for coordinate in [raster_rect.x, raster_rect.y] {
            let phase = if coordinate >= 0.0 {
                coordinate.rem_euclid(4.0)
            } else {
                coordinate
            };
            phase.to_bits().hash(state);
        }
    }
}

/// Where a text draws in the scene, snapped, and the scale its glyphs
/// raster at; `None` for a text that draws nothing.
pub(crate) fn text_logical_geometry_for_draw(
    text_draw: &TextDraw,
    root_scale: f32,
) -> Option<(Rect, f32)> {
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

    let snap_delta = text_draw
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
        .unwrap_or_default();
    let logical_rect = text_draw.rect.translate(snap_delta.x, snap_delta.y);
    Some((logical_rect, text_scale))
}

pub(crate) fn text_raster_geometry_for_draw(
    text_draw: &TextDraw,
    root_scale: f32,
) -> Option<(Rect, Rect, Option<Rect>, f32, bool)> {
    text_logical_geometry_for_draw(text_draw, root_scale)
        .map(|logical| text_raster_geometry_at(text_draw, logical, root_scale))
}

/// The raster geometry of a text whose logical geometry is
/// `(logical_rect, text_scale)`: see [`text_raster_geometry_for_draw`].
fn text_raster_geometry_at(
    text_draw: &TextDraw,
    (logical_rect, text_scale): (Rect, f32),
    root_scale: f32,
) -> (Rect, Rect, Option<Rect>, f32, bool) {
    let static_text_motion = text_draw.text.static_motion();
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
    (
        logical_rect,
        raster_rect,
        clip,
        text_scale,
        static_text_motion,
    )
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
    rect_is_visible_in_rect(rect, clip, viewport.scene_rect(root_scale))
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
    text_logical_geometry_for_draw(text, root_scale)
        .and_then(|(logical_rect, _)| clipped_bounds(logical_rect, text.clip))
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

/// The logical geometry of `text` (see [`text_logical_geometry_for_draw`])
/// when it draws any pixel inside `viewport_rect`.
pub(crate) fn text_draw_geometry_in_rect(
    text: &TextDraw,
    viewport_rect: Rect,
    root_scale: f32,
) -> Option<(Rect, f32)> {
    text_logical_geometry_for_draw(text, root_scale).filter(|(logical_rect, _)| {
        clipped_bounds(*logical_rect, text.clip)
            .is_some_and(|bounds| bounds.intersect(viewport_rect).is_some())
    })
}

pub(crate) fn hash_device_offset<H: Hasher>(
    value: f32,
    origin: f32,
    root_scale: f32,
    state: &mut H,
) {
    hash_f32_for_cache((value - origin) * root_scale, state);
}

pub(crate) fn hash_device_rect<H: Hasher>(
    rect: Rect,
    origin_x: f32,
    origin_y: f32,
    root_scale: f32,
    state: &mut H,
) {
    hash_device_offset(rect.x, origin_x, root_scale, state);
    hash_device_offset(rect.y, origin_y, root_scale, state);
    hash_device_offset(rect.width, 0.0, root_scale, state);
    hash_device_offset(rect.height, 0.0, root_scale, state);
}

/// Hashes the rect `placement` is clipped to, `clip`, and its rounding, in
/// device pixels from `origin`.
fn hash_placement_clips<H: Hasher>(
    placement: &crate::scene::Placement,
    clip: Option<Rect>,
    (origin_x, origin_y): (f32, f32),
    root_scale: f32,
    state: &mut H,
) {
    let rounding = placement.rounding;
    for (rect, radius) in [
        (clip, 0.0),
        (
            rounding.map(|rounding| rounding.rect),
            rounding.map_or(0.0, |rounding| rounding.radius),
        ),
    ] {
        rect.is_some().hash(state);
        if let Some(rect) = rect {
            let [x, y, width, height] =
                crate::run_store::placement_device_rect(placement, rect, root_scale);
            for value in [
                x - origin_x,
                y - origin_y,
                width,
                height,
                radius * root_scale,
            ] {
                hash_f32_for_cache(value, state);
            }
        }
    }
}

pub(crate) fn hash_run_item_with_clip<H: Hasher>(
    run: &RunDraw,
    clip: Option<Rect>,
    origin_x: f32,
    origin_y: f32,
    root_scale: f32,
    state: &mut H,
) {
    let tables = run.tables();
    tables.fingerprint().hash(state);
    run.segments.start.hash(state);
    run.segments.end.hash(state);
    let placement = &run.placement;
    let snap = placement
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
        .unwrap_or_default();
    let offset = Point::new(placement.offset.x + snap.x, placement.offset.y + snap.y);
    let canonicalize = placement.snap_anchor.is_some();
    canonicalize.hash(state);
    for segment in &tables.segments[run.segments.start as usize..run.segments.end as usize] {
        if segment.lane != RecordLane::Shapes {
            continue;
        }
        let exact_placement =
            !canonicalize || segment.kinds & ((1u8 << FRAGMENT_KIND_ARC) | LINE_KINDS) != 0;
        exact_placement.hash(state);
        if exact_placement {
            for value in [offset.x, offset.y, origin_x, origin_y, root_scale] {
                hash_f32_for_cache(value, state);
            }
            continue;
        }
        for index in segment.start..segment.start + segment.count {
            let record = tables
                .shapes
                .get(index as usize)
                .expect("shape segment record");
            let half_width = if record.is_stroked() {
                record.stroke_width * 0.5
            } else {
                0.0
            };
            let [left, top, right, bottom] = crate::run_geometry::device_shape_edges(
                record.rect,
                half_width,
                offset,
                root_scale,
                canonicalize,
            );
            for value in [left - origin_x, top - origin_y, right - left, bottom - top] {
                hash_f32_for_cache(value, state);
            }
        }
    }
    hash_placement_clips(placement, clip, (origin_x, origin_y), root_scale, state);
    if tables.segments[run.segments.start as usize..run.segments.end as usize]
        .iter()
        .any(|segment| segment.gradient || segment.vertex_gradient)
    {
        let dither = placement
            .snap_anchor
            .map(|anchor| snapped_anchor_device_origin(anchor, root_scale))
            .unwrap_or_default();
        hash_f32_for_cache(dither.x - origin_x, state);
        hash_f32_for_cache(dither.y - origin_y, state);
    }
    hash_f32_for_cache(placement.alpha, state);
    match &placement.color_filter {
        Some(filter) => {
            1u8.hash(state);
            filter.render_hash().hash(state);
        }
        None => 0u8.hash(state),
    }
}

/// The vertex entry point of every fullscreen strip.
pub(crate) const FULLSCREEN_VS: &str = "fullscreen_vs";

pub(crate) fn create_render_pipeline_logged<'a>(
    device: &wgpu::Device,
    cache: Option<&'a wgpu::PipelineCache>,
    tag: &str,
    mut descriptor: wgpu::RenderPipelineDescriptor<'a>,
) -> wgpu::RenderPipeline {
    descriptor.cache = cache;
    build_pipeline_logged(tag, || device.create_render_pipeline(&descriptor))
}

pub(crate) fn build_pipeline_logged<T>(tag: &str, build: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let pipeline = build();
    log::info!(
        "[pipeline-create] {tag} {:.1}ms on {}",
        instant_ms(started, Instant::now()),
        std::thread::current().name().unwrap_or("unnamed thread"),
    );
    crate::thread_state::with(|state| {
        if state.off_frame_builds.get() {
            PIPELINES_CREATED_OFF_FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else {
            PIPELINES_CREATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            state.thread_builds.set(state.thread_builds.get() + 1);
        }
    });
    #[cfg(not(target_arch = "wasm32"))]
    crate::pipeline_disk_cache::note_change();
    pipeline
}

/// Pipelines this process has built on a thread that draws.
///
/// A build runs the backend's shader compiler, and whoever asks for one while
/// drawing waits for it there. A count that grows across an interaction names
/// work a person waited on, whatever the driver's own caches made a compile
/// cost on this machine. Builds handed to [`crate::pipeline_compiler`] are
/// counted apart, by [`pipelines_created_off_frame`]: they cost a frame
/// nothing.
static PIPELINES_CREATED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PIPELINES_CREATED_OFF_FRAME: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// The pipelines this thread has built while drawing: a frame that drew
/// with this unchanged built nothing itself.
#[cfg(not(target_arch = "wasm32"))]
fn thread_builds() -> u64 {
    crate::thread_state::with(|state| state.thread_builds.get())
}

/// Declares that pipelines built on this thread are built away from any
/// frame. The compiler thread says so once, when it starts.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn mark_thread_off_frame() {
    crate::thread_state::with(|state| state.off_frame_builds.set(true));
}

/// The pipelines this process has built on a thread that draws, since it started. A count
/// that grows during an interaction names a shader compile a frame waited for.
pub fn pipelines_created() -> u64 {
    PIPELINES_CREATED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Pipelines built away from every frame, on the compiler thread.
pub fn pipelines_created_off_frame() -> u64 {
    PIPELINES_CREATED_OFF_FRAME.load(std::sync::atomic::Ordering::Relaxed)
}

/// What every draw of one pass shares: its target's size and whether the
/// pass has a depth buffer its opaque interiors fill first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PassFrame {
    pub(crate) size: (u32, u32),
    pub(crate) depth: PassDepth,
}

impl PassFrame {
    /// The scissor a draw bounded by `scissor`, or by nothing, sets.
    pub(crate) fn scissor(self, scissor: Option<(u32, u32, u32, u32)>) -> (u32, u32, u32, u32) {
        scissor.unwrap_or((0, 0, self.size.0, self.size.1))
    }
}

/// Which of a pass's two stages a shape draw records: the front-to-back
/// opaque interiors, or the paint in draw order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RunStage {
    Interiors,
    Paint,
}

/// The depth buffer of a pass that lays opaque interiors down first.
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Depth steps one draw apart in a pass's order, as the shaders'
/// `DEPTH_STEP`.
const PASS_DEPTH_STEP: f32 = 1.0 / 1_048_576.0;

/// The depth of the draw at `index` in its pass's order, as the shaders'
/// `record_depth` and `batch_depth` place it: later draws nearer.
pub(crate) fn pass_order_depth(index: u32) -> f32 {
    (1.0 - (index as f32 + 1.0) * PASS_DEPTH_STEP).max(0.0)
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
        3 => Float32x4,
    ];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as wgpu::BufferAddress,
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
    transform: [f32; 4],
    translation: [f32; 2],
    reserved: [f32; 2],
    inverse: [f32; 4],
    origin: [f32; 2],
    origin_reserved: [f32; 2],
    placement: PlacementData,
}

impl Uniforms {
    fn of(params: ViewportUniformParams, placement: PlacementData, tier: RunTier) -> Self {
        let (transform, translation, inverse) = params.transform.uniform_parts();
        Self {
            viewport: [params.width as f32, params.height as f32],
            viewport_offset: params.offset,
            transform,
            translation,
            reserved: [params.depth_base, f32::from(tier as u8)],
            inverse,
            origin: params.origin,
            origin_reserved: [0.0; 2],
            placement,
        }
    }
}

fn survive_gpu_errors_enabled() -> bool {
    !cranpose_core::DIAGNOSTICS.survive_gpu_errors.equals("0")
}

#[derive(Default)]
pub(crate) struct ViewportUniforms {
    uploads: FrameUploadAllocators,
    slots: Vec<UniformUpload>,
}

impl ViewportUniforms {
    fn begin_frame(&mut self) {
        self.slots.clear();
        self.uploads.reset();
    }

    fn claim(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniforms: &Uniforms,
    ) -> usize {
        let slot = self.slots.len();
        self.slots.push(self.uploads.upload_uniform(
            UploadAllocatorId::Viewport,
            UploadAllocatorSpec::uniform(
                "Viewport Uniform Buffer",
                "Viewport Uniform Bind Group",
                size_of::<Uniforms>() as u64,
            ),
            device,
            layout,
            bytemuck::bytes_of(uniforms),
        ));
        slot
    }

    // Claims the frame's next slot for a viewport of `params`.
    pub(crate) fn claim_viewport(
        &mut self,
        pipelines: &RendererPipelines,
        params: ViewportUniformParams,
    ) -> usize {
        let uniforms = Uniforms::of(params, PlacementData::zeroed(), RunTier::Arena);
        self.claim(
            &pipelines.device,
            &pipelines.uniform_bind_group_layout,
            &uniforms,
        )
    }

    fn bind(&self, pass: &mut TrackedPass<'_, '_>, slot: usize) -> Result<(), String> {
        let uniform = self
            .slots
            .get(slot)
            .ok_or_else(|| "viewport uniform slot was never claimed this frame".to_string())?;
        pass.set_bind_group(0, &uniform.bind_group, &[uniform.offset]);
        Ok(())
    }

    fn stage_pending(
        &mut self,
        device: &wgpu::Device,
        recorder: &mut impl FrameCommandRecorder,
    ) -> FrameCommandStats {
        self.uploads.stage_pending(|buffer, offset, bytes| {
            recorder.stage_frame_buffer_copy(device, buffer, offset, bytes)
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewportUniformParams {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) offset: [f32; 2],
    pub(crate) transform: SegmentTransform,
    /// Where the drawn vertices' origin sits in the segment's device space,
    /// added before the transform: zero, except for a retained glyph run
    /// drawn under a transform, whose vertices sit at its raster origin. The
    /// sum is the one the shared path writes, so both draw the same pixels.
    pub(crate) origin: [f32; 2],
    /// The pass-order index of the batch's first shape record, from which
    /// each record's depth counts.
    pub(crate) depth_base: f32,
}

impl ViewportUniformParams {
    // The fallback meshes' target in this viewport, counting depth from its
    // base in a pass with the depth buffer.
    fn mesh_target(self, depth: PassDepth) -> crate::mesh_fallback::MeshTarget {
        crate::mesh_fallback::MeshTarget {
            offset: self.offset,
            size: [self.width as f32, self.height as f32],
            depth_base: depth.is_on().then_some(self.depth_base),
        }
    }

    /// The logical rect of the drawn scene that the target shows.
    pub(crate) fn scene_rect(self, root_scale: f32) -> Rect {
        segment_scene_rect(
            self.transform,
            Rect {
                x: self.offset[0],
                y: self.offset[1],
                width: self.width as f32,
                height: self.height as f32,
            },
            root_scale,
        )
    }
}

/// The logical rect of a scene drawn under `transform` that the device rect
/// `device` of the target's scene space shows: the rect itself when the
/// scene is not transformed, else the bounds it maps back to.
pub(crate) fn segment_scene_rect(
    transform: SegmentTransform,
    device: Rect,
    root_scale: f32,
) -> Rect {
    let device = if transform.is_identity() {
        device
    } else {
        transform.segment_bounds(device)
    };
    Rect {
        x: device.x / root_scale,
        y: device.y / root_scale,
        width: device.width / root_scale,
        height: device.height / root_scale,
    }
}

/// A stored run's draws for one pass: the tables current when it was
/// prepared, the uniform slot holding its placement, and the range of the
/// pass's run draws with the pipeline and vertex range of each segment's
/// quads and bands.
pub(crate) struct StoreRunBatch {
    pub(crate) tables: StoredTables,
    pub(crate) uniform_slot: usize,
    pub(crate) draws: std::ops::Range<usize>,
    /// The scissor an unturned run's clip puts on its paint: see
    /// [`clip_scissor`].
    pub(crate) clip: Option<TargetRect>,
}

struct CompositionTarget {
    target: Rc<OffscreenTarget>,
    output_bind_group: wgpu::BindGroup,
    /// The frame the target's pixels show.
    kept: KeptWindow,
}

/// Where a frame renders: straight into the presentable image, or into the
/// reusable composition target that then goes into the image.
enum FrameRoot {
    /// Straight into the presentable image `image`; copied into the
    /// composition target `keep` too when that target is to keep the frame
    /// for images that come cleared.
    Surface {
        image: Rc<OffscreenTarget>,
        keep: Option<CompositionTarget>,
    },
    /// Into the composition target, which is copied whole into an image
    /// that comes `cleared`, and otherwise drawn into the image by the
    /// output conversion.
    Composition {
        composition: CompositionTarget,
        cleared: Option<Rc<OffscreenTarget>>,
    },
}

/// What goes into the presentable image or out of it once the frame is
/// drawn.
#[derive(Clone, Copy)]
enum OutputStep<'a> {
    /// Nothing: the frame rendered into the presentable image.
    None,
    /// The output conversion of `mode` into the view, from the bind group.
    Convert {
        view: &'a wgpu::TextureView,
        bind_group: &'a wgpu::BindGroup,
        mode: OutputMode,
    },
    /// A whole copy, between the composition target and an image that
    /// comes cleared.
    Copy {
        from: &'a OffscreenTarget,
        to: &'a OffscreenTarget,
    },
}

impl FrameRoot {
    fn target(&self) -> &Rc<OffscreenTarget> {
        match self {
            Self::Surface { image, .. } => image,
            Self::Composition { composition, .. } => &composition.target,
        }
    }

    /// What the frame ends with between its root and the presentable image.
    fn output<'a>(
        &'a self,
        output_view: Option<&'a wgpu::TextureView>,
        screenshot_bind_group: Option<&'a wgpu::BindGroup>,
    ) -> OutputStep<'a> {
        match self {
            Self::Surface { keep: None, .. } => OutputStep::None,
            Self::Surface {
                image,
                keep: Some(keep),
            } => OutputStep::Copy {
                from: image,
                to: &keep.target,
            },
            Self::Composition {
                composition,
                cleared: Some(image),
            } => OutputStep::Copy {
                from: &composition.target,
                to: image,
            },
            Self::Composition {
                composition,
                cleared: None,
            } => output_view.map_or(OutputStep::None, |view| match screenshot_bind_group {
                Some(bind_group) => OutputStep::Convert {
                    view,
                    bind_group,
                    mode: OutputMode::Screenshot,
                },
                None => OutputStep::Convert {
                    view,
                    bind_group: &composition.output_bind_group,
                    mode: OutputMode::Display,
                },
            }),
        }
    }

    /// The composition target the frame drew into or copied into, which
    /// the renderer keeps for the next frame.
    fn into_composition(self) -> Option<CompositionTarget> {
        match self {
            Self::Surface { keep, .. } => keep,
            Self::Composition { composition, .. } => Some(composition),
        }
    }
}

/// What a presentable image must allow to be the frame's root target:
/// rendering, and the copies a capture and the opaque-prefix cache take of
/// the page and put back.
const DIRECT_SURFACE_ROOT_USAGES: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT
    .union(wgpu::TextureUsages::COPY_SRC)
    .union(wgpu::TextureUsages::COPY_DST);

/// The usages to ask of a presentable image: rendering, and whichever of
/// the copies and sampling the surface offers. Sampling lets a frame whose
/// backdrops read the page draw straight into the image too.
pub fn presentable_root_usages(supported: wgpu::TextureUsages) -> wgpu::TextureUsages {
    wgpu::TextureUsages::RENDER_ATTACHMENT
        | (supported & (DIRECT_SURFACE_ROOT_USAGES | wgpu::TextureUsages::TEXTURE_BINDING))
}

/// Whether the presented image can be the frame's root target: its bytes
/// are the composition format (so the 8-bit output conversion would be an
/// identity), it is the viewport's size, it can be copied as the page is,
/// and it can be sampled when the frame `reads_page` through a backdrop.
/// Metal's images cannot be sampled, so only a frame without backdrops
/// draws straight into one there.
fn surface_is_direct_root(
    texture: &wgpu::Texture,
    composition_format: wgpu::TextureFormat,
    viewport: (u32, u32),
    reads_page: bool,
) -> bool {
    let usage = texture.usage();
    texture.format().remove_srgb_suffix() == composition_format
        && usage.contains(DIRECT_SURFACE_ROOT_USAGES)
        && (!reads_page || usage.contains(wgpu::TextureUsages::TEXTURE_BINDING))
        && (texture.width(), texture.height()) == viewport
}

/// What a frame draws into its page: the root and overlay scenes at the
/// root's scale over the clear colour, all of the page or a part of it.
#[derive(Clone, Copy)]
struct FrameDraw<'a> {
    root: &'a LayerScene,
    overlay: Option<&'a LayerScene>,
    root_scale: f32,
    clear: wgpu::Color,
    repaint: Repaint,
}

fn frame_damage(damage: Option<Damage>) -> gpu_stats::FrameDamage {
    match damage {
        None => gpu_stats::FrameDamage::Untracked,
        Some(Damage::Skipped) => gpu_stats::FrameDamage::Skipped,
        Some(Damage::Full) => gpu_stats::FrameDamage::Full,
        Some(Damage::Part(rects)) => {
            let mut window = gpu_stats::WindowRects::default();
            for rect in rects.as_slice() {
                window.push([rect.x, rect.y, rect.width, rect.height].map(|value| value as u32));
            }
            gpu_stats::FrameDamage::Part(window)
        }
    }
}

#[derive(Clone, Copy)]
enum OutputMode {
    Display,
    Screenshot,
}

/// Where a frame goes: displayed in a presentable image, or read back as a
/// screenshot.
#[derive(Clone, Copy)]
pub(crate) struct FrameOutput<'a> {
    mode: OutputMode,
    view: Option<&'a wgpu::TextureView>,
    texture: Option<&'a wgpu::Texture>,
    /// What `texture` holds as the frame draws into it.
    image: ImageContent,
}

impl<'a> FrameOutput<'a> {
    // A frame displayed in `texture` through `view`, which holds `image`.
    pub(crate) fn display(
        texture: &'a wgpu::Texture,
        view: &'a wgpu::TextureView,
        image: ImageContent,
    ) -> Self {
        Self {
            mode: OutputMode::Display,
            view: Some(view),
            texture: Some(texture),
            image,
        }
    }
}

/// The kept frame a frame's damage is measured against.
#[derive(Clone, Copy)]
enum Keeper {
    /// The renderer's composition target.
    Composition,
    /// A swapchain image.
    Image(PresentedImage),
    /// The frame drawn last into an image that comes cleared; `keeping`
    /// when the composition target takes a copy of this frame to keep.
    Cleared { keeping: bool },
}

/// What a frame changes against the frame its target kept, what it draws
/// for that, and which target kept it.
#[derive(Clone, Copy)]
struct FrameDamagePlan {
    damage: Damage,
    repaint: Repaint,
    keeper: Keeper,
}

pub(crate) struct GpuRenderer {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    device_errors: Arc<DeviceErrorSentry>,
    renderer_epoch: u64,
    #[cfg(not(target_arch = "wasm32"))]
    display_format: wgpu::TextureFormat,
    composition_target: Option<CompositionTarget>,
    output_converter: OutputConverter,
    screenshot_converter: OutputConverter,
    pub(crate) pipelines: RendererPipelines,
    pub(crate) depth_targets: DepthTargets,
    /// The pipeline and mesh buffers of the 3D scenes; nothing in it is made
    /// until a scene draws.
    pub(crate) scene_pass: crate::scene_pass::ScenePass,
    pub(crate) depth_gate: crate::depth_gate::DepthGate,
    /// The size of the atlas the layer surfaces drawn again go into.
    pub(crate) surface_atlas_sizes: crate::frame::SurfaceAtlasSizes,
    text: TextResources,
    pub(crate) viewport_uniforms: ViewportUniforms,
    run_store: RunStore,
    images: ImageTextures,
    /// How run tables and retained glyph runs reach the GPU, and whether
    /// kept layer surfaces stay in their atlas: see [`UploadMode::copy_free`].
    pub(crate) copy_free: UploadMode,
    /// Text runs the frame has drawn so far, cached or not.
    pub(crate) pass_scratch: crate::draw_pass::PassScratch,
    pub(crate) stage_side_scratch: StageSideScratch,
    deferred_offscreen_releases: Vec<OffscreenTarget>,
    pub(crate) effect_renderer: EffectRenderer,
    pub(crate) layer_cache: LayerCache,
    pub(crate) ablation: Ablation,
    pub(crate) ablation_frames: u32,
    pub(crate) nesting_overflow_reported: bool,
    window_damage: WindowDamage,
    /// The frames the swapchain's images hold.
    swapchain: SwapchainHistory,
    /// The frames drawn into images that come cleared.
    cleared: ClearedImages,
    /// The colour a frame drawn in a window last filled it with, and a
    /// pixel of it.
    pub(crate) window_fill: Option<(wgpu::Color, Rc<OffscreenTarget>)>,
    pub(crate) gates: admission_gate::AdmissionGates,
    transparent_sources: HashMap<(u32, u32), Rc<OffscreenTarget>>,
    shadow_surfaces: ShadowSurfaces,
    pub(crate) frame_stats: gpu_stats::FrameStats,
    history: FrameHistory,
    /// Whether the pipeline cache file holds the last launch's first screen,
    /// which builds ahead of this renderer's first frame.
    #[cfg(not(target_arch = "wasm32"))]
    first_screen_recorded: bool,
    /// How many requested shader warm-ups this renderer has queued.
    shader_warm_ups_queued: usize,
}

/// What a frame clears to before it draws: nothing for a transparent
/// window, the framework's background for every other surface.
pub fn frame_clear_color(transparent: bool) -> wgpu::Color {
    if transparent {
        wgpu::Color::TRANSPARENT
    } else {
        CLEAR_COLOR
    }
}

/// What a [`GpuRenderer`] is built from. It crosses to the present thread
/// whole, so it holds only owned, `Send` parts.
pub(crate) struct GpuRendererInit {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pub(crate) surface_format: wgpu::TextureFormat,
    pub(crate) adapter_backend: wgpu::Backend,
    pub(crate) adapter_downlevel: wgpu::DownlevelFlags,
    pub(crate) text_fonts: SoftwareTextFontSet,
    pub(crate) renderer_epoch: u64,
    pub(crate) pipeline_compilation: PipelineCompilation,
    pub(crate) on_device_lost: Option<crate::DeviceLostHandler>,
}

#[expect(clippy::multiple_inherent_impl, reason = "draw passes in draw_pass")]
impl GpuRenderer {
    pub(crate) fn new(init: GpuRendererInit) -> Self {
        let GpuRendererInit {
            device,
            queue,
            surface_format,
            adapter_backend,
            adapter_downlevel,
            text_fonts,
            renderer_epoch,
            pipeline_compilation,
            on_device_lost,
        } = init;
        let display_format = surface_format;
        let construction_started = Instant::now();
        let device_errors = Arc::new(DeviceErrorSentry::default());
        if survive_gpu_errors_enabled() {
            let sentry = Arc::clone(&device_errors);
            device.on_uncaptured_error(Arc::new(move |error| sentry.record(&error)));
        }
        let composition_format = crate::offscreen::composition_format(display_format);
        device.set_device_lost_callback(move |reason, message| {
            log::error!("[gpu-device] device lost ({reason:?}): {message}");
            if let Some(on_device_lost) = &on_device_lost {
                on_device_lost();
            }
        });
        let copy_free = UploadMode::copy_free(&device, adapter_backend);
        let mut run_store = RunStore::new(
            &device,
            RunBufferMode::for_device(&device, adapter_downlevel),
            copy_free,
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
                        min_binding_size: wgpu::BufferSize::new(size_of::<Uniforms>() as u64),
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
            GlyphSamplers {
                nearest: &image_nearest_sampler,
                linear: &image_linear_sampler,
            },
            TEXT_GLYPH_ATLAS_MIN_SIZE,
        );
        let viewport_uniforms = ViewportUniforms::default();

        if cranpose_core::DIAGNOSTICS.glass_material_folds.equals("1") {
            cranpose_ui_graphics::set_glass_material_folds(true);
        } else if cranpose_core::DIAGNOSTICS.glass_material_folds.equals("0") {
            cranpose_ui_graphics::set_glass_material_folds(false);
        }
        log::info!(
            "[gpu-init] liquid glass material folds {}",
            if cranpose_ui_graphics::glass_material_folds_enabled() {
                "on: a pipeline per material's feature set"
            } else {
                "off: one pipeline per blend mode"
            }
        );

        #[cfg(not(target_arch = "wasm32"))]
        let crate::pipeline_disk_cache::Loaded {
            cache: pipeline_cache,
            records,
            updated,
            holds_recorded,
        } = crate::pipeline_disk_cache::load(&device);
        #[cfg(not(target_arch = "wasm32"))]
        let first_screen: Vec<u64> = records
            .shapes
            .iter()
            .filter(|shape| shape.first_screen)
            .map(|shape| shape.entry)
            .collect();
        #[cfg(target_arch = "wasm32")]
        let (pipeline_cache, first_screen): (Option<wgpu::PipelineCache>, Vec<u64>) =
            (None, Vec::new());
        let recorder = crate::pipeline_recorder::PipelineRecorder::default();
        #[cfg(not(target_arch = "wasm32"))]
        let pipeline_cache_watcher = crate::pipeline_disk_cache::spawn_persist_watcher(
            pipeline_cache.clone(),
            Arc::clone(&records),
            recorder.clone(),
        );

        let effects_started = Instant::now();
        let pipeline_compiler =
            PipelineCompiler::for_compilation(pipeline_compilation, adapter_backend);
        let depth_gate = crate::depth_gate::DepthGate::new(
            crate::draw_pass::lays_interiors_first(adapter_backend),
            pipeline_compiler.is_active(),
        );
        let effect_renderer = EffectRenderer::new(
            &device,
            pipeline_compiler.clone(),
            pipeline_cache.clone(),
            composition_format,
            adapter_backend,
            recorder.clone(),
        );
        let output_converter = OutputConverter::new(&device, adapter_backend, display_format);
        let screenshot_converter =
            OutputConverter::new(&device, adapter_backend, wgpu::TextureFormat::Rgba8Unorm);
        let effects_ms = instant_ms(effects_started, Instant::now());
        let shape_shader = SharedShader::new(
            &device,
            adapter_backend,
            "Shape Shader",
            shape_shader_code(run_store.mode()),
            &[Some(&uniform_bind_group_layout), Some(run_store.layout())],
        );
        if run_store.mode().trig_fill {
            let fill = ArcTrigFill::new(
                &device,
                adapter_backend,
                pipeline_cache.clone(),
                shape_shader.clone(),
                recorder.clone(),
            );
            if first_screen.contains(&crate::arc_trig_fill::FIRST_SCREEN_KEY) {
                fill.warm(&pipeline_compiler);
            }
            run_store.attach_trig_fill(fill);
        }
        let shape_pipelines = ShapePipelines::new(
            ShapePipelineFactory {
                device: Arc::clone(&device),
                cache: pipeline_cache.clone(),
                format: composition_format,
                shader: shape_shader,
                mode: run_store.mode(),
            },
            adapter_backend,
            &pipeline_compiler,
            first_screen
                .into_iter()
                .filter(|&bits| bits != crate::arc_trig_fill::FIRST_SCREEN_KEY)
                .filter_map(ShapePipelineKey::from_bits),
            recorder.clone(),
        );
        let image_layouts = [
            Some(&uniform_bind_group_layout),
            Some(&image_bind_group_layout),
        ];
        let image_shader = SharedShader::new(
            &device,
            adapter_backend,
            "Image Shader",
            shaders::IMAGE,
            &image_layouts,
        );
        let glyph_atlas_shader = SharedShader::new(
            &device,
            adapter_backend,
            "Glyph Atlas Shader",
            shaders::GLYPH_ATLAS,
            &image_layouts,
        );
        let rrect_shadow_shader = SharedShader::new(
            &device,
            adapter_backend,
            "Round Rect Shadow Shader",
            shaders::RRECT_SHADOW,
            &[Some(&uniform_bind_group_layout)],
        );

        let mut renderer = Self {
            queue,
            device_errors,
            renderer_epoch,
            #[cfg(not(target_arch = "wasm32"))]
            display_format,
            composition_target: None,
            output_converter,
            screenshot_converter,
            pipelines: RendererPipelines {
                device: Arc::clone(&device),
                composition_format,
                adapter_backend,
                pipeline_cache,
                #[cfg(not(target_arch = "wasm32"))]
                _pipeline_cache_watcher: pipeline_cache_watcher,
                #[cfg(not(target_arch = "wasm32"))]
                recent_after_update: updated.then(|| Arc::clone(&records)),
                recorder,
                #[cfg(not(target_arch = "wasm32"))]
                landing: pipeline_compiler.landing(),
                compiler: pipeline_compiler,
                #[cfg(not(target_arch = "wasm32"))]
                built_at_frame_start: 0,
                drew_placeholder: Cell::new(false),
                shape_pipelines,
                image_pipeline: IMAGE_SRC_OVER_PIPELINES.map(FixedPipeline::new),
                image_pipeline_dst_out: IMAGE_DST_OUT_PIPELINES.map(FixedPipeline::new),
                glyph_atlas_pipeline: GLYPH_ATLAS_PIPELINES.map(FixedPipeline::new),
                rrect_shadow_pipeline: RRECT_SHADOW_PIPELINES.map(FixedPipeline::new),
                #[cfg(not(target_arch = "wasm32"))]
                mesh_pipelines: MESH_FALLBACK_PIPELINES.map(FixedPipeline::new),
                image_shader,
                glyph_atlas_shader,
                rrect_shadow_shader,
                uniform_bind_group_layout,
                image_bind_group_layout,
                image_nearest_sampler,
                image_linear_sampler,
            },
            device,
            depth_targets: DepthTargets::default(),
            scene_pass: crate::scene_pass::ScenePass::default(),
            depth_gate,
            surface_atlas_sizes: crate::frame::SurfaceAtlasSizes::for_backend(adapter_backend),
            text: TextResources::new(text_fonts, text_glyph_atlas, copy_free),
            viewport_uniforms,
            run_store,
            images: ImageTextures::new(),
            copy_free,
            pass_scratch: crate::draw_pass::PassScratch::default(),
            stage_side_scratch: StageSideScratch::default(),
            deferred_offscreen_releases: Vec::new(),
            effect_renderer,
            layer_cache: LayerCache::new(),
            ablation: Ablation::default(),
            ablation_frames: 0,
            nesting_overflow_reported: false,
            window_damage: WindowDamage::default(),
            swapchain: SwapchainHistory::default(),
            cleared: ClearedImages::default(),
            window_fill: None,
            gates: admission_gate::AdmissionGates::default(),
            transparent_sources: HashMap::default(),
            shadow_surfaces: ShadowSurfaces::new(),
            frame_stats: gpu_stats::FrameStats::default(),
            history: FrameHistory::default(),
            #[cfg(not(target_arch = "wasm32"))]
            first_screen_recorded: records.shapes.iter().any(|shape| shape.first_screen),
            shader_warm_ups_queued: 0,
        };
        #[cfg(not(target_arch = "wasm32"))]
        renderer.warm_first_screen(&records, holds_recorded);
        renderer.warm_requested_shaders();
        log::info!(
            "[gpu-init] {:?} renderer ready in {:.1} ms (effects {:.1} ms)",
            adapter_backend,
            instant_ms(construction_started, Instant::now()),
            effects_ms,
        );
        renderer
    }

    /// Prepares the first screen the last launches recorded: its pipelines
    /// queue as the renderer starts, behind those of the fallback triangles,
    /// and its first frame waits for none of them. Shapes still compiling
    /// draw as fallback triangles, runtime shaders as placeholders, and the
    /// other draws are left out until theirs land. A driver cache that
    /// `holds_recorded` pipelines builds them as cache hits, so those draw
    /// from it, and need no stand-in.
    #[cfg(not(target_arch = "wasm32"))]
    fn warm_first_screen(
        &mut self,
        records: &crate::pipeline_records::PipelineRecords,
        holds_recorded: bool,
    ) {
        self.pipelines.warm_mesh_fallback();
        self.warm_fixed_pipelines(&|label| {
            records
                .fixed
                .iter()
                .any(|fixed| fixed.first_screen && fixed.entry == label)
        });
        if holds_recorded {
            self.effect_renderer.trust_cached(records);
        }
        self.effect_renderer.warm_first_screen_shaders(
            records
                .shaders
                .iter()
                .filter(|shader| shader.first_screen)
                .map(|shader| &shader.entry),
        );
        if !records.shapes.iter().any(|shape| shape.first_screen) {
            self.warm_every_screen();
        }
    }

    /// Queues, for a launch with no record of its first screen, the
    /// pipelines most screens draw with that take no fallback triangles:
    /// text on whole pixels and turned, layer composites, rounded images
    /// and shadows, then flat solid rects. They build while the first
    /// screen composes, when the compiler's threads have nothing else, so
    /// its first frame draws them. On a Mate 20 X the gauntlet's first
    /// frame then drew its whole screen, its shapes as fallback triangles,
    /// where without them its layers, turned text, avatars and shadows
    /// joined over the next 150 ms.
    #[cfg(not(target_arch = "wasm32"))]
    fn warm_every_screen(&mut self) {
        let lane = CompileLane::WarmUp;
        let (compiler, backend) = (&self.pipelines.compiler, self.pipelines.adapter_backend);
        for quads in [GlyphQuads::Aligned, GlyphQuads::Turned] {
            self.pipelines
                .glyph_atlas_pipeline_resource(PassDepth::Off, quads)
                .queue(
                    compiler,
                    lane,
                    backend,
                    self.pipelines
                        .glyph_atlas_pipeline_job(PassDepth::Off, quads),
                );
        }
        self.effect_renderer.warm_composites(&self.device);
        let rounded = ImageVariant {
            depth: PassDepth::Off,
            alpha_mask: false,
            rounded: true,
        };
        self.pipelines
            .image_pipeline_resource(BlendMode::SrcOver, rounded)
            .queue(
                compiler,
                lane,
                backend,
                self.pipelines
                    .image_pipeline_job(BlendMode::SrcOver, rounded),
            );
        self.pipelines.rrect_shadow_pipeline[PassDepth::Off.index()].queue(
            compiler,
            lane,
            backend,
            self.pipelines.rrect_shadow_pipeline_job(PassDepth::Off),
        );
        self.pipelines
            .shape_pipelines
            .warm(ShapePipelineKey::SOLID_RECT);
    }

    /// Queues the shader warm-ups requested since the last call, each at the
    /// target it draws to (`cranpose_ui_graphics::request_shader_warm_ups`).
    fn warm_requested_shaders(&mut self) {
        let requested = cranpose_ui_graphics::shader_warm_ups_after(self.shader_warm_ups_queued);
        if requested.is_empty() {
            return;
        }
        self.shader_warm_ups_queued += requested.len();
        self.effect_renderer.warm_shaders(&requested);
    }

    /// Queues every pipeline recent launches of another build drew with:
    /// after an update their compiled pipelines are gone, and each would
    /// otherwise compile inside the first frame that draws with it.
    #[cfg(not(target_arch = "wasm32"))]
    fn warm_recent(&mut self, records: &crate::pipeline_records::PipelineRecords) {
        self.warm_fixed_pipelines(&|label| records.fixed.iter().any(|fixed| fixed.entry == label));
        let shapes = records
            .shapes
            .iter()
            .map(|shape| shape.entry)
            .filter(|&bits| bits != crate::arc_trig_fill::FIRST_SCREEN_KEY)
            .filter_map(ShapePipelineKey::from_bits);
        for key in shapes {
            self.pipelines.shape_pipelines.warm(key);
        }
        self.effect_renderer
            .warm_recorded_shaders(records.shaders.iter().map(|shader| &shader.entry));
    }

    /// Queues the fixed pipelines whose labels `wanted` accepts.
    #[cfg(not(target_arch = "wasm32"))]
    fn warm_fixed_pipelines(&self, wanted: &dyn Fn(&str) -> bool) {
        let compiler = self.effect_renderer.compiler();
        self.for_each_fixed_pipeline(&mut |pipeline, job| {
            if wanted(pipeline.label()) {
                pipeline.queue(
                    compiler,
                    CompileLane::WarmUp,
                    self.pipelines.adapter_backend,
                    job(),
                );
            }
        });
    }

    /// Builds here, ahead of the first frame, the pipelines named by the
    /// first screens in `records` that this renderer's backend drew (see
    /// [`crate::WgpuRenderer::build_first_screen_pipelines`]).
    pub(crate) fn build_first_screen(&mut self, records: &[u8], viewport: (u32, u32)) {
        let records = crate::pipeline_records::decode_first_screen(
            records,
            self.pipelines.adapter_backend,
            viewport,
        );
        for shape in &records.shapes {
            if let Some(key) = ShapePipelineKey::from_bits(shape.entry) {
                self.pipelines.shape_pipelines.build_ahead(key);
            }
        }
        self.for_each_fixed_pipeline(&mut |pipeline, job| {
            if records
                .fixed
                .iter()
                .any(|fixed| fixed.entry == pipeline.label())
            {
                pipeline.build_ahead(
                    &self.pipelines.recorder,
                    self.pipelines.adapter_backend,
                    job(),
                );
            }
        });
    }

    /// See [`crate::WgpuRenderer::first_screen_recorded`].
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn first_screen_recorded(&self) -> bool {
        self.first_screen_recorded
    }

    /// Hands `visit` every fixed pipeline, the effect renderer's too, with
    /// the job that builds it.
    fn for_each_fixed_pipeline(
        &self,
        visit: &mut dyn FnMut(&FixedPipeline, &dyn Fn() -> FixedPipelineJob),
    ) {
        self.effect_renderer.for_each_fixed(&self.device, visit);
        for depth in PassDepth::BOTH {
            for (alpha_mask, rounded) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                let variant = ImageVariant {
                    depth,
                    alpha_mask,
                    rounded,
                };
                for blend_mode in [BlendMode::SrcOver, BlendMode::DstOut] {
                    visit(
                        self.pipelines.image_pipeline_resource(blend_mode, variant),
                        &|| Box::new(self.pipelines.image_pipeline_job(blend_mode, variant)),
                    );
                }
            }
            for quads in [GlyphQuads::Plain, GlyphQuads::Aligned, GlyphQuads::Turned] {
                visit(
                    self.pipelines.glyph_atlas_pipeline_resource(depth, quads),
                    &|| Box::new(self.pipelines.glyph_atlas_pipeline_job(depth, quads)),
                );
            }
            visit(
                &self.pipelines.rrect_shadow_pipeline[depth.index()],
                &|| Box::new(self.pipelines.rrect_shadow_pipeline_job(depth)),
            );
        }
        for kind in crate::scene_pass::ScenePipeline::ALL {
            visit(self.scene_pipeline_resource(kind), &|| {
                Box::new(self.scene_surface_pipeline_job(kind))
            });
        }
    }

    fn scene_pipeline_resource(&self, kind: crate::scene_pass::ScenePipeline) -> &FixedPipeline {
        self.scene_pass.pipeline(kind)
    }

    /// A scene pipeline for a scene's pass, or `None` while it compiles:
    /// the draws it takes are left out, and the frame after it lands draws
    /// them.
    pub(crate) fn scene_surface_pipeline(
        &self,
        kind: crate::scene_pass::ScenePipeline,
    ) -> Option<&wgpu::RenderPipeline> {
        self.pipelines.ready_or_left_out(
            &self.frame_stats,
            self.scene_pipeline_resource(kind),
            || self.scene_surface_pipeline_job(kind),
        )
    }

    /// The pipeline of `variant` of a volume shader a frame noted; `None`
    /// while it compiles, when the frame after it lands draws the volumes,
    /// or when the shader does not build.
    pub(crate) fn scene_volume_pipeline(
        &self,
        shader: &Arc<str>,
        variant: crate::scene_pass::VolumeVariant,
    ) -> Option<&wgpu::RenderPipeline> {
        let kept = self.scene_pass.volume(shader)?;
        self.pipelines.app_shader_pipeline(
            &self.frame_stats,
            kept.pipeline(variant as usize),
            || {
                crate::scene_pass::volume_pipeline_job(
                    &self.device,
                    self.pipelines.pipeline_cache.clone(),
                    (
                        self.pipelines.composition_format,
                        self.pipelines.adapter_backend,
                        variant,
                    ),
                    (self.scene_pipeline_parts(), kept),
                )
            },
        )
    }

    /// The pipeline of `variant` of a shaded surface's shader a frame
    /// noted; `None` while it compiles, when the frame after it lands draws
    /// the surfaces with it, or when the shader does not build.
    pub(crate) fn scene_shaded_pipeline(
        &self,
        shader: &Arc<str>,
        variant: crate::scene_pass::ShadedVariant,
    ) -> Option<&wgpu::RenderPipeline> {
        let kept = self.scene_pass.shaded(shader)?;
        self.pipelines.app_shader_pipeline(
            &self.frame_stats,
            kept.pipeline(variant as usize),
            || {
                crate::scene_pass::shaded_pipeline_job(
                    &self.device,
                    self.pipelines.pipeline_cache.clone(),
                    (
                        self.pipelines.composition_format,
                        self.pipelines.adapter_backend,
                        variant,
                    ),
                    (self.scene_pipeline_parts(), kept),
                )
            },
        )
    }

    /// The bind group layout of a scene surface's uniforms.
    pub(crate) fn scene_uniform_layout(&self) -> &wgpu::BindGroupLayout {
        &self.scene_pipeline_parts().uniform_layout
    }

    /// What a scene binds in its shadow map's place when it casts none.
    pub(crate) fn scene_no_shadow(&self) -> &wgpu::BindGroup {
        &self.scene_pipeline_parts().no_shadow
    }

    fn scene_pipeline_parts(&self) -> &crate::scene_pass::ScenePipelineParts {
        self.scene_pass.parts(
            &self.device,
            self.pipelines.adapter_backend,
            &self.effect_renderer.effect_texture_bind_group_layout,
        )
    }

    fn scene_surface_pipeline_job(
        &self,
        kind: crate::scene_pass::ScenePipeline,
    ) -> impl FnOnce() -> wgpu::RenderPipeline + CompilerSend + 'static {
        crate::scene_pass::scene_pipeline_job(
            &self.device,
            self.pipelines.pipeline_cache.clone(),
            (self.pipelines.composition_format, kind),
            self.scene_pipeline_parts(),
        )
    }

    /// Lays out as fallback triangles a stored run's draws in `range` of
    /// `draws` whose shape pipelines still compile.
    pub(crate) fn mesh_store_run(
        &self,
        run: &RunDraw,
        space: DrawSpace,
        range: std::ops::Range<usize>,
        draws: &[RunDrawCall],
        depth: PassDepth,
        meshes: &mut crate::mesh_fallback::PassMeshes,
    ) {
        if self.pipelines.mesh_pipeline(depth).is_none()
            || !self.pipelines.awaits_shape_pipeline(draws, range.clone())
        {
            return;
        }
        let tables = run.tables();
        let records = crate::mesh_fallback::MeshRecords {
            bodies: tables.shapes.bodies(),
            curves: tables.shapes.curves(),
            brushes: &tables.brushes,
            stops: &tables.stops,
        };
        let placement = PlacementData::of(
            &run.placement,
            space.scale,
            space.viewport.transform,
            [0.0; 2],
        )
        .mesh();
        let target = space.viewport.mesh_target(depth);
        self.pipelines
            .mesh_unready(draws, range, records, &|_| Some(placement), target, meshes);
    }

    /// Lays out as fallback triangles the arena draws in `range` of `draws`,
    /// just cut or closed, whose shape pipelines still compile.
    pub(crate) fn mesh_arena_draws(
        &self,
        viewport: ViewportUniformParams,
        range: std::ops::Range<usize>,
        draws: &[RunDrawCall],
        depth: PassDepth,
        meshes: &mut crate::mesh_fallback::PassMeshes,
    ) {
        if self.pipelines.mesh_pipeline(depth).is_none()
            || !self.pipelines.awaits_shape_pipeline(draws, range.clone())
        {
            return;
        }
        let (records, placements) = self.run_store.arena_mesh_records();
        self.pipelines.mesh_unready(
            draws,
            range,
            records,
            &|body| {
                placements
                    .get(body.placement as usize)
                    .map(PlacementData::mesh)
            },
            viewport.mesh_target(depth),
            meshes,
        );
    }

    /// Draws `instances` of the pass's uploaded shadow instances, bound with
    /// `uniform_slot` under `scissor`.
    pub(crate) fn draw_shadow_instances(
        &self,
        pass: &mut TrackedPass<'_, '_>,
        buffer: &BufferUpload,
        uniform_slot: usize,
        instances: std::ops::Range<u32>,
        scissor: Option<(u32, u32, u32, u32)>,
        frame: PassFrame,
    ) -> Result<(), String> {
        let depth = frame.depth;
        let Some(pipeline) = self.pipelines.ready_or_left_out(
            &self.frame_stats,
            &self.pipelines.rrect_shadow_pipeline[depth.index()],
            || self.pipelines.rrect_shadow_pipeline_job(depth),
        ) else {
            return Ok(());
        };
        let (x, y, width, height) = frame.scissor(scissor);
        pass.set_scissor_rect(x, y, width, height);
        pass.set_pipeline(pipeline);
        self.viewport_uniforms.bind(pass, uniform_slot)?;
        pass.set_vertex_buffer(0, buffer.slice());
        pass.draw(0..SHADOW_QUAD_CORNERS, instances);
        self.frame_stats.add_draw_calls(1);
        Ok(())
    }

    pub(crate) fn upload_shadow_instances<C: FrameCommandRecorder>(
        &self,
        recorder: &mut C,
        instances: &[ShadowInstance],
    ) -> BufferUpload {
        recorder.upload_buffer(
            UploadAllocatorSpec::vertex(
                "Shadow Instance Buffer",
                size_of::<ShadowInstance>() as u64,
            ),
            &self.device,
            bytemuck::cast_slice(instances),
        )
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

    /// Where the frame renders: straight into an image that can take it,
    /// unless the image comes cleared and the composition target keeps the
    /// frame for it; else into the composition target.
    fn frame_root(
        &mut self,
        output: FrameOutput<'_>,
        viewport: (u32, u32),
        reads_page: bool,
    ) -> FrameRoot {
        if let (OutputMode::Display, Some(view), Some(texture)) =
            (output.mode, output.view, output.texture)
            && surface_is_direct_root(
                texture,
                self.pipelines.composition_format,
                viewport,
                reads_page,
            )
        {
            let image = Rc::new(OffscreenTarget::from_surface(texture.clone(), view.clone()));
            if output.image == ImageContent::Cleared && self.cleared.copied {
                return FrameRoot::Composition {
                    composition: self.take_composition_target(viewport.0, viewport.1),
                    cleared: Some(image),
                };
            }
            return FrameRoot::Surface { image, keep: None };
        }
        FrameRoot::Composition {
            composition: self.take_composition_target(viewport.0.max(1), viewport.1.max(1)),
            cleared: None,
        }
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
            self.pipelines.composition_format,
            width,
            height,
        ));
        let output_bind_group = self.output_converter.bind_group(&self.device, &target.view);
        CompositionTarget {
            target,
            output_bind_group,
            kept: KeptWindow::default(),
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
            self.pipelines.composition_format,
        )
    }

    /// A texture of the given size that stays transparent: the input of a
    /// runtime shader whose layer draws nothing itself, so the shader needs
    /// no surface pass and reads the same empty content every frame.
    #[inline(never)]
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

    fn flush_deferred_offscreen_releases(&mut self, executor: &mut WgpuFrameGraphExecutor) {
        self.gates.end_frame(&mut self.layer_cache);
        for target in self.deferred_offscreen_releases.drain(..) {
            self.effect_renderer.release_offscreen(target);
        }
        self.layer_cache.end_frame();
        for (transient, target) in self.layer_cache.take_released() {
            match transient {
                Some(descriptor) => executor.return_cached_transient(descriptor, target),
                None => self.effect_renderer.release_offscreen(target),
            }
        }
        self.effect_renderer.end_offscreen_frame();
        executor.end_transient_frame();
        self.depth_targets.end_frame();
        self.scene_pass.end_frame();
        self.surface_atlas_sizes.end_frame();
    }

    pub(crate) fn render(
        &mut self,
        submit: &mut FrameSubmit,
        viewport: (u32, u32),
        packet: FramePacket,
        surface_epoch: u64,
        returns: &mut RenderReturns,
        output: FrameOutput<'_>,
    ) -> Result<(), String> {
        let (width, height) = viewport;
        let FrameOutput {
            mode: output_mode,
            view: output_view,
            image,
            ..
        } = output;
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
        #[cfg(not(target_arch = "wasm32"))]
        let builds_before = thread_builds();
        self.warm_requested_shaders();
        self.pipelines.shape_pipelines.begin_frame();
        self.pipelines.recorder.begin_frame((width, height));
        self.begin_placeholder_frame();
        self.viewport_uniforms.begin_frame();
        self.run_store.begin_frame(gpu_stats_enabled());
        self.text.begin_text_glyph_run_frame();
        self.pass_scratch.meshes.begin_frame();

        let reads_page = packet.root.contains_backdrop()
            || packet
                .overlay
                .as_ref()
                .is_some_and(|overlay| overlay.contains_backdrop());
        let mut frame_root = self.frame_root(output, (width, height), reads_page);
        let plan = self.damage_of(
            &packet,
            &mut frame_root,
            output_mode,
            (width, height),
            (surface_epoch, image),
        );
        let repaint = self.count_damage(plan, (width, height));
        let root = frame_root.target();
        let screenshot_bind_group = output_view.and_then(|_| {
            matches!(output_mode, OutputMode::Screenshot).then(|| {
                self.screenshot_converter
                    .bind_group(&self.device, &root.view)
            })
        });
        let output = frame_root.output(output_view, screenshot_bind_group.as_ref());
        let submitted = self.render_graph(submit, root, packet, repaint, returns, output);
        #[cfg(not(target_arch = "wasm32"))]
        self.effect_renderer
            .compiler()
            .spread_demand(thread_builds() == builds_before);
        if let Some(composition) = frame_root.into_composition() {
            self.composition_target = Some(composition);
        }
        let after_graph = Instant::now();
        if self.effect_renderer.take_skipped_draw() {
            self.pipelines.note_placeholder(&self.frame_stats);
        }
        self.flush_deferred_offscreen_releases(&mut submit.executor);
        self.pass_scratch.glyph_instances.end_frame();

        self.record_pool_sizes(&submit.executor);
        self.effect_renderer
            .merge_and_reset_debug_counters(&self.frame_stats);
        self.keep_window(plan, &submitted);
        let result = submitted.map(drop);
        submit.executor.reset_upload_allocators();
        self.close_frame_stats(submit);
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

    // Notes the sizes of the caches and pools the frame left.
    fn record_pool_sizes(&self, executor: &WgpuFrameGraphExecutor) {
        self.frame_stats
            .layer_cache_size
            .set(self.layer_cache.len() as u32);
        self.frame_stats
            .layer_cache_bytes
            .set(self.layer_cache.bytes());
        self.frame_stats.offscreen_pool_size.set(
            self.effect_renderer
                .retained_offscreen_count()
                .saturating_add(executor.retained_texture_count())
                .saturating_add(usize::from(self.composition_target.is_some())) as u32,
        );
        self.frame_stats.offscreen_pool_bytes.set(
            (self.effect_renderer.retained_offscreen_bytes() as u64)
                .saturating_add(executor.retained_texture_bytes())
                .saturating_add(self.composition_target.as_ref().map_or(0, |target| {
                    u64::from(target.target.width)
                        .saturating_mul(u64::from(target.target.height))
                        .saturating_mul(COMPOSITION_BYTES_PER_PIXEL)
                })),
        );
        self.frame_stats
            .text_pool_size
            .set(self.text.image_cache.len() as u32);
        self.frame_stats
            .image_cache_size
            .set(self.images.len() as u32);
    }

    // Takes the frame's stats, logs and prints them as asked, and resets
    // them for the next frame.
    fn close_frame_stats(&mut self, submit: &FrameSubmit) {
        self.history.close(&self.frame_stats, &self.device);
        submit.end_pass_timing_frame();
        self.frame_stats.reset();
    }

    /// Returns a packet unrendered, handing its scene back for recycling.
    pub(crate) fn cancel_packet(
        packet: FramePacket,
        reason: CancelReason,
        returns: &mut RenderReturns,
    ) -> Result<(), String> {
        returns.scene = Some(FrameSceneStorage {
            root: packet.root,
            overlay: packet.overlay,
        });
        returns.frame_id = packet.frame_id;
        returns.outcome = PresentOutcome::Cancelled(reason);
        Ok(())
    }

    // The parts a draw of text or an image borrows apart.
    pub(crate) fn draw_parts(&mut self) -> DrawParts<'_> {
        DrawParts {
            text: &mut self.text,
            images: &mut self.images,
            upload: FrameUploads {
                pipelines: &self.pipelines,
                uniforms: &mut self.viewport_uniforms,
                queue: &self.queue,
                stats: &mut self.frame_stats,
            },
        }
    }

    pub(crate) fn last_frame_stats(&self) -> Option<gpu_stats::FrameStatsSnapshot> {
        self.history.last()
    }

    pub(crate) fn needs_frame_warmup(&self) -> bool {
        self.history.warming_up() || self.pipelines.placeholder_awaits_frame()
    }

    /// Starts a frame: once a pipeline a placeholder waited for has landed,
    /// the retained layers, which may hold that placeholder, are dropped.
    fn begin_placeholder_frame(&mut self) {
        self.pipelines.drew_placeholder.set(false);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(landing) = &self.pipelines.landing {
            self.pipelines.built_at_frame_start = landing.built();
            if landing.take_landed() {
                self.drop_kept_drawing();
            }
        }
    }

    /// Drops what earlier frames drew and kept, the layers and the blurred
    /// shadows: pipelines the browser compiled in the background landed, and
    /// those were drawn while the draws that use them were left out.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn background_pipelines_landed(&mut self) {
        self.drop_kept_drawing();
    }

    /// Drops the layers, the blurred shadows and the window earlier frames
    /// drew and kept.
    fn drop_kept_drawing(&mut self) {
        self.layer_cache.clear();
        self.shadow_surfaces.clear();
        if let Some(composition) = self.composition_target.as_mut() {
            composition.kept.forget();
        }
        self.swapchain.forget();
    }

    /// What `packet` changes against the frame `frame_root` kept: a frame
    /// displayed through the composition target, or into a swapchain `image`
    /// that holds an earlier frame, draws only that, while a screenshot is
    /// drawn whole and so is a frame into any other presentable image. An
    /// image that comes cleared takes the frame from the composition target
    /// while the frames change a small part of a heavy window.
    fn damage_of(
        &mut self,
        packet: &FramePacket,
        frame_root: &mut FrameRoot,
        output_mode: OutputMode,
        viewport: (u32, u32),
        (surface_epoch, image): (u64, ImageContent),
    ) -> Option<FrameDamagePlan> {
        match frame_root {
            FrameRoot::Composition { composition, .. }
                if matches!(output_mode, OutputMode::Screenshot) =>
            {
                composition.kept.forget();
                None
            }
            FrameRoot::Composition {
                composition,
                cleared: Some(cleared),
            } => {
                let frame = self.frame_window(packet, viewport);
                let damage =
                    self.window_damage
                        .begin_as(&frame, &mut composition.kept, Summaries::COPIED);
                if matches!(damage, Damage::Part(_)) {
                    return Some(FrameDamagePlan {
                        damage,
                        repaint: damage.repaint(),
                        keeper: Keeper::Composition,
                    });
                }
                // The frame changes most of the window, or costs less drawn
                // whole than copied: it draws straight into the image, and
                // the composition target lets the frame go.
                self.cleared.copied = false;
                *frame_root = FrameRoot::Surface {
                    image: Rc::clone(cleared),
                    keep: None,
                };
                Some(FrameDamagePlan {
                    damage,
                    repaint: Repaint::Whole,
                    keeper: Keeper::Cleared { keeping: false },
                })
            }
            FrameRoot::Composition { composition, .. } => {
                // The swapchain image the frame is copied into holds a frame
                // the history does not follow.
                self.swapchain.forget();
                let frame = self.frame_window(packet, viewport);
                let damage = self.window_damage.begin(&frame, &mut composition.kept);
                Some(FrameDamagePlan {
                    damage,
                    repaint: damage.repaint(),
                    keeper: Keeper::Composition,
                })
            }
            FrameRoot::Surface { image: target, .. } => {
                let image = match image {
                    ImageContent::Untracked => return None,
                    ImageContent::Numbered(image) => image,
                    ImageContent::Cleared => {
                        let frame = self.frame_window(packet, viewport);
                        let (damage, keeping) =
                            self.cleared.damage(&mut self.window_damage, &frame);
                        if keeping {
                            *frame_root = FrameRoot::Surface {
                                image: Rc::clone(target),
                                keep: Some(self.take_composition_target(viewport.0, viewport.1)),
                            };
                        }
                        return Some(FrameDamagePlan {
                            damage,
                            repaint: Repaint::Whole,
                            keeper: Keeper::Cleared { keeping },
                        });
                    }
                };
                self.swapchain.begin(surface_epoch);
                if !image.kept {
                    // Drawn whole, and only a frame a lap of images later
                    // could draw over it, so the first frames of a surface
                    // take no summary.
                    self.swapchain.drop_image(image);
                    return None;
                }
                let frame = self.frame_window(packet, viewport);
                let damage = self.window_damage.begin(&frame, &mut self.swapchain.kept);
                let repaint = self
                    .swapchain
                    .changed_since(image, damage)
                    .map_or(Repaint::Whole, |changed| {
                        self.window_damage.repaint(changed, viewport)
                    });
                Some(FrameDamagePlan {
                    damage,
                    repaint,
                    keeper: Keeper::Image(image),
                })
            }
        }
    }

    fn frame_window<'p>(&self, packet: &'p FramePacket, viewport: (u32, u32)) -> FrameWindow<'p> {
        FrameWindow {
            root: &packet.root,
            overlay: packet.overlay.as_deref(),
            viewport,
            root_scale: packet.root_scale,
            setting: self.text.frame_setting(packet.clear),
        }
    }

    /// Counts what a frame of `plan` changes and draws of a page of
    /// `viewport` in the frame's stats, and returns what it draws.
    fn count_damage(&self, plan: Option<FrameDamagePlan>, viewport: (u32, u32)) -> Repaint {
        self.frame_stats
            .damage
            .set(frame_damage(plan.map(|plan| plan.damage)));
        let repaint = plan.map_or(Repaint::Whole, |plan| plan.repaint);
        self.frame_stats
            .repainted_pixels
            .set(repaint.pixels(viewport));
        repaint
    }

    /// Keeps the summary of the frame of `plan` just drawn into its target
    /// when it was drawn whole, with no stand-in. A frame that draws nothing
    /// into a presentable image submits nothing, and is drawn all the same.
    fn keep_window(&mut self, plan: Option<FrameDamagePlan>, submitted: &Result<bool, String>) {
        let Some(plan) = plan else {
            return;
        };
        let drawn = match submitted {
            Ok(submitted) => *submitted || plan.repaint == Repaint::Nothing,
            Err(_) => false,
        } && !self.frame_stats.drew_stand_in();
        match plan.keeper {
            Keeper::Composition => {
                if let Some(composition) = self.composition_target.as_mut() {
                    self.window_damage.end(&mut composition.kept, drawn);
                }
            }
            Keeper::Image(image) => {
                self.swapchain.record(image, plan.damage, drawn);
                self.window_damage.end(&mut self.swapchain.kept, drawn);
            }
            Keeper::Cleared { keeping } => {
                self.window_damage.end(&mut self.cleared.kept, drawn);
                if keeping && let Some(composition) = self.composition_target.as_mut() {
                    self.cleared.hand_over(&mut composition.kept);
                }
            }
        }
    }

    /// Lets go of the copy of the last frame the renderer keeps for a
    /// surface whose images come cleared, as a WebGPU canvas's do: for a
    /// runtime whose app holds still, so the copy's window of memory goes.
    /// The next frame draws whole into its image.
    pub(crate) fn release_kept_copy(&mut self) {
        if !self.cleared.copied {
            return;
        }
        match self.composition_target.take() {
            Some(mut composition) => self.cleared.take_back(&mut composition.kept),
            None => self.cleared.copied = false,
        }
    }

    /// Queues every pipeline the cache file records, as the first frame of
    /// an updated build does, and returns whether any is left to build: for
    /// a renderer that prepares an app's pipelines without drawing.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn prepare_recorded(&mut self) -> bool {
        self.effect_renderer.warm_after_first_frame();
        if let Some(records) = self.pipelines.recent_after_update.take() {
            self.warm_recent(&records);
        }
        self.pipelines.compiling()
    }

    pub(crate) fn debug_cpu_allocation_stats(&self) -> DebugCpuAllocationStats {
        DebugCpuAllocationStats {
            scene_graph_node_count: 0,
            scene_graph_heap_bytes: 0,
            scene_hits_len: 0,
            scene_hits_cap: 0,
            scene_node_index_len: 0,
            scene_node_index_cap: 0,
            text_renderer_pool_len: self.text.image_cache.len(),
            text_renderer_pool_cap: self.text.image_cache.cap().get(),
            image_texture_cache_len: self.images.len(),
            image_texture_cache_cap: self.images.capacity(),
            run_arena_staging_bytes: self.run_store.arena_staging_bytes(),
            run_store_bytes: self.run_store.stored_bytes(),
            run_store_runs: self.run_store.stored_count(),
            scratch_image_vertices_cap: self.pass_scratch.images.vertices.capacity(),
            scratch_image_indices_cap: self.pass_scratch.images.indices.capacity(),
            scratch_image_cmds_cap: self.pass_scratch.images.cmds.capacity(),
            scratch_glyph_instances_cap: self.pass_scratch.glyph_instances.plain.capacity()
                + self.pass_scratch.glyph_instances.turned.capacity(),
            layer_cache_len: self.layer_cache.len(),
            layer_cache_bytes: self.layer_cache.bytes(),
        }
    }
    pub(crate) fn render_to_rgba_pixels(
        &mut self,
        submit: &mut FrameSubmit,
        viewport: (u32, u32),
        packet: FramePacket,
        surface_epoch: u64,
        returns: &mut RenderReturns,
    ) -> Result<Vec<u8>, String> {
        let (width, height) = viewport;
        if width == 0 || height == 0 {
            return Err("Screenshot size must be non-zero".to_string());
        }
        let errors_before = self.device_errors.error_count();

        let output_texture = crate::offscreen::create_2d_texture(
            &self.device,
            wgpu::TextureFormat::Rgba8Unorm,
            width,
            height,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            Some("Screenshot Output Texture"),
        );
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let output = FrameOutput {
            mode: OutputMode::Screenshot,
            view: Some(&output_view),
            texture: None,
            image: ImageContent::Untracked,
        };
        self.render(submit, viewport, packet, surface_epoch, returns, output)?;
        if returns.outcome != PresentOutcome::Presented {
            return Err(format!(
                "Screenshot rendering did not complete: {:?}",
                returns.outcome
            ));
        }
        self.read_screenshot_pixels(submit, &output_texture, errors_before)
    }

    fn read_screenshot_pixels(
        &mut self,
        submit: &mut FrameSubmit,
        output_texture: &wgpu::Texture,
        errors_before: u64,
    ) -> Result<Vec<u8>, String> {
        let width = output_texture.width();
        let height = output_texture.height();
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

        let mut graph = WgpuFrameGraph::new(Some("Screenshot Copy Encoder"));
        let source = graph.import_surface("screenshot-copy-source");
        graph.add_fallible_command_pass(Some("Screenshot Copy Pass"), &[source], &[], |context| {
            context.encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: output_texture,
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
        let execution = submit.execute(graph);
        self.recall_frame_uploads();
        let execution = execution.map_err(|error| error.to_string())?;
        let submission_index = execution.submission;
        let copy_stats = execution.stats;
        self.history.add_command_stats(copy_stats);

        let buffer_slice = output_buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission_index),
                timeout: None,
            })
            .map_err(|error| format!("Screenshot GPU wait failed: {error}"))?;

        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Err(format!("Screenshot map_async failed: {err:?}")),
            Err(err) => return Err(format!("Screenshot readback timed out: {err}")),
        }
        if self.device_errors.error_count() != errors_before {
            output_buffer.unmap();
            return Err("GPU errors occurred while capturing the screenshot".to_string());
        }

        let mapped = buffer_slice
            .get_mapped_range()
            .map_err(|err| format!("Screenshot readback could not be read: {err}"))?;
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

    /// Asks the buffers the submitted frame unmapped back for later frames.
    fn recall_frame_uploads(&mut self) {
        self.viewport_uniforms.uploads.recall();
        self.run_store.recall();
    }

    /// Records and submits the frame: whether it submitted anything.
    fn render_graph(
        &mut self,
        submit: &mut FrameSubmit,
        root_target: &Rc<OffscreenTarget>,
        packet: FramePacket,
        repaint: Repaint,
        returns: &mut RenderReturns,
        output: OutputStep<'_>,
    ) -> Result<bool, String> {
        #[cfg(target_arch = "wasm32")]
        let graph_start = Instant::now();
        let FramePacket {
            root,
            overlay,
            root_scale,
            clear,
            ..
        } = packet;
        let page = Rc::clone(root_target);
        let draw = FrameDraw {
            root: &root,
            overlay: overlay.as_deref(),
            root_scale,
            clear,
            repaint,
        };

        #[cfg(not(target_arch = "wasm32"))]
        let (result, submitted) = self.execute_frame_graph(submit, draw, &page, output);

        #[cfg(target_arch = "wasm32")]
        let (result, submitted) = {
            let (result, execution) = {
                let mut frame_encoder = submit.begin(Some("Renderer Frame Encoder"));
                let initial_pass_count = frame_encoder.recorded_pass_count();
                let result = self.encode_frame(&mut frame_encoder, draw, Rc::clone(&page), output);
                let execution =
                    if result.is_ok() && frame_encoder.recorded_pass_count() > initial_pass_count {
                        Some(frame_encoder.finish())
                    } else {
                        None
                    };
                (result, execution)
            };
            let after_execute = Instant::now();
            if let Some(total_ms) = should_log_wgpu_render_stage(graph_start, after_execute) {
                log::warn!("[wgpu-render-stage:graph] total_ms={total_ms:.2}",);
            }
            let submitted = execution.is_some();
            if let Some(execution) = execution {
                self.frame_stats.record_command_stats(execution.stats);
            }
            (result, submitted)
        };
        #[cfg(not(target_arch = "wasm32"))]
        if submitted {
            self.pipelines.recorder.note_frame_drawn();
            self.effect_renderer.warm_after_first_frame();
            if let Some(records) = self.pipelines.recent_after_update.take() {
                self.warm_recent(&records);
            }
        }
        if !submitted {
            self.run_store.invalidate_uploads();
            self.text.glyph_gpu_run_cache.clear();
            self.text.glyph_run_arena = GlyphRunArena::new(self.copy_free);
        }
        returns.scene = Some(FrameSceneStorage { root, overlay });
        result.map(|()| submitted)
    }

    /// Records and submits the frame through the frame graph: whether it
    /// recorded without error, and whether it submitted anything.
    #[cfg(not(target_arch = "wasm32"))]
    fn execute_frame_graph(
        &mut self,
        submit: &mut FrameSubmit,
        draw: FrameDraw<'_>,
        page: &Rc<OffscreenTarget>,
        output: OutputStep<'_>,
    ) -> (Result<(), String>, bool) {
        let graph_start = Instant::now();
        let mut frame_graph = WgpuFrameGraph::new(Some("Renderer Frame Graph"));
        let surface = frame_graph.import_surface("renderer-surface");
        frame_graph.add_fallible_recorded_command_pass(
            Some("Renderer Frame Pass"),
            &[],
            &[surface],
            |frame_encoder| self.encode_frame(frame_encoder, draw, Rc::clone(page), output),
        );
        let after_build = Instant::now();
        let execution = submit.execute(frame_graph);
        let after_execute = Instant::now();
        self.recall_frame_uploads();
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
    }

    /// Records the frame: the root and overlay layer scenes into the frame's
    /// target, the output conversion when the target is not the presented
    /// image, and the viewport uniforms the recorded passes claimed.
    fn encode_frame<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        draw: FrameDraw<'_>,
        page: Rc<OffscreenTarget>,
        output: OutputStep<'_>,
    ) -> Result<(), String> {
        FrameExecutor::new(self, recorder).render_frame(
            draw.root,
            draw.overlay,
            page,
            draw.root_scale,
            wgpu::LoadOp::Clear(draw.clear),
            draw.repaint,
        )?;
        match output {
            OutputStep::None => {}
            OutputStep::Convert {
                view: output_view,
                bind_group,
                mode,
            } => {
                match mode {
                    OutputMode::Display => &self.output_converter,
                    OutputMode::Screenshot => &self.screenshot_converter,
                }
                .encode(
                    &self.device,
                    recorder,
                    output_view,
                    bind_group,
                    self.pipelines.adapter_backend,
                    &self.pipelines.recorder,
                );
                recorder.record_pass();
            }
            OutputStep::Copy { from, to } => {
                recorder.copy_texture_region(TextureRegionCopy {
                    source: from,
                    source_origin: [0, 0],
                    dest: to,
                    dest_origin: [0, 0],
                    size: [to.width, to.height],
                });
                // The image takes the copy even when the frame drew nothing.
                recorder.record_pass();
            }
        }
        self.stage_frame_uploads(recorder);
        self.pipelines.shape_pipelines.build_wanted();
        self.depth_gate.end_frame();
        self.pipelines.compiler.end_frame();
        self.viewport_uniforms.uploads.finish_frame();
        self.run_store.finish_frame();
        Ok(())
    }

    pub(crate) fn stage_frame_uploads(&mut self, recorder: &mut impl FrameCommandRecorder) {
        let mut upload = self.viewport_uniforms.stage_pending(&self.device, recorder);
        upload += self.run_store.stage_pending(&self.device, recorder);
        upload += self
            .text
            .glyph_run_arena
            .stage_pending(&self.device, recorder);
        self.run_store.fill_arena_trig(&self.device, recorder);
        self.frame_stats.record_command_stats(upload);
    }
    /// Resolves a blurred shadow at `z` into a texture and queues its
    /// composites. The shadow's shapes and texts render into a source the
    /// size of their blur footprint, blur in place and take the post-blur
    /// cutouts; the source is then blitted over the rect the shadow covers.
    /// Shape-only shadows live in the shadow cache, keyed by their content
    /// and device placement, so a scrolling card re-blits its cached blur.
    /// The blurred shadow texture and whether the cache held it: a
    /// shape-only shadow is cached by content and placement, a shadow with
    /// text renders every frame.
    fn blurred_shadow_source<C: FrameCommandRecorder>(
        &mut self,
        frame: &mut FrameRecording<'_, C>,
        shadow: &ShadowDraw,
        source_device: DevicePixelBounds,
        pixel_radius: f32,
        root_scale: f32,
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
        if let Some(target) = key.and_then(|key| self.shadow_surfaces.get(&key)) {
            return Some((Rc::clone(target), true, content));
        }
        // A shadow whose blur compiles is left out until the blur lands.
        if !self.effect_renderer.blur_draws_now(
            &self.device,
            TileMode::Decal,
            (source_device.width, source_device.height),
            pixel_radius,
            pixel_radius,
        ) {
            self.pipelines.note_placeholder(&self.frame_stats);
            return None;
        }
        if !shape_only {
            self.frame_stats.record_shadow_text_blur_fallback();
        }
        let source = self.render_shadow_source(
            frame,
            shadow,
            source_device,
            pixel_radius,
            root_scale,
            key.is_some(),
        )?;
        if let Some(key) = key {
            self.frame_stats
                .record_shadow_shape_cache_miss(source_device.width, source_device.height);
            self.frame_stats.maybe_print_shadow_shape_cache_miss(
                source_device,
                key.content_hash,
                pixel_radius,
                shadow.shapes.as_ref().map_or(0, RunDraw::record_count) as usize,
                shadow.clip,
            );
            self.shadow_surfaces.insert(key, Rc::clone(&source));
        }
        Some((source, false, content))
    }

    pub(crate) fn resolve_blurred_shadow<C: FrameCommandRecorder>(
        &mut self,
        frame: &mut FrameRecording<'_, C>,
        shadow: &ShadowDraw,
        z: usize,
        root_scale: f32,
        window: &Window,
        resolved: &mut Vec<ResolvedComposite>,
    ) {
        if !shadow.requires_surface()
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
        let Some(drawn) = window.cut_near(
            DeviceRect::from_logical(visible, root_scale),
            margin * root_scale,
        ) else {
            return;
        };
        let visible = Rect {
            x: drawn.x / root_scale,
            y: drawn.y / root_scale,
            width: drawn.width / root_scale,
            height: drawn.height / root_scale,
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
        let Some((source, hit, content)) =
            self.blurred_shadow_source(frame, shadow, source_device, pixel_radius, root_scale)
        else {
            return;
        };
        let dest = (
            source_device.x,
            source_device.y,
            source_device.width as f32,
            source_device.height as f32,
        );
        let mut coverage = window
            .cut(DeviceRect {
                x: dest.0,
                y: dest.1,
                width: dest.2,
                height: dest.3,
            })
            .map(|cut| (cut.x, cut.y, cut.width, cut.height));
        if let Some(clip) = shadow.clip {
            coverage = coverage.and_then(|coverage| {
                intersect_device_rects(coverage, anchored_rect_to_device(clip, anchor, root_scale))
            });
        }
        let Some(coverage) = coverage else {
            return;
        };
        if hit {
            self.frame_stats.record_shadow_shape_cache_hit(
                (coverage.2 as u64).saturating_mul(coverage.3 as u64),
            );
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
        resolved.push(ResolvedComposite {
            z_index: z,
            source,
            content,
            dest,
            scissor: Some(coverage),
            kind: ResolvedCompositeKind::Blit {
                alpha: 1.0,
                blend_mode: BlendMode::SrcOver,
                rounded_mask,
                sample_mode,
                source_viewport,
            },
        });
    }

    /// Draws a shadow's shapes and texts into a surface covering `bounds`
    /// and blurs it. A wide blur runs at its scratch size and its result
    /// stays there, read bilinearly by the composite; a post-blur cutout
    /// needs the surface's full size, so the blurred result is interpolated
    /// back into it first and the cutout drawn at that size. A retained
    /// result feeds the shadow cache; a transient one is registered with
    /// the frame's transients and released with them. `None` when the
    /// shadow draws nothing.
    fn render_shadow_source<C: FrameCommandRecorder>(
        &mut self,
        frame: &mut FrameRecording<'_, C>,
        shadow: &ShadowDraw,
        bounds: DevicePixelBounds,
        pixel_radius: f32,
        root_scale: f32,
        retained: bool,
    ) -> Option<Rc<OffscreenTarget>> {
        let (width, height) = (bounds.width, bounds.height);
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
            self.shadow_transient(frame, "Shadow Result", result_width, result_height)
        };
        let source = if full_size_result {
            Rc::clone(&result)
        } else {
            self.shadow_transient(frame, "Shadow Source", width, height)
        };
        let offset = [bounds.x, bounds.y];
        let target = PassTarget {
            view: &source.view,
            width,
            height,
        };
        let scene = shadow_scene(
            shadow.shapes.as_ref(),
            &shadow.texts,
            blur_reach(shadow.blur_radius, root_scale),
        );
        let segment = PassSegment::plain(&scene, offset, root_scale);
        let drew = self.encode_pass(
            frame.recorder,
            target,
            (std::slice::from_ref(&segment), SegmentLayout::Stacked),
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
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
        if pixel_radius > 0.0 {
            let scratch_descriptor = self.transient_offscreen_descriptor(
                "Shadow Blur Scratch",
                scratch_width,
                scratch_height,
            );
            let scratch = frame
                .recorder
                .acquire_transient_offscreen(&self.device, scratch_descriptor);
            let blurred = if full_size_result && (scratch_width, scratch_height) != (width, height)
            {
                Some(self.shadow_transient(
                    frame,
                    "Shadow Blur Result",
                    scratch_width,
                    scratch_height,
                ))
            } else {
                None
            };
            let (view, size) = match &blurred {
                Some(blurred) => (&blurred.view, (scratch_width, scratch_height)),
                None => (&result.view, (result_width, result_height)),
            };
            let passes = self.effect_renderer.encode_blur_scissored_ping_pong_passes(
                frame.recorder,
                &self.device,
                &source,
                &scratch,
                BlurDest {
                    view,
                    size,
                    scissor: None,
                },
                BlurSpec::uniform(pixel_radius, TileMode::Decal),
            );
            frame.recorder.record_passes(passes);
            self.effect_renderer.record_blur_pass();
            frame
                .recorder
                .release_transient_offscreen(scratch_descriptor, scratch);
            if let Some(blurred) = &blurred {
                self.effect_renderer.encode_upscale_pass(
                    frame.recorder,
                    &self.device,
                    blurred,
                    &result.view,
                );
                frame.recorder.record_pass();
            }
        }
        if let Some(cutout_run) = &shadow.post_blur_cutouts {
            let cutouts = shadow_scene(Some(cutout_run), &[], 0.0);
            let segment = PassSegment::plain(&cutouts, offset, root_scale);
            if let Err(error) = self.encode_pass(
                frame.recorder,
                target,
                (std::slice::from_ref(&segment), SegmentLayout::Stacked),
                wgpu::LoadOp::Load,
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
        frame: &mut FrameRecording<'_, C>,
        label: &'static str,
        width: u32,
        height: u32,
    ) -> Rc<OffscreenTarget> {
        let descriptor = self.transient_offscreen_descriptor(label, width, height);
        let target = Rc::new(
            frame
                .recorder
                .acquire_transient_offscreen(&self.device, descriptor),
        );
        frame.transients.push((descriptor, Rc::clone(&target)));
        target
    }

    /// Whether `run` draws from retained buffers keyed by its command.
    pub(crate) fn run_is_stored(&self, run: &RunDraw) -> bool {
        self.run_store.is_stored(run)
    }

    /// Brings a stored run's tables up to date and appends its draws, under
    /// a placement uniform of its own, to the pass's run draws `draws`, and
    /// the pipelines they draw with to `pipelines`.
    pub(crate) fn prepare_store_run<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        run: &RunDraw,
        space: DrawSpace,
        window: &std::ops::Range<u32>,
        (depth, mut depth_keys): (PassDepth, Option<&mut PipelineDemand>),
        (draws, pipelines): (&mut Vec<RunDrawCall>, &mut PipelineDemand),
    ) -> StoreRunBatch {
        let (viewport, root_scale) = (space.viewport, space.scale);
        let placement = &run.placement;
        let ablation = self.ablation.shape;
        let turns = ShapeTurns::of(viewport.transform, false);
        let scissor = clip_scissor(placement, root_scale, viewport);
        let clip = SegmentClip::of(placement, scissor.is_some());
        let start = draws.len();
        self.run_store.stored_run_draws(
            &self.device,
            run,
            &mut |segment| {
                let key = |depth| {
                    ShapePipelineKey::of_run(
                        segment,
                        placement,
                        RunTier::Store,
                        ablation,
                        turns,
                        (depth, clip),
                        viewport,
                    )
                };
                if let Some(keys) = depth_keys.as_deref_mut() {
                    keys.add(key(PassDepth::On));
                }
                key(depth)
            },
            draws,
        );
        window_draws(draws, start, window);
        let run_draws = &draws[start..];
        let upload_start = Instant::now();
        let (upload, fill, tables) = self.run_store.upload_stored(
            &self.device,
            recorder,
            run,
            root_scale,
            window,
            run_draws,
        );
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
        let uniforms = Uniforms::of(
            viewport,
            PlacementData::of(&run.placement, root_scale, viewport.transform, [0.0; 2]),
            RunTier::Store,
        );
        let uniform_slot = self.viewport_uniforms.claim(
            &self.device,
            &self.pipelines.uniform_bind_group_layout,
            &uniforms,
        );
        for draw in run_draws {
            pipelines.add(draw.key);
        }
        StoreRunBatch {
            tables,
            uniform_slot,
            draws: start..draws.len(),
            clip: scissor,
        }
    }

    pub(crate) fn open_arena(&mut self) -> usize {
        self.run_store.open_arena()
    }

    /// The records the open arena chunk holds so far.
    pub(crate) fn open_arena_records(&self) -> u32 {
        self.run_store.open_arena_records()
    }

    pub(crate) fn arena_accepts(&self, chunk: usize, run: &RunDraw) -> bool {
        self.run_store.arena_accepts(chunk, run)
    }

    /// Appends `window` of `run`'s records to the open arena chunk, their
    /// placement under `turn` and moved by `shift` (see
    /// [`PlacementData::of`]), keyed for a pass whose flat and turned
    /// records alternate often when `mixed_turns`, and the pipelines they
    /// draw with to `pipelines`. `clip` says how its variant tests its clip:
    /// an unturned run leaves a rect clip to its paint's scissor, never a
    /// rounded one.
    pub(crate) fn append_arena_run(
        &mut self,
        chunk: usize,
        run: &RunDraw,
        window: std::ops::Range<u32>,
        at: ArenaPlacement,
        clip: SegmentClip,
        (pipelines, mut depth_keys): (&mut PipelineDemand, Option<&mut PipelineDemand>),
    ) -> u32 {
        let placement = &run.placement;
        let ablation = self.ablation.shape;
        let (viewport, depth) = (at.space.viewport, at.depth);
        let turn = viewport.transform;
        let turns = ShapeTurns::of(turn, at.mixed_turns);
        self.run_store.append_arena(
            chunk,
            run,
            window,
            (at.space.scale, turn, at.shift),
            &mut |segment| {
                let key = |depth| {
                    ShapePipelineKey::of_run(
                        segment,
                        placement,
                        RunTier::Arena,
                        ablation,
                        turns,
                        (depth, clip),
                        viewport,
                    )
                };
                if let Some(keys) = depth_keys.as_deref_mut() {
                    keys.add(key(PassDepth::On));
                }
                key(depth)
            },
            pipelines,
        )
    }

    /// Moves the open chunk's draws since its last cut onto `out`, keeping
    /// it open.
    pub(crate) fn cut_arena(&mut self, chunk: usize, out: &mut Vec<RunDrawCall>) {
        self.run_store.cut_arena(&self.device, chunk, out);
    }

    /// Uploads the open chunk and moves its draws since its last cut onto
    /// `out`.
    pub(crate) fn close_arena(&mut self, chunk: usize, out: &mut Vec<RunDrawCall>) {
        if let Some(fill) = self.run_store.close_arena(&self.device, chunk, out) {
            self.frame_stats.add_shape_fill(fill);
        }
    }

    pub(crate) fn draw_run_calls(
        &self,
        pass: &mut TrackedPass<'_, '_>,
        tables: ArenaBinding<'_>,
        calls: RunCalls<'_>,
    ) -> Result<(), String> {
        let RunCalls {
            uniform_slot,
            draws,
            scissor,
            stage,
            meshes,
        } = calls;
        match stage {
            RunStage::Paint => {
                if draws.is_empty() {
                    return Ok(());
                }
                self.frame_stats.bump_shapes();
                self.record_run_draws(
                    pass,
                    tables,
                    uniform_slot,
                    scissor,
                    draws.iter().map(PlannedRunDraw::paint),
                    meshes,
                )
            }
            RunStage::Interiors => {
                let planned = interior_run_draws(draws);
                if planned.is_empty() {
                    return Ok(());
                }
                self.frame_stats
                    .shape_interior_draws
                    .set(self.frame_stats.shape_interior_draws.get() + planned.len() as u32);
                self.record_run_draws(
                    pass,
                    tables,
                    uniform_slot,
                    scissor,
                    planned.into_iter(),
                    None,
                )
            }
        }
    }

    /// Records `planned` against one batch's tables, uniform and scissor. A
    /// draw whose pipeline still compiles draws its fallback triangles from
    /// `meshes`, with the depth buffer or not, else it is left out.
    fn record_run_draws(
        &self,
        pass: &mut TrackedPass<'_, '_>,
        tables: ArenaBinding<'_>,
        uniform_slot: usize,
        scissor: (u32, u32, u32, u32),
        planned: impl ExactSizeIterator<Item = PlannedRunDraw>,
        meshes: Option<(crate::mesh_fallback::RunMeshes<'_>, PassDepth)>,
    ) -> Result<(), String> {
        self.frame_stats.add_draw_calls(planned.len() as u32);
        let (x, y, width, height) = scissor;
        pass.set_scissor_rect(x, y, width, height);
        self.viewport_uniforms.bind(pass, uniform_slot)?;
        pass.set_bind_group(1, tables.bind_group, &tables.offsets[2..]);
        let bind_records = |pass: &mut TrackedPass<'_, '_>| {
            for (slot, buffer) in tables.records.into_iter().enumerate() {
                pass.set_vertex_buffer(
                    slot as u32,
                    buffer.slice(u64::from(tables.offsets[slot])..),
                );
            }
        };
        bind_records(pass);
        let mut bound_class = None;
        let mut records_bound = true;
        for (position, draw) in planned.enumerate() {
            let key = draw.key;
            let Some((pipeline, fallback)) = self.pipelines.shape_pipelines.get(key) else {
                // Its pipeline compiles: the draw takes its fallback
                // triangles, or is left out, until it lands.
                let mesh = meshes.and_then(|(meshes, depth)| {
                    let range = meshes.ranges.get(position)?.clone()?;
                    Some((range, meshes, self.pipelines.mesh_pipeline(depth)?))
                });
                let Some((range, meshes, mesh_pipeline)) = mesh else {
                    self.pipelines.note_placeholder(&self.frame_stats);
                    continue;
                };
                self.pipelines.note_mesh_fallback(&self.frame_stats);
                pass.set_pipeline(mesh_pipeline);
                meshes.draw(pass, range);
                bound_class = None;
                records_bound = false;
                continue;
            };
            if !records_bound {
                bind_records(pass);
                records_bound = true;
            }
            if fallback {
                self.frame_stats
                    .shape_pipeline_fallback_draws
                    .set(self.frame_stats.shape_pipeline_fallback_draws.get() + 1);
            } else if !key.is_general() {
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
            pass.draw_indexed(draw.indices, 0, draw.records);
        }
        Ok(())
    }

    /// Draws a stored-run batch, whose draws are its range of `draws`.
    pub(crate) fn draw_store_run(
        &self,
        pass: &mut TrackedPass<'_, '_>,
        batch: &StoreRunBatch,
        draws: &[RunDrawCall],
        scissor: (u32, u32, u32, u32),
        stage: RunStage,
        meshes: Option<(crate::mesh_fallback::RunMeshes<'_>, PassDepth)>,
    ) -> Result<(), String> {
        let calls = RunCalls {
            uniform_slot: batch.uniform_slot,
            draws: &draws[batch.draws.clone()],
            scissor,
            stage,
            meshes: meshes.map(|(meshes, depth)| (meshes.of(batch.draws.clone()), depth)),
        };
        self.draw_run_calls(pass, batch.tables.binding(), calls)
    }

    pub(crate) fn draw_arena(
        &self,
        pass: &mut TrackedPass<'_, '_>,
        chunk: usize,
        calls: RunCalls<'_>,
    ) -> Result<(), String> {
        self.draw_run_calls(pass, self.run_store.arena_binding(chunk), calls)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn surface_format(&self) -> wgpu::TextureFormat {
        self.display_format
    }

    pub(crate) fn device_error_count(&self) -> u64 {
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
        self.empty_pass(recorder, "Clear Pass", view, load_op);
    }

    pub(crate) fn empty_pass<C: FrameCommandRecorder>(
        &self,
        recorder: &mut C,
        label: &'static str,
        view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) {
        let pass = recorder.begin_color_pass(label, view, load_op);
        drop(pass);
        recorder.record_pass();
    }
    pub(crate) fn draw_image_cmds(
        &self,
        pass: &mut TrackedPass<'_, '_>,
        image_slot: &ImageSlot,
        uniform_slot: usize,
        cmds: &[ImageDrawCmd],
        (blend_mode, depth): (BlendMode, PassDepth),
        bound: Option<(u32, u32, u32, u32)>,
    ) -> Result<(), String> {
        if cmds.is_empty() {
            return Ok(());
        }
        self.frame_stats.bump_images();
        self.frame_stats.add_draw_calls(cmds.len() as u32);
        let mut bound_pipeline = None;
        self.viewport_uniforms.bind(pass, uniform_slot)?;
        pass.set_index_buffer(image_slot.indices.slice(), wgpu::IndexFormat::Uint32);
        pass.set_vertex_buffer(0, image_slot.vertices.slice());
        let mut clips_bound = false;
        for cmd in cmds {
            let Some((x, y, width, height)) = bounded_scissor(cmd.scissor, bound) else {
                continue;
            };
            pass.set_scissor_rect(x, y, width, height);
            let variant = ImageVariant {
                depth,
                alpha_mask: cmd.image.alpha_mask,
                rounded: cmd.clip.is_some(),
            };
            if bound_pipeline != Some(variant) {
                let Some(pipeline) =
                    self.pipelines
                        .image_pipeline(&self.frame_stats, blend_mode, variant)
                else {
                    continue;
                };
                pass.set_pipeline(pipeline);
                bound_pipeline = Some(variant);
            }
            pass.set_bind_group(1, &cmd.image.bind_group, &[]);
            let instance = match cmd.clip {
                Some(clip) => {
                    if !clips_bound {
                        let clips = image_slot
                            .clips
                            .as_ref()
                            .ok_or_else(|| "rounded image without clip instances".to_string())?;
                        pass.set_vertex_buffer(1, clips.slice());
                        clips_bound = true;
                    }
                    clip
                }
                None => 0,
            };
            pass.draw_indexed(
                cmd.index_start..(cmd.index_start + 6),
                0,
                instance..instance + 1,
            );
        }
        Ok(())
    }

    pub(crate) fn draw_glyph_cmds(
        &self,
        pass: &mut TrackedPass<'_, '_>,
        (glyph_slot, turned_slot): (Option<&BufferUpload>, Option<&BufferUpload>),
        uniform_slot: usize,
        cmds: &[GlyphDrawCmd],
        bound: Option<(u32, u32, u32, u32)>,
        frame: PassFrame,
    ) -> Result<(), String> {
        if cmds.is_empty() {
            return Ok(());
        }
        let whole_target = frame.scissor(bound);
        self.frame_stats.bump_text();
        let mut bound_atlas = None;
        // Which shared instances are bound, turned or plain, and which
        // pipeline; retained runs draw with a plain one.
        let mut shared_bound: Option<GlyphKind> = None;
        let mut bound_pipeline: Option<GlyphQuads> = None;
        let mut bound_run_instances: Option<&wgpu::Buffer> = None;
        let mut draws = 0u32;
        for draw in GlyphDraws::new(cmds) {
            let scissor = match draw.scissor {
                Some(scissor) => bounded_scissor(scissor, bound),
                None => Some(whole_target),
            };
            let Some((x, y, width, height)) = scissor else {
                continue;
            };
            pass.set_scissor_rect(x, y, width, height);
            if !bound_atlas.is_some_and(|atlas| Rc::ptr_eq(atlas, draw.atlas)) {
                pass.set_bind_group(1, draw.atlas.as_ref(), &[]);
                bound_atlas = Some(draw.atlas);
            }
            draws += 1;
            match draw.step {
                GlyphDrawStep::Shared(instances, quads) => {
                    if bound_pipeline != Some(quads) {
                        let Some(pipeline) = self.pipelines.glyph_atlas_pipeline(
                            &self.frame_stats,
                            frame.depth,
                            quads,
                        ) else {
                            continue;
                        };
                        pass.set_pipeline(pipeline);
                        bound_pipeline = Some(quads);
                    }
                    let kind = quads.kind();
                    if shared_bound != Some(kind) {
                        let slot = match kind {
                            GlyphKind::Plain => glyph_slot,
                            GlyphKind::Turned => turned_slot,
                        }
                        .ok_or_else(|| "shared glyph draw without glyph instances".to_string())?;
                        self.viewport_uniforms.bind(pass, uniform_slot)?;
                        pass.set_vertex_buffer(0, slot.slice());
                        shared_bound = Some(kind);
                        bound_run_instances = None;
                    }
                    pass.draw(0..GLYPH_QUAD_CORNERS, instances);
                }
                GlyphDrawStep::Retained {
                    run,
                    uniform_slot: retained_slot,
                    aligned,
                } => {
                    let quads = if aligned {
                        GlyphQuads::Aligned
                    } else {
                        GlyphQuads::Plain
                    };
                    if bound_pipeline != Some(quads) {
                        let Some(pipeline) = self.pipelines.glyph_atlas_pipeline(
                            &self.frame_stats,
                            frame.depth,
                            quads,
                        ) else {
                            continue;
                        };
                        pass.set_pipeline(pipeline);
                        bound_pipeline = Some(quads);
                    }
                    shared_bound = None;
                    self.viewport_uniforms.bind(pass, retained_slot)?;
                    let instances = run.span.instance_buffer();
                    if bound_run_instances != Some(instances) {
                        pass.set_vertex_buffer(0, instances.slice(..));
                        bound_run_instances = Some(instances);
                    }
                    pass.draw(0..GLYPH_QUAD_CORNERS, run.span.instances());
                }
            }
        }
        self.frame_stats.add_draw_calls(draws);
        Ok(())
    }
    pub(crate) fn upload_glyph_instances<C: FrameCommandRecorder>(
        &self,
        recorder: &mut C,
        instances: &[GlyphInstance],
    ) -> BufferUpload {
        recorder.upload_buffer(
            glyph_instance_spec(),
            &self.device,
            bytemuck::cast_slice(instances),
        )
    }

    pub(crate) fn upload_turned_glyphs<C: FrameCommandRecorder>(
        &self,
        recorder: &mut C,
        glyphs: &[TurnedGlyph],
    ) -> BufferUpload {
        recorder.upload_buffer(
            turned_glyph_spec(),
            &self.device,
            bytemuck::cast_slice(glyphs),
        )
    }

    fn convert_surface_pixels_to_rgba(&self, pixels: &[u8]) -> Result<Vec<u8>, String> {
        if !pixels.len().is_multiple_of(4) {
            return Err("Screenshot readback has an incomplete pixel".to_string());
        }
        Ok(pixels.to_vec())
    }
}

fn align_to(value: u32, alignment: u32) -> u32 {
    debug_assert!(alignment > 0);
    value.div_ceil(alignment) * alignment
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
    let mut left = canonicalize_device_coordinate(rect.x * root_scale);
    let mut top = canonicalize_device_coordinate(rect.y * root_scale);
    let mut right = canonicalize_device_coordinate((rect.x + rect.width) * root_scale);
    let mut bottom = canonicalize_device_coordinate((rect.y + rect.height) * root_scale);
    if !viewport.transform.is_identity() {
        let bounds = viewport.transform.target_bounds(Rect {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        });
        (left, top) = (bounds.x, bounds.y);
        (right, bottom) = (bounds.x + bounds.width, bounds.y + bounds.height);
    }
    let left = (left - viewport.offset[0]).clamp(0.0, width).floor();
    let top = (top - viewport.offset[1]).clamp(0.0, height).floor();
    let right = (right - viewport.offset[0]).clamp(0.0, width).ceil();
    let bottom = (bottom - viewport.offset[1]).clamp(0.0, height).ceil();
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

/// The rounded clip an image takes in place, in device pixels: one instance
/// its draw reads, beside the quad's vertices.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct DeviceRoundedClip {
    rect: [f32; 4],
    radius: f32,
}

impl DeviceRoundedClip {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
        4 => Float32x4,
        5 => Float32,
    ];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBS,
        }
    }

    /// The rounded clip `image` takes at `root_scale`, scaled as a run's
    /// placement scales its clip.
    fn of(image: &ImageDraw, root_scale: f32) -> Option<Self> {
        let rounding = image.rounding.filter(|_| image.clip.is_some())?;
        let device =
            crate::geometry::device_rect(rounding.rect, root_scale, image.snap_anchor.is_some());
        Some(Self {
            rect: [device.x, device.y, device.width, device.height],
            radius: rounding.radius * root_scale,
        })
    }
}

/// One draw call of a run batch's stage: its pipeline, the strip index
/// buffer it binds and the indices and records it instances.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedRunDraw {
    pub(crate) key: ShapePipelineKey,
    pub(crate) band_class: u8,
    pub(crate) indices: std::ops::Range<u32>,
    pub(crate) records: std::ops::Range<u32>,
}

impl PlannedRunDraw {
    /// A draw of the paint, as it was recorded.
    fn paint(draw: &RunDrawCall) -> Self {
        Self {
            key: draw.key,
            band_class: draw.band_class,
            indices: draw.indices(),
            records: draw.records.clone(),
        }
    }
}

/// The draw calls laying a batch's opaque interiors down: last first, so
/// they go down front to back, one call over each stretch of neighbouring
/// draws that share the interior pipeline, cut to the span from the stretch's
/// first draw holding an occluder to its last. The draws between ride along:
/// the pipeline lays any record's exact interior down and skips the rest, so
/// taking them costs a few vertices rather than a call, and it shades only a
/// record's quad, the first two triangles of every band class's pattern.
pub(crate) fn interior_run_draws(draws: &[RunDrawCall]) -> SmallVec<[PlannedRunDraw; 4]> {
    struct Stretch {
        key: ShapePipelineKey,
        start: u32,
        span: Option<PlannedRunDraw>,
    }
    let mut planned: SmallVec<[PlannedRunDraw; 4]> = SmallVec::new();
    let mut stretch: Option<Stretch> = None;
    for draw in draws.iter().rev() {
        let Some(key) = draw.key.interior() else {
            planned.extend(stretch.take().and_then(|open| open.span));
            continue;
        };
        if stretch
            .as_ref()
            .is_some_and(|open| open.key != key || open.start != draw.records.end)
        {
            planned.extend(stretch.take().and_then(|open| open.span));
        }
        let open = stretch.get_or_insert(Stretch {
            key,
            start: draw.records.end,
            span: None,
        });
        open.start = draw.records.start;
        if !draw.occluders {
            continue;
        }
        match &mut open.span {
            Some(span) => span.records.start = draw.records.start,
            None => {
                open.span = Some(PlannedRunDraw {
                    key,
                    band_class: draw.band_class,
                    indices: 0..cranpose_ui_graphics::strip_indices(1),
                    records: draw.records.clone(),
                });
            }
        }
    }
    planned.extend(stretch.and_then(|open| open.span));
    planned
}

/// Drops the entries of `cache` no frame drew for more than `idle_frames`
/// before `frame`: the least recently used first, which is the order they
/// were last drawn in.
fn evict_idle<K: Clone + Eq + Hash, V>(
    cache: &mut BoundedLruCache<K, V>,
    frame: u64,
    idle_frames: u64,
    last_frame: impl Fn(&V) -> u64,
) {
    while cache
        .peek_lru()
        .is_some_and(|(_, value)| frame.saturating_sub(last_frame(value)) > idle_frames)
    {
        cache.pop_lru();
    }
}

/// Cuts the run draws from `start` on to `window` of their records, counted
/// across them in order, and drops the draws left with none.
fn window_draws(draws: &mut Vec<RunDrawCall>, start: usize, window: &std::ops::Range<u32>) {
    let mut relative = 0u32;
    let mut kept = start;
    for index in start..draws.len() {
        let draw = &mut draws[index];
        let count = draw.records.end - draw.records.start;
        let first = relative;
        relative += count;
        let keep_start = window.start.max(first).min(first + count);
        let keep_end = window.end.min(first + count).max(keep_start);
        draw.records =
            draw.records.start + (keep_start - first)..draw.records.start + (keep_end - first);
        if draw.records.start < draw.records.end {
            draws.swap(kept, index);
            kept += 1;
        }
    }
    draws.truncate(kept);
}

mod glyph_draws;
pub(crate) use glyph_draws::{
    GlyphDrawCmd, GlyphInstance, GlyphInstances, defer_retained_glyph_runs, group_glyph_kinds,
};
use text_resources::TextResources;
pub(crate) mod admission_gate;
mod glyph_atlas;
mod renderer_pipelines;
#[cfg(not(target_arch = "wasm32"))]
use renderer_pipelines::MESH_FALLBACK_PIPELINES;
use renderer_pipelines::{
    GLYPH_ATLAS_PIPELINES, IMAGE_DST_OUT_PIPELINES, IMAGE_SRC_OVER_PIPELINES,
    RRECT_SHADOW_PIPELINES, RendererPipelines,
};
mod draw_space;
mod image_draws;
pub(crate) use draw_space::{ArenaPlacement, DrawSpace};
mod image_textures;
use image_textures::ImageTextures;
mod text_glyph_cache;
mod text_resources;
pub(crate) use image_draws::{
    ImageBatch, ImageDrawCmd, ImageSlot, place_image_draws, resolve_image_geometry,
};
mod depth_targets;
mod pass_depth;
use depth_targets::DepthTargets;
pub(crate) use pass_depth::PassDepth;
mod frame_history;
mod frame_submit;
mod frame_uploads;
use frame_history::FrameHistory;
pub use frame_history::frames_presented;
pub(crate) use frame_submit::FrameSubmit;
pub(crate) use frame_uploads::{DrawParts, FrameUploads};
mod shadow_cache;
pub(crate) use shadow_cache::{shadow_composite_mask, shadow_draw_bounds};
mod shape_pipeline;
#[cfg(test)]
pub(crate) use shape_pipeline::ShapeVariant;
pub(crate) use shape_pipeline::{
    RunTier, SegmentClip, ShapeDepth, ShapePipelineKey, ShapeTurns, create_shape_pipeline,
};
mod text_raster_source;

use glyph_atlas::{
    GlyphAtlasEntry, GlyphAtlasSlotKey, GlyphSamplers, RunAtlasEntries, TEXT_GLYPH_ATLAS_MIN_SIZE,
    TextGlyphAtlas, create_glyph_atlas_pipeline, glyph_atlas_uv_rect,
};
use glyph_draws::{
    GLYPH_QUAD_CORNERS, GlyphDrawStep, GlyphDraws, GlyphExtent, GlyphKind, GlyphQuads, TurnedGlyph,
    for_each_visible_glyph, glyph_cut_edges, glyph_instance_spec, glyph_rect_aligned,
    shared_glyph_clip, turned_glyph_spec,
};
use image_draws::{
    ImageBindGroups, ImageBinding, ImageUvRect, ImageVariant, create_image_pipeline,
    image_sampler_descriptor, sampling_under,
};
use shadow_cache::{
    ShadowSurfaces, device_pixel_bounds, shadow_scene, shape_shadow_surface_cache_key,
    skip_shadow_draws,
};
use shape_pipeline::{LINE_KINDS, shape_shader_code};
use text_glyph_cache::{
    CachedGpuTextGlyphRun, GlyphRunBounds, GlyphRunQuads, MAX_TEXT_GLYPH_GPU_RUN_CACHE_ITEMS,
    MAX_TEXT_GLYPH_MASK_CACHE_ITEMS, MAX_TEXT_IMAGE_CACHE_ITEMS, MAX_TEXT_LINE_INDEX_CACHE_ITEMS,
    MIN_TEXT_GLYPH_RUN_CACHE_ITEMS, RETAINED_TEXT_GLYPH_RUN_MIN_QUADS,
    TEXT_GLYPH_GPU_RUN_IDLE_FRAMES, TEXT_GLYPH_RUN_IDLE_FRAMES, TextGlyphRunDraw,
    TextGlyphRunLookup, cached_text_glyph_instance, cached_text_glyph_quad_is_visible_in_viewport,
    log_text_atlas_fallback,
};
use text_raster_source::{
    TextRasterSource, clipped_text_raster_source_with_line_starts, line_start_offsets,
};

#[cfg(test)]
#[path = "tests/render_text_bounds_tests.rs"]
mod text_bounds_tests;

#[cfg(test)]
#[path = "tests/render_retained_glyph_tests.rs"]
mod retained_glyph_tests;

#[cfg(test)]
#[path = "tests/frame_clear_tests.rs"]
mod frame_clear_tests;

#[cfg(test)]
#[path = "tests/render_clip_scissor_tests.rs"]
mod clip_scissor_tests;
