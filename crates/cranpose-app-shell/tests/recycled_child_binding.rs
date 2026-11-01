// A child that leaves its parent, and a recycled node that takes its id in
// the same place, lay out as the new node: the parent's record of the old
// child still holds the old node's parts, and binds the new node's instead.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, NodeId, key, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_ui::{Column, ColumnSpec, Layout, Modifier};
use cranpose_ui_graphics::Size;
use cranpose_ui_layout::{
    Constraints, Measurable, MeasurePolicy, MeasureResult, MeasureScope, Placement,
};

mod support;
use support::{CheckingRenderer, checking_shell};

// The layouts of the screen.
#[derive(Clone, PartialEq)]
enum Policy {
    // A leaf of a fixed width, 10 high.
    Leaf(f32),
    // A layout that measures and places its child while open, and nothing
    // else.
    Gate { open: bool },
}

impl Policy {
    fn intrinsic_size(&self) -> Size {
        match *self {
            Self::Leaf(width) => Size {
                width,
                height: 10.0,
            },
            Self::Gate { .. } => Size::default(),
        }
    }
}

impl MeasurePolicy for Policy {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let size = match *self {
            Self::Leaf(width) => Size {
                width: width.clamp(constraints.min_width, constraints.max_width),
                height: 10.0,
            },
            Self::Gate { open } => {
                if open {
                    let child = measurables[0].measure(constraints);
                    child.place(0.0, 0.0);
                    placements.push(Placement::new(child.node_id(), 0.0, 0.0, 0.0));
                }
                Size {
                    width: constraints.max_width,
                    height: constraints.max_height,
                }
            }
        };
        MeasureResult::new(size, placements)
    }

    fn min_intrinsic_width(&self, _: &[Box<dyn Measurable>], _: f32) -> f32 {
        self.intrinsic_size().width
    }

    fn max_intrinsic_width(&self, _: &[Box<dyn Measurable>], _: f32) -> f32 {
        self.intrinsic_size().width
    }

    fn min_intrinsic_height(&self, _: &[Box<dyn Measurable>], _: f32) -> f32 {
        self.intrinsic_size().height
    }

    fn max_intrinsic_height(&self, _: &[Box<dyn Measurable>], _: f32) -> f32 {
        self.intrinsic_size().height
    }
}

// The screen's states and the leaf its column holds.
#[derive(Clone, Copy, PartialEq)]
struct Screen {
    open: MutableState<bool>,
    generation: MutableState<u32>,
    leaf: NodeId,
}

type Captured = Rc<RefCell<Option<Screen>>>;

fn width_for(generation: u32) -> f32 {
    40.0 + 10.0 * generation as f32
}

#[composable]
fn GatedScreen(captured: Captured) {
    let open = rememberMutableStateOf(|| true);
    let generation = rememberMutableStateOf(|| 0u32);
    Layout(
        Modifier::empty(),
        Policy::Gate { open: open.get() },
        move || {
            let captured = Rc::clone(&captured);
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                let current = generation.get();
                let leaf = key(current, || {
                    Layout(Modifier::empty(), Policy::Leaf(width_for(current)), || {})
                });
                *captured.borrow_mut() = Some(Screen {
                    open,
                    generation,
                    leaf,
                });
            });
        },
    );
}

fn width_of(shell: &mut AppShell<CheckingRenderer>, node: NodeId) -> Option<f32> {
    shell.node_layout_bounds(node).map(|bounds| bounds.2)
}

#[test]
fn a_recycled_node_with_the_id_of_a_removed_child_lays_out_as_itself() {
    let captured: Captured = Rc::new(RefCell::new(None));
    let (mut shell, _) = checking_shell(false, {
        let captured = Rc::clone(&captured);
        move || GatedScreen(Rc::clone(&captured))
    });
    let screen = |captured: &Captured| (*captured.borrow()).expect("the screen is composed");
    shell.update();
    let first = screen(&captured);
    assert_eq!(width_of(&mut shell, first.leaf), Some(width_for(0)));
    // The column skips a layout while a new leaf replaces the first, which
    // goes to the recycle pool.
    first.open.set(false);
    first.generation.set(1);
    shell.update();
    // The next leaf takes the first one's id from the pool, where the
    // column's record of the first still holds it.
    first.open.set(true);
    first.generation.set(2);
    shell.update();
    let third = screen(&captured);
    assert_eq!(
        third.leaf, first.leaf,
        "the third leaf took the first one's id"
    );
    assert_eq!(width_of(&mut shell, third.leaf), Some(width_for(2)));
}
