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
fn held_overlay_cells_answer_as_checking_every_held_draw() {
    let mut random = Lcg(7);
    let overlays = [Overlay::Glyphs, Overlay::Images, Overlay::StoreRun];
    let kinds = [
        None,
        Some(Overlay::Glyphs),
        Some(Overlay::Images),
        Some(Overlay::StoreRun),
    ];
    // Wide targets take several words of cells a row.
    for extent in [300, 2_200, 6_000] {
        for _ in 0..200 {
            let mut held = HeldOverlays::default();
            held.cells.reset(extent);
            let draws: Vec<(Overlay, TargetRect)> = (0..random.next(40))
                .map(|_| (overlays[random.next(3) as usize], random.rect(extent)))
                .collect();
            for (kind, rect) in &draws {
                held.add(*rect, *kind);
            }
            for _ in 0..20 {
                let query = random.rect(extent);
                let kind = kinds[random.next(4) as usize];
                let expected = draws
                    .iter()
                    .filter(|(held, rect)| {
                        Some(*held) != kind && target_rects_overlap(*rect, query)
                    })
                    .fold(Kinds::default(), |kinds, (held, _)| {
                        kinds | Kinds::of(*held)
                    });
                assert_eq!(
                    held.blocking(query, kind),
                    expected,
                    "{draws:?} against {query:?}"
                );
                held.gather(query, kind);
                assert_eq!(
                    held.gathered_kinds(),
                    expected,
                    "a run gathers what it overlaps"
                );
            }
            held.release(Kinds::ALL);
            assert!(
                held.blocking((0, 0, extent, extent), None).is_empty(),
                "releasing the held draws forgets every cell"
            );
        }
    }
}

/// A logical rect of a random place and size, at quarter points.
fn logical_rect(random: &mut Lcg) -> Rect {
    Rect {
        x: random.next(1_200) as f32 * 0.25,
        y: random.next(1_200) as f32 * 0.25,
        width: random.next(200) as f32 * 0.25,
        height: random.next(200) as f32 * 0.25,
    }
}

#[test]
fn a_record_meets_every_held_draw_its_target_pixels_meet() {
    let mut random = Lcg(11);
    let (mut met, mut kept) = (0, 0);
    for scale in [1.0, 1.5, 2.0, 2.625, 3.0] {
        let pixel = 1.0 / scale;
        for _ in 0..4_000 {
            let offset = [
                random.next(64) as f32 * 0.375,
                random.next(64) as f32 * 0.375,
            ];
            let viewport = ViewportUniformParams {
                width: 4_096,
                height: 4_096,
                offset,
                transform: SegmentTransform::IDENTITY,
                origin: [0.0; 2],
                depth_base: 0.0,
            };
            let held = random.rect(400);
            let candidate = Candidate::of(held, Overlay::Glyphs, scale, offset);
            let record = logical_rect(&mut random);
            let grown = Rect {
                x: record.x - pixel,
                y: record.y - pixel,
                width: record.width + 2.0 * pixel,
                height: record.height + 2.0 * pixel,
            };
            let reach = [
                grown.x,
                grown.y,
                grown.x + grown.width,
                grown.y + grown.height,
            ];
            if scissor_rect_for_rect(grown, scale, viewport)
                .is_some_and(|target| target_rects_overlap(target, held))
            {
                assert!(
                    candidate.reached_by(reach),
                    "{record:?} at {scale} from {offset:?} meets {held:?}"
                );
                met += 1;
            }
            let held = (
                random.next(400),
                random.next(400),
                random.next(8),
                random.next(8),
            );
            let candidate = Candidate::of(held, Overlay::Glyphs, scale, offset);
            let hole = logical_rect(&mut random);
            let device = |value: f32, origin: f32| value * scale - origin;
            let (left, top) = (
                (device(hole.x, offset[0]) + 1.0).ceil(),
                (device(hole.y, offset[1]) + 1.0).ceil(),
            );
            let (right, bottom) = (
                (device(hole.x + hole.width, offset[0]) - 1.0).floor(),
                (device(hole.y + hole.height, offset[1]) - 1.0).floor(),
            );
            let inside = left <= held.0 as f32
                && top <= held.1 as f32
                && (held.0 + held.2) as f32 <= right
                && (held.1 + held.3) as f32 <= bottom;
            if candidate.kept_by([hole, hole], pixel) {
                assert!(
                    inside,
                    "{held:?} lies inside the pixels {hole:?} keeps at {scale} from {offset:?}"
                );
                kept += 1;
            }
        }
    }
    assert!(
        met > 1_000 && kept > 50,
        "both checks ran: {met} met, {kept} kept"
    );
}

#[test]
fn a_cell_grid_counts_what_a_rect_touches_at_its_edges() {
    let mut grid = CellGrid::<6>::default();
    grid.reset(200);
    grid.mark((60, 0, 10, 10));
    assert!(
        grid.touched((70, 5, 0, 0)),
        "an empty rect counts as the pixel it stands on"
    );
    assert!(!grid.touched((128, 0, 10, 10)));
    grid.mark((5_000, 300, 10, 10));
    assert!(
        grid.touched((4_100, 300, 1, 1)),
        "columns past a row's words share its last"
    );
}
