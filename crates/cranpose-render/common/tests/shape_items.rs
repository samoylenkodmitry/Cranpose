use std::sync::Arc;

use cranpose_render_common::scene_store::{Item, SceneStore};
use cranpose_ui_graphics::{
    ArcRecordArgs, BlendMode, Brush, BrushRecord, Color, DrawPrimitive, GradientStopRecord, Point,
    RecordLane, RecordSegment, Rect, ShapeRecordBody, ShapeRecordCurve, ShapeRecorder, Stroke,
};

// A chunk's worth of shape items. Every sixteenth stays, a gradient rect
// beside a stroked arc; the others are solid rects.
const ITEMS: u32 = 256;
const STAYS_EVERY: u32 = 16;

fn record(index: u32, recorder: &mut ShapeRecorder) {
    let at = index as f32;
    let rect = Rect {
        x: at,
        y: at,
        width: 10.0,
        height: 8.0,
    };
    if !index.is_multiple_of(STAYS_EVERY) {
        recorder.push_rect(
            rect,
            &Brush::solid(Color(at / 256.0, 0.5, 0.0, 1.0)),
            None,
            BlendMode::SrcOver,
        );
        return;
    }
    let gradient = Brush::linear_gradient_range(
        vec![Color(1.0, 0.0, at / 256.0, 1.0), Color(0.0, 0.0, 1.0, 0.5)],
        Point { x: at, y: 0.0 },
        Point {
            x: at + 10.0,
            y: 8.0,
        },
    );
    recorder.push_rect(rect, &gradient, None, BlendMode::SrcOver);
    recorder.push_arc(
        rect,
        &ArcRecordArgs {
            brush: &gradient,
            center: Point {
                x: at + 5.0,
                y: 4.0,
            },
            radius: 4.0,
            start_angle: 0.25,
            sweep_angle: 2.0 + at / 256.0,
            stroke: Some(Stroke::new(1.5)),
            inner_radius: 0.0,
            blend_mode: BlendMode::SrcOver,
        },
    );
}

// What an item draws: the primitives it was recorded from, and what the GPU
// reads apart from where in its chunk the item lies.
#[derive(Debug, PartialEq)]
enum Drawn {
    Primitive(DrawPrimitive),
    Segment(RecordSegment),
    Shape(ShapeRecordBody, ShapeRecordCurve),
    Brush(BrushRecord, Vec<GradientStopRecord>, Option<Vec<f32>>),
}

fn drawn(store: &SceneStore, item: Item) -> Vec<Drawn> {
    let shape = store.shapes(item);
    let tables = shape.recorder().tables();
    let mut drawn: Vec<Drawn> = shape.primitives().map(Drawn::Primitive).collect();
    let segments = shape.segments();
    for segment in &tables.segments[segments.start as usize..segments.end as usize] {
        drawn.push(Drawn::Segment(RecordSegment {
            start: 0,
            ..*segment
        }));
        if segment.lane != RecordLane::Shapes {
            continue;
        }
        for index in segment.range() {
            let body = tables.shapes.bodies()[index];
            drawn.push(Drawn::Shape(
                ShapeRecordBody { brush: 0, ..body },
                tables.shapes.curves()[index],
            ));
            if let Some(brush) = body.brush.checked_sub(1) {
                let brush = tables.brushes()[brush as usize];
                let stops =
                    brush.stop_start as usize..(brush.stop_start + brush.stop_count) as usize;
                let explicit = brush.explicit_start as usize
                    ..brush.explicit_start.saturating_add(brush.explicit_len) as usize;
                drawn.push(Drawn::Brush(
                    BrushRecord {
                        stop_start: 0,
                        explicit_start: 0,
                        ..brush
                    },
                    tables.stops()[stops].to_vec(),
                    tables.explicit_stops().get(explicit).map(<[f32]>::to_vec),
                ));
            }
        }
    }
    drawn
}

#[test]
fn staying_shapes_draw_as_recorded_once_a_held_chunk_gives_up_the_others() {
    let mut store = SceneStore::new();
    let (mut staying, mut going) = (Vec::new(), Vec::new());
    for index in 0..ITEMS {
        let item = store.add_shapes(u64::from(index), |recorder| record(index, recorder));
        if index.is_multiple_of(STAYS_EVERY) {
            staying.push((item, drawn(&store, item)));
        } else {
            going.push(item);
        }
    }
    // A frame packet in flight holds the chunk the items share.
    let lease = Arc::clone(store.shapes(staying[0].0).recorder());
    let leased = (lease.fingerprint(), lease.tables().shapes.len());
    for item in going {
        store.remove_item(item);
    }
    for index in ITEMS..3 * ITEMS {
        store.add_shapes(u64::from(index), |recorder| record(index, recorder));
    }
    for (item, recorded) in &staying {
        assert_eq!(
            &drawn(&store, *item),
            recorded,
            "a staying item draws what it recorded"
        );
    }
    assert_eq!(
        (lease.fingerprint(), lease.tables().shapes.len()),
        leased,
        "the packet reads the chunk as it held it"
    );
}
