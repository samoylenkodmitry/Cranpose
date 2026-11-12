// A scene that grew to many rows and shrank back holds about the heap of a
// scene that held the rows that remain from its start, once both went on
// with the same updates.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_ui::{BoxSpec, ColumnSpec, Modifier, Size, TestComposition};
use cranpose_ui_graphics::{Brush, Color, Rect};

use crate::scene_probe::{initial_graph, update_scene};

const VIEWPORT: Size = Size {
    width: 200.0,
    height: 4000.0,
};

// How each row paints its color.
#[derive(Clone, Copy)]
enum Paint {
    // A rect its draw records.
    Rect,
    // A background, which the store keeps among its shapes.
    Background,
}

#[derive(Clone, Copy)]
struct RowStates {
    count: MutableState<usize>,
    tint: MutableState<u32>,
}

// A column of `count` rows, each painting a color of its own as `paint`
// says; the first row's follows `tint`.
fn rows(states: Rc<RefCell<Option<RowStates>>>, paint: Paint) -> TestComposition {
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
                    let color = Color(red, 0.5, 1.0, 1.0);
                    let row = Modifier::empty().fill_max_width().height(2.0);
                    let row = match paint {
                        Paint::Rect => row.draw_behind(move |scope| {
                            scope.draw_rect_at(
                                Rect {
                                    x: 0.0,
                                    y: 0.0,
                                    width: 8.0,
                                    height: 2.0,
                                },
                                Brush::solid(color),
                            );
                        }),
                        Paint::Background => row.background(color),
                    };
                    cranpose_ui::Box(row, BoxSpec::default(), || {});
                }
            },
        );
    })
}

// The store heap of a scene of 100 rows, after it took `peak` rows when
// `peak` is more, then after 20 updates that recolor its first row.
fn heap_after(paint: Paint, peak: Option<usize>) -> usize {
    let states = Rc::default();
    let mut composition = rows(Rc::clone(&states), paint);
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
    scene.store().heap_bytes()
}

fn assert_shrunk_holds_the_heap_of_one_that_never_grew(paint: Paint) {
    let shrunk = heap_after(paint, Some(2000));
    let steady = heap_after(paint, None);
    assert!(
        shrunk * 100 <= steady * 105,
        "the shrunk scene holds {shrunk} bytes, one that never grew {steady}"
    );
}

#[test]
fn a_scene_grown_and_shrunk_holds_the_heap_of_one_that_never_grew() {
    assert_shrunk_holds_the_heap_of_one_that_never_grew(Paint::Rect);
}

#[test]
fn a_scene_of_backgrounds_grown_and_shrunk_holds_the_heap_of_one_that_never_grew() {
    assert_shrunk_holds_the_heap_of_one_that_never_grew(Paint::Background);
}
