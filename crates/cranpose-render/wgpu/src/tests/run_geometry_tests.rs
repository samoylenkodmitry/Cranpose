use cranpose_ui_graphics::{
    ARC_BAND_MIN_RADIUS, Brush, Color, DrawScope, DrawScopeDefault, Size, Stroke, StrokeCap, TAU,
    band_class_segments,
};

use super::*;

fn inside_triangle(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let sign = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| {
        (p[0] - r[0]) * (q[1] - r[1]) - (q[0] - r[0]) * (p[1] - r[1])
    };
    let d1 = sign(p, a, b);
    let d2 = sign(p, b, c);
    let d3 = sign(p, c, a);
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

fn strip_covers(strip: &BandStrip, point: [f32; 2]) -> bool {
    strip
        .triangles()
        .into_iter()
        .any(|[a, b, c]| inside_triangle(point, a, b, c))
}

fn sdf_arc_band(p: [f32; 2], record: &ShapeRecord, scale: f32) -> f32 {
    let center = [record.arc[0] * scale, record.arc[1] * scale];
    let inner = record.arc_band[2] * scale;
    let outer = record.arc_band[3] * scale;
    let start = record.arc_normalized[0];
    let sweep = record.arc_normalized[1];
    let ra = (outer + inner) * 0.5;
    let rb = ((outer - inner) * 0.5).max(0.0);
    let (mid_sin, mid_cos, half_sin, half_cos) = if sweep >= TAU && start == 0.0 {
        (0.0, -1.0, 0.0, -1.0)
    } else {
        let half = sweep.clamp(0.0, TAU) * 0.5;
        let (ms, mc) = (start + half).sin_cos();
        let (hs, hc) = half.sin_cos();
        (ms, mc, hs.max(0.0), hc)
    };
    let d = [p[0] - center[0], p[1] - center[1]];
    let mut q = [
        -mid_sin * d[0] + mid_cos * d[1],
        mid_cos * d[0] + mid_sin * d[1],
    ];
    q[0] = q[0].abs();
    let mut dist = if half_cos * q[0] > half_sin * q[1] {
        let dx = q[0] - half_sin * ra;
        let dy = q[1] - half_cos * ra;
        (dx * dx + dy * dy).sqrt() - rb
    } else {
        ((q[0] * q[0] + q[1] * q[1]).sqrt() - ra).abs() - rb
    };
    let plane = half_cos * q[0] - half_sin * q[1];
    match record.band_cap() {
        StrokeCap::Butt => dist = dist.max(plane),
        StrokeCap::Square => dist = dist.max(plane - rb),
        StrokeCap::Round => {}
    }
    dist
}

fn shader_shades(record: &ShapeRecord, scale: f32, point: [f32; 2]) -> bool {
    let dist = sdf_arc_band(point, record, scale);
    let t = ((dist + 0.5).clamp(0.0, 1.0)).powi(2) * (3.0 - 2.0 * (dist + 0.5).clamp(0.0, 1.0));
    1.0 - t >= 0.001
}

fn recorded_arcs(record: impl FnOnce(&mut DrawScopeDefault)) -> Vec<ShapeRecord> {
    let mut scope = DrawScopeDefault::new(Size::new(600.0, 600.0));
    record(&mut scope);
    scope.finish().shapes().iter().collect()
}

fn assert_strip_covers_shader(record: &ShapeRecord, scale: f32) {
    let strip = BandStrip::of(record, Point::default(), scale, record.band_segments());
    let rect = record.coverage_rect();
    let left = ((rect.x * scale).floor() as i32) - 2;
    let top = ((rect.y * scale).floor() as i32) - 2;
    let right = (((rect.x + rect.width) * scale).ceil() as i32) + 2;
    let bottom = (((rect.y + rect.height) * scale).ceil() as i32) + 2;
    let mut shaded = 0usize;
    for y in top..bottom {
        for x in left..right {
            let point = [x as f32 + 0.5, y as f32 + 0.5];
            if shader_shades(record, scale, point) {
                shaded += 1;
                assert!(
                    strip_covers(&strip, point),
                    "pixel {point:?} is shaded by the arc SDF but outside the strip of {record:?}"
                );
            }
        }
    }
    assert!(shaded > 0, "the arc must shade something: {record:?}");
    let disc = quad_area(record, scale);
    assert!(
        strip.area() < disc,
        "the strip must cost less than the disc: {} vs {disc}",
        strip.area()
    );
}

#[test]
fn fill_excludes_records_outside_the_draw_window() {
    let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
    for width in [10.0, 30.0] {
        scope.draw_rect_at(
            cranpose_ui_graphics::Rect {
                x: 0.0,
                y: 0.0,
                width,
                height: 20.0,
            },
            Brush::solid(Color::WHITE),
        );
    }
    let recording = scope.finish();
    let selected = ShapeFill::of_draws(
        recording.tables(),
        Point::ZERO,
        1.0,
        [(1..2, Some(1))].into_iter(),
    );
    assert_eq!(selected.total(), 600.0);
    assert_eq!(selected.vertices, 4);
    let empty = ShapeFill::of_draws(
        recording.tables(),
        Point::ZERO,
        1.0,
        [(2..2, Some(1))].into_iter(),
    );
    assert_eq!(empty, ShapeFill::default());
}

#[test]
fn every_pixel_the_arc_shader_shades_lies_inside_its_strip() {
    let brush = Brush::solid(Color::WHITE);
    let records = recorded_arcs(|scope| {
        let center = Point::new(300.0, 300.0);
        scope.draw_arc(brush.clone(), center, 20.0, 0.0, TAU, Stroke::new(4.0));
        scope.draw_arc(brush.clone(), center, 90.0, 0.3, 1.2, Stroke::new(6.0));
        scope.draw_arc(brush.clone(), center, 200.0, 4.0, 2.5, Stroke::new(12.0));
        scope.draw_arc(brush.clone(), center, 250.0, 5.5, 2.0, Stroke::new(3.0));
        scope.draw_annular_sector(brush.clone(), center, 100.0, 140.0, 1.0, 0.4);
        scope.draw_annular_sector(brush.clone(), center, 12.0, 30.0, 2.0, 3.0);
        scope.draw_arc(
            brush.clone(),
            Point::new(100.0, 100.0),
            ARC_BAND_MIN_RADIUS,
            0.0,
            TAU,
            Stroke::new(2.0),
        );
        scope.draw_annular_sector(brush.clone(), center, 10.0, 30.0, 0.7, 0.3);
        scope.draw_arc(brush.clone(), center, 80.0, 5.0, 0.2, Stroke::new(2.0));
        scope.draw_arc(
            brush.clone(),
            center,
            60.0,
            2.0,
            0.15,
            Stroke::new(14.0).with_cap(StrokeCap::Square),
        );
        scope.draw_arc(
            brush,
            center,
            40.0,
            3.0,
            0.5,
            Stroke::new(16.0).with_cap(StrokeCap::Round),
        );
    });
    let banded: Vec<bool> = records.iter().map(ShapeRecord::is_banded).collect();
    assert_eq!(
        banded,
        [
            true, true, true, true, true, true, false, true, true, true, true
        ],
        "the ring at the smallest band radius costs more as a strip than as \
         its quad once its vertices are charged"
    );
    for record in records.iter().filter(|record| record.is_banded()) {
        for scale in [1.0, 2.75] {
            assert_strip_covers_shader(record, scale);
        }
    }
}

#[test]
fn the_analytic_strip_area_equals_its_triangles() {
    let records = recorded_arcs(|scope| {
        let center = Point::new(300.0, 300.0);
        let brush = Brush::solid(Color::WHITE);
        scope.draw_arc(brush.clone(), center, 20.0, 0.0, TAU, Stroke::new(4.0));
        scope.draw_arc(brush.clone(), center, 90.0, 0.3, 1.2, Stroke::new(6.0));
        scope.draw_annular_sector(brush, center, 100.0, 140.0, 1.0, 0.4);
    });
    for record in &records {
        let strip = BandStrip::of(record, Point::new(3.0, 7.0), 1.5, record.band_segments());
        let analytic = strip.area();
        let summed = strip.triangle_area_sum();
        assert!(
            (analytic - summed).abs() <= summed * 1e-4,
            "{analytic} vs {summed} for {record:?}"
        );
    }
}

#[test]
fn the_fill_estimate_counts_strips_for_bands_and_quads_for_the_rest() {
    let mut scope = DrawScopeDefault::new(Size::new(600.0, 600.0));
    scope.draw_arc(
        Brush::solid(Color::WHITE),
        Point::new(300.0, 300.0),
        200.0,
        0.0,
        TAU,
        Stroke::new(4.0),
    );
    scope.draw_rect_at(
        cranpose_ui_graphics::Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 20.0,
        },
        Brush::solid(Color::WHITE),
    );
    let recording = scope.finish();
    let tables = recording.tables();
    let draws = |bands: bool| {
        tables.segments.iter().map(move |segment| {
            (
                segment.start..segment.start + segment.count,
                bands.then(|| band_class_segments(segment.band_class)),
            )
        })
    };
    let banded = ShapeFill::of_draws(tables, Point::default(), 1.0, draws(true));
    let quads = ShapeFill::of_draws(tables, Point::default(), 1.0, draws(false));
    assert_eq!(banded.pixels[0], 200.0);
    assert_eq!(quads.pixels[0], 200.0);
    assert!(banded.pixels[4] < 2.0 * 6284.0 * 8.0);
    assert!(quads.pixels[4] > 150_000.0);
    assert_eq!(banded.total(), banded.pixels[0] + banded.pixels[4]);
    let [segment] = tables.segments.as_slice() else {
        panic!(
            "the ring and the rect share one segment: {:?}",
            tables.segments
        );
    };
    assert_eq!(
        banded.vertices,
        2 * u64::from(strip_vertices(band_class_segments(segment.band_class))),
        "every record of a segment is charged the stride the segment draws at, the rect's \
         pinned vertices included"
    );
    assert_eq!(quads.vertices, 2 * u64::from(QUAD_VERTICES));
}
