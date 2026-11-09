use std::{cell::RefCell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_render_common::graph_scene::Scene;
use cranpose_ui::{BoxSpec, Modifier, TestComposition};
use cranpose_ui_graphics::{Brush, Color, Rect, Size};

use crate::scene_probe::{
    fresh_graph, initial_graph, solid_rect_colors, solid_rects, update_scene,
};

const STILL_BOXES: usize = 20;

// Boxes with still backgrounds beside `tinted` boxes whose background tint
// follows `tint`.
fn boxes_with_tints(
    tint: Rc<RefCell<Option<MutableState<u32>>>>,
    tinted: usize,
) -> TestComposition {
    cranpose_ui::run_test_composition(move || {
        let state = cranpose_core::rememberMutableStateOf(|| 0u32);
        *tint.borrow_mut() = Some(state);
        cranpose_ui::Box(
            Modifier::empty().size(Size::new(200.0, 200.0)),
            BoxSpec::default(),
            move || {
                for index in 0..STILL_BOXES {
                    cranpose_ui::Box(
                        Modifier::empty()
                            .size(Size::new(8.0, 8.0))
                            .background(Color(0.0, index as f32 / STILL_BOXES as f32, 1.0, 1.0)),
                        BoxSpec::default(),
                        || {},
                    );
                }
                for index in 0..tinted {
                    let step = (state.get() + index as u32 * 64) % 256;
                    cranpose_ui::Box(
                        Modifier::empty()
                            .size(Size::new(40.0, 40.0))
                            .background(Color(step as f32 / 255.0, 0.0, 0.0, 1.0)),
                        BoxSpec::default(),
                        || {},
                    );
                }
            },
        );
    })
}

// Redraws the tinted backgrounds for `frames` frames and returns the store's
// heap after each frame, with the scene and its composition.
fn redraw(tinted: usize, frames: u32) -> (Vec<usize>, Scene, TestComposition) {
    let tint = Rc::default();
    let mut composition = boxes_with_tints(Rc::clone(&tint), tinted);
    let root = composition.root().expect("a root");
    let viewport = Size::new(240.0, 240.0);
    let mut scene = initial_graph(&mut composition, root, viewport);
    let state = tint.borrow().expect("the state");
    let heap = (1..=frames)
        .map(|frame| {
            state.set(frame);
            update_scene(&mut composition, root, viewport, &mut scene);
            scene.store().heap_bytes()
        })
        .collect();
    (heap, scene, composition)
}

fn assert_paints_as_fresh(scene: &Scene, composition: &mut TestComposition) {
    let root = composition.root().expect("a root");
    assert_eq!(
        solid_rect_colors(scene),
        solid_rect_colors(&fresh_graph(composition, root)),
        "the updated scene paints what a fresh one does"
    );
}

#[test]
fn a_background_drawn_anew_each_frame_takes_the_place_of_its_old_shapes() {
    let (heap, scene, mut composition) = redraw(1, 150);
    assert!(
        heap[2..].iter().all(|&bytes| bytes == heap[1]),
        "the store's heap after each frame: {heap:?}"
    );
    assert_paints_as_fresh(&scene, &mut composition);
}

#[test]
fn backgrounds_drawn_anew_in_turn_keep_the_store_heap_flat() {
    let (heap, scene, mut composition) = redraw(2, 150);
    assert!(
        heap[149] <= heap[29],
        "the store holds {} bytes after 150 frames, {} after 30",
        heap[149],
        heap[29]
    );
    assert_paints_as_fresh(&scene, &mut composition);
}

// Sets the step `content` reads to each frame's number for `frames` frames
// and checks after each that the updated scene paints the rects a fresh
// one does, where they lie.
fn assert_steps_paint_as_fresh(content: impl Fn(MutableState<u32>) + 'static, frames: u32) {
    let step = Rc::new(RefCell::new(None));
    let kept = Rc::clone(&step);
    let content: Rc<dyn Fn(MutableState<u32>)> = Rc::new(content);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let state = cranpose_core::rememberMutableStateOf(|| 0u32);
        *kept.borrow_mut() = Some(state);
        let content = Rc::clone(&content);
        cranpose_ui::Box(
            Modifier::empty().size(Size::new(200.0, 200.0)),
            BoxSpec::default(),
            move || content(state),
        );
    });
    let root = composition.root().expect("a root");
    let viewport = Size::new(240.0, 240.0);
    let mut scene = initial_graph(&mut composition, root, viewport);
    let state = step.borrow().expect("the state");
    for frame in 1..=frames {
        state.set(frame);
        update_scene(&mut composition, root, viewport, &mut scene);
        assert_eq!(
            solid_rects(&scene),
            solid_rects(&fresh_graph(&mut composition, root)),
            "frame {frame}: the updated scene paints the rects a fresh one does"
        );
    }
}

#[test]
fn a_node_drawn_anew_over_a_still_background_paints_both() {
    assert_steps_paint_as_fresh(
        |step| {
            cranpose_ui::Box(
                Modifier::empty()
                    .size(Size::new(40.0, 40.0))
                    .background(Color(0.0, 0.5, 0.0, 1.0))
                    .draw_behind(move |scope| {
                        let width = (step.get() % 30) as f32 + 1.0;
                        scope.draw_rect_at(
                            Rect {
                                x: 0.0,
                                y: 0.0,
                                width,
                                height: 4.0,
                            },
                            Brush::solid(Color(0.0, 0.0, 1.0, 1.0)),
                        );
                    }),
                BoxSpec::default(),
                || {},
            );
        },
        40,
    );
}

#[test]
fn a_background_follows_its_color_and_its_node_size_beside_a_still_one() {
    assert_steps_paint_as_fresh(
        |step| {
            let step = step.get();
            cranpose_ui::Box(
                Modifier::empty()
                    .size(Size::new(40.0, 40.0))
                    .background(Color((step % 7) as f32 / 7.0, 0.0, 0.0, 1.0))
                    .padding(4.0)
                    .background(Color(0.0, 0.0, 0.5, 1.0)),
                BoxSpec::default(),
                || {},
            );
            cranpose_ui::Box(
                Modifier::empty()
                    .size(Size::new(10.0 + (step % 20) as f32, 10.0))
                    .background(Color(0.5, 0.5, 0.0, 1.0)),
                BoxSpec::default(),
                || {},
            );
        },
        40,
    );
}
