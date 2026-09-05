use std::sync::Arc;

use cranpose_ui_graphics::{
    Brush, Color, CommandRecording, DrawPrimitive, Point, Rect, Stroke, StrokeCap, TAU, TileMode,
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
    let mut recording = CommandRecording::from_primitives(expected.clone());
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
    let retained = Arc::clone(recording.tables());
    let retained_hash = retained.fingerprint();
    let retained_shapes: Vec<_> = retained.shapes.iter().collect();
    for index in 5003..10007 {
        recording.push_primitive(primitive(index));
    }
    assert_eq!(
        recording.primitives_with_markers().collect::<Vec<_>>(),
        (0..10007).map(primitive).collect::<Vec<_>>()
    );
    assert_eq!(retained.fingerprint(), retained_hash);
    assert_eq!(retained.shapes.iter().collect::<Vec<_>>(), retained_shapes);
    recording.clear();
    recording.push_primitive(primitive(1));
    assert_eq!(
        recording.primitives_with_markers().collect::<Vec<_>>(),
        vec![primitive(1)]
    );
    assert_eq!(retained.shapes.iter().collect::<Vec<_>>(), retained_shapes);
}

#[test]
fn appending_after_a_fingerprint_updates_the_recording_identity() {
    let mutations: [fn(&mut CommandRecording); 3] = [
        |recording| recording.push_primitive(primitive(2)),
        CommandRecording::push_content,
        |recording| recording.push_other(primitive(17)),
    ];
    for append in mutations {
        let mut recording = CommandRecording::from_primitives([primitive(1)]);
        let first = recording.fingerprint();
        append(&mut recording);
        assert_ne!(recording.fingerprint(), first);
        let mut expected = CommandRecording::from_primitives([primitive(1)]);
        append(&mut expected);
        assert_eq!(recording.fingerprint(), expected.fingerprint());
    }
}
