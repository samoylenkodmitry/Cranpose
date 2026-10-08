use super::*;
use crate::{
    DrawScope, DrawScopeDefault, DrawTextStyle, ImageBitmap, ImageSampling, Size, TextPrimitive,
};

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn solid() -> Brush {
    Brush::Solid(Color(0.1, 0.2, 0.3, 0.4))
}

fn linear_explicit() -> Brush {
    Brush::LinearGradient {
        colors: vec![Color::RED, Color::GREEN, Color::BLUE],
        stops: Some(vec![0.0, 0.25, 1.0]),
        start: Point::new(1.0, 2.0),
        end: Point::new(3.0, 4.0),
        tile_mode: TileMode::Repeated,
    }
}

fn linear_mismatched_stops() -> Brush {
    Brush::LinearGradient {
        colors: vec![Color::RED, Color::BLUE],
        stops: Some(vec![0.5]),
        start: Point::new(0.0, 0.0),
        end: Point::new(0.0, 10.0),
        tile_mode: TileMode::Clamp,
    }
}

fn radial() -> Brush {
    Brush::RadialGradient {
        colors: vec![Color::WHITE, Color::BLACK],
        stops: None,
        center: Point::new(5.0, 6.0),
        radius: 7.0,
        tile_mode: TileMode::Mirror,
    }
}

fn sweep() -> Brush {
    Brush::SweepGradient {
        colors: vec![Color::RED],
        stops: Some(vec![]),
        center: Point::new(8.0, 9.0),
    }
}

fn image() -> ImageBitmap {
    ImageBitmap::from_rgba8(1, 1, vec![255, 0, 0, 255]).expect("a one-pixel image")
}

fn text() -> DrawPrimitive {
    DrawPrimitive::Text(Box::new(TextPrimitive {
        rect: rect(1.0, 1.0, 20.0, 10.0),
        text: "hi".into(),
        style: DrawTextStyle::default(),
        color: Color::WHITE,
    }))
}

#[test]
fn explicit_shape_blends_match_wrapped_shapes_and_preserve_other_primitives() {
    for mode in [BlendMode::SrcOver, BlendMode::DstOut, BlendMode::Plus] {
        for primitive in every_primitive() {
            let mut direct = ShapeRecorder::default();
            let mut wrapped = ShapeRecorder::default();
            let expected = primitive.clone();
            match direct.push_shape_primitive(primitive, mode) {
                Recorded::Shape(bounds) => {
                    let result = wrapped.push_primitive(DrawPrimitive::Blend {
                        primitive: Box::new(expected),
                        blend_mode: mode,
                    });
                    assert!(matches!(result, Recorded::Shape(other) if other == bounds));
                    assert!(
                        direct
                            .tables()
                            .segments
                            .iter()
                            .all(|segment| segment.blend == mode)
                    );
                    assert!(
                        direct
                            .tables()
                            .shapes
                            .iter()
                            .all(|body| body.blend_mode() == mode)
                    );
                    assert_eq!(direct, wrapped);
                }
                Recorded::Other(other) => {
                    assert_eq!(other, expected);
                    assert!(direct.is_empty());
                }
            }
        }
    }
}

fn every_primitive() -> Vec<DrawPrimitive> {
    let stroke = Stroke {
        width: 3.0,
        cap: StrokeCap::Round,
        join: StrokeJoin::Bevel,
    };
    vec![
        DrawPrimitive::Rect {
            rect: rect(0.0, 0.0, 10.0, 10.0),
            brush: solid(),
            stroke: None,
        },
        DrawPrimitive::Rect {
            rect: rect(1.0, 2.0, 3.0, 4.0),
            brush: linear_explicit(),
            stroke: Some(stroke),
        },
        DrawPrimitive::RoundRect {
            rect: rect(5.0, 5.0, 20.0, 10.0),
            brush: radial(),
            radii: CornerRadii {
                top_left: 1.0,
                top_right: 2.0,
                bottom_right: 3.0,
                bottom_left: 4.0,
            },
            stroke: Some(Stroke::new(1.0)),
        },
        DrawPrimitive::Arc {
            rect: rect(-1.0, -1.0, 2.0, 2.0),
            brush: sweep(),
            center: Point::new(0.0, 0.0),
            radius: 5.0,
            start_angle: 1.0,
            sweep_angle: -2.0,
            stroke: Some(stroke),
            inner_radius: 0.0,
        },
        DrawPrimitive::Arc {
            rect: rect(-5.0, -5.0, 10.0, 10.0),
            brush: linear_mismatched_stops(),
            center: Point::new(0.0, 0.0),
            radius: 5.0,
            start_angle: 0.0,
            sweep_angle: crate::TAU,
            stroke: None,
            inner_radius: 2.0,
        },
        DrawPrimitive::Arc {
            rect: rect(0.0, 0.0, 0.0, 0.0),
            brush: solid(),
            center: Point::new(0.0, 0.0),
            radius: 5.0,
            start_angle: 0.0,
            sweep_angle: 0.0,
            stroke: None,
            inner_radius: 0.0,
        },
        DrawPrimitive::Blend {
            primitive: Box::new(DrawPrimitive::Rect {
                rect: rect(0.0, 0.0, 1.0, 1.0),
                brush: solid(),
                stroke: None,
            }),
            blend_mode: BlendMode::Plus,
        },
        DrawPrimitive::Blend {
            primitive: Box::new(DrawPrimitive::Blend {
                primitive: Box::new(DrawPrimitive::Rect {
                    rect: rect(0.0, 0.0, 1.0, 1.0),
                    brush: solid(),
                    stroke: None,
                }),
                blend_mode: BlendMode::Xor,
            }),
            blend_mode: BlendMode::Luminosity,
        },
        DrawPrimitive::Blend {
            primitive: Box::new(DrawPrimitive::Image {
                rect: rect(0.0, 0.0, 1.0, 1.0),
                image: image(),
                alpha: 0.5,
                color_filter: None,
                sampling: ImageSampling::Linear,
                src_rect: None,
            }),
            blend_mode: BlendMode::Screen,
        },
        DrawPrimitive::Image {
            rect: rect(2.0, 2.0, 4.0, 4.0),
            image: image(),
            alpha: 1.0,
            color_filter: None,
            sampling: ImageSampling::Nearest,
            src_rect: Some(rect(0.0, 0.0, 1.0, 1.0)),
        },
        text(),
        DrawPrimitive::Shadow(crate::ShadowPrimitive::Drop {
            shape: Box::new(DrawPrimitive::Rect {
                rect: rect(0.0, 0.0, 1.0, 1.0),
                brush: solid(),
                stroke: None,
            }),
            cutout: None,
            blur_radius: 2.0,
            blend_mode: BlendMode::SrcOver,
        }),
        DrawPrimitive::Content,
        DrawPrimitive::Rect {
            rect: rect(9.0, 9.0, 1.0, 1.0),
            brush: solid(),
            stroke: None,
        },
        DrawPrimitive::Line {
            rect: rect(1.0, 2.0, 10.0, 5.0),
            brush: linear_explicit(),
            start: Point::new(1.0, 2.0),
            end: Point::new(11.0, 7.0),
            stroke: Stroke {
                width: 4.0,
                cap: StrokeCap::Square,
                join: StrokeJoin::Miter,
            },
        },
        DrawPrimitive::Line {
            rect: rect(3.0, 3.0, 0.0, 0.0),
            brush: solid(),
            start: Point::new(3.0, 3.0),
            end: Point::new(3.0, 3.0),
            stroke: Stroke::new(2.0),
        },
    ]
}

fn scan_summary(primitives: &[DrawPrimitive]) -> RecordingSummary {
    let mut summary = RecordingSummary::default();
    for primitive in primitives {
        summary.note(primitive);
    }
    summary
}

/// The facts of `summary` a scan of primitive kinds finds, without the
/// lanes the recording put them in.
fn kinds_of(summary: RecordingSummary) -> RecordingSummary {
    RecordingSummary {
        has_shapes: false,
        has_others: false,
        has_content_markers: false,
        ..summary
    }
}

#[test]
fn every_primitive_round_trips_through_the_record_byte_for_byte() {
    let primitives = every_primitive();
    let recording = CommandRecording::from_primitives(primitives.clone());
    assert_eq!(recording.into_primitives_with_markers(), primitives);
}

#[test]
fn shapes_and_blended_shapes_are_records_everything_else_is_not() {
    let recording = CommandRecording::from_primitives(every_primitive());
    assert_eq!(
        recording.shapes().len(),
        10,
        "six shapes and one blended, then a rect and two lines after content"
    );
    assert_eq!(
        recording.others().len(),
        5,
        "a nested blend, a blended image, an image, a text and a shadow"
    );
    assert_eq!(recording.content_markers(), 1);
    assert_eq!(recording.len(), 16);
}

#[test]
fn the_scope_records_the_arc_bounds_the_primitive_used_to_carry() {
    let center = Point::new(50.0, 40.0);
    let stroke = Stroke::new(4.0);
    let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
    scope.draw_arc(solid(), center, 30.0, 0.5, -1.5, stroke);
    scope.draw_annular_sector(radial(), center, 10.0, 20.0, 0.0, 2.0);
    scope.draw_arc(solid(), center, 30.0, 0.5, 0.0, stroke);
    let (band_inner, band_outer, cap) = arc_band(30.0, 0.0, Some(stroke));
    let stroked_bounds = ArcGeometry::new(center, band_inner, band_outer, 0.5, -1.5, cap).bounds();
    let sector_bounds = ArcGeometry::new(center, 10.0, 20.0, 0.0, 2.0, StrokeCap::Butt).bounds();
    assert_eq!(
        scope.into_primitives(),
        vec![
            DrawPrimitive::Arc {
                rect: stroked_bounds,
                brush: solid(),
                center,
                radius: 30.0,
                start_angle: 0.5,
                sweep_angle: -1.5,
                stroke: Some(stroke),
                inner_radius: 0.0,
            },
            DrawPrimitive::Arc {
                rect: sector_bounds,
                brush: radial(),
                center,
                radius: 20.0,
                start_angle: 0.0,
                sweep_angle: 2.0,
                stroke: None,
                inner_radius: 10.0,
            },
        ],
        "a zero sweep records nothing, as the scope always did"
    );
}

#[test]
fn the_arc_record_carries_the_normalised_band_the_fragment_stage_reads() {
    let recording = CommandRecording::from_primitives(vec![DrawPrimitive::Arc {
        rect: rect(0.0, 0.0, 1.0, 1.0),
        brush: solid(),
        center: Point::new(3.0, 4.0),
        radius: 10.0,
        start_angle: 1.0,
        sweep_angle: -2.0,
        stroke: Some(Stroke::new(4.0).with_cap(StrokeCap::Square)),
        inner_radius: 0.0,
    }]);
    let record = recording.shapes().get(0).unwrap();
    let geometry = record.arc_geometry().expect("an arc");
    let expected = ArcGeometry::new(
        Point::new(3.0, 4.0),
        8.0,
        12.0,
        1.0,
        -2.0,
        StrokeCap::Square,
    );
    assert_eq!(geometry, expected);
    assert_eq!(
        record.radii, [0.0; 4],
        "an arc records no trig: the vertex stage derives it from its angles"
    );
    assert_eq!(
        record.arc_normalized[..2],
        [expected.start_angle, expected.sweep_angle]
    );
    assert_eq!(
        record.arc_normalized[2..],
        [0.0, 0.0],
        "the vertex stage pads the strip's sweep in device pixels"
    );
    assert!(BandRing::of_geometry(&expected).range > 2.0);
    assert!(!record.is_degenerate_arc());
    assert!(!record.has_loose_rect());
    assert_eq!(record.rect_value(), rect(0.0, 0.0, 1.0, 1.0));
    let degenerate = CommandRecording::from_primitives(vec![DrawPrimitive::Arc {
        rect: rect(0.0, 0.0, 1.0, 1.0),
        brush: solid(),
        center: Point::new(3.0, 4.0),
        radius: 10.0,
        start_angle: 1.0,
        sweep_angle: 0.0,
        stroke: None,
        inner_radius: 0.0,
    }]);
    assert!(degenerate.shapes().get(0).unwrap().is_degenerate_arc());
}

#[test]
fn segments_cut_on_blend_and_brush_class_and_lane_never_on_kind() {
    let mut recording = CommandRecorder::default();
    recording.push_rect(rect(0.0, 0.0, 1.0, 1.0), &solid(), None, BlendMode::SrcOver);
    recording.push_round_rect(
        rect(0.0, 0.0, 1.0, 1.0),
        &solid(),
        CornerRadii::uniform(1.0),
        Some(Stroke::new(1.0)),
        BlendMode::SrcOver,
    );
    recording.push_arc(
        rect(0.0, 0.0, 1.0, 1.0),
        &ArcRecordArgs {
            brush: &solid(),
            center: Point::new(0.0, 0.0),
            radius: 5.0,
            start_angle: 0.0,
            sweep_angle: 1.0,
            stroke: None,
            inner_radius: 1.0,
            blend_mode: BlendMode::SrcOver,
        },
    );
    recording.push_rect(
        rect(0.0, 0.0, 1.0, 1.0),
        &radial(),
        None,
        BlendMode::SrcOver,
    );
    recording.push_rect(rect(0.0, 0.0, 1.0, 1.0), &solid(), None, BlendMode::Plus);
    recording.push_other(text());
    recording.push_other(text());
    recording.push_content();
    recording.push_rect(rect(0.0, 0.0, 1.0, 1.0), &solid(), None, BlendMode::SrcOver);
    let recording = recording.finish();
    let lanes: Vec<(RecordLane, u32, u32, BlendMode, bool, u8)> = recording
        .segments()
        .iter()
        .map(|segment| {
            (
                segment.lane,
                segment.start,
                segment.count,
                segment.blend,
                segment.gradient,
                segment.kinds,
            )
        })
        .collect();
    assert_eq!(
        lanes,
        vec![
            (RecordLane::Shapes, 0, 3, BlendMode::SrcOver, false, 0b111),
            (RecordLane::Shapes, 3, 1, BlendMode::SrcOver, true, 0b1),
            (RecordLane::Shapes, 4, 1, BlendMode::Plus, false, 0b1),
            (RecordLane::Others, 0, 2, BlendMode::SrcOver, false, 0),
            (RecordLane::Content, 0, 1, BlendMode::SrcOver, false, 0),
            (RecordLane::Shapes, 5, 1, BlendMode::SrcOver, false, 0b1),
        ]
    );
    assert_eq!(recording.segments()[0].uniform_kind(), None);
    assert_eq!(
        recording.segments()[1].uniform_kind(),
        Some(FRAGMENT_KIND_FILL)
    );
}

#[test]
fn a_segment_reports_its_one_brush_kind_and_none_for_a_mixed_or_empty_mask() {
    let mut recording = CommandRecorder::default();
    recording.push_rect(
        rect(0.0, 0.0, 1.0, 1.0),
        &linear_explicit(),
        None,
        BlendMode::SrcOver,
    );
    recording.push_rect(
        rect(1.0, 0.0, 1.0, 1.0),
        &linear_explicit(),
        None,
        BlendMode::SrcOver,
    );
    recording.push_rect(rect(2.0, 0.0, 1.0, 1.0), &solid(), None, BlendMode::SrcOver);
    recording.push_rect(
        rect(3.0, 0.0, 1.0, 1.0),
        &linear_explicit(),
        None,
        BlendMode::SrcOver,
    );
    recording.push_rect(
        rect(4.0, 0.0, 1.0, 1.0),
        &radial(),
        None,
        BlendMode::SrcOver,
    );
    recording.push_other(text());
    let recording = recording.finish();
    let brushes: Vec<(u8, Option<u32>)> = recording
        .segments()
        .iter()
        .map(|segment| (segment.brushes, segment.uniform_brush()))
        .collect();
    assert_eq!(
        brushes,
        vec![
            (1 << BRUSH_KIND_LINEAR, Some(BRUSH_KIND_LINEAR)),
            (1, Some(0)),
            ((1 << BRUSH_KIND_LINEAR) | (1 << BRUSH_KIND_RADIAL), None),
            (0, None),
        ],
        "two linears agree, a solid run has no brush, a linear beside a radial \
         disagree, and a lane without shape records carries an empty mask"
    );
}

#[test]
fn the_content_split_follows_the_last_marker() {
    let recording = CommandRecording::from_primitives(vec![
        DrawPrimitive::Rect {
            rect: rect(1.0, 0.0, 1.0, 1.0),
            brush: solid(),
            stroke: None,
        },
        DrawPrimitive::Content,
        DrawPrimitive::Rect {
            rect: rect(2.0, 0.0, 1.0, 1.0),
            brush: solid(),
            stroke: None,
        },
        DrawPrimitive::Content,
        DrawPrimitive::Rect {
            rect: rect(3.0, 0.0, 1.0, 1.0),
            brush: solid(),
            stroke: None,
        },
    ]);
    let xs = |segments: Range<u32>| -> Vec<f32> {
        recording
            .primitives(segments)
            .map(|primitive| match primitive {
                DrawPrimitive::Rect { rect, .. } => rect.x,
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    };
    assert_eq!(xs(recording.content_split(true)), [1.0, 2.0]);
    assert_eq!(xs(recording.content_split(false)), [3.0]);
    assert_eq!(xs(recording.all_segments()), [1.0, 2.0, 3.0]);
    let unsplit = CommandRecording::from_primitives(vec![DrawPrimitive::Rect {
        rect: rect(4.0, 0.0, 1.0, 1.0),
        brush: solid(),
        stroke: None,
    }]);
    assert!(unsplit.is_empty_in(&unsplit.content_split(true)));
    assert_eq!(unsplit.content_split(false), unsplit.all_segments());
    assert_eq!(unsplit.len_in(&unsplit.all_segments()), 1);
}

#[test]
fn segment_iteration_preserves_order_bounds_and_marker_filtering() {
    let primitives = every_primitive();
    let recording = CommandRecording::from_primitives(primitives.clone());
    let offsets: Vec<_> = std::iter::once(0)
        .chain(recording.segments().iter().scan(0, |offset, segment| {
            *offset += segment.count as usize;
            Some(*offset)
        }))
        .collect();
    assert_eq!(offsets.last(), Some(&primitives.len()));
    for start in 0..offsets.len() {
        for end in start..offsets.len() {
            let selected = &primitives[offsets[start]..offsets[end]];
            let segments = start as u32..end as u32;
            let expected: Vec<_> = selected
                .iter()
                .filter(|primitive| !matches!(primitive, DrawPrimitive::Content))
                .cloned()
                .collect();
            assert_eq!(
                recording.primitives(segments.clone()).collect::<Vec<_>>(),
                expected
            );
            let expected: Vec<_> = selected
                .iter()
                .filter_map(primitive_coverage_rect)
                .collect();
            assert_eq!(
                recording.coverage_rects(segments).collect::<Vec<_>>(),
                expected
            );
        }
    }
    assert_eq!(recording.into_primitives_with_markers(), primitives);
}

#[test]
fn a_record_reach_rect_holds_its_coverage_rect() {
    let recording = CommandRecording::from_primitives(every_primitive());
    let shapes = recording.shapes();
    for (index, body) in shapes.bodies().iter().enumerate() {
        let reach = body.reach_rect();
        let coverage = shapes.get(index).expect("a recorded shape").coverage_rect();
        assert!(
            reach.x <= coverage.x
                && reach.y <= coverage.y
                && reach.x + reach.width >= coverage.x + coverage.width
                && reach.y + reach.height >= coverage.y + coverage.height,
            "record {index}: {reach:?} misses {coverage:?}"
        );
    }
}

#[test]
fn summary_and_bounds_are_what_a_scan_of_the_primitives_finds() {
    let primitives = every_primitive();
    let recording = CommandRecording::from_primitives(primitives.clone());
    assert_eq!(kinds_of(recording.summary()), scan_summary(&primitives));
    let expected_bounds = primitives
        .iter()
        .filter_map(primitive_coverage_rect)
        .reduce(|a, b| a.union(b));
    assert_eq!(recording.bounds(), expected_bounds);
    assert_eq!(
        kinds_of(recording.summary_in(&recording.content_split(false))),
        scan_summary(&primitives[primitives.len() - 1..])
    );
    let rects: Vec<Rect> = recording.coverage_rects(recording.all_segments()).collect();
    let expected: Vec<Rect> = primitives
        .iter()
        .filter_map(primitive_coverage_rect)
        .collect();
    assert_eq!(rects, expected);
}

#[test]
fn a_scope_arc_keeps_the_disc_and_derives_the_tight_bounds() {
    let center = Point::new(50.0, 40.0);
    let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
    scope.draw_arc(solid(), center, 30.0, 0.5, 1.0, Stroke::new(4.0));
    let recording = scope.finish();
    let record = recording.shapes().get(0).unwrap();
    assert!(record.has_loose_rect());
    let tight = ArcGeometry::new(center, 28.0, 32.0, 0.5, 1.0, StrokeCap::Butt).bounds();
    assert_eq!(record.rect_value(), tight);
    assert_eq!(record.coverage_rect(), expand_rect(tight, 2.0));
    let stored = record.stored_rect();
    assert!(stored.x <= tight.x && stored.y <= tight.y);
    assert!(stored.x + stored.width >= tight.x + tight.width);
    assert!(stored.y + stored.height >= tight.y + tight.height);
    let bounds = recording.bounds().expect("one arc gives bounds");
    assert_eq!(bounds, expand_rect(stored, 2.0));
}

#[test]
fn a_shadow_only_recording_summarises_as_shadow() {
    let recording = CommandRecording::from_primitives(vec![DrawPrimitive::Shadow(
        crate::ShadowPrimitive::Drop {
            shape: Box::new(DrawPrimitive::Rect {
                rect: rect(0.0, 0.0, 1.0, 1.0),
                brush: solid(),
                stroke: None,
            }),
            cutout: None,
            blur_radius: 1.0,
            blend_mode: BlendMode::SrcOver,
        },
    )]);
    assert_eq!(
        recording.summary(),
        RecordingSummary {
            has_shadow: true,
            has_others: true,
            ..RecordingSummary::default()
        }
    );
    assert_eq!(recording.bounds(), None);
}

#[test]
fn the_fingerprint_sees_every_stop_the_order_and_the_blend() {
    let base = || CommandRecording::from_primitives(every_primitive());
    assert_eq!(base().fingerprint(), base().fingerprint());
    let mut primitives = every_primitive();
    let DrawPrimitive::Rect { brush, .. } = &mut primitives[1] else {
        unreachable!()
    };
    let Brush::LinearGradient { colors, .. } = brush else {
        unreachable!()
    };
    colors[1] = Color::WHITE;
    let recoloured_stop = CommandRecording::from_primitives(primitives);
    assert_ne!(recoloured_stop.fingerprint(), base().fingerprint());
    assert_eq!(
        recoloured_stop.shapes(),
        base().shapes(),
        "the record itself is unchanged by a stop colour, which is why the fingerprint must cover the stops"
    );
    let mut primitives = every_primitive();
    let DrawPrimitive::Rect { brush, .. } = &mut primitives[1] else {
        unreachable!()
    };
    let Brush::LinearGradient { stops, .. } = brush else {
        unreachable!()
    };
    *stops = Some(vec![0.0, 0.5, 1.0]);
    assert_ne!(
        CommandRecording::from_primitives(primitives).fingerprint(),
        base().fingerprint()
    );
    let mut primitives = every_primitive();
    primitives.swap(0, 1);
    assert_ne!(
        CommandRecording::from_primitives(primitives).fingerprint(),
        base().fingerprint()
    );
    let mut primitives = every_primitive();
    let DrawPrimitive::Blend { blend_mode, .. } = &mut primitives[6] else {
        unreachable!()
    };
    *blend_mode = BlendMode::Screen;
    assert_ne!(
        CommandRecording::from_primitives(primitives).fingerprint(),
        base().fingerprint()
    );
    let mut primitives = every_primitive();
    primitives.pop();
    assert_ne!(
        CommandRecording::from_primitives(primitives).fingerprint(),
        base().fingerprint()
    );
}

#[test]
fn clearing_keeps_the_capacity_and_forgets_the_content() {
    let recording = CommandRecording::from_primitives(every_primitive());
    let capacity = recording.shape_capacity();
    let fingerprint = recording.fingerprint();
    let recording = CommandRecorder::reusing(recording).finish();
    assert!(recording.is_empty());
    assert_eq!(recording.shape_capacity(), capacity);
    assert_eq!(recording.segments().len(), 0);
    assert_eq!(recording.bounds(), None);
    assert_eq!(recording.summary(), RecordingSummary::default());
    assert_ne!(recording.fingerprint(), fingerprint);
    assert_eq!(
        recording.fingerprint(),
        CommandRecording::default().fingerprint()
    );
}

#[test]
fn publishing_and_unique_reuse_move_the_shape_columns() {
    let mut recorder = CommandRecorder::default();
    assert!(recorder.is_empty());
    recorder.reserve_shapes(512);
    recorder.push_primitive(every_primitive().remove(0));
    recorder.push_content();
    assert_eq!(recorder.len(), 2);
    assert_eq!(recorder.content_markers(), 1);
    let body_pointer = recorder.shapes.tables.shapes.bodies().as_ptr();
    let published = recorder.finish();
    assert_eq!(published.shapes().bodies().as_ptr(), body_pointer);
    let capacity = published.shape_capacity();
    let mut reused = CommandRecorder::reusing(published);
    assert!(reused.is_empty());
    assert_eq!(reused.content_markers(), 0);
    reused.push_primitive(every_primitive().remove(0));
    let published = reused.finish();
    assert_eq!(published.len(), 1);
    assert_eq!(published.shape_capacity(), capacity);
    assert_eq!(published.shapes().bodies().as_ptr(), body_pointer);
}

#[test]
fn a_recording_nothing_else_holds_is_recorded_again_in_its_own_allocation() {
    let first = CommandRecording::from_primitives(every_primitive());
    let allocation = Arc::as_ptr(first.shape_recorder());
    let mut reused = CommandRecorder::reusing(first);
    reused.push_primitive(every_primitive().remove(0));
    let again = reused.finish();
    assert_eq!(Arc::as_ptr(again.shape_recorder()), allocation);
    assert_eq!(again.len(), 1);

    let edited = again.into_recorder().finish();
    assert_eq!(Arc::as_ptr(edited.shape_recorder()), allocation);
    assert_eq!(edited.len(), 1);
}

#[test]
fn a_recording_a_reader_still_holds_keeps_its_shapes() {
    let first = CommandRecording::from_primitives(every_primitive());
    let held = first.clone();
    let mut reused = CommandRecorder::reusing(first);
    reused.push_primitive(every_primitive().remove(0));
    let again = reused.finish();
    assert!(!Arc::ptr_eq(again.shape_recorder(), held.shape_recorder()));
    assert_eq!(held, CommandRecording::from_primitives(every_primitive()));
    assert_eq!(again.len(), 1);

    let recorder = CommandRecorder::reusing(again.clone());
    let copy = recorder.clone();
    assert!(!Arc::ptr_eq(
        recorder.finish().shape_recorder(),
        again.shape_recorder()
    ));
    assert!(!Arc::ptr_eq(
        copy.finish().shape_recorder(),
        again.shape_recorder()
    ));
}

#[test]
fn empty_recordings_share_their_shapes() {
    let first = CommandRecording::default();
    let second = CommandRecording::default();
    assert!(Arc::ptr_eq(first.shape_recorder(), second.shape_recorder()));
    assert!(first.is_empty());
    assert_eq!(first, CommandRecorder::default().finish());
}

#[test]
fn the_record_is_seven_rows() {
    assert_eq!(std::mem::size_of::<ShapeRecord>(), 112);
    assert_eq!(std::mem::size_of::<BrushRecord>(), 48);
    assert_eq!(std::mem::size_of::<GradientStopRecord>(), 32);
}

fn only_segment_occluders(record: impl FnOnce(&mut ShapeRecorder)) -> bool {
    let mut recorder = ShapeRecorder::default();
    record(&mut recorder);
    let segments = &recorder.tables().segments;
    assert_eq!(segments.len(), 1, "one segment: {segments:?}");
    segments[0].occluders
}

fn opaque() -> Brush {
    Brush::Solid(Color(0.1, 0.2, 0.3, 1.0))
}

#[test]
fn opaque_backgrounds_and_cards_mark_their_segment_as_occluding() {
    assert!(only_segment_occluders(|recorder| {
        recorder.push_rect(
            rect(0.0, 0.0, 300.0, 200.0),
            &opaque(),
            None,
            BlendMode::SrcOver,
        );
    }));
    assert!(only_segment_occluders(|recorder| {
        recorder.push_round_rect(
            rect(0.0, 0.0, 120.0, 80.0),
            &opaque(),
            CornerRadii::uniform(12.0),
            None,
            BlendMode::SrcOver,
        );
    }));
}

type Recording = Box<dyn FnOnce(&mut ShapeRecorder)>;

#[test]
fn chips_translucent_gradient_and_stroked_fills_occlude_nothing() {
    let unmarked: [Recording; 4] = [
        Box::new(|recorder| {
            recorder.push_round_rect(
                rect(0.0, 0.0, 50.0, 20.0),
                &opaque(),
                CornerRadii::uniform(2.0),
                None,
                BlendMode::SrcOver,
            );
        }),
        Box::new(|recorder| {
            recorder.push_rect(
                rect(0.0, 0.0, 300.0, 200.0),
                &solid(),
                None,
                BlendMode::SrcOver,
            );
        }),
        Box::new(|recorder| {
            recorder.push_rect(
                rect(0.0, 0.0, 300.0, 200.0),
                &linear_explicit(),
                None,
                BlendMode::SrcOver,
            );
        }),
        Box::new(|recorder| {
            recorder.push_rect(
                rect(0.0, 0.0, 300.0, 200.0),
                &opaque(),
                Some(Stroke {
                    width: 4.0,
                    cap: StrokeCap::Butt,
                    join: StrokeJoin::Miter,
                }),
                BlendMode::SrcOver,
            );
        }),
    ];
    for record in unmarked {
        assert!(!only_segment_occluders(record));
    }
}

fn only_segment_interiors(record: impl FnOnce(&mut ShapeRecorder)) -> bool {
    let mut recorder = ShapeRecorder::default();
    record(&mut recorder);
    let segments = &recorder.tables().segments;
    assert_eq!(segments.len(), 1, "one segment: {segments:?}");
    segments[0].interiors
}

#[test]
fn a_card_with_a_large_interior_marks_its_segment() {
    assert!(only_segment_interiors(|recorder| {
        recorder.push_round_rect(
            rect(0.0, 0.0, 300.0, 200.0),
            &solid(),
            CornerRadii::uniform(16.0),
            None,
            BlendMode::SrcOver,
        );
    }));
}

#[test]
fn an_opaque_circle_occludes_by_its_inset_square_once_that_is_large() {
    let circle = |diameter: f32| {
        move |recorder: &mut ShapeRecorder| {
            recorder.push_round_rect(
                rect(0.0, 0.0, diameter, diameter),
                &opaque(),
                CornerRadii::uniform(diameter / 2.0),
                None,
                BlendMode::SrcOver,
            );
        }
    };
    // Half of a 200 × 200 circle's rect lies inside its inset square; half
    // of a 40 × 40 one is under the least an occluder takes.
    assert!(only_segment_occluders(circle(200.0)));
    assert!(!only_segment_occluders(circle(40.0)));
}

#[test]
fn plain_rects_and_strokes_leave_the_segment_unmarked() {
    assert!(!only_segment_interiors(|recorder| {
        recorder.push_rect(
            rect(0.0, 0.0, 300.0, 200.0),
            &solid(),
            None,
            BlendMode::SrcOver,
        );
    }));
    assert!(!only_segment_interiors(|recorder| {
        recorder.push_round_rect(
            rect(0.0, 0.0, 300.0, 200.0),
            &solid(),
            CornerRadii::uniform(16.0),
            Some(Stroke {
                width: 2.0,
                ..Default::default()
            }),
            BlendMode::SrcOver,
        );
    }));
}

#[test]
fn a_circle_whose_corners_take_all_of_it_leaves_the_segment_unmarked() {
    // Its bands are empty: a gradient batch would test nothing.
    assert!(!only_segment_interiors(|recorder| {
        recorder.push_round_rect(
            rect(0.0, 0.0, 24.0, 24.0),
            &solid(),
            CornerRadii::uniform(12.0),
            None,
            BlendMode::SrcOver,
        );
    }));
}

#[test]
fn a_chip_marks_the_segment_by_the_band_between_its_corners() {
    // Inset by its corners on every side, the chip keeps 44 × 8 of its 60 × 24;
    // the band between its left and right corners keeps 44 × 24.
    assert!(only_segment_interiors(|recorder| {
        recorder.push_round_rect(
            rect(0.0, 0.0, 60.0, 24.0),
            &solid(),
            CornerRadii::uniform(8.0),
            None,
            BlendMode::SrcOver,
        );
    }));
}

/// How far in from each side a corner of `radius` leaves the inset rect.
fn arc_inset(radius: f32) -> f32 {
    radius * (1.0 - std::f32::consts::FRAC_1_SQRT_2)
}

#[test]
fn a_fill_band_is_the_larger_one_its_corners_leave_whole() {
    assert_eq!(band_interior_area(60.0, 24.0, [8.0; 4]), 44.0 * 24.0);
    assert_eq!(band_interior_area(24.0, 60.0, [8.0; 4]), 24.0 * 44.0);
    // Top-left, top-right, bottom-right, bottom-left: the left side's larger
    // corner and the right side's bound the band across.
    assert_eq!(
        band_interior_area(100.0, 40.0, [4.0, 10.0, 2.0, 6.0]),
        (100.0 - 6.0 - 10.0) * 40.0
    );
    assert_eq!(band_interior_area(24.0, 24.0, [12.0; 4]), 0.0);
    assert_eq!(band_interior_area(10.0, 10.0, [-3.0; 4]), 100.0);
}

#[test]
fn a_fill_interior_is_the_largest_of_its_two_bands_and_its_inset_rect() {
    // A long pill keeps its band: its corners take a whole height's worth
    // off either end, but only a little off every side of the inset rect.
    assert_eq!(fill_interior_area(200.0, 40.0, [20.0; 4]), 160.0 * 40.0);
    assert_eq!(fill_interior_area(40.0, 200.0, [20.0; 4]), 40.0 * 160.0);
    // Top-left, top-right, bottom-right, bottom-left: the left side's larger
    // corner and the right side's bound the band across.
    assert_eq!(
        fill_interior_area(200.0, 60.0, [20.0, 30.0, 10.0, 16.0]),
        (200.0 - 20.0 - 30.0) * 60.0
    );
    // A card whose corners are small next to it keeps the inset rect.
    let card = (60.0 - 2.0 * arc_inset(8.0)) * (24.0 - 2.0 * arc_inset(8.0));
    assert!((fill_interior_area(60.0, 24.0, [8.0; 4]) - card).abs() < 1e-3);
    assert!(card > 44.0 * 24.0);
    // A circle's bands are empty; its inset square is half its rect.
    assert!((fill_interior_area(24.0, 24.0, [12.0; 4]) - 288.0).abs() < 1e-3);
    assert_eq!(fill_interior_area(30.0, 20.0, [0.0; 4]), 600.0);
    assert_eq!(fill_interior_area(10.0, 10.0, [-3.0; 4]), 100.0);
}

#[test]
fn one_large_interior_marks_the_segment_the_small_shapes_share() {
    assert!(only_segment_interiors(|recorder| {
        recorder.push_round_rect(
            rect(0.0, 0.0, 40.0, 40.0),
            &solid(),
            CornerRadii::uniform(20.0),
            None,
            BlendMode::SrcOver,
        );
        recorder.push_round_rect(
            rect(0.0, 0.0, 300.0, 200.0),
            &solid(),
            CornerRadii::uniform(16.0),
            None,
            BlendMode::SrcOver,
        );
    }));
}

#[test]
fn arc_trig_is_the_mid_and_half_sweep_trig_with_the_full_circle_sentinel() {
    for (start, sweep) in [
        (0.3, 0.1),
        (0.4, 0.2),
        (1.0, 2.0),
        (5.9, 3.0),
        (0.0, -0.0),
        (0.2, TAU),
        (0.0, 7.0),
    ] {
        let expected = if sweep >= TAU && start == 0.0 {
            [0.0, -1.0, 0.0, -1.0]
        } else {
            let half = sweep.clamp(0.0, TAU) * 0.5;
            let (mid_sin, mid_cos) = (start + half).sin_cos();
            let (half_sin, half_cos) = half.sin_cos();
            [mid_sin, mid_cos, half_sin.max(0.0), half_cos]
        };
        assert_eq!(
            arc_trig(start, sweep).map(f32::to_bits),
            expected.map(f32::to_bits),
            "start={start} sweep={sweep}"
        );
    }
}

fn only_segment_flags(brush: &Brush) -> (bool, bool, bool) {
    let mut recorder = ShapeRecorder::default();
    recorder.push_round_rect(
        rect(0.0, 0.0, 200.0, 120.0),
        brush,
        CornerRadii::uniform(12.0),
        None,
        BlendMode::SrcOver,
    );
    let segments = &recorder.tables().segments;
    assert_eq!(segments.len(), 1, "one segment: {segments:?}");
    let segment = &segments[0];
    (segment.interiors, segment.occluders, segment.bare_interiors)
}

#[test]
fn only_a_gradient_or_translucent_fill_leaves_its_interior_bare() {
    assert_eq!(
        only_segment_flags(&opaque()),
        (true, true, false),
        "an opaque card's interior is laid down ahead"
    );
    assert_eq!(
        only_segment_flags(&solid()),
        (true, false, true),
        "a translucent card's interior is shaded apart"
    );
    assert_eq!(
        only_segment_flags(&linear_explicit()),
        (true, false, true),
        "a gradient card's interior is shaded apart"
    );
}

#[test]
fn a_segment_keeps_every_interior_flag_its_records_would_set() {
    let card = |recorder: &mut ShapeRecorder, brush: &Brush| {
        recorder.push_round_rect(
            rect(0.0, 0.0, 200.0, 120.0),
            brush,
            CornerRadii::uniform(12.0),
            None,
            BlendMode::SrcOver,
        );
    };
    let mut recorder = ShapeRecorder::default();
    card(&mut recorder, &opaque());
    card(&mut recorder, &opaque());
    card(&mut recorder, &solid());
    let segments = &recorder.tables().segments;
    assert_eq!(segments.len(), 1, "one segment: {segments:?}");
    let segment = &segments[0];
    assert_eq!(
        (segment.interiors, segment.occluders, segment.bare_interiors),
        (true, true, true),
        "a translucent card joining opaque ones still leaves its interior bare"
    );
}

#[test]
fn a_drawn_line_records_as_a_line_and_its_slanted_square_ends_widen_the_bounds() {
    let mut scope = crate::DrawScopeDefault::new(crate::Size::new(64.0, 64.0));
    let stroke = Stroke {
        width: 10.0,
        cap: StrokeCap::Square,
        join: StrokeJoin::Miter,
    };
    let (start, end) = (Point::new(20.0, 20.0), Point::new(50.0, 60.0));
    scope.draw_line(solid(), start, end, stroke);
    let line = crate::LineGeometry::new(start, end, stroke);
    assert_eq!(
        scope.into_primitives(),
        vec![DrawPrimitive::Line {
            rect: line.end_bounds(),
            brush: solid(),
            start,
            end,
            stroke,
        }]
    );

    let mut recorder = ShapeRecorder::default();
    let coverage = recorder.push_line(&line, &solid(), stroke, BlendMode::SrcOver);
    assert_eq!(coverage, line.bounds());
    assert!(
        coverage.x < line.end_bounds().x - stroke.width * 0.5,
        "a square end on a slant reaches past half the width"
    );
    assert_eq!(recorder.bounds(), Some(line.bounds()));
    let record = recorder.tables().shapes.get(0).expect("the line's record");
    assert_eq!(record.fragment_kind(), FRAGMENT_KIND_LINE);
    assert_eq!(record.line_geometry(), Some(line));
    assert_eq!(record.coverage_rect(), line.bounds());
}

#[test]
fn a_draw_scope_skips_a_line_that_covers_nothing() {
    let mut scope = crate::DrawScopeDefault::new(crate::Size::new(64.0, 64.0));
    let point = Point::new(8.0, 8.0);
    scope.draw_line(solid(), point, point, Stroke::new(4.0));
    scope.draw_line(solid(), point, Point::new(20.0, 8.0), Stroke::new(0.0));
    assert!(scope.into_primitives().is_empty());
}

#[test]
fn a_blended_line_keeps_its_blend_mode() {
    let mut scope = crate::DrawScopeDefault::new(crate::Size::new(64.0, 64.0));
    let (start, end, stroke) = (
        Point::new(4.0, 4.0),
        Point::new(40.0, 4.0),
        Stroke::new(2.0),
    );
    scope.draw_line_blend(solid(), start, end, stroke, BlendMode::Plus);
    assert_eq!(
        scope.into_primitives(),
        vec![DrawPrimitive::Blend {
            primitive: Box::new(DrawPrimitive::Line {
                rect: crate::LineGeometry::new(start, end, stroke).end_bounds(),
                brush: solid(),
                start,
                end,
                stroke,
            }),
            blend_mode: BlendMode::Plus,
        }]
    );
}

#[test]
fn only_a_run_of_shapes_alone_reads_as_shapes_only() {
    let shape = || DrawPrimitive::Rect {
        rect: rect(0.0, 0.0, 2.0, 2.0),
        brush: solid(),
        stroke: None,
    };
    let shapes = CommandRecording::from_primitives([shape(), shape()]);
    assert!(shapes.summary().shapes_only());

    let with_text = CommandRecording::from_primitives([shape(), text(), shape()]);
    assert!(!with_text.summary().shapes_only());
    assert!(
        !with_text
            .summary_in(&with_text.all_segments())
            .shapes_only()
    );

    let with_content =
        CommandRecording::from_primitives([shape(), DrawPrimitive::Content, shape()]);
    assert!(!with_content.summary().shapes_only());
    let behind = with_content.content_split(true);
    let overlay = with_content.content_split(false);
    assert!(with_content.summary_in(&behind).shapes_only());
    assert!(with_content.summary_in(&overlay).shapes_only());

    assert!(
        !CommandRecording::from_primitives([text()])
            .summary()
            .shapes_only()
    );
}
