use std::ops::Range;

use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawPrimitive, Point, Rect, ShapeRecorder, Stroke,
};

fn rect(x: f32) -> Rect {
    Rect {
        x,
        y: 4.0,
        width: 20.0,
        height: 10.0,
    }
}

// The shapes of five runs, one recorded after another: lines and arcs keep
// their arguments beside their records, and the gradient paints with a
// brush of the recorder's tables.
fn runs() -> Vec<DrawPrimitive> {
    vec![
        DrawPrimitive::Rect {
            rect: rect(0.0),
            brush: Brush::solid(Color::RED),
            stroke: None,
        },
        DrawPrimitive::Line {
            rect: rect(10.0),
            brush: Brush::solid(Color::BLUE),
            start: Point::new(10.0, 4.0),
            end: Point::new(30.0, 14.0),
            stroke: Stroke::new(2.0),
        },
        DrawPrimitive::Rect {
            rect: rect(20.0),
            brush: Brush::linear_gradient(vec![Color::RED, Color::GREEN]),
            stroke: None,
        },
        DrawPrimitive::Arc {
            rect: rect(30.0),
            brush: Brush::solid(Color::GREEN),
            center: Point::new(40.0, 9.0),
            radius: 5.0,
            start_angle: 0.5,
            sweep_angle: 2.0,
            stroke: Some(Stroke::new(1.5)),
            inner_radius: 0.0,
        },
        DrawPrimitive::RoundRect {
            rect: rect(40.0),
            brush: Brush::solid(Color::BLUE),
            radii: CornerRadii::uniform(3.0),
            stroke: None,
        },
    ]
}

// Records each primitive as a run of its own, and returns the segments of
// each run with the primitives the recorder gives back for it.
fn recorded(recorder: &mut ShapeRecorder) -> Vec<(Range<u32>, Vec<DrawPrimitive>)> {
    runs()
        .into_iter()
        .map(|primitive| {
            let start = recorder.all_segments().end;
            recorder.push_primitive(primitive);
            recorder.end_segment();
            let segments = start..recorder.all_segments().end;
            let drawn = recorder.shape_primitives_in(&segments).collect();
            (segments, drawn)
        })
        .collect()
}

// Keeps the runs at `kept` and checks that each draws what it drew before
// from the segments that follow the runs kept before it.
fn keep(kept: &[usize]) -> ShapeRecorder {
    let mut recorder = ShapeRecorder::default();
    let runs = recorded(&mut recorder);
    recorder.keep_segments(|segment| kept.iter().any(|&index| runs[index].0.contains(&segment)));
    let mut start = 0;
    for &index in kept {
        let (segments, drawn) = &runs[index];
        let moved = start..start + segments.len() as u32;
        assert_eq!(
            recorder.shape_primitives_in(&moved).collect::<Vec<_>>(),
            *drawn,
            "run {index} draws what it drew"
        );
        start = moved.end;
    }
    assert_eq!(recorder.all_segments(), 0..start, "only the kept runs stay");
    recorder
}

#[test]
fn kept_runs_draw_what_they_drew_before() {
    let recorder = keep(&[1, 3, 4]);
    assert!(
        recorder.tables().brushes().is_empty(),
        "no kept shape paints with a brush"
    );
}

#[test]
fn a_kept_gradient_keeps_its_brush() {
    let recorder = keep(&[0, 2, 3]);
    assert!(!recorder.tables().brushes().is_empty());
}

#[test]
fn a_run_recorded_after_a_keep_starts_a_segment_of_its_own() {
    let mut recorder = keep(&[4]);
    let start = recorder.all_segments().end;
    recorder.push_primitive(runs().remove(4));
    assert_eq!(
        recorder.shape_count_in(&(start..recorder.all_segments().end)),
        1,
        "the new shape does not join the kept run"
    );
}
