use read_fonts::{
    TableProvider,
    model::{Font, glyph::outline::PathSink},
    tables::glyf::{
        Glyph, PointFlags,
        outline::{PathContourStart, outline_to_path},
    },
    types::{BigEndian, F26Dot6, GlyphId, Point},
};

const ON_STACK: usize = 64;

// Draws `glyph` of `font` into `sink` straight from its points when it is a
// simple TrueType glyph of a font at its default location, as read-fonts
// draws it in font units. The general loader serves every kind of outline,
// and on a small device walking its code costs more than the drawing.
// `None` when the glyph is of another kind or does not read, which may
// leave part of a path in `sink`. The sink is a trait object, as the
// loader passes it, so both draw through one copy of the contour walk.
#[inline(never)]
pub(crate) fn draw(font: &Font, glyph: GlyphId, mut sink: &mut dyn PathSink) -> Option<()> {
    if font
        .normalized_coords()
        .iter()
        .any(|coord| coord.to_bits() != 0)
    {
        return None;
    }
    let tables = font.tables();
    // A `VARC` composite draws in place of the glyph it covers.
    if tables.varc().is_ok() {
        return None;
    }
    let glyf = tables.glyf().ok()?;
    let simple = match tables.loca(None).ok()?.get_glyf(glyph, &glyf).ok()? {
        Some(Glyph::Simple(simple)) => simple,
        Some(Glyph::Composite(_)) => return None,
        None => return Some(()),
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
    let bearing = tables
        .hmtx()
        .ok()
        .and_then(|hmtx| hmtx.side_bearing(glyph))
        .unwrap_or(0);
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
    outline_to_path(
        points,
        flags,
        contours,
        PathContourStart::default(),
        &mut sink,
    )
    .then_some(())
}
