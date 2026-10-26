use cranpose_core::NodeId;
use cranpose_render_common::raster_cache::{
    LAYER_RASTER_CACHE_KIND_COUNT, LAYER_RASTER_CACHE_KIND_LABELS,
};
use cranpose_ui_graphics::Rect;

use crate::{debug_toggles::DebugToggle, frame_graph::FrameCommandStats, run_geometry::ShapeFill};

/// A diagnostics line: logcat on Android, where nothing reads stderr, and
/// stderr elsewhere.
macro_rules! stats_line {
    ($($arg:tt)*) => {{
        #[cfg(target_os = "android")]
        log::info!($($arg)*);
        #[cfg(not(target_os = "android"))]
        eprintln!($($arg)*);
    }};
}

mod frame_stats;
pub(crate) use frame_stats::FrameStats;

const TOP_ISOLATED_LAYER_LIMIT: usize = 8;

/// A layer the renderer drew into an offscreen texture of its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IsolatedLayerStat {
    /// The node of the layer, or `None` when no node owns it.
    pub node_id: Option<NodeId>,
    /// The layer's bounds in logical pixels.
    pub logical_rect: Rect,
    /// Width of the layer's offscreen texture in device pixels.
    pub width: u32,
    /// Height of the layer's offscreen texture in device pixels.
    pub height: u32,
}

impl IsolatedLayerStat {
    fn pixel_area(self) -> u64 {
        (self.width as u64) * (self.height as u64)
    }
}

impl Default for IsolatedLayerStat {
    fn default() -> Self {
        Self {
            node_id: None,
            logical_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            width: 0,
            height: 0,
        }
    }
}

/// The window pixels a frame changes against the frame the renderer's
/// composition target kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FrameDamage {
    /// The frame drew straight into the presentable image, which keeps no
    /// earlier frame, or was a screenshot, drawn whole.
    #[default]
    Untracked,
    /// Recent frames changed most of the window, so the frame skipped
    /// comparing.
    Skipped,
    /// The whole window: the target kept no frame to compare with, or the
    /// frame changes most of it.
    Full,
    /// Only the pixels of these rects; none when nothing changes.
    Part(WindowRects),
}

/// Up to four rects of window pixels that share no pixel, each as
/// `[x, y, width, height]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowRects {
    rects: [[u32; 4]; crate::frame::MAX_DAMAGE_RECTS],
    len: usize,
}

impl WindowRects {
    /// The rects.
    pub fn rects(&self) -> &[[u32; 4]] {
        &self.rects[..self.len]
    }

    /// Whether the pixel at `x`, `y` lies in one of the rects.
    pub fn contains(&self, x: u32, y: u32) -> bool {
        self.rects().iter().any(|&[left, top, width, height]| {
            x >= left && y >= top && x < left + width && y < top + height
        })
    }

    /// The pixels of all the rects.
    pub fn pixels(&self) -> u64 {
        self.rects()
            .iter()
            .map(|&[_, _, width, height]| u64::from(width) * u64::from(height))
            .sum()
    }

    /// Adds `rect`, past the rects held: the renderer's damage holds no
    /// more than four.
    pub(crate) fn push(&mut self, rect: [u32; 4]) {
        if let Some(slot) = self.rects.get_mut(self.len) {
            *slot = rect;
            self.len += 1;
        }
    }
}

/// The counters of one drawn frame, from `WgpuRenderer::last_frame_stats`. Pixel counts are in
/// device pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameStatsSnapshot {
    /// Submissions to the GPU queue during the frame.
    pub submits: u32,
    /// Command encoders the frame recorded.
    pub encoder_count: u32,
    /// Submissions to the GPU queue, as counted from the recorded commands; equals `submits`.
    pub submit_count: u32,
    /// Render passes the frame recorded.
    pub pass_count: u32,
    /// Sum of every render pass's first color target area: the tile traffic a
    /// tiling GPU pays whatever the passes draw.
    pub pass_pixels: u64,
    /// Texture regions copied outside any pass, and their texels.
    pub copy_count: u32,
    /// Texels of those copies.
    pub copy_pixels: u64,
    /// Offscreen textures the frame took from the pool or created.
    pub offscreen_acquires: u32,
    /// Of the acquired offscreen textures, those created because the pool held none that fit.
    pub offscreen_news: u32,
    /// Bytes of every offscreen texture the frame acquired, new or reused.
    pub offscreen_total_bytes: u64,
    /// Bytes of the textures the frame used for one frame only.
    pub transient_texture_bytes: u64,
    /// Bytes of the textures kept between frames: the offscreen pool and the layer cache.
    pub retained_texture_bytes: u64,
    /// Bytes the frame uploaded to the GPU.
    pub upload_bytes: u64,
    /// Buffer writes the frame staged: uploads that reach the GPU as one
    /// write each, however many draws they carry.
    pub upload_writes: u32,
    /// Layers the frame drew into an offscreen texture of their own.
    pub isolated_layer_renders: u32,
    /// Pixels of the offscreen textures of those layers.
    pub isolated_layer_pixels: u64,
    /// Layers drawn from the layer cache instead of drawn again.
    pub layer_cache_hits: u32,
    /// Capture passes that replayed ops not yet on the page.
    pub capture_fixup_passes: u32,
    /// Layers drawn anew and stored in the layer cache, because it held no match.
    pub layer_cache_misses: u32,
    /// Pixels of the layers drawn from the layer cache.
    pub layer_cache_hit_pixels: u64,
    /// Pixels of the layers drawn anew and stored in the layer cache.
    pub layer_cache_miss_pixels: u64,
    /// Hits per key kind, indexed by `LayerRasterCacheKey::kind_slot`.
    pub layer_cache_hits_by_kind: [u32; LAYER_RASTER_CACHE_KIND_COUNT],
    /// Stores per key kind, indexed by `LayerRasterCacheKey::kind_slot`.
    pub layer_cache_misses_by_kind: [u32; LAYER_RASTER_CACHE_KIND_COUNT],
    /// Stored pixels per key kind, indexed by `LayerRasterCacheKey::kind_slot`.
    pub layer_cache_miss_pixels_by_kind: [u64; LAYER_RASTER_CACHE_KIND_COUNT],
    /// Shadows drawn from a cached blurred texture.
    pub shadow_shape_cache_hits: u32,
    /// Shadows blurred anew because the shadow cache held no match.
    pub shadow_shape_cache_misses: u32,
    /// Pixels composited from the cached shadow textures.
    pub shadow_shape_cache_hit_pixels: u64,
    /// Pixels of the shadow textures blurred anew.
    pub shadow_shape_cache_miss_pixels: u64,
    /// Shadows whose source holds more than shapes, such as text, that found no cached texture and
    /// were blurred anew.
    pub shadow_text_blur_fallbacks: u32,
    /// Blur passes the frame encoded, each downsample, horizontal and vertical pass counting one.
    pub blur_passes: u32,
    /// Backdrop stages resolved: every stage flushes the page and captures
    /// once, so the layer pass count is one more than this.
    pub stages: u32,
    /// Backdrops whose stage texture the layer cache pinned this frame, so
    /// later frames replay their composites from it.
    pub backdrop_admissions: u32,
    /// Opaque page prefixes copied into the layer cache this frame.
    pub prefix_admissions: u32,
    /// Captures averaged into a quarter-size substrate for a shader's wide
    /// taps.
    pub substrates: u32,
    /// Passes that draw at least one resolved composite: a final pass holding
    /// twelve glass tails counts once, a composite resolved into its own
    /// texture counts its own pass.
    pub composite_passes: u32,
    /// Render effects, such as offsets and runtime shaders, the frame applied to an offscreen
    /// source.
    pub effect_applies: u32,
    /// Pixels shaded by runtime shader composites: what every glass surface
    /// costs a tiling GPU per frame, each capture that re-shades one included.
    pub shader_pixels: u64,
    /// Pixels the runtime shader draws rasterize once each draw is split into its interior and rim
    /// regions.
    pub glass_rasterized_pixels: u64,
    /// Pixels the blur passes write: every downsample, horizontal and
    /// vertical pass of a backdrop's blur and of its substrates, within
    /// the scissor the pass was given.
    pub blur_pixels: u64,
    /// Pixels the shape draws rasterize: a band drawn as a mesh counts its
    /// mesh, everything else its quad. Fill rate is what a scene of rings
    /// costs a tiling GPU, however few draws it takes.
    pub shape_fill_pixels: u64,
    /// `shape_fill_pixels` split by shape kind and brush, in the order of
    /// `ShapeFill::LABELS`: which records a fill-bound frame pays for.
    pub shape_fill_pixels_by_class: [u64; ShapeFill::CLASSES],
    /// Vertices the shape draws run through the vertex stage: six per quad,
    /// six per band segment. A tiling GPU stores every vertex's varyings
    /// before it shades a pixel, so this bounds a frame as fill does.
    pub shape_vertices: u64,
    /// Batches of shape draws the frame recorded.
    pub shape_passes: u32,
    /// Shape draws that took a stand-in while their pipeline compiled: a
    /// general pipeline, or triangles the CPU laid out.
    pub shape_pipeline_fallback_draws: u32,
    /// Shape draws using a completed specialized pipeline.
    pub shape_specialized_draws: u32,
    /// Shape draws laying opaque interiors down in a pass's depth pre-pass.
    pub shape_interior_draws: u32,
    /// Render passes that took a depth buffer for their opaque interiors.
    pub depth_passes: u32,
    /// Pipelines this process has built, counted since it started rather than
    /// over the frame. A count that grows while a person is interacting names
    /// a backend shader compile they waited through.
    pub pipelines_created: u64,
    /// Runtime shader draws using a shader's general pipeline while the
    /// specialization they asked for compiles in the background.
    pub shader_pipeline_fallback_draws: u32,
    /// Runtime shader draws using a completed specialized pipeline.
    pub shader_specialized_draws: u32,
    /// Effects drawn as their placeholder, or left out, because no pipeline
    /// they need had compiled yet.
    pub placeholder_draws: u32,
    /// Batches of image draws the frame recorded.
    pub image_passes: u32,
    /// Batches of text draws the frame recorded.
    pub text_passes: u32,
    /// Shape, image, glyph and composite draws recorded this frame. The
    /// `*_passes` counters above count *batches* or passes, so a single image
    /// batch reports `image_passes=1` however many images it draws; this
    /// counts what the driver actually sees. Blur and effect quads are not
    /// included: they are one draw each and already counted by `blur_passes`
    /// and `effect_applies`.
    pub draw_calls: u32,
    /// The window pixels the frame changes against the frame the
    /// renderer's composition target kept.
    pub damage: FrameDamage,
    /// The window pixels the frame drew again: all of them, or those of the
    /// damage, over what the frame before left in the rest.
    pub repainted_pixels: u64,
    /// Texts drawn from the cache of rasterized text images.
    pub text_image_cache_hits: u32,
    /// Texts rasterized into an image because the cache held none.
    pub text_image_cache_misses: u32,
    /// Pixels of the text images taken from the cache.
    pub text_image_cache_hit_pixels: u64,
    /// Pixels of the text images rasterized.
    pub text_image_cache_miss_pixels: u64,
    /// Bytes of the text images rasterized, four per pixel.
    pub text_image_raster_bytes: u64,
    /// Glyphs found in the glyph atlas.
    pub text_glyph_atlas_hits: u32,
    /// Glyphs rasterized and added to the glyph atlas.
    pub text_glyph_atlas_misses: u32,
    /// Pixels of the glyphs added to the glyph atlas.
    pub text_glyph_atlas_miss_pixels: u64,
    /// Texts whose glyph runs the frame laid out again: a text drawn as it
    /// was before takes its run from the cache.
    pub text_glyph_runs_collected: u32,
    /// Textures held for reuse at the end of the frame, with the composition target.
    pub offscreen_pool_size: u32,
    /// Bytes of the textures counted by `offscreen_pool_size`.
    pub offscreen_pool_bytes: u64,
    /// Rasterized text images in the text image cache at the end of the frame.
    pub text_pool_size: u32,
    /// Layers in the layer cache at the end of the frame.
    pub layer_cache_size: u32,
    /// Bytes of the layer cache's textures at the end of the frame.
    pub layer_cache_bytes: u64,
    /// Image textures cached at the end of the frame.
    pub image_cache_size: u32,
    /// The largest isolated layers of the frame by pixel area, largest first, in the first
    /// `top_isolated_layer_count` entries; the rest are `None`.
    pub top_isolated_layers: [Option<IsolatedLayerStat>; TOP_ISOLATED_LAYER_LIMIT],
    /// How many entries of `top_isolated_layers` are filled, at most eight.
    pub top_isolated_layer_count: usize,
}

impl FrameStatsSnapshot {
    fn shape_fill_by_class_text(&self) -> String {
        let mut text = String::new();
        for (label, pixels) in ShapeFill::LABELS
            .iter()
            .zip(self.shape_fill_pixels_by_class)
        {
            if pixels > 0 {
                use std::fmt::Write;
                let _ = write!(text, " {label}={:.2}MP", pixels as f64 / 1_000_000.0);
            }
        }
        text
    }

    pub(crate) fn with_command_stats_added(mut self, stats: FrameCommandStats) -> Self {
        self.submits = self.submits.saturating_add(stats.submit_count);
        self.encoder_count = self.encoder_count.saturating_add(stats.encoder_count);
        self.submit_count = self.submit_count.saturating_add(stats.submit_count);
        self.pass_count = self.pass_count.saturating_add(stats.pass_count);
        self.pass_pixels = self.pass_pixels.saturating_add(stats.pass_pixels);
        self.copy_count = self.copy_count.saturating_add(stats.copy_count);
        self.copy_pixels = self.copy_pixels.saturating_add(stats.copy_pixels);
        self.transient_texture_bytes = self
            .transient_texture_bytes
            .saturating_add(stats.transient_texture_bytes);
        self.retained_texture_bytes = self
            .retained_texture_bytes
            .max(stats.retained_texture_bytes);
        self.upload_bytes = self.upload_bytes.saturating_add(stats.upload_bytes);
        self.upload_writes = self.upload_writes.saturating_add(stats.upload_writes);
        self.offscreen_acquires = self
            .offscreen_acquires
            .saturating_add(stats.transient_acquires);
        self.offscreen_news = self.offscreen_news.saturating_add(stats.transient_news);
        self
    }

    /// The largest isolated surface of the frame as `WxH@(x,y)` in device
    /// pixels and logical origin, or `-`.
    fn top_isolated_display(&self) -> String {
        match &self.top_isolated_layers[0] {
            Some(top) => format!(
                "{}x{}@({:.0},{:.0})",
                top.width, top.height, top.logical_rect.x, top.logical_rect.y
            ),
            None => "-".to_string(),
        }
    }

    /// The largest isolated layers of the frame by pixel area, largest first.
    pub fn top_isolated_layers(self) -> impl Iterator<Item = IsolatedLayerStat> {
        self.top_isolated_layers
            .into_iter()
            .flatten()
            .take(self.top_isolated_layer_count)
    }

    fn layer_cache_hit_rate(self) -> f64 {
        let total = self.layer_cache_hits + self.layer_cache_misses;
        if total > 0 {
            (self.layer_cache_hits as f64 / total as f64) * 100.0
        } else {
            0.0
        }
    }

    fn miss_pixels_by_kind_display(&self) -> String {
        by_kind_display(&self.layer_cache_miss_pixels_by_kind, |pixels| {
            format!("{:.2}MP", pixels as f64 / 1_000_000.0)
        })
    }

    fn hits_by_kind_display(&self) -> String {
        by_kind_display(&self.layer_cache_hits_by_kind, |hits| hits.to_string())
    }

    fn print(self, frame_count: u64) {
        let mb = self.offscreen_total_bytes as f64 / (1024.0 * 1024.0);
        let upload_mb = self.upload_bytes as f64 / (1024.0 * 1024.0);
        let retained_mb = self.retained_texture_bytes as f64 / (1024.0 * 1024.0);
        let pool_mb = self.offscreen_pool_bytes as f64 / (1024.0 * 1024.0);
        let layer_cache_hit_mpx = self.layer_cache_hit_pixels as f64 / 1_000_000.0;
        let layer_cache_miss_mpx = self.layer_cache_miss_pixels as f64 / 1_000_000.0;
        let shadow_cache_hit_mpx = self.shadow_shape_cache_hit_pixels as f64 / 1_000_000.0;
        let shadow_cache_miss_mpx = self.shadow_shape_cache_miss_pixels as f64 / 1_000_000.0;
        let layer_cache_mb = self.layer_cache_bytes as f64 / (1024.0 * 1024.0);
        let isolated_layer_mpx = self.isolated_layer_pixels as f64 / 1_000_000.0;
        stats_line!(
            "[GPU f#{}] encoders={} submits={} passes={} pass_px={:.2}MP copies={} copy_px={:.2}MP | offscreen: acq={} new={} {:.1}MB pool={}({:.1}MB) retained={:.1}MB | \
             uploads={:.2}MB writes={} | \
             isolated_layers={} area={:.2}MP top={} | \
             layer_cache: hit={} miss={} {:.1}% hit_px={:.2}MP miss_px={:.2}MP size={}({:.1}MB) hit_by_kind={} miss_px_by_kind={} | \
             shadow_cache: shape_hit={} shape_miss={} hit_px={:.2}MP miss_px={:.2}MP text_blur_fallback={} | \
             stages={} admit={} blur={} substrate={} composite={} effect={} shader_px={:.2}MP glass_raster_px={:.2}MP blur_px={:.2}MP | shape={} shape_fill_px={:.2}MP{} shape_verts={} interiors={} depth_passes={} image={} text={} draws={} | \
             text_img_cache: hit={} miss={} hit_px={:.2}MP miss_px={:.2}MP raster={:.2}MB | \
             text_glyph_atlas: hit={} miss={} miss_px={:.2}MP runs_collected={} | \
             caches: text_pool={} img={}",
            frame_count,
            self.encoder_count,
            self.submit_count,
            self.pass_count,
            self.pass_pixels as f64 / 1_000_000.0,
            self.copy_count,
            self.copy_pixels as f64 / 1_000_000.0,
            self.offscreen_acquires,
            self.offscreen_news,
            mb,
            self.offscreen_pool_size,
            pool_mb,
            retained_mb,
            upload_mb,
            self.upload_writes,
            self.isolated_layer_renders,
            isolated_layer_mpx,
            self.top_isolated_display(),
            self.layer_cache_hits,
            self.layer_cache_misses,
            self.layer_cache_hit_rate(),
            layer_cache_hit_mpx,
            layer_cache_miss_mpx,
            self.layer_cache_size,
            layer_cache_mb,
            self.hits_by_kind_display(),
            self.miss_pixels_by_kind_display(),
            self.shadow_shape_cache_hits,
            self.shadow_shape_cache_misses,
            shadow_cache_hit_mpx,
            shadow_cache_miss_mpx,
            self.shadow_text_blur_fallbacks,
            self.stages,
            self.backdrop_admissions,
            self.blur_passes,
            self.substrates,
            self.composite_passes,
            self.effect_applies,
            self.shader_pixels as f64 / 1_000_000.0,
            self.glass_rasterized_pixels as f64 / 1_000_000.0,
            self.blur_pixels as f64 / 1_000_000.0,
            self.shape_passes,
            self.shape_fill_pixels as f64 / 1_000_000.0,
            self.shape_fill_by_class_text(),
            self.shape_vertices,
            self.shape_interior_draws,
            self.depth_passes,
            self.image_passes,
            self.text_passes,
            self.draw_calls,
            self.text_image_cache_hits,
            self.text_image_cache_misses,
            self.text_image_cache_hit_pixels as f64 / 1_000_000.0,
            self.text_image_cache_miss_pixels as f64 / 1_000_000.0,
            self.text_image_raster_bytes as f64 / (1024.0 * 1024.0),
            self.text_glyph_atlas_hits,
            self.text_glyph_atlas_misses,
            self.text_glyph_atlas_miss_pixels as f64 / 1_000_000.0,
            self.text_glyph_runs_collected,
            self.text_pool_size,
            self.image_cache_size,
        );
        for (index, layer) in self.top_isolated_layers().enumerate() {
            stats_line!(
                "  [isolated #{index}] node={:?} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{}",
                layer.node_id,
                layer.logical_rect.x,
                layer.logical_rect.y,
                layer.logical_rect.width,
                layer.logical_rect.height,
                layer.width,
                layer.height,
            );
        }
    }
}

static GPU_STATS: DebugToggle = DebugToggle::new("CRANPOSE_GPU_STATS");

pub(crate) fn gpu_stats_enabled() -> bool {
    GPU_STATS.flag()
}

pub(crate) fn print_gpu_memory_report(device: &wgpu::Device, frame_count: u64) {
    let Some(report) = device.generate_allocator_report() else {
        return;
    };

    const MB: f64 = 1024.0 * 1024.0;
    let mut blocks = String::new();
    for block in &report.blocks {
        if !blocks.is_empty() {
            blocks.push('+');
        }
        blocks.push_str(&format!("{:.1}", block.size as f64 / MB));
    }

    stats_line!(
        "[GPU-MEM f#{}] reserved={:.1}MB allocated={:.1}MB blocks={}[{}MB] allocations={} | largest={:.6?}",
        frame_count,
        report.total_reserved_bytes as f64 / MB,
        report.total_allocated_bytes as f64 / MB,
        report.blocks.len(),
        blocks,
        report.allocations.len(),
        report,
    );
}

static SHADOW_CACHE_DIAG: DebugToggle = DebugToggle::new("CRANPOSE_GPU_SHADOW_CACHE_DIAG");

fn shadow_cache_diagnostics_enabled() -> bool {
    SHADOW_CACHE_DIAG.flag()
}

fn by_kind_display<T: Copy + Default + PartialEq>(
    by_kind: &[T; LAYER_RASTER_CACHE_KIND_COUNT],
    format: impl Fn(T) -> String,
) -> String {
    LAYER_RASTER_CACHE_KIND_LABELS
        .iter()
        .zip(by_kind)
        .filter(|(_, value)| **value != T::default())
        .map(|(label, value)| format!("{label}={}", format(*value)))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
#[path = "tests/gpu_stats_tests.rs"]
mod tests;
