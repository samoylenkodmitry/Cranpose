//! The renderer's text state: the caches from laid-out text to pixels, the
//! glyph atlas and the scratch the text draws reuse.

use std::{
    cell::Cell,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{collections::bounded_lru::BoundedLruCache, hash::default as default_hash};
use cranpose_render_common::{
    glyph_mask::GlyphMaskCache,
    text_atlas::{AtlasGlyph, collect_solid_text_atlas_run},
    text_canvas::TextPixels,
    text_raster::rasterize_text,
};
use cranpose_ui_graphics::{FxHasher, ImageBitmap, ImageSampling, Point, Rect};

use super::{
    DrawSpace, FrameUploads, GlyphAtlasSlotKey, GlyphDrawCmd, GlyphExtent, GlyphInstances,
    GlyphQuads, GlyphRunBounds, GlyphRunQuads, GlyphSamplers, ImageBatch, ImageTextures,
    MAX_TEXT_GLYPH_GPU_RUN_CACHE_ITEMS, MAX_TEXT_GLYPH_MASK_CACHE_ITEMS,
    MAX_TEXT_IMAGE_CACHE_ITEMS, MIN_TEXT_GLYPH_RUN_CACHE_ITEMS, RETAINED_TEXT_GLYPH_RUN_MIN_QUADS,
    RunAtlasEntries, TEXT_GLYPH_GPU_RUN_IDLE_FRAMES, TEXT_GLYPH_RUN_IDLE_FRAMES, TextGlyphRunDraw,
    TextGlyphRunLookup, TurnedGlyph, ViewportUniformParams, evict_idle, for_each_visible_glyph,
    glyph_atlas::{GlyphAtlasEntry, TextGlyphAtlas},
    glyph_cut_edges, glyph_rect_aligned, hash_text_draw_for_cache,
    hash_text_raster_geometry_for_cache, log_text_atlas_fallback, sampling_under,
    scissor_rect_for_layer, shared_glyph_clip, text_draw_is_visible_in_viewport,
    text_glyph_cache::{
        CachedGpuTextGlyphRun, CachedTextGlyphRun, CachedTextImage, TextGlyphRunCacheKey,
        TextImageCacheKey,
    },
    text_raster_geometry_at, text_raster_geometry_for_draw,
};
use crate::{
    frame_graph::UploadMode,
    glyph_run::{RunGlyphScratch, RunGlyphs},
    glyph_run_arena::GlyphRunArena,
    gpu_stats::FrameStats,
    scene::TextDraw,
};

pub(crate) struct TextResources {
    pub(super) image_cache: BoundedLruCache<TextImageCacheKey, CachedTextImage>,
    pub(super) glyph_atlas: TextGlyphAtlas,
    pub(super) glyph_run_cache: BoundedLruCache<TextGlyphRunCacheKey, CachedTextGlyphRun>,
    pub(super) glyph_gpu_run_cache:
        BoundedLruCache<TextGlyphRunCacheKey, Rc<CachedGpuTextGlyphRun>>,
    pub(super) glyph_run_arena: GlyphRunArena,
    pub(super) glyph_run_frame: u64,
    pub(super) glyph_runs_drawn: usize,
    pub(super) glyph_mask_cache: GlyphMaskCache,
    pub(super) scratch_glyph_run: Vec<AtlasGlyph>,
    pub(super) scratch_glyph_entries: Vec<GlyphAtlasEntry>,
    pub(super) run_glyph_scratch: RunGlyphScratch,
}

impl TextResources {
    pub(super) fn new(glyph_atlas: TextGlyphAtlas, copy_free: UploadMode) -> Self {
        Self {
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
            glyph_mask_cache: GlyphMaskCache::with_capacity_at_least_one(
                MAX_TEXT_GLYPH_MASK_CACHE_ITEMS,
            ),
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
        state.write_u64(cranpose_core::debug_toggles::debug_toggle_generation());
        state.finish()
    }

    fn rasterize_text_draw_to_image(
        &mut self,
        text_draw: &TextDraw,
        raster_rect: Rect,
        source_origin: Point,
        text_scale: f32,
    ) -> Option<ImageBitmap> {
        rasterize_text(
            text_draw
                .raster_request(raster_rect, text_scale)
                .with_origin(source_origin),
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

    // The part of `text_draw`'s raster rect to rasterize: a static text cut
    // by `clip` rasterizes only the part the clip shows, its glyphs outside
    // skipped.
    fn text_image_raster_rect(
        text_draw: &TextDraw,
        raster_rect: Rect,
        clip: Option<Rect>,
        root_scale: f32,
    ) -> Rect {
        match clip {
            Some(clip) if text_draw.text.static_motion() => {
                let device_clip = Rect {
                    x: (clip.x * root_scale).floor(),
                    y: (clip.y * root_scale).floor(),
                    width: ((clip.x + clip.width) * root_scale).ceil()
                        - (clip.x * root_scale).floor(),
                    height: ((clip.y + clip.height) * root_scale).ceil()
                        - (clip.y * root_scale).floor(),
                };
                raster_rect.intersect(device_clip).unwrap_or(raster_rect)
            }
            _ => raster_rect,
        }
    }

    /// The run of `text_draw` from the cache, or collected into
    /// `collected_run` and cached; `None` when the text cannot draw from the
    /// atlas.
    fn text_glyph_run(
        &mut self,
        text_draw: &TextDraw,
        raster_rect: Rect,
        text_scale: f32,
        run_key: TextGlyphRunCacheKey,
        collected_run: &mut Vec<AtlasGlyph>,
        frame_stats: &FrameStats,
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
            &mut self.glyph_mask_cache,
            collected_run,
        )
        .is_none()
        {
            log_text_atlas_fallback(text_draw);
            return None;
        }
        let Some(glyphs) =
            RunGlyphs::of(collected_run.iter().copied(), &mut self.run_glyph_scratch)
        else {
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

    fn glyph_atlas_entry_for_cached(
        &mut self,
        glyph: &AtlasGlyph,
        frame_stats: &FrameStats,
    ) -> Option<GlyphAtlasEntry> {
        let entry = self
            .glyph_atlas
            .entry(&GlyphAtlasSlotKey::new(glyph.key, glyph.color))?;
        frame_stats.record_text_glyph_atlas_hits(1);
        Some(entry)
    }

    /// Writes the glyphs the pass placed in the atlas to its texture, ahead
    /// of the submit that draws them.
    pub(crate) fn flush_glyph_uploads(&mut self, upload: &mut FrameUploads<'_>) {
        self.glyph_atlas.flush_uploads(upload.queue, upload.stats);
    }

    // The atlas entry of `glyph`, its mask uploaded from the mask cache when
    // the atlas lacks it. An atlas that fills is reset and the run fails.
    fn glyph_atlas_entry_for_placement(
        &mut self,
        glyph: &AtlasGlyph,
        upload: &mut FrameUploads<'_>,
    ) -> Result<GlyphAtlasEntry, String> {
        if let Some(entry) = self.glyph_atlas_entry_for_cached(glyph, upload.stats) {
            return Ok(entry);
        }
        let Some(mask) = self.glyph_mask_cache.cached(&glyph.key) else {
            return Err("text glyph placement has no retained raster mask".to_string());
        };
        if let Some(entry) = self.glyph_atlas.upload_glyph((glyph, mask), upload.stats) {
            return Ok(entry);
        }

        // The draws queued so far bind the full atlas: the glyphs they placed
        // reach it before a new atlas takes its place.
        self.flush_glyph_uploads(upload);
        self.glyph_atlas.reset(
            &upload.pipelines.device,
            &upload.pipelines.image_bind_group_layout,
            GlyphSamplers {
                nearest: &upload.pipelines.image_nearest_sampler,
                linear: &upload.pipelines.image_linear_sampler,
            },
        );
        Err("text glyph atlas filled and was reset".to_string())
    }

    /// The atlas entries of a run's drawing glyphs: of `cached_glyph_run`,
    /// which holds only those, or of the drawing glyphs of `collected_run`,
    /// a run just collected, whose new glyphs are uploaded here.
    fn prepare_text_glyph_entries(
        &mut self,
        run_key: TextGlyphRunCacheKey,
        atlas_generation: u64,
        cached_glyph_run: Option<&RunGlyphs>,
        collected_run: &[AtlasGlyph],
        entries: &mut Vec<GlyphAtlasEntry>,
        upload: &mut FrameUploads<'_>,
    ) -> Result<Rc<[GlyphAtlasEntry]>, String> {
        entries.clear();
        let mut seen = RunAtlasEntries::new();
        if let Some(glyph_run) = cached_glyph_run {
            for glyph in glyph_run.iter() {
                entries.push(
                    seen.entry((&mut *self, &mut *upload), &glyph, |text, upload| {
                        text.glyph_atlas_entry_for_placement(&glyph, upload)
                    })?,
                );
            }
        } else {
            for glyph in collected_run {
                entries.push(
                    seen.entry((&mut *self, &mut *upload), glyph, |text, upload| {
                        text.glyph_atlas_entry_for_placement(glyph, upload)
                    })?,
                );
            }
        }

        let entries: Rc<[GlyphAtlasEntry]> = Rc::from(entries.as_slice());
        if let Some(cached) = self.glyph_run_cache.get_mut(&run_key) {
            cached.atlas_entries = Some(Rc::clone(&entries));
            cached.atlas_generation = atlas_generation;
        }
        Ok(entries)
    }

    fn append_text_glyph_quad_run(
        source_raster_rect: Rect,
        quads: GlyphRunQuads<'_>,
        (clip, cut): (Option<Rect>, Option<[f32; 4]>),
        space: DrawSpace,
        glyph_instances: &mut GlyphInstances,
        hits: Option<&FrameStats>,
    ) -> (GlyphQuads, GlyphExtent) {
        let (viewport, root_scale) = (space.viewport, space.scale);
        let start = glyph_instances.lens();
        let mut extent = GlyphExtent::EMPTY;
        let origin = [source_raster_rect.x, source_raster_rect.y];
        let bounds = quads.bounds.at(origin);
        let kind = if viewport.transform.is_identity()
            && cut.is_none_or(|edges| {
                bounds[0] >= edges[0]
                    && bounds[1] >= edges[1]
                    && bounds[2] <= edges[2]
                    && bounds[3] <= edges[3]
            })
            && quads
                .bounds
                .within_viewport(source_raster_rect, clip, viewport, root_scale)
        {
            // Every quad shows whole and uncut: they sit at whole pixels from
            // the run's origin, which then decides their alignment, and
            // their extent is the run's bounds.
            glyph_instances.plain.extend(quads.instances_at(origin));
            extent.add(bounds);
            GlyphQuads::plain_at(
                [origin[0], origin[1], origin[0], origin[1]],
                viewport.offset,
            )
        } else if viewport.transform.is_identity() {
            let plain = &mut glyph_instances.plain;
            let mut aligned = true;
            for_each_visible_glyph(
                source_raster_rect,
                quads,
                (clip, cut),
                viewport,
                root_scale,
                |glyph| {
                    aligned &= glyph_rect_aligned(glyph.rect, viewport.offset);
                    extent.add(glyph.rect);
                    plain.push(glyph);
                },
            );
            if aligned {
                GlyphQuads::Aligned
            } else {
                GlyphQuads::Plain
            }
        } else {
            let turn = viewport.transform;
            let turned = &mut glyph_instances.turned;
            for_each_visible_glyph(
                source_raster_rect,
                quads,
                (clip, cut),
                viewport,
                root_scale,
                |glyph| turned.push(TurnedGlyph::new(glyph, turn)),
            );
            GlyphQuads::Turned
        };
        let end = glyph_instances.lens();
        let appended = (end.0 - start.0) + (end.1 - start.1);
        if let Some(stats) = hits {
            stats.record_text_glyph_atlas_hits(u32::try_from(appended).unwrap_or(u32::MAX));
        }
        (kind, extent)
    }

    /// The viewport a retained glyph run draws under: its vertices sit at
    /// its raster rect's origin, so an untransformed target moves its offset
    /// back by that origin, and a transformed one adds the origin to each
    /// vertex before its transform, as the shared path's vertices hold it.
    pub(super) fn retained_glyph_viewport(
        viewport: ViewportUniformParams,
        source_raster_rect: Rect,
    ) -> ViewportUniformParams {
        if !viewport.transform.is_identity() {
            return ViewportUniformParams {
                origin: [source_raster_rect.x, source_raster_rect.y],
                ..viewport
            };
        }
        ViewportUniformParams {
            offset: [
                viewport.offset[0] - source_raster_rect.x,
                viewport.offset[1] - source_raster_rect.y,
            ],
            ..viewport
        }
    }

    pub(super) fn emit_retained_text_glyph_run_if_ready(
        &mut self,
        cache_key: TextGlyphRunCacheKey,
        quads: GlyphRunQuads<'_>,
        (viewport, source_raster_rect): (ViewportUniformParams, Rect),
        scissor: (u32, u32, u32, u32),
        glyph_cmds: &mut Vec<GlyphDrawCmd>,
        upload: &mut FrameUploads<'_>,
    ) -> bool {
        let Some(run) = self.retained_text_glyph_run(cache_key).or_else(|| {
            if self.ensure_retained_text_glyph_run(cache_key, quads, &upload.pipelines.device) {
                self.retained_text_glyph_run(cache_key)
            } else {
                None
            }
        }) else {
            return false;
        };
        let uniform_slot = upload.uniforms.claim_viewport(
            upload.pipelines,
            Self::retained_glyph_viewport(viewport, source_raster_rect),
        );
        upload
            .stats
            .record_text_glyph_atlas_hits(u32::try_from(quads.len()).unwrap_or(u32::MAX));
        // The run's quads sit at whole texels from its raster origin.
        let aligned = viewport.transform.is_identity()
            && glyph_rect_aligned(
                [source_raster_rect.x, source_raster_rect.y, 0.0, 0.0],
                [viewport.offset[0], viewport.offset[1]],
            );
        glyph_cmds.push(GlyphDrawCmd::retained(
            (run, uniform_slot, aligned),
            scissor,
            self.glyph_atlas.bind_group(viewport.transform),
        ));
        true
    }

    /// Appends the glyph atlas draws of `text_draw`, whose logical geometry
    /// `logical` the caller found drawing into a rect `viewport` shows.
    /// `false` when the text cannot draw from the atlas (animated motion, or
    /// a run the atlas cannot hold): nothing was appended, and the caller
    /// draws the text as a rasterized image instead.
    pub(crate) fn append_text_glyph_draws(
        &mut self,
        (text_draw, logical): (&TextDraw, (Rect, f32)),
        viewport: ViewportUniformParams,
        root_scale: f32,
        glyph_instances: &mut GlyphInstances,
        glyph_cmds: &mut Vec<GlyphDrawCmd>,
        upload: &mut FrameUploads<'_>,
    ) -> bool {
        let (_, raster_rect, _, text_scale, static_text_motion) =
            text_raster_geometry_at(text_draw, logical, root_scale);
        if !static_text_motion {
            return false;
        }
        let run_key = Self::text_glyph_run_cache_key(text_draw, raster_rect, text_scale);
        let mut collected_run = std::mem::take(&mut self.scratch_glyph_run);
        let mut generated_entries = std::mem::take(&mut self.scratch_glyph_entries);
        let initial_instance_len = glyph_instances.lens();
        let initial_cmd_len = glyph_cmds.len();
        let drew = self
            .text_glyph_run(
                text_draw,
                raster_rect,
                text_scale,
                run_key,
                &mut collected_run,
                upload.stats,
            )
            .is_some_and(|run| {
                self.append_text_glyph_run(
                    TextGlyphRunDraw {
                        text_draw,
                        raster_rect,
                        run_key,
                        run,
                    },
                    (viewport, root_scale),
                    (&collected_run, &mut generated_entries),
                    glyph_instances,
                    glyph_cmds,
                    upload,
                )
            });
        self.scratch_glyph_run = collected_run;
        self.scratch_glyph_entries = generated_entries;
        if !drew {
            glyph_instances.truncate(initial_instance_len);
            glyph_cmds.truncate(initial_cmd_len);
        }
        drew
    }

    /// Appends the draws of a text's run: its retained quads when it has
    /// them, else its visible quads into the frame's shared glyphs. `false`
    /// when the atlas cannot hold its glyphs.
    fn append_text_glyph_run(
        &mut self,
        draw: TextGlyphRunDraw<'_>,
        (viewport, root_scale): (ViewportUniformParams, f32),
        (collected_run, generated_entries): (&[AtlasGlyph], &mut Vec<GlyphAtlasEntry>),
        glyph_instances: &mut GlyphInstances,
        glyph_cmds: &mut Vec<GlyphDrawCmd>,
        upload: &mut FrameUploads<'_>,
    ) -> bool {
        let TextGlyphRunDraw {
            text_draw,
            raster_rect,
            run_key,
            run,
        } = draw;
        let draw_rect = Rect {
            x: raster_rect.x / root_scale,
            y: raster_rect.y / root_scale,
            width: raster_rect.width / root_scale,
            height: raster_rect.height / root_scale,
        };
        let Some(scissor) = scissor_rect_for_layer(draw_rect, text_draw.clip, root_scale, viewport)
        else {
            return true;
        };
        // A retained run's quads are uploaded whole: under a turn only the
        // shared path cuts them to a clip.
        let clip_needs_cut = !viewport.transform.is_identity() && text_draw.clip.is_some();
        if let Some(entries) = run.entries.as_ref()
            && entries.len() >= RETAINED_TEXT_GLYPH_RUN_MIN_QUADS
            && !clip_needs_cut
            && self.emit_retained_text_glyph_run_if_ready(
                run_key,
                GlyphRunQuads {
                    glyphs: &run.glyphs,
                    entries,
                    texel: self.glyph_atlas.texel(),
                    bounds: run.bounds,
                },
                (viewport, raster_rect),
                scissor,
                glyph_cmds,
                upload,
            )
        {
            return true;
        }

        let turned = !viewport.transform.is_identity();
        let instance_start = glyph_instances.len_for(viewport.transform);
        let (entries, entries_cached) = match run.entries {
            Some(entries) => (entries, true),
            None => {
                let Ok(entries) = self.prepare_text_glyph_entries(
                    run_key,
                    self.glyph_atlas.generation(),
                    run.cached.then_some(&run.glyphs),
                    collected_run,
                    generated_entries,
                    upload,
                ) else {
                    return false;
                };
                (entries, false)
            }
        };
        let quads = GlyphRunQuads {
            glyphs: &run.glyphs,
            entries: &entries,
            texel: self.glyph_atlas.texel(),
            bounds: run.bounds,
        };
        let cut = glyph_cut_edges(text_draw.clip, scissor, viewport, root_scale);
        let (kind, extent) = Self::append_text_glyph_quad_run(
            raster_rect,
            quads,
            (text_draw.clip, cut),
            DrawSpace {
                viewport,
                scale: root_scale,
            },
            glyph_instances,
            entries_cached.then_some(&*upload.stats),
        );
        let instance_end = glyph_instances.len_for(viewport.transform);
        if instance_end > instance_start {
            let (clip, bounds) = if turned {
                (None, scissor)
            } else {
                shared_glyph_clip(extent, scissor, viewport)
            };
            glyph_cmds.push(GlyphDrawCmd::shared(
                (instance_start..instance_end, kind),
                clip,
                bounds,
                self.glyph_atlas.bind_group(viewport.transform),
            ));
        }
        true
    }

    /// Appends `text_draw` as a rasterized image if `viewport` shows it.
    pub(crate) fn append_text_image_draw_cmds(
        &mut self,
        text_draw: &TextDraw,
        space: DrawSpace,
        (images, textures): (&mut ImageBatch, &mut ImageTextures),
        upload: &mut FrameUploads<'_>,
    ) -> Result<(), String> {
        let (viewport, root_scale) = (space.viewport, space.scale);
        let Some((logical_rect, raster_rect, clip, text_scale, static_text_motion)) =
            text_raster_geometry_for_draw(text_draw, root_scale)
        else {
            return Ok(());
        };
        if !text_draw_is_visible_in_viewport(logical_rect, clip, viewport, root_scale) {
            return Ok(());
        }

        let source_draw = text_draw;
        let source_raster_rect =
            Self::text_image_raster_rect(text_draw, raster_rect, clip, root_scale);
        let source_origin = Point::new(raster_rect.x, raster_rect.y);

        let mut cache_key = Self::text_image_cache_key(
            source_draw,
            source_raster_rect,
            text_scale,
            static_text_motion,
        );
        // A part of the text rasterizes its glyphs from the text's corner.
        let mut state = default_hash::new();
        cache_key.0.hash(&mut state);
        (source_origin.x - source_raster_rect.x)
            .to_bits()
            .hash(&mut state);
        (source_origin.y - source_raster_rect.y)
            .to_bits()
            .hash(&mut state);
        cache_key = TextImageCacheKey(state.finish());
        let image = if let Some(cached) = self.image_cache.get(&cache_key) {
            upload
                .stats
                .record_text_image_cache_hit(cached.image.width(), cached.image.height());
            cached.image.clone()
        } else {
            let Some(image) = self.rasterize_text_draw_to_image(
                source_draw,
                source_raster_rect,
                source_origin,
                text_scale,
            ) else {
                return Ok(());
            };
            upload
                .stats
                .record_text_image_cache_miss(image.width(), image.height());
            self.image_cache.put(
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
        images.push_bitmap(
            &image,
            (draw_rect, clip),
            sampling_under(ImageSampling::Nearest, viewport.transform),
            space,
            (textures, upload),
        )
    }

    fn text_image_cache_key(
        text_draw: &TextDraw,
        raster_rect: Rect,
        text_scale: f32,
        static_text_motion: bool,
    ) -> TextImageCacheKey {
        let mut state = default_hash::new();
        hash_text_draw_for_cache(text_draw, raster_rect, text_scale, &mut state);
        hash_text_raster_geometry_for_cache(raster_rect, static_text_motion, &mut state);
        TextImageCacheKey(state.finish())
    }

    /// The key of a static text's glyph run. A run places its glyphs from
    /// the text's origin and aligns its lines within the text block, so the
    /// size of the rect it draws in does not change it: only an empty rect,
    /// which draws nothing, does.
    fn text_glyph_run_cache_key(
        text_draw: &TextDraw,
        raster_rect: Rect,
        text_scale: f32,
    ) -> TextGlyphRunCacheKey {
        let mut state = default_hash::new();
        hash_text_draw_for_cache(text_draw, raster_rect, text_scale, &mut state);
        (raster_rect.width <= 0.0 || raster_rect.height <= 0.0).hash(&mut state);
        TextGlyphRunCacheKey(state.finish())
    }
}
