use std::sync::atomic::{AtomicU8, Ordering};

use cranpose_ui_graphics::Rect;
use read_fonts::model::metrics::GlyphExtents;

use crate::{
    glyph_id::GlyphId,
    index_range::widen,
    layout_body::LayoutBody,
    line_box::FontExtent,
    shaped_item::Piece,
    text_layout::{GlyphRun, LayoutLine, LineRun},
    typeface::Typeface,
};

// How some glyph of a layout may put ink past the visible text of its line
// or past the layout's box, the farther reach last: not at all; as its line
// places it, on a line too short for the band its typeface's ASCII glyphs
// ink or spaced closer than its advance; or as its own ink leaves its
// advance or that band, as the shaper finds it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum InkReach {
    #[default]
    Within,
    Lines,
    Glyphs,
}

impl InkReach {
    // How the ink of a line whose baseline lies `baseline` below its top
    // and whose box is `height` tall reaches, for fonts reaching `fonts`
    // above and below the baseline whose glyphs ink the band `band` gives,
    // which lies within: past the box when it is too short for them. The
    // band is found only for a box too short for the fonts.
    pub(crate) fn of_line(
        baseline: f32,
        height: f32,
        (fonts, band): (FontExtent, impl FnOnce() -> FontExtent),
    ) -> Self {
        let cuts =
            |extent: FontExtent| baseline < extent.ascent || height - baseline < extent.descent;
        if cuts(fonts) && cuts(band()) {
            Self::Lines
        } else {
            Self::Within
        }
    }
}

// What a glyph's two bits say: not read yet, its ink inside its advance and
// the band the typeface's printable ASCII glyphs ink, inside its advance and
// the typeface's ascent and descent, or reaching past one of them.
const UNREAD: u8 = 0;
const TIGHT: u8 = 1;
const LOOSE: u8 = 2;
const OUT: u8 = 3;

// Where the ink of each glyph of a typeface lies, read for a glyph the first
// time a layout holds it: two bits a glyph, so a layout tests each of its
// glyphs with one load.
pub(crate) struct GlyphInks {
    states: Box<[AtomicU8]>,
    // The ascent and the descent, below zero, in design units.
    ascent: f32,
    descent: f32,
    // The top and the bottom, below zero, of the ink of the typeface's
    // printable ASCII glyphs that stay inside their advances and the
    // ascent and descent, in design units: most text inks between them,
    // well inside the ascent and the descent.
    band: (f32, f32),
}

impl GlyphInks {
    pub(crate) fn of(typeface: &Typeface) -> Self {
        let glyphs = usize::try_from(typeface.font().num_glyphs()).unwrap_or(0);
        let metrics = typeface.line_metrics();
        let lines = (metrics.ascent, metrics.descent);
        let band = (' '..='~')
            .filter_map(|ch| {
                let glyph = typeface.glyph(ch)?;
                let ink = typeface.glyph_extents(glyph)?;
                (ink.width > 0.0
                    && ink.height > 0.0
                    && !reaches_out(&ink, typeface.advance(glyph), lines))
                .then_some((ink.y_bearing, ink.y_bearing - ink.height))
            })
            .reduce(|(top, bottom), (ink_top, ink_bottom)| {
                (top.max(ink_top), bottom.min(ink_bottom))
            })
            .unwrap_or(lines);
        Self {
            states: (0..glyphs.div_ceil(4))
                .map(|_| AtomicU8::new(UNREAD))
                .collect(),
            ascent: metrics.ascent,
            descent: metrics.descent,
            band,
        }
    }

    // Whether the ink of `glyph` of `typeface`, whose table this is, leaves
    // its advance or the band.
    pub(crate) fn leaves_band(&self, typeface: &Typeface, glyph: GlyphId) -> bool {
        self.state(typeface, glyph) != TIGHT
    }

    // Where the ink of `glyph` of `typeface` lies. A glyph the font does not
    // count is out.
    fn state(&self, typeface: &Typeface, glyph: GlyphId) -> u8 {
        let index = widen(glyph.0);
        let Some(cell) = self.states.get(index / 4) else {
            return OUT;
        };
        let shift = (index % 4) * 2;
        match (cell.load(Ordering::Relaxed) >> shift) & 3 {
            UNREAD => self.read(typeface, glyph, (cell, shift)),
            state => state,
        }
    }

    // `state` of a glyph not read yet, which it reads and keeps. Out of
    // line, as a typeface meets each glyph once.
    #[inline(never)]
    fn read(&self, typeface: &Typeface, glyph: GlyphId, (cell, shift): (&AtomicU8, usize)) -> u8 {
        let state = typeface
            .glyph_extents(glyph)
            .filter(|ink| ink.width > 0.0 && ink.height > 0.0)
            .map_or(TIGHT, |ink| {
                let (top, bottom) = (ink.y_bearing, ink.y_bearing - ink.height);
                if reaches_out(&ink, typeface.advance(glyph), (self.ascent, self.descent)) {
                    OUT
                } else if top > self.band.0 || bottom < self.band.1 {
                    LOOSE
                } else {
                    TIGHT
                }
            });
        cell.fetch_or(state << shift, Ordering::Relaxed);
        state
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

// The band of the ASCII ink of the typefaces of `pieces`, the pieces of a
// line of `faces`, above and below the line's baseline, as `line_extent`
// gives their ascent and descent.
pub(crate) fn line_band(pieces: &[Piece], faces: &[Typeface]) -> FontExtent {
    pieces
        .iter()
        .fold(FontExtent::new(0.0, 0.0, 0.0), |line, piece| {
            let (top, bottom) = faces[piece.face].glyph_inks().band;
            let shift = piece.baseline_shift;
            FontExtent::new(
                line.ascent.max(top * piece.scale + shift.max(0.0)),
                line.descent.max(-bottom * piece.scale - shift.min(0.0)),
                0.0,
            )
        })
}

// The box around the ink that the glyphs of `layout` put past the visible
// text of its lines, which reaches from `left` to `right`, or past the
// layout's box; `None` when every glyph's ink lies within them. A glyph
// whose ink stays inside its advance and its typeface's ascent and descent
// draws within the visible text of its line, and within the layout's box
// unless the line is too short for the typeface, where a glyph inside the
// band of the typeface's ASCII ink still does unless the line cuts the band
// too, and then inks within the band. Other glyphs, and the glyphs of a run
// that spaces them closer than their advances or that moves them off their
// pen, take their outlines. Out of line: few layouts hold a glyph that may
// ink past their lines.
#[inline(never)]
pub(crate) fn find_ink_past_lines(
    body: &LayoutBody,
    lines: (f32, f32),
    reach: InkReach,
) -> Option<Rect> {
    let mut past = PastInk {
        lines,
        height: body.height,
        glyphs_leave: reach == InkReach::Glyphs,
        ink: None,
    };
    for (index, line) in body.lines.iter().enumerate() {
        let runs = &body.runs[line.runs.range()];
        for (run, glyph_run) in runs.iter().zip(body.glyph_runs(index)) {
            past.add_run(body, (line, run), &glyph_run);
        }
    }
    past.ink
}

// The ink the glyphs of a layout put past the visible text of its lines,
// which reaches across `lines`, or past its box, `height` tall. Only when
// `glyphs_leave` does a glyph inked within its advance and the band need its
// state read.
struct PastInk {
    lines: (f32, f32),
    height: f32,
    glyphs_leave: bool,
    ink: Option<Rect>,
}

impl PastInk {
    // Adds `rect` where it reaches past the lines or the box.
    fn add(&mut self, rect: Rect) {
        let (left, right) = self.lines;
        if rect.x < left
            || rect.x + rect.width > right
            || rect.y < 0.0
            || rect.y + rect.height > self.height
        {
            self.ink = Some(self.ink.map_or(rect, |ink| ink.union(rect)));
        }
    }

    // Adds the ink of `glyph_run`, the glyphs of `run` on `line` of `body`.
    fn add_run(
        &mut self,
        body: &LayoutBody,
        (line, run): (&LayoutLine, &LineRun),
        glyph_run: &GlyphRun<'_>,
    ) {
        let face = glyph_run.typeface;
        let inks = face.glyph_inks();
        let scale = glyph_run.size / f32::from(face.units_per_em());
        let baseline = line.baseline - run.baseline_shift;
        // Whether the box cuts the lines from `top` down to `bottom`.
        let cuts = |(top, bottom): (f32, f32)| {
            baseline - top * scale < 0.0 || baseline - bottom * scale > body.height
        };
        let short = cuts((inks.ascent, inks.descent));
        let moved = moves_glyphs(body, run);
        if short && !moved && cuts(inks.band) {
            // The glyphs inside the band ink within it, across the visible
            // text: a box, as tracing each would cost more.
            let (top, bottom) = inks.band;
            let (left, right) = self.lines;
            self.add(Rect {
                x: left,
                y: baseline - top * scale,
                width: right - left,
                height: (top - bottom) * scale,
            });
        }
        let traced = |state: u8| moved || state == OUT || (short && state == LOOSE);
        if moved
            || (self.glyphs_leave
                && body
                    .glyphs
                    .run(run.glyphs.range())
                    .any_id(|glyph| traced(inks.state(face, glyph))))
        {
            self.add_outlines(glyph_run, (inks, scale), traced);
        }
    }

    // Adds the outlines of the glyphs of `glyph_run` whose state `traced`
    // takes, `scale` pixels a design unit.
    fn add_outlines(
        &mut self,
        glyph_run: &GlyphRun<'_>,
        (inks, scale): (&GlyphInks, f32),
        traced: impl Fn(u8) -> bool,
    ) {
        let face = glyph_run.typeface;
        for glyph in glyph_run.positions() {
            if !traced(inks.state(face, glyph.id)) {
                continue;
            }
            let Some(extents) = face.glyph_extents(glyph.id) else {
                continue;
            };
            if extents.width <= 0.0 || extents.height <= 0.0 {
                continue;
            }
            self.add(Rect {
                x: glyph.x + extents.x_bearing * scale,
                y: glyph.y - extents.y_bearing * scale,
                width: extents.width * scale,
                height: extents.height * scale,
            });
        }
    }
}

// Whether `run` spaces its glyphs closer than their advances or moves them
// off their pen, so that a glyph whose ink stays inside its advance may
// still reach past the visible text of its line.
fn moves_glyphs(body: &LayoutBody, run: &LineRun) -> bool {
    run.spacing < 0.0
        || body
            .glyphs
            .offsets(run.glyphs.range())
            .is_some_and(|offsets| offsets.iter().any(|offset| offset.x != 0 || offset.y != 0))
}

// Whether a glyph of `body` may put ink past the visible text of its line or
// past the layout's box, `reach` telling whether the layouter found a glyph
// whose ink reaches past its own advance or its typeface's lines, glyphs
// spaced closer than their advances or a line too short for its typefaces:
// the test `find_ink_past_lines` makes before it places any glyph, which
// most layouts pass.
pub(crate) fn may_ink_past_lines(body: &LayoutBody, reach: InkReach) -> bool {
    reach != InkReach::Within || body.glyphs.has_offsets()
}
