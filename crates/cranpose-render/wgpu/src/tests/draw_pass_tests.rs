use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, Point, ShapeRecorder};

use super::*;
use crate::scene::{Placement, ShadowDraw};

#[test]
fn shadow_run_items_preserve_geometry_culling_and_first_run_window() {
    let run = |x| {
        let mut recorder = ShapeRecorder::default();
        for y in [0.0, 8.0] {
            recorder.push_primitive(DrawPrimitive::Rect {
                rect: Rect {
                    x,
                    y,
                    width: 8.0,
                    height: 8.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            });
        }
        RunDraw::whole(
            std::sync::Arc::new(recorder),
            Placement::at(Point::default(), None, None),
        )
        .unwrap()
    };
    let mut scene = CompositorScene::new();
    scene.push_run(run(0.0));
    for x in [16.0, 128.0] {
        scene.push_shadow_draw(ShadowDraw {
            shapes: Some(run(x)),
            post_blur_cutouts: None,
            texts: Vec::new(),
            blur_radius: 0.0,
            clip: None,
            rounded_clip: None,
            occluder: None,
            z_index: 0,
        });
    }
    let segment = PassSegment {
        scene: &scene,
        ops: &scene.draw_ops,
        composites: &[],
        offset: [0.0, 0.0],
        scissor: None,
        first_run_window: Some(1..2),
        transform: SegmentTransform::IDENTITY,
    };
    let items = merge_items(
        &segment,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 64.0,
        },
        1.0,
        (64, 64),
        false,
    )
    .collect::<Vec<_>>();
    assert_eq!(items.len(), 2);
    let Item::Run(ordinary, window) = &items[0] else {
        panic!("expected the ordinary run")
    };
    assert!(std::ptr::eq(*ordinary, &scene.runs[0]));
    assert_eq!(*window, Some(1..2));
    let Item::Run(shadow, window) = &items[1] else {
        panic!("expected the visible shadow run")
    };
    assert!(std::ptr::eq(
        *shadow,
        scene.shadow_draws[0].shapes.as_ref().unwrap()
    ));
    assert_eq!(*window, None);
    assert_eq!(shadow.record_count(), 2);
    for (x, expected) in [(16.0, Some(0)), (128.0, Some(1)), (256.0, None)] {
        let mut visible = merge_items(
            &segment,
            Rect {
                x,
                y: 0.0,
                width: 8.0,
                height: 16.0,
            },
            1.0,
            (64, 64),
            false,
        );
        match (visible.next(), expected) {
            (Some(Item::Run(run, None)), Some(index)) => assert!(std::ptr::eq(
                run,
                scene.shadow_draws[index].shapes.as_ref().unwrap()
            )),
            (None, None) => {}
            _ => panic!("incorrect first visible shadow at x={x}"),
        }
        assert!(visible.next().is_none());
    }
}
