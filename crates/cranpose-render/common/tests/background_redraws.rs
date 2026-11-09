use std::{cell::RefCell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_ui::{BoxSpec, Modifier};
use cranpose_ui_graphics::{Color, Size};

use crate::scene_probe::{fresh_graph, initial_graph, solid_rect_colors, update_scene};

const STILL_BOXES: usize = 20;

// Boxes with still backgrounds beside one whose background tint follows
// `tint`.
fn boxes_with_a_tint(tint: Rc<RefCell<Option<MutableState<u32>>>>) -> cranpose_ui::TestComposition {
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
                let step = state.get() % 256;
                cranpose_ui::Box(
                    Modifier::empty()
                        .size(Size::new(40.0, 40.0))
                        .background(Color(step as f32 / 255.0, 0.0, 0.0, 1.0)),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    })
}

#[test]
fn a_background_drawn_anew_each_frame_keeps_the_store_heap_flat() {
    let tint = Rc::default();
    let mut composition = boxes_with_a_tint(Rc::clone(&tint));
    let root = composition.root().expect("a root");
    let viewport = Size::new(240.0, 240.0);
    let mut scene = initial_graph(&mut composition, root, viewport);
    let state = tint.borrow().expect("the state");
    let mut settled = 0;
    for frame in 1..=150 {
        state.set(frame);
        update_scene(&mut composition, root, viewport, &mut scene);
        if frame == 30 {
            settled = scene.store().heap_bytes();
        }
    }

    assert!(
        scene.store().heap_bytes() <= settled,
        "the store holds {} bytes after 150 frames, {settled} after 30",
        scene.store().heap_bytes()
    );
    assert_eq!(
        solid_rect_colors(&scene),
        solid_rect_colors(&fresh_graph(&mut composition, root)),
        "the updated scene paints what a fresh one does"
    );
}
