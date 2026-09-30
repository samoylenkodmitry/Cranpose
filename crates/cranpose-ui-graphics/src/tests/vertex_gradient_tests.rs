use crate::{
    ArcRecordArgs, BlendMode, Brush, Color, CommandRecorder, CommandRecording, CornerRadii, Point,
    Rect, Stroke, TileMode,
};

const QUAD: Rect = Rect {
    x: 10.0,
    y: 20.0,
    width: 80.0,
    height: 140.0,
};
const TOP: Color = Color(0.9, 0.3, 0.1, 1.0);
const BOTTOM: Color = Color(0.1, 0.2, 0.8, 0.6);

fn vertical(start_y: f32, end_y: f32, tile_mode: TileMode) -> Brush {
    Brush::vertical_gradient_tiled(vec![TOP, BOTTOM], start_y, end_y, tile_mode)
}

fn vertical_stops(stops: [f32; 2], start_y: f32, end_y: f32) -> Brush {
    Brush::vertical_gradient_stops(
        vec![(stops[0], TOP), (stops[1], BOTTOM)],
        start_y,
        end_y,
        TileMode::Clamp,
    )
}

fn recorded(push: impl FnOnce(&mut CommandRecorder)) -> CommandRecording {
    let mut recorder = CommandRecorder::default();
    push(&mut recorder);
    recorder.finish()
}

fn fill(brush: &Brush) -> CommandRecording {
    recorded(|recorder| recorder.push_rect(QUAD, brush, None, BlendMode::SrcOver))
}

fn shades_per_vertex(recording: &CommandRecording) -> bool {
    let record = recording.shapes().get(0).expect("one record");
    assert!(record.is_gradient(), "the record keeps its gradient brush");
    record.is_vertex_gradient()
}

#[test]
fn a_two_stop_vertical_gradient_over_a_rect_shades_per_vertex() {
    let recording = fill(&vertical(0.0, QUAD.height, TileMode::Clamp));
    assert!(shades_per_vertex(&recording));
    let segment = recording.segments()[0];
    assert!(segment.vertex_gradient);
    assert!(!segment.gradient, "it draws with the solid fills");
    assert_eq!(
        recording.brush_of(&recording.shapes().get(0).expect("one record")),
        vertical(0.0, QUAD.height, TileMode::Clamp),
        "it materialises as the gradient the app drew"
    );
}

#[test]
fn three_stops_keep_the_fragment_gradient() {
    let brush = Brush::vertical_gradient(vec![TOP, BOTTOM, TOP], 0.0, QUAD.height);
    let recording = fill(&brush);
    assert!(!shades_per_vertex(&recording));
    let segment = recording.segments()[0];
    assert!(segment.gradient && !segment.vertex_gradient);
}

#[test]
fn a_band_narrower_than_the_quad_keeps_the_fragment_gradient() {
    for brush in [
        vertical(20.0, QUAD.height, TileMode::Clamp),
        vertical(0.0, QUAD.height - 1.0, TileMode::Clamp),
        vertical_stops([0.1, 1.0], 0.0, QUAD.height),
        vertical_stops([0.0, 0.9], 0.0, QUAD.height),
        vertical_stops([-0.5, 1.5], 0.0, QUAD.height * 0.5),
    ] {
        assert!(!shades_per_vertex(&fill(&brush)), "{brush:?}");
    }
}

#[test]
fn a_band_that_holds_the_quad_shades_per_vertex() {
    for brush in [
        vertical(-20.0, QUAD.height + 30.0, TileMode::Clamp),
        vertical(QUAD.height, 0.0, TileMode::Clamp),
        vertical_stops([0.25, 0.75], -QUAD.height * 0.5, QUAD.height * 1.5),
        vertical_stops([-0.5, 1.5], 0.0, QUAD.height),
        Brush::linear_gradient(vec![TOP, BOTTOM]),
        Brush::horizontal_gradient_default(vec![TOP, BOTTOM]),
    ] {
        assert!(shades_per_vertex(&fill(&brush)), "{brush:?}");
    }
}

#[test]
fn only_a_gradient_continuous_at_its_ends_shades_per_vertex() {
    assert!(shades_per_vertex(&fill(&vertical(
        0.0,
        QUAD.height,
        TileMode::Mirror
    ))));
    for tile_mode in [TileMode::Repeated, TileMode::Decal] {
        assert!(
            !shades_per_vertex(&fill(&vertical(0.0, QUAD.height, tile_mode))),
            "{tile_mode:?}"
        );
    }
    let radial = Brush::radial_gradient(vec![TOP, BOTTOM], Point::new(40.0, 70.0), 500.0);
    assert!(!shades_per_vertex(&fill(&radial)));
    let degenerate = vertical(0.0, 0.5, TileMode::Clamp);
    let sliver = recorded(|recorder| {
        recorder.push_rect(
            Rect {
                height: 0.5,
                ..QUAD
            },
            &degenerate,
            None,
            BlendMode::SrcOver,
        );
    });
    assert!(
        !shades_per_vertex(&sliver),
        "a ramp under a pixel long is left to the fragment stage"
    );
}

#[test]
fn a_rounded_fill_shades_per_vertex_and_strokes_and_arcs_do_not() {
    let brush = vertical(0.0, QUAD.height, TileMode::Clamp);
    let rounded = recorded(|recorder| {
        recorder.push_round_rect(
            QUAD,
            &brush,
            CornerRadii::uniform(12.0),
            None,
            BlendMode::SrcOver,
        );
    });
    assert!(shades_per_vertex(&rounded));
    let stroked = recorded(|recorder| {
        recorder.push_rect(QUAD, &brush, Some(Stroke::new(2.0)), BlendMode::SrcOver);
    });
    assert!(!shades_per_vertex(&stroked));
    let arc = recorded(|recorder| {
        recorder.push_arc(
            QUAD,
            &ArcRecordArgs {
                brush: &brush,
                center: Point::new(50.0, 90.0),
                radius: 30.0,
                start_angle: 0.0,
                sweep_angle: 1.0,
                stroke: None,
                inner_radius: 0.0,
                blend_mode: BlendMode::SrcOver,
            },
        );
    });
    assert!(!shades_per_vertex(&arc));
}

#[test]
fn a_gradient_with_nan_endpoints_keeps_fragment_sampling() {
    let brush = crate::BrushRecord {
        kind: crate::BRUSH_KIND_LINEAR,
        stop_start: 0,
        stop_count: 2,
        tile_mode: TileMode::Clamp as u32,
        params: [0.0, f32::NAN, 0.0, QUAD.height],
        explicit_start: 0,
        explicit_len: u32::MAX,
        reserved: [0; 2],
    };
    let stops = [
        crate::GradientStopRecord {
            position: [0.0; 4],
            color: [0.0; 4],
        },
        crate::GradientStopRecord {
            position: [1.0; 4],
            color: [1.0; 4],
        },
    ];
    assert!(!super::spans_quad(
        &brush,
        &stops,
        [0.0, 0.0, QUAD.width, QUAD.height]
    ));
}

#[test]
fn vertex_gradients_segment_apart_from_solid_and_fragment_gradients() {
    let solid = Brush::solid(TOP);
    let vertex = vertical(0.0, QUAD.height, TileMode::Clamp);
    let fragment = Brush::vertical_gradient(vec![TOP, BOTTOM, TOP], 0.0, QUAD.height);
    let recording = recorded(|recorder| {
        for brush in [&solid, &vertex, &vertex, &fragment, &solid] {
            recorder.push_rect(QUAD, brush, None, BlendMode::SrcOver);
        }
    });
    let segments: Vec<(u32, bool, bool)> = recording
        .segments()
        .iter()
        .map(|segment| (segment.count, segment.gradient, segment.vertex_gradient))
        .collect();
    assert_eq!(
        segments,
        [
            (1, false, false),
            (2, false, true),
            (1, true, false),
            (1, false, false)
        ]
    );
}
