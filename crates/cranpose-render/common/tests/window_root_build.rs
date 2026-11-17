// A scene built from the root of another window than the main one holds
// about the heap of its own nodes, however many nodes the applier holds
// beside them.

use std::{any::Any, rc::Rc};

use cranpose_core::NodeId;
use cranpose_ui::{
    BoxSpec, ColumnSpec, LayoutNode, Modifier, Size, TestComposition, WindowRootDescriptor,
};
use cranpose_ui_graphics::Color;

use crate::scene_probe::{initial_graph, with_applier};

const PANE: Size = Size {
    width: 100.0,
    height: 40.0,
};

struct Pane;

impl WindowRootDescriptor for Pane {
    fn layout_size(&self) -> Size {
        PANE
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// A column whose first child is the root of a window that holds one box,
// then `rows` boxes of the main window.
fn screen(rows: usize) -> TestComposition {
    cranpose_ui::run_test_composition(move || {
        cranpose_ui::Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default(),
            move || {
                cranpose_ui::Box(
                    Modifier::empty().window_root(Rc::new(Pane)),
                    BoxSpec::default(),
                    || {
                        cranpose_ui::Box(
                            Modifier::empty()
                                .size_points(90.0, 30.0)
                                .background(Color(0.2, 0.4, 0.8, 1.0)),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
                for index in 0..rows {
                    cranpose_ui::Box(
                        Modifier::empty()
                            .fill_max_width()
                            .height(2.0)
                            .background(Color(index as f32 / 4000.0, 0.5, 1.0, 1.0)),
                        BoxSpec::default(),
                        || {},
                    );
                }
            },
        );
    })
}

// The store heap of the window's scene, built once beside `rows` rows of
// the main window.
fn window_heap(rows: usize) -> usize {
    let mut composition = screen(rows);
    let root = composition.root().expect("a root");
    let window: NodeId = with_applier(&mut composition, |applier| {
        applier
            .with_node::<LayoutNode, _>(root, |column| column.children()[0])
            .expect("the column is a layout node")
    });
    let scene = initial_graph(&mut composition, window, PANE);
    scene.store().heap_bytes()
}

#[test]
fn a_window_scene_holds_the_heap_of_its_own_nodes() {
    let beside_few = window_heap(10);
    let beside_many = window_heap(4000);
    println!("window store heap: beside 10 rows {beside_few} bytes, beside 4000 {beside_many}");
    assert_eq!(
        beside_many, beside_few,
        "the window's scene took room for the main window's nodes"
    );
}
