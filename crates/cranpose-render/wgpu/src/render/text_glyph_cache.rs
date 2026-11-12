use std::{cell::Cell, hash::Hash, rc::Rc};

use cranpose_render_common::text_atlas::AtlasGlyph;
use cranpose_ui_graphics::{ImageBitmap, Rect};

use super::{
    GlyphAtlasEntry, GlyphInstance, ImageUvRect, ViewportUniformParams,
    draw_rect_is_visible_in_viewport, glyph_atlas_uv_rect,
};
use crate::{
    glyph_run::{RunGlyphBox, RunGlyphs},
    glyph_run_arena::GlyphRunSpan,
    scene::TextDraw,
};

pub(super) const MAX_TEXT_IMAGE_CACHE_ITEMS: usize = 1024;

pub(super) const MAX_TEXT_GLYPH_MASK_CACHE_ITEMS: usize = 8192;

/// The fewest text runs the run cache holds. It holds twice the runs the
/// last frame drew when that is more, so a frame never evicts runs the next
/// one draws again; idle runs leave it after `TEXT_GLYPH_RUN_IDLE_FRAMES`.
pub(super) const MIN_TEXT_GLYPH_RUN_CACHE_ITEMS: usize = 1024;

pub(super) const MAX_TEXT_GLYPH_GPU_RUN_CACHE_ITEMS: usize = 1024;

/// Frames a text run may go undrawn before its glyphs and quads are freed,
/// on the CPU and in retained GPU buffers alike. Shorter than the shape
/// store's span: a list scrolling at speed leaves several screens of text
/// behind each second, which at 120 frames held 12 to 19 MB of GPU quads on
/// a scrolling feed, and the CPU runs, bounded only by their count, held
/// another 4.6 MB of placements and quads.
pub(super) const TEXT_GLYPH_RUN_IDLE_FRAMES: u64 = 30;

/// Frames a retained run's quads stay on the GPU undrawn. Writing a run's
/// quads again costs far less than laying its glyphs out, which the CPU run
/// keeps for [`TEXT_GLYPH_RUN_IDLE_FRAMES`]. On the desktop gauntlet at tier
/// 16, 270 retained runs drew each frame while 30 frames held 650 of them;
/// 8 frames hold 400 and write 25 runs a frame again instead of 16.
pub(super) const TEXT_GLYPH_GPU_RUN_IDLE_FRAMES: u64 = 8;

/// Text runs with at least this many glyphs keep their quads in retained GPU
/// buffers and draw on their own; shorter ones are written into the frame's
/// shared quads each frame, where consecutive runs share a draw.
pub(super) const RETAINED_TEXT_GLYPH_RUN_MIN_QUADS: usize = 64;

fn text_atlas_fallback_diag_enabled() -> bool {
    cranpose_core::DIAGNOSTICS.text_atlas_fallback_diag.is_set()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct TextImageCacheKey(pub(super) u64);

pub(super) struct CachedTextImage {
    pub(super) image: ImageBitmap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct TextGlyphRunCacheKey(pub(super) u64);

// One glyph of a run as a quad: where it lies from the run's origin, its
// colour and its place in the atlas.
#[derive(Clone, Copy)]
pub(super) struct CachedTextGlyphQuad {
    pub(super) offset: [f32; 2],
    pub(super) size: [f32; 2],
    pub(super) color: [f32; 4],
    pub(super) uv: ImageUvRect,
}

impl CachedTextGlyphQuad {
    // The quad of a run whose origin lies at `origin`.
    #[inline]
    pub(super) fn at(&self, origin: [f32; 2]) -> GlyphInstance {
        let x0 = origin[0] + self.offset[0];
        let y0 = origin[1] + self.offset[1];
        GlyphInstance {
            rect: [x0, y0, x0 + self.size[0], y0 + self.size[1]],
            uv: [
                self.uv.min[0],
                self.uv.min[1],
                self.uv.max[0],
                self.uv.max[1],
            ],
            uv_bounds: self.uv.sample_bounds,
            color: self.color,
        }
    }
}

pub(super) struct CachedTextGlyphRun {
    /// The run's glyphs that draw: zero-sized and transparent ones are left
    /// out when the run is collected.
    pub(super) glyphs: RunGlyphs,
    pub(super) bounds: GlyphRunBounds,
    /// Where each of `glyphs` sits in the atlas at `atlas_generation`. A
    /// glyph's quad is derived from the two as it is drawn, so no glyph is
    /// held twice.
    pub(super) atlas_entries: Option<Rc<[GlyphAtlasEntry]>>,
    pub(super) atlas_generation: u64,
    /// The frame the run last drew in; see [`TEXT_GLYPH_RUN_IDLE_FRAMES`].
    pub(super) last_frame: Cell<u64>,
}

/// A cached run drawn as quads: its glyphs and their atlas entries, each
/// quad derived as it is read.
#[derive(Clone, Copy)]
pub(super) struct GlyphRunQuads<'a> {
    pub(super) glyphs: &'a RunGlyphs,
    pub(super) entries: &'a [GlyphAtlasEntry],
    /// One atlas texel's share of the atlas side: see
    /// [`TextGlyphAtlas::texel`].
    pub(super) texel: f32,
    pub(super) bounds: GlyphRunBounds,
}

/// The pixel box a run's drawing glyphs cover, from its raster origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GlyphRunBounds {
    pub(super) min: [i32; 2],
    pub(super) max: [i32; 2],
}

impl GlyphRunBounds {
    pub(crate) fn of(glyphs: impl IntoIterator<Item = AtlasGlyph>) -> Self {
        let mut bounds = Self {
            min: [i32::MAX; 2],
            max: [i32::MIN; 2],
        };
        for glyph in glyphs {
            let width = i32::try_from(glyph.width).unwrap_or(i32::MAX);
            let height = i32::try_from(glyph.height).unwrap_or(i32::MAX);
            bounds.min = [bounds.min[0].min(glyph.x), bounds.min[1].min(glyph.y)];
            bounds.max = [
                bounds.max[0].max(glyph.x.saturating_add(width)),
                bounds.max[1].max(glyph.y.saturating_add(height)),
            ];
        }
        bounds
    }

    /// The box the glyphs cover from `origin`, as left, top, right, bottom.
    pub(super) fn at(self, origin: [f32; 2]) -> [f32; 4] {
        [
            origin[0] + self.min[0] as f32,
            origin[1] + self.min[1] as f32,
            origin[0] + self.max[0] as f32,
            origin[1] + self.max[1] as f32,
        ]
    }

    // Whether the run at `source_raster_rect` shows whole and uncut: its
    // viewport is not turned, its box lies inside `cut`, when the text is
    // cut, and inside what `viewport` shows of `clip`.
    pub(super) fn shows_whole(
        self,
        source_raster_rect: Rect,
        (clip, cut): (Option<Rect>, Option<[f32; 4]>),
        viewport: ViewportUniformParams,
        root_scale: f32,
    ) -> bool {
        let origin = [source_raster_rect.x, source_raster_rect.y];
        viewport.transform.is_identity()
            && self.within_cut(origin, cut)
            && self.within_viewport(source_raster_rect, clip, viewport, root_scale)
    }

    // Whether the run's box from `origin` lies inside the edges of `cut`,
    // when the text is cut: no glyph of it changes.
    pub(super) fn within_cut(self, origin: [f32; 2], cut: Option<[f32; 4]>) -> bool {
        let bounds = self.at(origin);
        cut.is_none_or(|edges| {
            bounds[0] >= edges[0]
                && bounds[1] >= edges[1]
                && bounds[2] <= edges[2]
                && bounds[3] <= edges[3]
        })
    }

    /// Whether every glyph of the run at `source_raster_rect` lies inside
    /// what `viewport` shows of `clip`, so none needs its own test.
    pub(crate) fn within_viewport(
        self,
        source_raster_rect: Rect,
        clip: Option<Rect>,
        viewport: ViewportUniformParams,
        root_scale: f32,
    ) -> bool {
        if self.min[0] > self.max[0] || !root_scale.is_finite() || root_scale <= 0.0 {
            return false;
        }
        let left = (source_raster_rect.x + self.min[0] as f32) / root_scale;
        let top = (source_raster_rect.y + self.min[1] as f32) / root_scale;
        let right = (source_raster_rect.x + self.max[0] as f32) / root_scale;
        let bottom = (source_raster_rect.y + self.max[1] as f32) / root_scale;
        let viewport_rect = viewport.scene_rect(root_scale);
        let visible = match clip {
            Some(clip) => clip.intersect(viewport_rect),
            None => Some(viewport_rect),
        };
        visible
            .is_some_and(|visible| visible.contains(left, top) && visible.contains(right, bottom))
    }
}

/// A text's glyph run as the frame found it.
pub(super) struct TextGlyphRunLookup {
    pub(super) glyphs: RunGlyphs,
    pub(super) bounds: GlyphRunBounds,
    /// Whether the run came from the cache rather than from this frame's
    /// collection.
    pub(super) cached: bool,
    /// The glyphs' atlas entries, while the atlas they were taken from holds.
    pub(super) entries: Option<Rc<[GlyphAtlasEntry]>>,
}

/// A text's glyph run and where it draws.
pub(super) struct TextGlyphRunDraw<'a> {
    pub(super) text_draw: &'a TextDraw,
    pub(super) raster_rect: Rect,
    pub(super) text_scale: f32,
    pub(super) run_key: TextGlyphRunCacheKey,
    pub(super) run: TextGlyphRunLookup,
}

/// Logs why a text fell back from the glyph atlas, when
/// `CRANPOSE_TEXT_ATLAS_FALLBACK_DIAG` asks.
pub(super) fn log_text_atlas_fallback(text_draw: &TextDraw) {
    if !text_atlas_fallback_diag_enabled() {
        return;
    }
    let preview: String = text_draw.text.string().text().chars().take(96).collect();
    log::warn!(
        "[text-atlas-fallback] node={:?} spans={} links={} text_len={} preview={:?} span_style={:?} paragraph_style={:?}",
        text_draw.node_id,
        text_draw.text.string().span_styles().len(),
        text_draw.text.string().links().len(),
        text_draw.text.string().text().len(),
        preview,
        text_draw.text_style.span_style,
        text_draw.text_style.paragraph_style,
    );
}

impl GlyphRunQuads<'_> {
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Each glyph as an instance at `origin`. Every glyph of a run draws: its
    /// collection leaves out the zero-sized and the transparent ones.
    pub(super) fn instances_at(
        &self,
        origin: [f32; 2],
    ) -> impl Iterator<Item = GlyphInstance> + '_ {
        self.iter().map(move |quad| quad.at(origin))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = CachedTextGlyphQuad> + '_ {
        self.glyphs
            .boxes()
            .zip(self.entries)
            .map(|(glyph, entry)| cached_text_glyph_quad(&glyph, *entry, self.texel))
    }
}

pub(super) struct CachedGpuTextGlyphRun {
    pub(super) span: GlyphRunSpan,
    pub(super) atlas_generation: u64,
    /// The frame the run last drew in; a run idle for
    /// [`TEXT_GLYPH_GPU_RUN_IDLE_FRAMES`] gives its quads back.
    pub(super) last_frame: Cell<u64>,
}

pub(super) fn cached_text_glyph_quad(
    glyph: &RunGlyphBox,
    entry: GlyphAtlasEntry,
    texel: f32,
) -> CachedTextGlyphQuad {
    CachedTextGlyphQuad {
        offset: [glyph.x as f32, glyph.y as f32],
        size: [f32::from(glyph.width), f32::from(glyph.height)],
        color: [
            glyph.color.0.clamp(0.0, 1.0),
            glyph.color.1.clamp(0.0, 1.0),
            glyph.color.2.clamp(0.0, 1.0),
            glyph.color.3.clamp(0.0, 1.0),
        ],
        uv: glyph_atlas_uv_rect(entry, texel),
    }
}

/// `quad` at `source_raster_rect`'s origin. `None` for a quad that draws
/// nothing.
pub(super) fn cached_text_glyph_instance(
    source_raster_rect: Rect,
    quad: &CachedTextGlyphQuad,
) -> Option<GlyphInstance> {
    if quad.size[0] == 0.0 || quad.size[1] == 0.0 || quad.color[3] <= 0.0 {
        return None;
    }
    Some(quad.at([source_raster_rect.x, source_raster_rect.y]))
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
        x: (source_raster_rect.x + quad.offset[0]) / root_scale,
        y: (source_raster_rect.y + quad.offset[1]) / root_scale,
        width: quad.size[0] / root_scale,
        height: quad.size[1] / root_scale,
    })
}

pub(super) fn cached_text_glyph_quad_is_visible_in_viewport(
    source_raster_rect: Rect,
    quad: &CachedTextGlyphQuad,
    clip: Option<Rect>,
    viewport: ViewportUniformParams,
    root_scale: f32,
) -> bool {
    cached_text_glyph_quad_logical_rect(source_raster_rect, quad, root_scale)
        .is_some_and(|rect| draw_rect_is_visible_in_viewport(rect, clip, viewport, root_scale))
}

#[cfg(test)]
#[path = "tests/glyph_run_bounds_tests.rs"]
mod tests;
