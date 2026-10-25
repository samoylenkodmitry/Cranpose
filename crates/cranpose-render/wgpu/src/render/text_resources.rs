//! The renderer's text state: its fonts, the caches from text to pixels, the
//! glyph atlas and the scratch the text draws reuse.

use std::{borrow::Cow, cell::Cell, hash::Hasher, rc::Rc};

use cranpose_core::collections::bounded_lru::BoundedLruCache;
use cranpose_render_common::software_text_raster::{
    SoftwareGlyphAtlasPlacement, SoftwareGlyphAtlasRunGlyph, SoftwareGlyphRasterCache,
    SoftwareTextFontSet, TextPixels, collect_solid_text_atlas_run, rasterize_text,
};
use cranpose_ui_graphics::{FxHasher, ImageBitmap, Point, Rect};

use super::{
    GlyphAtlasSlotKey, GlyphRunBounds, GlyphRunQuads, MAX_TEXT_GLYPH_GPU_RUN_CACHE_ITEMS,
    MAX_TEXT_GLYPH_MASK_CACHE_ITEMS, MAX_TEXT_IMAGE_CACHE_ITEMS, MAX_TEXT_LINE_INDEX_CACHE_ITEMS,
    MIN_TEXT_GLYPH_RUN_CACHE_ITEMS, TEXT_GLYPH_GPU_RUN_IDLE_FRAMES, TEXT_GLYPH_RUN_IDLE_FRAMES,
    TextGlyphRunLookup, TextRasterSource, clipped_text_raster_source_with_line_starts, evict_idle,
    glyph_atlas::{GlyphAtlasEntry, TextGlyphAtlas},
    log_text_atlas_fallback,
    text_glyph_cache::{
        CachedGpuTextGlyphRun, CachedTextGlyphRun, CachedTextImage, TextGlyphRunCacheKey,
        TextImageCacheKey, TextLineIndexCache,
    },
};
use crate::{
    frame_graph::UploadMode,
    glyph_run::{RunGlyphScratch, RunGlyphs},
    glyph_run_arena::GlyphRunArena,
    gpu_stats,
    scene::TextDraw,
};

pub(super) struct TextResources {
    pub(super) fonts: SoftwareTextFontSet,
    pub(super) image_cache: BoundedLruCache<TextImageCacheKey, CachedTextImage>,
    pub(super) glyph_atlas: TextGlyphAtlas,
    pub(super) glyph_run_cache: BoundedLruCache<TextGlyphRunCacheKey, CachedTextGlyphRun>,
    pub(super) glyph_gpu_run_cache:
        BoundedLruCache<TextGlyphRunCacheKey, Rc<CachedGpuTextGlyphRun>>,
    pub(super) glyph_run_arena: GlyphRunArena,
    pub(super) glyph_run_frame: u64,
    pub(super) glyph_runs_drawn: usize,
    pub(super) glyph_mask_cache: SoftwareGlyphRasterCache,
    pub(super) line_index_cache: TextLineIndexCache,
    pub(super) scratch_glyph_run: Vec<SoftwareGlyphAtlasRunGlyph>,
    pub(super) scratch_glyph_entries: Vec<GlyphAtlasEntry>,
    pub(super) run_glyph_scratch: RunGlyphScratch,
}

impl TextResources {
    pub(super) fn new(
        fonts: SoftwareTextFontSet,
        glyph_atlas: TextGlyphAtlas,
        copy_free: UploadMode,
    ) -> Self {
        Self {
            fonts,
            image_cache: BoundedLruCache::with_capacity_at_least_one(MAX_TEXT_IMAGE_CACHE_ITEMS),
            glyph_atlas,
            glyph_run_cache: BoundedLruCache::with_capacity_at_least_one(
                MIN_TEXT_GLYPH_RUN_CACHE_ITEMS,
            ),
            glyph_gpu_run_cache: BoundedLruCache::with_capacity_at_least_one(
                MAX_TEXT_GLYPH_GPU_RUN_CACHE_ITEMS,
            ),
            glyph_run_arena: GlyphRunArena::new(copy_free),
            glyph_run_frame: 0,
            glyph_runs_drawn: 0,
            glyph_mask_cache: SoftwareGlyphRasterCache::with_capacity_at_least_one(
                MAX_TEXT_GLYPH_MASK_CACHE_ITEMS,
            ),
            line_index_cache: TextLineIndexCache::new(MAX_TEXT_LINE_INDEX_CACHE_ITEMS),
            scratch_glyph_run: Vec::new(),
            scratch_glyph_entries: Vec::new(),
            run_glyph_scratch: RunGlyphScratch::default(),
        }
    }

    /// Opens a frame for text runs: runs no frame drew for
    /// [`TEXT_GLYPH_GPU_RUN_IDLE_FRAMES`] leave the GPU cache and those no
    /// frame drew for [`TEXT_GLYPH_RUN_IDLE_FRAMES`] the CPU cache, and the
    /// arena takes back the quads dropped retained runs held.
    pub(super) fn begin_text_glyph_run_frame(&mut self) {
        #[cfg(feature = "installed-fonts")]
        if let Some(fonts) = self.fonts.with_installed_faces() {
            // Text drawn before the install may show a missing glyph where an
            // installed face now has one.
            self.fonts = fonts;
            self.image_cache.clear();
            self.glyph_run_cache.clear();
            self.glyph_gpu_run_cache.clear();
        }
        self.glyph_run_frame += 1;
        let frame = self.glyph_run_frame;
        let demand = std::mem::take(&mut self.glyph_runs_drawn).saturating_mul(2);
        self.glyph_run_cache.set_cap(
            std::num::NonZeroUsize::new(demand.max(MIN_TEXT_GLYPH_RUN_CACHE_ITEMS))
                .unwrap_or(std::num::NonZeroUsize::MIN),
        );
        evict_idle(
            &mut self.glyph_gpu_run_cache,
            frame,
            TEXT_GLYPH_GPU_RUN_IDLE_FRAMES,
            |run| run.last_frame.get(),
        );
        evict_idle(
            &mut self.glyph_run_cache,
            frame,
            TEXT_GLYPH_RUN_IDLE_FRAMES,
            |run| run.last_frame.get(),
        );
        self.glyph_run_arena.begin_frame();
        let evacuating = self.glyph_run_arena.evacuating();
        if !evacuating.is_empty() {
            let moving: Vec<TextGlyphRunCacheKey> = self
                .glyph_gpu_run_cache
                .iter()
                .filter(|(_, run)| evacuating.contains(&run.span.chunk()))
                .map(|(key, _)| *key)
                .collect();
            for key in &moving {
                self.glyph_gpu_run_cache.pop(key);
            }
        }
    }

    /// What every pixel of a frame depends on besides its items: the clear
    /// colour, the glyph atlas and the debug switches.
    pub(super) fn frame_setting(&self, clear: wgpu::Color) -> u64 {
        let mut state = FxHasher::default();
        for channel in [clear.r, clear.g, clear.b, clear.a] {
            state.write_u64(channel.to_bits());
        }
        state.write_u64(self.glyph_atlas.generation());
        state.write_u64(cranpose_render_common::debug_toggles::debug_toggle_generation());
        state.finish()
    }

    pub(super) fn rasterize_text_draw_to_image(
        &mut self,
        text_draw: &TextDraw,
        raster_rect: Rect,
        source_origin: Point,
        text_scale: f32,
    ) -> Option<ImageBitmap> {
        rasterize_text(
            text_draw
                .raster_request(raster_rect, text_scale)
                .with_origin(if text_draw.text.string().span_styles().is_empty() {
                    Point::new(raster_rect.x, raster_rect.y)
                } else {
                    source_origin
                }),
            &self.fonts,
            Some(&mut self.glyph_mask_cache),
        )
        .and_then(TextPixels::into_image)
    }

    pub(super) fn retained_text_glyph_run(
        &mut self,
        cache_key: TextGlyphRunCacheKey,
    ) -> Option<Rc<CachedGpuTextGlyphRun>> {
        let atlas_generation = self.glyph_atlas.generation();
        let frame = self.glyph_run_frame;
        self.glyph_gpu_run_cache
            .get(&cache_key)
            .filter(|cached| cached.atlas_generation == atlas_generation)
            .inspect(|cached| cached.last_frame.set(frame))
            .cloned()
    }

    pub(super) fn text_image_raster_source<'a>(
        &mut self,
        text_draw: &'a TextDraw,
        logical_rect: Rect,
        raster_rect: Rect,
        clip: Option<Rect>,
        root_scale: f32,
    ) -> TextRasterSource<'a> {
        let Some(clip) = clip else {
            return TextRasterSource {
                draw: Cow::Borrowed(text_draw),
                raster_rect,
            };
        };
        if !text_draw.text.static_motion() || text_draw.text.string().text().find('\n').is_none() {
            return TextRasterSource {
                draw: Cow::Borrowed(text_draw),
                raster_rect,
            };
        }

        if !text_draw.text.string().span_styles().is_empty() {
            let device_clip = Rect {
                x: (clip.x * root_scale).floor(),
                y: (clip.y * root_scale).floor(),
                width: ((clip.x + clip.width) * root_scale).ceil() - (clip.x * root_scale).floor(),
                height: ((clip.y + clip.height) * root_scale).ceil()
                    - (clip.y * root_scale).floor(),
            };
            return TextRasterSource {
                draw: Cow::Borrowed(text_draw),
                raster_rect: raster_rect.intersect(device_clip).unwrap_or(raster_rect),
            };
        }

        let line_starts = self.line_index_cache.line_starts(text_draw.text.string());
        clipped_text_raster_source_with_line_starts(
            text_draw,
            logical_rect,
            raster_rect,
            clip,
            root_scale,
            line_starts.as_ref(),
        )
    }

    /// The run of `text_draw` from the cache, or collected into
    /// `collected_run` and cached; `None` when the text cannot draw from the
    /// atlas.
    pub(super) fn text_glyph_run(
        &mut self,
        text_draw: &TextDraw,
        raster_rect: Rect,
        text_scale: f32,
        run_key: TextGlyphRunCacheKey,
        collected_run: &mut Vec<SoftwareGlyphAtlasRunGlyph>,
        frame_stats: &gpu_stats::FrameStats,
    ) -> Option<TextGlyphRunLookup> {
        let atlas_generation = self.glyph_atlas.generation();
        let frame = self.glyph_run_frame;
        self.glyph_runs_drawn += 1;
        if let Some(cached) = self.glyph_run_cache.get(&run_key) {
            cached.last_frame.set(frame);
            return Some(TextGlyphRunLookup {
                glyphs: cached.glyphs.clone(),
                bounds: cached.bounds,
                cached: true,
                entries: (cached.atlas_generation == atlas_generation)
                    .then(|| cached.atlas_entries.as_ref().map(Rc::clone))
                    .flatten(),
            });
        }
        collected_run.clear();
        if collect_solid_text_atlas_run(
            text_draw.raster_request(raster_rect, text_scale),
            &self.fonts,
            &mut self.glyph_mask_cache,
            collected_run,
        )
        .is_none()
        {
            log_text_atlas_fallback(text_draw);
            return None;
        }
        let Some(glyphs) = RunGlyphs::of(
            collected_run
                .iter()
                .map(SoftwareGlyphAtlasRunGlyph::placement),
            &mut self.run_glyph_scratch,
        ) else {
            log_text_atlas_fallback(text_draw);
            return None;
        };
        let bounds = GlyphRunBounds::of(glyphs.iter());
        frame_stats.record_text_glyph_run_collected();
        // A run this frame drew stays: the cache grows rather than evict it.
        let cache = &mut self.glyph_run_cache;
        if cache.len() == cache.cap().get()
            && cache
                .peek_lru()
                .is_some_and(|(_, run)| run.last_frame.get() == frame)
        {
            cache.set_cap(cache.cap().saturating_add(cache.len()));
        }
        self.glyph_run_cache.put(
            run_key,
            CachedTextGlyphRun {
                glyphs: glyphs.clone(),
                bounds,
                atlas_entries: None,
                atlas_generation: 0,
                last_frame: Cell::new(frame),
            },
        );
        Some(TextGlyphRunLookup {
            glyphs,
            bounds,
            cached: false,
            entries: None,
        })
    }

    pub(super) fn ensure_retained_text_glyph_run(
        &mut self,
        cache_key: TextGlyphRunCacheKey,
        quads: GlyphRunQuads<'_>,
        device: &wgpu::Device,
    ) -> bool {
        let atlas_generation = self.glyph_atlas.generation();
        if self
            .glyph_gpu_run_cache
            .peek(&cache_key)
            .is_some_and(|cached| cached.atlas_generation == atlas_generation)
        {
            return true;
        }

        let Some(span) = self
            .glyph_run_arena
            .insert(device, quads.instances_at([0.0, 0.0]))
        else {
            return false;
        };
        self.glyph_gpu_run_cache.put(
            cache_key,
            Rc::new(CachedGpuTextGlyphRun {
                span,
                atlas_generation,
                last_frame: Cell::new(self.glyph_run_frame),
            }),
        );
        true
    }

    pub(super) fn glyph_atlas_entry_for_cached(
        &mut self,
        glyph: &SoftwareGlyphAtlasPlacement,
        frame_stats: &gpu_stats::FrameStats,
    ) -> Option<GlyphAtlasEntry> {
        let entry = self
            .glyph_atlas
            .entry(&GlyphAtlasSlotKey::new(glyph.key, glyph.color))?;
        frame_stats.record_text_glyph_atlas_hits(1);
        Some(entry)
    }
}
