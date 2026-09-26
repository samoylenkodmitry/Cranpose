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
    ]
}

fn scan_summary(primitives: &[DrawPrimitive]) -> RecordingSummary {
    let mut summary = RecordingSummary::default();
    for primitive in primitives {
        summary.note(primitive);
    }
    summary
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
        8,
        "six shapes, one blended, one after content"
    );
    assert_eq!(
        recording.others().len(),
        5,
        "a nested blend, a blended image, an image, a text and a shadow"
    );
    assert_eq!(recording.content_markers(), 1);
    assert_eq!(recording.len(), 14);
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
        record.radii,
        arc_trig(&expected),
        "the trig row the fragment stage reads is computed once, when recorded"
    );
    assert_eq!(
        record.radii[1],
        (expected.start_angle + expected.sweep_angle * 0.5).cos()
    );
    let ring = BandRing::of_geometry(&expected);
    assert_eq!(
        record.arc_normalized[2..],
        [ring.range_start, ring.range],
        "the strip's padded sweep the vertex stage reads is computed once, when recorded"
    );
    assert!(ring.range_start < expected.start_angle && ring.range > 2.0);
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
fn summary_and_bounds_are_what_a_scan_of_the_primitives_finds() {
    let primitives = every_primitive();
    let recording = CommandRecording::from_primitives(primitives.clone());
    assert_eq!(recording.summary(), scan_summary(&primitives));
    let expected_bounds = primitives
        .iter()
        .filter_map(primitive_coverage_rect)
        .reduce(|a, b| a.union(b));
    assert_eq!(recording.bounds(), expected_bounds);
    assert_eq!(
        recording.summary_in(&recording.content_split(false)),
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
fn the_record_is_seven_rows() {
    assert_eq!(std::mem::size_of::<ShapeRecord>(), 112);
    assert_eq!(std::mem::size_of::<BrushRecord>(), 48);
    assert_eq!(std::mem::size_of::<GradientStopRecord>(), 32);
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
fn circles_plain_rects_and_strokes_leave_the_segment_unmarked() {
    assert!(!only_segment_interiors(|recorder| {
        recorder.push_round_rect(
            rect(0.0, 0.0, 40.0, 40.0),
            &solid(),
            CornerRadii::uniform(20.0),
            None,
            BlendMode::SrcOver,
        );
    }));
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
fn a_chip_whose_corners_take_most_of_it_leaves_the_segment_unmarked() {
    assert!(!only_segment_interiors(|recorder| {
        recorder.push_round_rect(
            rect(0.0, 0.0, 60.0, 24.0),
            &solid(),
            CornerRadii::uniform(8.0),
            None,
            BlendMode::SrcOver,
        );
    }));
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
