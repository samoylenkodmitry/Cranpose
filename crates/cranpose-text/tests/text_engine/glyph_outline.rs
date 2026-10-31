use ab_glyph::{Font as _, FontRef, PxScale, point};
use cranpose_text::{
    FontFamily, FontStyle, FontWeight, GlyphId, GlyphOutline, OutlineSegment, OutlineTransform,
    Typeface,
};
use cranpose_ui_graphics::Point;
use read_fonts::{
    model::{
        Blob, Font, Variation,
        glyph::outline::{DrawMemory, DrawOptions, PathSink},
    },
    types::{GlyphId as FontGlyphId, Tag},
};

use crate::fonts::{
    ARABIC_VF, DEJAVU, DEVANAGARI_VF, MONASPACE_VF, NOTO_SANS_LATIN, ROBOTO, bytes, typeface,
};

// A whole pixel count or position as a float.
fn float(value: i32) -> f32 {
    f32::from(i16::try_from(value).expect("a glyph box is small"))
}

fn index(x: u32, y: u32, width: u32) -> usize {
    usize::try_from(y * width + x).expect("a glyph box is small")
}

#[test]
fn outlines_draw_the_coverage_ab_glyph_draws() {
    for (path, text) in [(ROBOTO, "agQ@"), (DEVANAGARI_VF, "कष")] {
        let face = typeface(path);
        let font_bytes = bytes(path);
        let font = FontRef::try_from_slice(&font_bytes).expect("ab_glyph reads the font");
        let units = font.units_per_em().expect("the font has an em");
        let mut rasterizer = ab_glyph_rasterizer::Rasterizer::new(0, 0);
        for ch in text.chars() {
            for (size, offset) in [(12.5, 0.0), (31.0, 0.25)] {
                let glyph = face.glyph(ch).expect("the font maps the character");
                let outline = GlyphOutline::of(
                    &face,
                    glyph,
                    OutlineTransform {
                        size,
                        x_offset: offset,
                        y_offset: offset,
                        skew: 0.0,
                    },
                )
                .expect("the glyph has an outline");
                let scale = PxScale::from(size * font.height_unscaled() / units);
                let expected_glyph = font
                    .glyph_id(ch)
                    .with_scale_and_position(scale, point(offset, offset));
                let expected = font
                    .outline_glyph(expected_glyph)
                    .expect("ab_glyph outlines it");
                let bounds = expected.px_bounds();
                let width = outline.width();
                assert_eq!(
                    (
                        float(outline.left()),
                        float(outline.top()),
                        float(i32::try_from(width).expect("small")),
                        float(i32::try_from(outline.height()).expect("small")),
                    ),
                    (bounds.min.x, bounds.min.y, bounds.width(), bounds.height()),
                    "{ch} at {size}"
                );
                let cells = index(0, outline.height(), width);
                let (mut ours, mut theirs) = (vec![0.0; cells], vec![0.0; cells]);
                outline.draw(&mut rasterizer, |x, y, value| {
                    ours[index(x, y, width)] = value;
                });
                expected.draw(|x, y, value| theirs[index(x, y, width)] = value);
                let worst = ours
                    .iter()
                    .zip(&theirs)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0_f32, f32::max);
                assert!(worst < 1e-4, "{ch} at {size}: {worst}");
            }
        }
    }
}

#[test]
fn a_space_has_no_outline() {
    let face = typeface(ROBOTO);
    let space = face.glyph(' ').expect("the font maps a space");

    assert!(
        GlyphOutline::of(
            &face,
            space,
            OutlineTransform {
                size: 16.0,
                ..OutlineTransform::default()
            }
        )
        .is_none()
    );
}

// An outline read-fonts draws in font units with y growing down, from the
// corner of its box: what `GlyphOutline` gives at one pixel a unit.
#[derive(Default)]
struct UnitOutline {
    segments: Vec<OutlineSegment>,
    start: Point,
    pen: Point,
}

impl UnitOutline {
    fn of(font: &Font, glyph: u32) -> Option<(Vec<OutlineSegment>, (i32, i32))> {
        let mut outline = Self::default();
        font.outline_glyphs()
            .get(FontGlyphId::new(glyph))?
            .draw(&DrawOptions::new(), DrawMemory::Scratch, &mut outline)
            .ok()?;
        let points = || {
            outline.segments.iter().flat_map(|segment| match *segment {
                OutlineSegment::Line(a, b) => [a, b, b, b],
                OutlineSegment::Quad(a, b, c) => [a, b, c, c],
                OutlineSegment::Cubic(a, b, c, d) => [a, b, c, d],
            })
        };
        let left = points().map(|at| at.x).reduce(f32::min)?.floor();
        let top = points().map(|at| at.y).reduce(f32::min)?.floor();
        let moved = |at: Point| Point::new(at.x - left, at.y - top);
        let segments = outline
            .segments
            .iter()
            .map(|segment| match *segment {
                OutlineSegment::Line(a, b) => OutlineSegment::Line(moved(a), moved(b)),
                OutlineSegment::Quad(a, b, c) => OutlineSegment::Quad(moved(a), moved(b), moved(c)),
                OutlineSegment::Cubic(a, b, c, d) => {
                    OutlineSegment::Cubic(moved(a), moved(b), moved(c), moved(d))
                }
            })
            .collect();
        Some((segments, (whole(left), whole(top))))
    }
}

impl PathSink for UnitOutline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = Point::new(x, -y);
        self.pen = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = Point::new(x, -y);
        self.segments.push(OutlineSegment::Line(self.pen, to));
        self.pen = to;
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let to = Point::new(x, -y);
        self.segments
            .push(OutlineSegment::Quad(self.pen, Point::new(cx0, -cy0), to));
        self.pen = to;
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let to = Point::new(x, -y);
        self.segments.push(OutlineSegment::Cubic(
            self.pen,
            Point::new(cx0, -cy0),
            Point::new(cx1, -cy1),
            to,
        ));
        self.pen = to;
    }

    fn close(&mut self) {
        if self.pen != self.start {
            self.segments
                .push(OutlineSegment::Line(self.pen, self.start));
        }
        self.pen = self.start;
    }
}

fn whole(value: f32) -> i32 {
    format!("{value}")
        .parse()
        .expect("a box corner is a whole number")
}

// Every glyph of `face` draws the outline read-fonts draws of `font`, the
// same font at the same location.
fn assert_outlines_match(path: &str, face: &Typeface, font: &Font) {
    let size = f32::from(face.units_per_em());
    for glyph in 0..font.num_glyphs() {
        let ours = GlyphOutline::of(
            face,
            GlyphId(glyph),
            OutlineTransform {
                size,
                ..OutlineTransform::default()
            },
        )
        .map(|outline| {
            (
                outline.segments().collect::<Vec<_>>(),
                (outline.left(), outline.top()),
            )
        });
        assert!(
            ours == UnitOutline::of(font, glyph),
            "{path}: glyph {glyph}"
        );
    }
}

#[test]
fn every_glyph_draws_the_outline_read_fonts_draws() {
    for path in [
        ROBOTO,
        DEJAVU,
        NOTO_SANS_LATIN,
        MONASPACE_VF,
        ARABIC_VF,
        DEVANAGARI_VF,
    ] {
        let font = Font::new(Blob::from(bytes(path)), 0).expect("the font reads");
        assert_outlines_match(path, &typeface(path), &font);
    }
}

#[test]
fn every_glyph_of_a_variable_instance_draws_the_outline_read_fonts_draws() {
    let family = FontFamily::named("Instance");
    for path in [MONASPACE_VF, ARABIC_VF, DEVANAGARI_VF] {
        let face = Typeface::from_registered(
            &family,
            FontWeight::new(400),
            FontStyle::Normal,
            bytes(path),
            &[(*b"wght", 700.0)],
        )
        .expect("the font has a weight axis");
        let font = Font::new(Blob::from(bytes(path)), 0)
            .expect("the font reads")
            .instance_builder()
            .variations([Variation::new(Tag::new(b"wght"), 700.0)])
            .build();
        assert_outlines_match(path, &face, &font);
    }
}
