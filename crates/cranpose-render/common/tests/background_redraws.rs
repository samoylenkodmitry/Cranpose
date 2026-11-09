use std::{cell::RefCell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_render_common::graph_scene::Scene;
use cranpose_ui::{BoxSpec, Modifier, TestComposition};
use cranpose_ui_graphics::{Color, Size};

use crate::scene_probe::{fresh_graph, initial_graph, solid_rect_colors, update_scene};

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
