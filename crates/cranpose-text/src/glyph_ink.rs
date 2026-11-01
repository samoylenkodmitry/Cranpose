use std::sync::atomic::{AtomicU8, Ordering};

use cranpose_ui_graphics::Rect;
use read_fonts::{
    model::{Font, metrics::GlyphExtents},
    types::GlyphId as FontGlyphId,
};

use crate::{
    glyph_id::GlyphId,
    index_range::widen,
    laid_glyphs::RunGlyphs,
    layout_body::LayoutBody,
    line_box::FontExtent,
    text_layout::{LayoutLine, LineRun, font_units},
    typeface::Typeface,
};

// Whether some glyph of a layout puts ink past its own advance or its
// typeface's lines, as the shaper finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InkReach {
    Within,
    Past,
}

impl InkReach {
    // How the ink of a line whose baseline lies `baseline` below its top
    // and whose box is `height` tall reaches, for typefaces reaching
    // `extent` above and below the baseline: past the box when it is too
    // short for them.
    pub(crate) fn of_line(baseline: f32, height: f32, extent: FontExtent) -> Self {
        if baseline < extent.ascent || height - baseline < extent.descent {
            Self::Past
        } else {
            Self::Within
        }
    }
}

// What a glyph's two bits say: not read yet, its ink inside its advance and
// the typeface's ascent and descent, or reaching past one of them.
const UNREAD: u8 = 0;
const INSIDE: u8 = 1;
const OUT: u8 = 2;

// Whether each glyph of a typeface keeps its ink inside its advance and the
// typeface's ascent and descent, read for a glyph the first time a layout
// holds it: two bits a glyph, so a layout tests each of its glyphs with one
// load.
pub(crate) struct GlyphInks {
    states: Box<[AtomicU8]>,
    // The ascent and the descent, below zero, in design units.
    ascent: f32,
    descent: f32,
}

impl GlyphInks {
    pub(crate) fn of(typeface: &Typeface) -> Self {
        let glyphs = usize::try_from(typeface.font().num_glyphs()).unwrap_or(0);
        let metrics = typeface.line_metrics();
        Self {
            states: (0..glyphs.div_ceil(4))
                .map(|_| AtomicU8::new(UNREAD))
                .collect(),
            ascent: metrics.ascent,
            descent: metrics.descent,
        }
    }

    // Whether the ink of `glyph` of `typeface`, whose table this is, reaches
    // past its advance, the ascent or the descent. A glyph the font does not
    // count is out.
    pub(crate) fn out(&self, typeface: &Typeface, glyph: GlyphId) -> bool {
        let index = widen(glyph.0);
        let Some(cell) = self.states.get(index / 4) else {
            return true;
        };
        let shift = (index % 4) * 2;
        match (cell.load(Ordering::Relaxed) >> shift) & 3 {
            UNREAD => self.read(typeface, glyph, (cell, shift)),
            state => state == OUT,
        }
    }

    // `out` of a glyph not read yet, which it reads and keeps. Out of line,
    // as a typeface meets each glyph once.
    #[inline(never)]
    fn read(&self, typeface: &Typeface, glyph: GlyphId, (cell, shift): (&AtomicU8, usize)) -> bool {
        let out = typeface.glyph_extents(glyph).is_some_and(|ink| {
            reaches_out(&ink, typeface.advance(glyph), (self.ascent, self.descent))
        });
        cell.fetch_or((if out { OUT } else { INSIDE }) << shift, Ordering::Relaxed);
        out
    }
}

// Whether the ink `extents` gives reaches past `advance` or the lines from
// `ascent` down to `descent`, all in design units, the descent below zero.
fn reaches_out(extents: &GlyphExtents<f32>, advance: f32, (ascent, descent): (f32, f32)) -> bool {
    extents.width > 0.0
        && extents.height > 0.0
        && (extents.x_bearing < 0.0
            || extents.x_bearing + extents.width > advance
            || extents.y_bearing > ascent
            || extents.y_bearing - extents.height < descent)
}

// Whether the ink of glyph `glyph` of `font`, whose advance is `advance`,
// reaches past it or the font's lines, as `GlyphInks` tells of a typeface's
// glyph.
pub(crate) fn font_glyph_inks_out(font: &Font, glyph: u32, advance: i32) -> bool {
    let (ascent, descent) = font.metrics().h_line().map_or((0.0, 0.0), |line| {
        (line.ascender.to_f32(), line.descender.to_f32())
    });
    font.glyph_metrics()
        .extents(FontGlyphId::new(glyph))
        .is_some_and(|ink| reaches_out(&ink, font_units(advance), (ascent, descent)))
}

// The box around the ink that the glyphs of `layout` put past the visible
// text of their lines or past the layout's box, as their outlines give it;
// `None` when every glyph's ink lies within them. A glyph whose ink stays
// inside its advance and its typeface's ascent and descent draws within
// both, unless its run spaces its glyphs closer than their advances, moves
// them off their pen, or sits on a line too short for the typeface.
pub(crate) fn find_ink_past_lines(body: &LayoutBody) -> Option<Rect> {
    let mut ink: Option<Rect> = None;
    for (index, line) in body.lines.iter().enumerate() {
        let runs = &body.runs[line.runs.range()];
        for (run, glyph_run) in runs.iter().zip(body.glyph_runs(index)) {
            let face = glyph_run.typeface;
            let inks = face.glyph_inks();
            let every = run_leaves_lines(body, line, run, inks);
            if !every && !any_out(body.glyphs.run(run.glyphs.range()), face, inks) {
                continue;
            }
            let scale = glyph_run.size / f32::from(face.units_per_em());
            for glyph in glyph_run.positions() {
                if !every && !inks.out(face, glyph.id) {
                    continue;
                }
                let Some(extents) = face.glyph_extents(glyph.id) else {
                    continue;
                };
                if extents.width <= 0.0 || extents.height <= 0.0 {
                    continue;
                }
                let glyph_ink = Rect {
                    x: glyph.x + extents.x_bearing * scale,
                    y: glyph.y - extents.y_bearing * scale,
                    width: extents.width * scale,
                    height: extents.height * scale,
                };
                ink = Some(ink.map_or(glyph_ink, |ink| ink.union(glyph_ink)));
            }
        }
    }
    ink
}

// Whether a glyph of `run` may reach past the visible text of `line` or the
// layout's box though its own ink stays inside its advance and the
// typeface's lines, `inks`: its glyphs spaced closer than their advances or
// moved off their pen, or its typeface's lines past the box.
fn run_leaves_lines(body: &LayoutBody, line: &LayoutLine, run: &LineRun, inks: &GlyphInks) -> bool {
    let face = &body.faces[widen(run.face)];
    let scale = run.size / f32::from(face.units_per_em());
    let baseline = line.baseline - run.baseline_shift;
    run.spacing < 0.0
        || baseline - inks.ascent * scale < 0.0
        || baseline - inks.descent * scale > body.height
        || body
            .glyphs
            .offsets(run.glyphs.range())
            .is_some_and(|offsets| offsets.iter().any(|offset| offset.x != 0 || offset.y != 0))
}

// Whether a glyph of `glyphs`, of `face`, reaches past its advance or the
// face's lines.
fn any_out(glyphs: RunGlyphs<'_>, face: &Typeface, inks: &GlyphInks) -> bool {
    match glyphs {
        RunGlyphs::Packed(glyphs) => glyphs.iter().any(|glyph| inks.out(face, glyph.id())),
        RunGlyphs::Full(glyphs) => glyphs.iter().any(|glyph| inks.out(face, glyph.id())),
    }
}

// Whether a glyph of `body` may put ink past the visible text of its line or
// past the layout's box, `reach` telling whether the layouter found a glyph
// whose ink reaches past its own advance or its typeface's lines, glyphs
// spaced closer than their advances or a line too short for its typefaces:
// the test `find_ink_past_lines` makes before it places any glyph, which
// most layouts pass.
pub(crate) fn may_ink_past_lines(body: &LayoutBody, reach: InkReach) -> bool {
    reach == InkReach::Past || body.glyphs.has_offsets()
}
