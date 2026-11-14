// A scene that grew to many rows and shrank back holds at rest about the
// heap of a scene that held the rows that remain from its start, once both
// went on with the same updates.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_render_common::RenderScene;
use cranpose_ui::{BoxSpec, ColumnSpec, Modifier, Size, TestComposition};
use cranpose_ui_graphics::{Brush, Color, Rect};

use crate::scene_probe::{initial_graph, update_scene};

const VIEWPORT: Size = Size {
    width: 200.0,
    height: 4000.0,
};

#[derive(Clone, Copy)]
struct RowStates {
    count: MutableState<usize>,
    tint: MutableState<u32>,
}

// A column of `count` rows, each painting a rect of its own color; the
// first row's follows `tint`.
fn rows(states: Rc<RefCell<Option<RowStates>>>) -> TestComposition {
    cranpose_ui::run_test_composition(move || {
        let count = cranpose_core::rememberMutableStateOf(|| 100usize);
        let tint = cranpose_core::rememberMutableStateOf(|| 0u32);
        *states.borrow_mut() = Some(RowStates { count, tint });
        cranpose_ui::Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default(),
            move || {
                for index in 0..count.get() {
                    let red = match index {
                        0 => (tint.get() % 256) as f32 / 255.0,
                        _ => index as f32 / 2000.0,
                    };
                    cranpose_ui::Box(
                        Modifier::empty()
                            .fill_max_width()
                            .height(2.0)
                            .draw_behind(move |scope| {
                                scope.draw_rect_at(
                                    Rect {
                                        x: 0.0,
                                        y: 0.0,
                                        width: 8.0,
                                        height: 2.0,
                                    },
                                    Brush::solid(Color(red, 0.5, 1.0, 1.0)),
                                );
                            }),
                        BoxSpec::default(),
                        || {},
                    );
                }
            },
        );
    })
}

// The store heap of a scene of 100 rows at rest, after it took `peak` rows
// when `peak` is more, then 20 updates that recolor its first row.
fn heap_after(peak: Option<usize>) -> usize {
    let states = Rc::default();
    let mut composition = rows(Rc::clone(&states));
    let root = composition.root().expect("a root");
    let mut scene = initial_graph(&mut composition, root, VIEWPORT);
    let RowStates { count, tint } = states.borrow().expect("the row states");
    if let Some(peak) = peak {
        count.set(peak);
        update_scene(&mut composition, root, VIEWPORT, &mut scene);
        count.set(100);
        update_scene(&mut composition, root, VIEWPORT, &mut scene);
    }
    for step in 1..=20 {
        tint.set(step);
        update_scene(&mut composition, root, VIEWPORT, &mut scene);
    }
    scene.trim();
    scene.store().heap_bytes()
}

#[test]
fn a_scene_grown_and_shrunk_holds_at_rest_the_heap_of_one_that_never_grew() {
    let shrunk = heap_after(Some(2000));
    let steady = heap_after(None);
    println!("store heap: shrunk {shrunk} bytes, never grown {steady}");
    assert!(
        shrunk * 100 <= steady * 105,
        "the shrunk scene holds {shrunk} bytes, one that never grew {steady}"
    );
}
