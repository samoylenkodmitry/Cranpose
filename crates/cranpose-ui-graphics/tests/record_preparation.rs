use std::sync::Arc;

use cranpose_ui_graphics::{
    ArcRecordArgs, BlendMode, Brush, Color, CommandRecorder, CommandRecording, CornerRadii,
    DrawPrimitive, DrawScope, DrawScopeDefault, Point, Rect, Size, Stroke, StrokeCap, TAU,
    TileMode, normalized_band,
};

fn primitive(index: usize) -> DrawPrimitive {
    if index.is_multiple_of(127) {
        return DrawPrimitive::Content;
    }
    let brush = if index.is_multiple_of(37) {
        Brush::LinearGradient {
            colors: vec![Color::RED, Color::BLUE],
            stops: Some(vec![0.125, 0.875]),
            start: Point::new(index as f32, 1.0),
            end: Point::new(2.0, 3.0),
            tile_mode: TileMode::Mirror,
        }
    } else {
        Brush::Solid(Color(index as f32 / 8192.0, 0.25, 0.75, 0.5))
    };
    let radius = 12.0 + (index % 400) as f32;
    let rect = Rect {
        x: -(radius + 5.0),
        y: -(radius + 5.0),
        width: radius * 2.0 + 10.0,
        height: radius * 2.0 + 10.0,
    };
    if index.is_multiple_of(17) {
        return DrawPrimitive::Rect {
            rect,
            brush,
            stroke: None,
        };
    }
    DrawPrimitive::Arc {
        rect,
        brush,
        center: Point::new(0.125, -0.25),
        radius,
        start_angle: index as f32 * -0.125,
        sweep_angle: if index.is_multiple_of(11) {
            TAU
        } else {
            0.0625
        },
        stroke: Some(
            Stroke::new(1.25)
                .with_cap([StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square][index % 3]),
        ),
        inner_radius: radius - 2.0,
    }
}

#[test]
fn large_recordings_preserve_every_primitive_and_retained_snapshot() {
    let expected: Vec<_> = (0..5003).map(primitive).collect();
    let recording = CommandRecording::from_primitives(expected.clone());
    assert_eq!(
        recording.primitives_with_markers().collect::<Vec<_>>(),
        expected
    );
    let mut shape_index = 0;
    let mut brush_index = 0;
    for primitive in &expected {
        let single = CommandRecording::from_primitives([primitive.clone()]);
        let Some(mut expected_record) = single.shapes().get(0) else {
            continue;
        };
        if expected_record.brush != 0 {
            brush_index += 1;
            expected_record.brush = brush_index;
        }
        let actual_record = recording.shapes().get(shape_index).unwrap();
        assert_eq!(
            bytemuck::bytes_of(&actual_record),
            bytemuck::bytes_of(&expected_record),
            "shape {shape_index}"
        );
        shape_index += 1;
    }
    let retained = Arc::clone(recording.shape_recorder());
    let retained_hash = retained.fingerprint();
    let retained_shapes: Vec<_> = retained.tables().shapes.iter().collect();
    let mut recorder = recording.into_recorder();
    for index in 5003..10007 {
        recorder.push_primitive(primitive(index));
    }
    let recording = recorder.finish();
    assert_eq!(
        recording.primitives_with_markers().collect::<Vec<_>>(),
        (0..10007).map(primitive).collect::<Vec<_>>()
    );
    assert_eq!(retained.fingerprint(), retained_hash);
    assert_eq!(
        retained.tables().shapes.iter().collect::<Vec<_>>(),
        retained_shapes
    );
    let mut recorder = CommandRecorder::reusing(recording);
    recorder.push_primitive(primitive(1));
    let recording = recorder.finish();
    assert_eq!(
        recording.primitives_with_markers().collect::<Vec<_>>(),
        vec![primitive(1)]
    );
    assert_eq!(
        retained.tables().shapes.iter().collect::<Vec<_>>(),
        retained_shapes
    );
}

#[test]
fn reusable_recording_waits_for_retained_readers_and_resets_after_transfer() {
    let original = primitive(9);
    let mut recording = CommandRecording::from_primitives([original.clone()]);
    let retained = Arc::clone(recording.shape_recorder());

    assert!(
        recording.try_take_reusable().is_none(),
        "recorded shapes remain owned by retained readers"
    );
    assert_eq!(
        recording.primitives_with_markers().collect::<Vec<_>>(),
        vec![original]
    );
    drop(retained);
    let Some(storage) = recording.try_take_reusable() else {
        panic!("unshared shape data can be transferred for reuse");
    };
    assert!(
        recording.is_empty(),
        "the source becomes empty after transfer"
    );

    let mut next = CommandRecorder::reusing(storage);
    next.push_primitive(primitive(21));
    let next = next.finish();
    assert_eq!(
        next.primitives_with_markers().collect::<Vec<_>>(),
        vec![primitive(21)]
    );
}

#[test]
fn appending_after_a_fingerprint_updates_the_recording_identity() {
    let mutations: [fn(&mut CommandRecorder); 3] = [
        |recording| recording.push_primitive(primitive(2)),
        CommandRecorder::push_content,
        |recording| recording.push_other(primitive(17)),
    ];
    for append in mutations {
        let recording = CommandRecording::from_primitives([primitive(1)]);
        let first = recording.fingerprint();
        let mut recorder = recording.into_recorder();
        append(&mut recorder);
        let recording = recorder.finish();
        assert_ne!(recording.fingerprint(), first);
        let mut expected = CommandRecorder::from_primitives([primitive(1)]);
        append(&mut expected);
        assert_eq!(recording.fingerprint(), expected.finish().fingerprint());
    }
}

fn scope_shape(scope: &mut DrawScopeDefault, expected: &mut CommandRecorder, index: usize) {
    let radius = if index.is_multiple_of(251) {
        f32::NAN
    } else {
        12.0 + (index % 400) as f32
    };
    let brush = Brush::Solid(Color(
        if index.is_multiple_of(163) {
            f32::from_bits(0x7fc0_0021)
        } else {
            index as f32 / 16384.0
        },
        -0.0,
        0.75,
        0.5,
    ));
    let blend = if (index / 64).is_multiple_of(2) {
        BlendMode::SrcOver
    } else {
        BlendMode::Plus
    };
    let rect = Rect {
        x: -0.0,
        y: -3.0,
        width: 40.0,
        height: 40.0,
    };
    let stroke = Stroke::new(1.25)
        .with_cap([StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square][index % 3]);
    match index % 13 {
        0 => {
            scope.draw_rect_at_blend(rect, brush.clone(), blend);
            expected.push_rect(rect, &brush, None, blend);
        }
        1 => {
            let radii = CornerRadii::uniform(20.0);
            scope.draw_round_rect_at_stroked_blend(rect, brush.clone(), radii, stroke, blend);
            expected.push_round_rect(rect, &brush, radii, Some(stroke), blend);
        }
        _ => {
            let center = Point::new(0.125, -0.25);
            let start = if index.is_multiple_of(19) {
                -0.0
            } else {
                index as f32 * -0.125
            };
            let sweep = [TAU, 0.0625, -0.25, -0.0][index % 4];
            scope.draw_arc_blend(brush.clone(), center, radius, start, sweep, stroke, blend);
            let args = ArcRecordArgs {
                brush: &brush,
                center,
                radius,
                start_angle: start,
                sweep_angle: sweep,
                stroke: Some(stroke),
                inner_radius: 0.0,
                blend_mode: blend,
            };
            let geometry = normalized_band(&args);
            if !geometry.is_degenerate() {
                expected.push_scope_arc(&args, &geometry);
            }
        }
    }
}

fn assert_recording_bits(actual: &CommandRecording, expected: &CommandRecording) {
    let actual_records: Vec<_> = actual.shapes().iter().collect();
    let expected_records: Vec<_> = expected.shapes().iter().collect();
    assert_eq!(actual_records.len(), expected_records.len());
    for (index, (actual, expected)) in actual_records.iter().zip(&expected_records).enumerate() {
        assert_eq!(
            bytemuck::bytes_of(actual),
            bytemuck::bytes_of(expected),
            "record {index}"
        );
    }
    assert_eq!(actual.segments(), expected.segments());
    assert_eq!(actual.brushes(), expected.brushes());
    assert_eq!(actual.stops(), expected.stops());
    assert_eq!(actual.others(), expected.others());
    assert_eq!(actual.content_markers(), expected.content_markers());
    assert_eq!(actual.bounds(), expected.bounds());
    assert_eq!(actual.summary(), expected.summary());
    assert_eq!(actual.fingerprint(), expected.fingerprint());
}

#[test]
fn scope_batches_preserve_bits_order_metadata_and_retained_frames() {
    let mut scope = DrawScopeDefault::new(Size::new(408.0, 408.0));
    let mut expected = CommandRecorder::default();
    for range in [0..5003, 5003..10007] {
        for index in range {
            scope_shape(&mut scope, &mut expected, index);
        }
        let marker = primitive(37);
        scope.push_recorded(vec![marker.clone()]);
        expected.push_primitive(marker);
        scope.draw_content();
        expected.push_content();
    }
    let actual = scope.finish();
    assert_recording_bits(&actual, &expected.finish());
    let retained = Arc::clone(actual.shape_recorder());
    let retained_records: Vec<_> = retained.tables().shapes.iter().collect();
    let retained_hash = retained.fingerprint();
    let mut recorder = CommandRecorder::reusing(actual);
    recorder.push_primitive(primitive(1));
    assert_eq!(recorder.finish().len(), 1);
    assert_eq!(retained.fingerprint(), retained_hash);
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&retained.tables().shapes.iter().collect::<Vec<_>>()),
        bytemuck::cast_slice::<_, u8>(&retained_records)
    );
}

#[test]
fn a_one_shape_recording_holds_room_for_one_record_and_one_segment() {
    let rect = || DrawPrimitive::Rect {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: 8.0,
            height: 4.0,
        },
        brush: Brush::Solid(Color::RED),
        stroke: None,
    };
    let one = CommandRecording::from_primitives([rect()]);
    assert_eq!(one.shape_capacity(), 1);
    assert_eq!(
        one.pod_heap_bytes(),
        std::mem::size_of::<cranpose_ui_graphics::ShapeRecordBody>()
            + std::mem::size_of::<cranpose_ui_graphics::ShapeRecordCurve>()
            + std::mem::size_of::<cranpose_ui_graphics::RecordSegment>(),
        "a background or border recording keeps no room it never fills"
    );
    let mut again = CommandRecorder::reusing(one);
    again.push_primitive(rect());
    assert_eq!(
        again.finish().shape_capacity(),
        1,
        "recording it again keeps the one slot"
    );
    let two = CommandRecording::from_primitives([rect(), rect()]);
    assert_eq!(two.len(), 2);
    assert!(
        two.shape_capacity() >= 2,
        "a second shape grows the columns as usual"
    );
    let nine = CommandRecording::from_primitives(std::iter::repeat_with(rect).take(9));
    assert_eq!(
        nine.shape_capacity(),
        10,
        "a new recording keeps an eighth more than it holds, not the sixteen its growth reached"
    );
    let mut redrawn = CommandRecorder::reusing(nine);
    for _ in 0..10 {
        redrawn.push_primitive(rect());
    }
    assert_eq!(
        redrawn.finish().shape_capacity(),
        10,
        "drawing it again into its allocation keeps that room"
    );
}
