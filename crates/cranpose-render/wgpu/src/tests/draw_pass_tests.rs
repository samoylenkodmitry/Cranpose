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
        scale: 1.0,
    };
    let items = merge_items(
        &segment,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 64.0,
        },
        (64, 64),
        false,
    )
    .collect::<Vec<_>>();
    assert_eq!(items.len(), 2);
    let Item::Run(ordinary, window, _) = &items[0] else {
        panic!("expected the ordinary run")
    };
    assert!(std::ptr::eq(*ordinary, &scene.runs[0]));
    assert_eq!(*window, Some(1..2));
    let Item::Run(shadow, window, _) = &items[1] else {
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
            (64, 64),
            false,
        );
        match (visible.next(), expected) {
            (Some(Item::Run(run, None, _)), Some(index)) => assert!(std::ptr::eq(
                run,
                scene.shadow_draws[index].shapes.as_ref().unwrap()
            )),
            (None, None) => {}
            _ => panic!("incorrect first visible shadow at x={x}"),
        }
        assert!(visible.next().is_none());
    }
}

/// A small deterministic generator for the held-glyph property test.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, below: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % u64::from(below)) as u32
    }

    fn rect(&mut self, extent: u32) -> TargetRect {
        (
            self.next(extent),
            self.next(extent),
            self.next(300),
            self.next(300),
        )
    }
}

#[test]
fn held_glyph_cells_answer_as_checking_every_held_draw() {
    let mut random = Lcg(7);
    // Past 64 cell columns the last bit is shared, so wide targets are
    // covered too.
    for extent in [300, 2_200, 6_000] {
        for _ in 0..200 {
            let mut pending = HeldDraws::default();
            let held: Vec<TargetRect> = (0..random.next(40)).map(|_| random.rect(extent)).collect();
            pending.hold(0..1, held.iter().copied());
            for _ in 0..20 {
                let query = random.rect(extent);
                let expected = held.iter().any(|rect| target_rects_overlap(*rect, query));
                assert_eq!(
                    pending.overlaps(query),
                    expected,
                    "{held:?} against {query:?}"
                );
            }
            assert!(pending.take().glyphs.is_some());
            assert!(
                !pending.overlaps((0, 0, extent, extent)),
                "taking forgets every cell"
            );
        }
    }
}

#[test]
fn a_held_draw_marks_every_cell_it_touches() {
    assert_eq!(
        held_cells((70, 0, 0, 10)),
        (0..1, 0b10),
        "an empty side counts as the pixel it stands on"
    );
    assert_eq!(held_cells((63, 64, 2, 1)), (1..2, 0b11));
    assert_eq!(held_cells((0, 0, 64 * 64 + 1, 1)), (0..1, u64::MAX));
    assert_eq!(
        held_cells((5_000, 0, 10, 10)),
        (0..1, 1 << 63),
        "columns past the last share its bit"
    );
}
