use super::*;
use crate::{DrawScope, DrawScopeDefault, Size};

#[test]
fn wide_arcs_are_banded_by_sweep_and_radius_and_narrow_ones_stay_quads() {
    let mut scope = DrawScopeDefault::new(Size::new(400.0, 400.0));
    let brush = Brush::Solid(Color::WHITE);
    let center = Point::new(200.0, 200.0);
    scope.draw_annular_sector(brush.clone(), center, 4.0, 8.0, 0.0, 1.0);
    scope.draw_annular_sector(brush.clone(), center, 10.0, 20.0, 0.0, 1.0);
    scope.draw_arc(brush.clone(), center, 50.0, 0.0, 1.0, Stroke::new(3.0));
    scope.draw_arc(brush.clone(), center, 200.0, 0.0, 1.0, Stroke::new(3.0));
    scope.draw_arc(brush.clone(), center, 500.0, 0.0, 1.0, Stroke::new(3.0));
    scope.draw_annular_sector(brush.clone(), center, 0.0, 40.0, 0.0, 3.0);
    scope.draw_annular_sector(brush, center, 30.0, 40.0, 0.0, 0.05);
    let recording = scope.finish();
    let banded: Vec<bool> = recording
        .shapes()
        .iter()
        .map(|record| record.is_banded())
        .collect();
    assert_eq!(
        banded,
        [false, true, true, true, true, false, true],
        "a disc stays a quad: its strip would be the disc and more; a sliver's \
         strip beats the disc the quad path would draw"
    );
    let segments: Vec<u32> = recording
        .shapes()
        .iter()
        .map(|record| record.band_segments())
        .collect();
    assert_eq!(
        segments[1..5],
        [4, 4, 8, 16],
        "a band takes the segments its padded sweep needs at the ring step of its radius"
    );
    assert_eq!(
        segments[6], 2,
        "the wide band needs two segments to cover its padded sweep"
    );
    let classes: Vec<usize> = recording
        .shapes()
        .iter()
        .map(|record| record.band_class())
        .collect();
    assert_eq!(classes, [0, 2, 2, 3, 4, 0, 1]);
    let segment_classes: Vec<u8> = recording
        .tables()
        .segments
        .iter()
        .map(|segment| segment.band_class)
        .collect();
    assert_eq!(
        segment_classes,
        [4],
        "a few records of mixed classes share one segment at the largest \
         class, so one draw keeps record order"
    );
    assert_eq!(band_bucket(1), 0);
    assert_eq!(band_bucket(64), ARC_BUCKETS - 1);
}

#[test]
fn a_strip_pattern_stays_within_its_vertices_including_a_single_segment() {
    for segments in ARC_BUCKET_SEGMENTS {
        let indices: Vec<u32> = strip_index_pattern(segments).collect();
        assert_eq!(indices.len() as u32, strip_indices(segments));
        assert_eq!(
            indices.iter().max().copied(),
            Some(strip_vertices(segments) - 1)
        );
    }
    let ring = BandRing::new(20.0, 22.0, 0.0, 0.01);
    assert_eq!(ring.segments(), BAND_MIN_SEGMENTS);
    assert_eq!(band_bucket(BAND_MIN_SEGMENTS), 0);
}

#[test]
fn minimum_band_keeps_segment_boundaries_exact() {
    let mut values = vec![f32::NAN, -1.0, -0.0, 0.0, f32::MIN_POSITIVE];
    for segments in ARC_BUCKET_SEGMENTS {
        let boundary = segments as f32;
        values.extend([
            f32::from_bits(boundary.to_bits() - 1),
            boundary,
            f32::from_bits(boundary.to_bits() + 1),
        ]);
    }
    for range in values {
        let ring = BandRing {
            mid: 0.0,
            ring_half: 0.0,
            range_start: 0.0,
            range,
            segments_per_radian: 1.0,
        };
        let expected = (range.ceil() as u32)
            .max(BAND_MIN_SEGMENTS)
            .next_power_of_two()
            .min(ARC_BUCKET_SEGMENTS[ARC_BUCKETS - 1]);
        assert_eq!(ring.segments(), expected, "segment count at {range:?}");
    }
}

#[test]
fn a_segment_is_cut_where_its_largest_class_would_collapse_more_than_a_draw_is_worth() {
    let mut scope = DrawScopeDefault::new(Size::new(2000.0, 2000.0));
    let brush = Brush::Solid(Color::WHITE);
    let quad = Rect {
        x: 1.0,
        y: 1.0,
        width: 4.0,
        height: 4.0,
    };
    let rects = 40;
    for _ in 0..rects {
        scope.draw_rect_at(quad, brush.clone());
    }
    scope.draw_arc(
        brush.clone(),
        Point::new(500.0, 500.0),
        400.0,
        0.0,
        TAU,
        Stroke::new(3.0),
    );
    for _ in 0..rects {
        scope.draw_rect_at(quad, brush.clone());
    }
    let recording = scope.finish();
    let ring = recording.shapes().get(rects).unwrap();
    assert!(ring.is_banded());
    let ring_quads = ring.band_segments();
    assert!(ring_quads > 1);
    let segments: Vec<(u32, u8)> = recording
        .tables()
        .segments
        .iter()
        .map(|segment| (segment.count, segment.band_class))
        .collect();
    let after = SEGMENT_WASTE_QUADS / (ring_quads - 1);
    assert_eq!(
        segments,
        [
            (rects as u32, 0),
            (1 + after, ring.band_class() as u8),
            (rects as u32 - after, 0)
        ],
        "the ring would collapse {rects} quads times {} vertices each, more than a draw is \
         worth, so it opens a segment; the rects after it join until their own collapse \
         passes the budget",
        ring_quads - 1
    );
}

#[test]
fn a_stroked_circle_is_a_band_and_a_stroked_pill_is_not() {
    let mut scope = DrawScopeDefault::new(Size::new(400.0, 400.0));
    let brush = Brush::Solid(Color::WHITE);
    let square = Rect {
        x: 10.0,
        y: 10.0,
        width: 100.0,
        height: 100.0,
    };
    scope.draw_round_rect_at_stroked(
        square,
        brush.clone(),
        CornerRadii::uniform(50.0),
        Stroke::new(4.0),
    );
    scope.draw_round_rect_at_stroked(
        square,
        brush.clone(),
        CornerRadii::uniform(20.0),
        Stroke::new(4.0),
    );
    scope.draw_round_rect_at(square, brush.clone(), CornerRadii::uniform(50.0));
    scope.draw_round_rect_at_stroked(
        Rect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 60.0,
        },
        brush,
        CornerRadii::uniform(30.0),
        Stroke::new(4.0),
    );
    let recording = scope.finish();
    let banded: Vec<bool> = recording
        .shapes()
        .iter()
        .map(|record| record.is_banded())
        .collect();
    assert_eq!(banded, [true, false, false, false]);
    let ring = recording.shapes().get(0).unwrap();
    assert_eq!(ring.kind(), RECORD_KIND_ROUND_RECT);
    assert_eq!(ring.fragment_kind(), FRAGMENT_KIND_STROKE);
    assert_eq!(ring.arc, [60.0, 60.0, 50.0, 48.0]);
    assert_eq!(ring.arc_band, [0.0, TAU, 48.0, 52.0]);
    assert_eq!(ring.band_segments(), 16);
    assert_eq!(ring.band_class(), 4);
}

#[test]
fn the_tables_are_shared_until_recorded_into_again() {
    let recording = CommandRecording::from_primitives(vec![DrawPrimitive::Rect {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        brush: Brush::Solid(Color::WHITE),
        stroke: None,
    }]);
    let held = Arc::clone(recording.shape_recorder());
    let recording = CommandRecorder::reusing(recording).finish();
    assert!(!Arc::ptr_eq(&held, recording.shape_recorder()));
    assert_eq!(held.tables().shapes.len(), 1);
    assert!(recording.is_empty());
    assert_ne!(held.tables(), CommandRecording::default().tables());
}
