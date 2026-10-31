use ab_glyph_rasterizer::{Point, Rasterizer, point};
use cranpose_ui_graphics::{Point as PixelPoint, whole};
use read_fonts::{
    model::glyph::outline::{DrawMemory, DrawOptions, PathSink},
    types::GlyphId as FontGlyphId,
};

use crate::{glyph_id::GlyphId, simple_glyf, typeface::Typeface};

/// A glyph's outline at one size and offset, in pixels with y growing down,
/// ready to draw as coverage. Its box holds every pixel the outline touches,
/// placed from the glyph's origin on the baseline.
pub struct GlyphOutline {
    // Relative to the box's top left corner.
    segments: Vec<Segment>,
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy)]
enum Segment {
    Line(Point, Point),
    Quad(Point, Point, Point),
    Cubic(Point, Point, Point, Point),
}

/// One piece of a glyph's outline, in pixels from its box's top left corner
/// with y growing down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutlineSegment {
    /// A straight line from the first point to the second.
    Line(PixelPoint, PixelPoint),
    /// A quadratic curve from the first point to the third, pulled by the second.
    Quad(PixelPoint, PixelPoint, PixelPoint),
    /// A cubic curve from the first point to the fourth, pulled by the middle two.
    Cubic(PixelPoint, PixelPoint, PixelPoint, PixelPoint),
}

/// How a glyph's outline is placed in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OutlineTransform {
    /// Pixels per em.
    pub size: f32,
    /// How far right of the pixel grid the glyph's origin sits: the fraction
    /// of a pixel a subpixel position adds.
    pub x_offset: f32,
    /// How far down of the pixel grid the glyph's origin sits.
    pub y_offset: f32,
    /// How far right each pixel of height above the baseline moves the
    /// outline: the slant of a synthesized italic, 0.25 on Android.
    pub skew: f32,
}

impl GlyphOutline {
    /// The outline of `glyph` of `typeface` placed as `transform` says.
    /// `None` for a glyph with no outline, such as a space.
    pub fn of(typeface: &Typeface, glyph: GlyphId, transform: OutlineTransform) -> Option<Self> {
        let font = typeface.font();
        let glyph = FontGlyphId::new(glyph.0);
        // The outline comes in font units and scales in `f32`, as ab_glyph
        // scales it, so the coverage is the one the renderers drew before.
        let mut path = PathCollector {
            segments: Vec::new(),
            start: point(0.0, 0.0),
            pen: point(0.0, 0.0),
            offset: point(transform.x_offset, transform.y_offset),
            scale: transform.size / f32::from(typeface.units_per_em()),
            skew: transform.skew,
        };
        let simple = typeface
            .glyf_tables()
            .and_then(|tables| simple_glyf::draw(font, tables, glyph, &mut path));
        if simple.is_none() {
            path.segments.clear();
            font.outline_glyphs()
                .get(glyph)?
                .draw(
                    &DrawOptions::new().with_ppem(None),
                    DrawMemory::Scratch,
                    &mut path,
                )
                .ok()?;
        }
        let (min, max) = path.bounds()?;
        let (left, top) = (whole::floor(min.x), whole::floor(min.y));
        let width = pixels(whole::ceil(max.x) - left);
        let height = pixels(whole::ceil(max.y) - top);
        let corner = point(left, top);
        let moved = |at: Point| point(at.x - corner.x, at.y - corner.y);
        for segment in &mut path.segments {
            *segment = match *segment {
                Segment::Line(a, b) => Segment::Line(moved(a), moved(b)),
                Segment::Quad(a, b, c) => Segment::Quad(moved(a), moved(b), moved(c)),
                Segment::Cubic(a, b, c, d) => {
                    Segment::Cubic(moved(a), moved(b), moved(c), moved(d))
                }
            };
        }
        Some(Self {
            segments: path.segments,
            left: whole::to_i32(left),
            top: whole::to_i32(top),
            width,
            height,
        })
    }

    /// How far right of the glyph's origin the box starts.
    pub fn left(&self) -> i32 {
        self.left
    }

    /// How far below the baseline the box starts; negative above it.
    pub fn top(&self) -> i32 {
        self.top
    }

    /// The box's width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The box's height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The outline's pieces, in pixels from the box's top left corner.
    pub fn segments(&self) -> impl ExactSizeIterator<Item = OutlineSegment> + '_ {
        let pixel = |at: Point| PixelPoint::new(at.x, at.y);
        self.segments.iter().map(move |segment| match *segment {
            Segment::Line(a, b) => OutlineSegment::Line(pixel(a), pixel(b)),
            Segment::Quad(a, b, c) => OutlineSegment::Quad(pixel(a), pixel(b), pixel(c)),
            Segment::Cubic(a, b, c, d) => {
                OutlineSegment::Cubic(pixel(a), pixel(b), pixel(c), pixel(d))
            }
        })
    }

    /// Passes the coverage of each pixel of the box, from 0 to 1, row by row,
    /// with its column and row; `rasterizer` is the scratch the drawing
    /// accumulates in.
    pub fn draw(&self, rasterizer: &mut Rasterizer, visit: impl FnMut(u32, u32, f32)) {
        rasterizer.reset(usize_of(self.width), usize_of(self.height));
        for segment in &self.segments {
            match *segment {
                Segment::Line(a, b) => rasterizer.draw_line(a, b),
                Segment::Quad(a, b, c) => rasterizer.draw_quad(a, b, c),
                Segment::Cubic(a, b, c, d) => rasterizer.draw_cubic(a, b, c, d),
            }
        }
        rasterizer.for_each_pixel_2d(visit);
    }
}

// Collects an outline's segments in pixels with y growing down, from font
// units `scale` pixels each, slanted right by `skew` a unit of height.
pub(crate) struct PathCollector {
    segments: Vec<Segment>,
    start: Point,
    pen: Point,
    offset: Point,
    scale: f32,
    skew: f32,
}

impl PathCollector {
    // Makes room for `segments` more pieces at once.
    pub(crate) fn reserve(&mut self, segments: usize) {
        self.segments.reserve(segments);
    }

    fn point(&self, x: f32, y: f32) -> Point {
        point(
            (x + y * self.skew) * self.scale + self.offset.x,
            -y * self.scale + self.offset.y,
        )
    }

    // The smallest box around every point of the path, `None` for an empty
    // one.
    fn bounds(&self) -> Option<(Point, Point)> {
        let mut points = self.segments.iter().flat_map(|segment| match *segment {
            Segment::Line(a, b) => [a, b, b, b],
            Segment::Quad(a, b, c) => [a, b, c, c],
            Segment::Cubic(a, b, c, d) => [a, b, c, d],
        });
        let first = points.next()?;
        Some(points.fold((first, first), |(min, max), at| {
            (
                point(min.x.min(at.x), min.y.min(at.y)),
                point(max.x.max(at.x), max.y.max(at.y)),
            )
        }))
    }
}

impl PathSink for PathCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = self.point(x, y);
        self.pen = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.point(x, y);
        self.segments.push(Segment::Line(self.pen, to));
        self.pen = to;
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let to = self.point(x, y);
        self.segments
            .push(Segment::Quad(self.pen, self.point(cx0, cy0), to));
        self.pen = to;
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let to = self.point(x, y);
        self.segments.push(Segment::Cubic(
            self.pen,
            self.point(cx0, cy0),
            self.point(cx1, cy1),
            to,
        ));
        self.pen = to;
    }

    fn close(&mut self) {
        if self.pen != self.start {
            self.segments.push(Segment::Line(self.pen, self.start));
        }
        self.pen = self.start;
    }
}

// A size in whole pixels, at least zero.
fn pixels(value: f32) -> u32 {
    u32::try_from(whole::to_i32(value).max(0)).unwrap_or(0)
}

fn usize_of(value: u32) -> usize {
    usize::try_from(value).unwrap_or(0)
}
