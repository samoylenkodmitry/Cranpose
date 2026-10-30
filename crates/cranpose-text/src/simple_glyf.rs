use std::ops::Range;

use read_fonts::{
    FontData, FontRead, TableProvider,
    model::{Font, Source, glyph::outline::PathSink},
    tables::{
        glyf::{
            Glyph, PointFlags,
            outline::{PathContourStart, outline_to_path},
        },
        loca::Loca,
    },
    types::{BigEndian, F26Dot6, GlyphId, Point},
};

use crate::glyph_outline::PathCollector;

const ON_STACK: usize = 64;

// Where a TrueType font at its default location keeps its simple glyphs:
// the ranges its tables take in the font's bytes. Found once, they let a
// glyph draw with no table read again.
pub(crate) struct GlyfTables {
    glyf: Range<usize>,
    loca: Range<usize>,
    long_loca: bool,
    // The `hmtx` advance and side bearing pairs, and the side bearings of
    // the glyphs after them; empty without the table.
    metrics: Range<usize>,
    bearings: Range<usize>,
}

impl GlyfTables {
    // The tables of `font`, `None` for a font the general loader draws: one
    // at a variation location, with a `VARC` table, whose composites draw in
    // place of the glyphs they cover, or without `glyf`.
    pub(crate) fn of(font: &Font) -> Option<Self> {
        if font
            .normalized_coords()
            .iter()
            .any(|coord| coord.to_bits() != 0)
        {
            return None;
        }
        let bytes = font_bytes(font)?;
        let tables = font.tables();
        if tables.varc().is_ok() {
            return None;
        }
        let glyf = range_in(bytes, tables.glyf().ok()?.offset_data().as_bytes())?;
        let (loca, long_loca) = match tables.loca(None).ok()? {
            Loca::Short(offsets) => (range_in(bytes, offsets)?, false),
            Loca::Long(offsets) => (range_in(bytes, offsets)?, true),
        };
        let (metrics, bearings) = tables
            .hmtx()
            .ok()
            .and_then(|hmtx| {
                Some((
                    range_in(bytes, hmtx.h_metrics())?,
                    range_in(bytes, hmtx.left_side_bearings())?,
                ))
            })
            .unwrap_or_default();
        Some(Self {
            glyf,
            loca,
            long_loca,
            metrics,
            bearings,
        })
    }

    // Where the data of glyph `index` starts in `glyf`.
    fn offset(&self, bytes: &[u8], index: usize) -> Option<usize> {
        let loca = &bytes[self.loca.clone()];
        if self.long_loca {
            usize::try_from(u32::from_be_bytes(entry(loca, index)?)).ok()
        } else {
            Some(usize::from(u16::from_be_bytes(entry(loca, index)?)) * 2)
        }
    }

    // The left side bearing `hmtx` gives glyph `index`.
    fn side_bearing(&self, bytes: &[u8], index: usize) -> Option<i16> {
        let long = self.metrics.len() / 4;
        let bearing = match index.checked_sub(long) {
            // A long metric is the advance, then the bearing.
            None => {
                let [_, _, high, low] = entry(&bytes[self.metrics.clone()], index)?;
                [high, low]
            }
            Some(after) => entry(&bytes[self.bearings.clone()], after)?,
        };
        Some(i16::from_be_bytes(bearing))
    }
}

// Draws `glyph` of `font` into `path` straight from its points when it is a
// simple TrueType glyph, as read-fonts draws it in font units. The general
// loader serves every kind of outline, and on a small device walking its
// code costs more than the drawing. `None` when the glyph is of another
// kind or does not read, which may leave part of a path in `path`. The
// contour walk draws into a trait object, as the loader does, so both draw
// through one copy of it.
#[inline(never)]
pub(crate) fn draw(
    font: &Font,
    tables: &GlyfTables,
    glyph: GlyphId,
    path: &mut PathCollector,
) -> Option<()> {
    let bytes = font_bytes(font)?;
    let index = usize::try_from(glyph.to_u32()).ok()?;
    let (start, end) = (
        tables.offset(bytes, index)?,
        tables.offset(bytes, index.checked_add(1)?)?,
    );
    if start == end {
        return Some(());
    }
    let data = bytes[tables.glyf.clone()].get(start..end)?;
    let simple = match Glyph::read(FontData::new(data)).ok()? {
        Glyph::Simple(simple) => simple,
        Glyph::Composite(_) => return None,
    };
    // The points of a glyph of a text face fit on the stack; a larger one
    // takes the heap.
    let count = simple.num_points();
    let mut stack_points = [Point::<F26Dot6>::default(); ON_STACK];
    let mut stack_flags = [PointFlags::default(); ON_STACK];
    let (mut heap_points, mut heap_flags);
    let (points, flags) = if count <= ON_STACK {
        (&mut stack_points[..count], &mut stack_flags[..count])
    } else {
        heap_points = vec![Point::default(); count];
        heap_flags = vec![PointFlags::default(); count];
        (&mut heap_points[..], &mut heap_flags[..])
    };
    simple.read_points_fast(points, flags).ok()?;
    // The loader places the outline at the side bearing `hmtx` states, so a
    // glyph whose bearing differs from its `xMin` moves by the difference.
    let bearing = tables.side_bearing(bytes, index).unwrap_or(0);
    let shift = F26Dot6::from_i32(i32::from(simple.x_min()) - i32::from(bearing));
    if shift != F26Dot6::ZERO {
        for point in points.iter_mut() {
            point.x -= shift;
        }
    }
    let ends = simple.end_pts_of_contours();
    let mut stack_contours = [0u16; ON_STACK];
    let heap_contours: Vec<u16>;
    let contours = if ends.len() <= ON_STACK {
        let contours = &mut stack_contours[..ends.len()];
        for (contour, end) in contours.iter_mut().zip(ends) {
            *contour = end.get();
        }
        &*contours
    } else {
        heap_contours = ends.iter().map(BigEndian::get).collect();
        &heap_contours
    };
    // A contour draws a piece for each of its points at most, and one that
    // closes it.
    path.reserve(count + contours.len());
    let mut sink: &mut dyn PathSink = path;
    outline_to_path(
        points,
        flags,
        contours,
        PathContourStart::default(),
        &mut sink,
    )
    .then_some(())
}

// Entry `index` of a table of `N`-byte entries.
fn entry<const N: usize>(table: &[u8], index: usize) -> Option<[u8; N]> {
    let at = index.checked_mul(N)?;
    table.get(at..at.checked_add(N)?)?.try_into().ok()
}

// The bytes a font was read from; `None` for a font read table by table.
fn font_bytes(font: &Font) -> Option<&[u8]> {
    let Source::Blob(bytes) = font.source() else {
        return None;
    };
    Some(bytes.as_ref())
}

// Where `part`, a slice of `bytes`, sits in them.
fn range_in<T>(bytes: &[u8], part: &[T]) -> Option<Range<usize>> {
    let start = part.as_ptr().addr().checked_sub(bytes.as_ptr().addr())?;
    let end = start.checked_add(size_of_val(part))?;
    bytes.get(start..end).map(|_| start..end)
}
