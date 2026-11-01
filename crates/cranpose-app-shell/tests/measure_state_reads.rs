// A state that only a layout's measure policy reads measures the layout
// again when it changes, as Compose observes the state reads of a measure:
// nothing composes again, and the next frame lays the layout out at the new
// value. A measure that no longer reads a state is not measured again for
// it, and a child's intrinsic size that reads a state measures the parent
// that asked for it again.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, NodeId, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_ui::{Column, ColumnSpec, Layout, Modifier};
use cranpose_ui_graphics::Size;
use cranpose_ui_layout::{
    Constraints, Measurable, MeasurePolicy, MeasureResult, MeasureScope, Placement,
};

mod support;
use support::{CheckingRenderer, checking_shell};

// A count of measures, equal only to itself.
#[derive(Clone, Default)]
struct Measures(Rc<Cell<usize>>);

impl PartialEq for Measures {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

// What a leaf's width follows. Its measure reads the states only through
// the width it takes.
#[derive(Clone, PartialEq)]
enum LeafWidth {
    // The state's value.
    State(MutableState<f32>),
    // The width state's value while the switch says to read it, 30 else.
    Switched {
        reads_width: MutableState<bool>,
        width: MutableState<f32>,
    },
    // The width its parent gives it, with the state's value as its
    // intrinsic width.
    Given(MutableState<f32>),
}

// A leaf 10 high.
#[derive(Clone, PartialEq)]
struct Leaf {
    width: LeafWidth,
    measures: Measures,
}

impl Leaf {
    fn intrinsic_width(&self) -> f32 {
        match &self.width {
            LeafWidth::State(width) | LeafWidth::Given(width) => width.get(),
            LeafWidth::Switched { reads_width, width } => {
                if reads_width.get() {
                    width.get()
                } else {
                    30.0
                }
            }
        }
    }
}

impl MeasurePolicy for Leaf {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        self.measures.0.set(self.measures.0.get() + 1);
        let width = match self.width {
            LeafWidth::Given(_) => constraints.min_width,
            _ => self
                .intrinsic_width()
                .clamp(constraints.min_width, constraints.max_width),
        };
        MeasureResult::new(
            Size {
                width,
                height: 10.0,
            },
            Vec::new(),
        )
    }

    fn min_intrinsic_width(&self, _: &[Box<dyn Measurable>], _: f32) -> f32 {
        self.intrinsic_width()
    }

    fn max_intrinsic_width(&self, _: &[Box<dyn Measurable>], _: f32) -> f32 {
        self.intrinsic_width()
    }

    fn min_intrinsic_height(&self, _: &[Box<dyn Measurable>], _: f32) -> f32 {
        10.0
    }

    fn max_intrinsic_height(&self, _: &[Box<dyn Measurable>], _: f32) -> f32 {
        10.0
    }
}

// A layout as wide as its child's intrinsic width, which gives the child
// that width.
#[derive(Clone, PartialEq)]
struct AsWideAsChild;

impl MeasurePolicy for AsWideAsChild {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        _constraints: Constraints,
    ) -> MeasureResult {
        let width = measurables[0].max_intrinsic_width(10.0);
        let child = measurables[0].measure(Constraints::tight(width, 10.0));
        child.place(0.0, 0.0);
        MeasureResult::new(
            Size {
                width,
                height: 10.0,
            },
            vec![Placement::new(child.node_id(), 0.0, 0.0, 0)],
        )
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables[0].min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables[0].max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables[0].min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables[0].max_intrinsic_height(width)
    }
}

// The screen's states and the node whose width the tests read.
#[derive(Clone, Copy, PartialEq)]
struct Screen {
    reads_width: MutableState<bool>,
    width: MutableState<f32>,
    node: NodeId,
}

type Captured = Rc<RefCell<Option<Screen>>>;

// How the screen lays its leaf out.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    // The leaf reads its width state.
    Leaf,
    // The leaf reads its width state while its switch says so.
    Switched,
    // A parent takes the leaf's intrinsic width, which reads the state.
    Parent,
}

#[composable]
fn MeasuredScreen(shape: Shape, captured: Captured, measures: Measures) {
    let reads_width = rememberMutableStateOf(|| true);
    let width = rememberMutableStateOf(|| 40.0f32);
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        let leaf = |width: LeafWidth| Leaf {
            width,
            measures: measures.clone(),
        };
        let node = match shape {
            Shape::Leaf => Layout(Modifier::empty(), leaf(LeafWidth::State(width)), || {}),
            Shape::Switched => Layout(
                Modifier::empty(),
                leaf(LeafWidth::Switched { reads_width, width }),
                || {},
            ),
            Shape::Parent => {
                let child = leaf(LeafWidth::Given(width));
                Layout(Modifier::empty(), AsWideAsChild, move || {
                    Layout(Modifier::empty(), child.clone(), || {});
                })
            }
        };
        *captured.borrow_mut() = Some(Screen {
            reads_width,
            width,
            node,
        });
    });
}

fn screen(shape: Shape, measures: &Measures) -> (AppShell<CheckingRenderer>, Screen) {
    let captured: Captured = Rc::new(RefCell::new(None));
    let (mut shell, _) = checking_shell(false, {
        let captured = Rc::clone(&captured);
        let measures = measures.clone();
        move || MeasuredScreen(shape, Rc::clone(&captured), measures.clone())
    });
    shell.update();
    let screen = (*captured.borrow()).expect("the screen is composed");
    (shell, screen)
}

fn width_of(shell: &mut AppShell<CheckingRenderer>, node: NodeId) -> Option<f32> {
    shell.node_layout_bounds(node).map(|bounds| bounds.2)
}

#[test]
fn a_state_only_a_measure_policy_reads_measures_the_layout_again() {
    let (mut shell, screen) = screen(Shape::Leaf, &Measures::default());
    assert_eq!(width_of(&mut shell, screen.node), Some(40.0));
    for value in [120.0, 75.0, 75.0, 10.0] {
        screen.width.set(value);
        shell.update();
        assert_eq!(
            width_of(&mut shell, screen.node),
            Some(value),
            "the layout's width after its measure's state became {value}"
        );
    }
}

#[test]
fn a_measure_that_stops_reading_a_state_is_not_measured_again_for_it() {
    let measures = Measures::default();
    let (mut shell, screen) = screen(Shape::Switched, &measures);
    screen.width.set(50.0);
    shell.update();
    assert_eq!(width_of(&mut shell, screen.node), Some(50.0));
    screen.reads_width.set(false);
    shell.update();
    assert_eq!(width_of(&mut shell, screen.node), Some(30.0));
    let before = measures.0.get();
    screen.width.set(70.0);
    shell.update();
    assert_eq!(
        measures.0.get(),
        before,
        "a state the last measure did not read measured the layout again"
    );
    assert_eq!(width_of(&mut shell, screen.node), Some(30.0));
    screen.reads_width.set(true);
    shell.update();
    assert_eq!(width_of(&mut shell, screen.node), Some(70.0));
}

#[test]
fn an_intrinsic_size_of_a_child_that_reads_a_state_measures_its_parent_again() {
    let (mut shell, screen) = screen(Shape::Parent, &Measures::default());
    assert_eq!(width_of(&mut shell, screen.node), Some(40.0));
    for value in [90.0, 15.0, 15.0, 60.0] {
        screen.width.set(value);
        shell.update();
        assert_eq!(
            width_of(&mut shell, screen.node),
            Some(value),
            "the parent's width after its child's intrinsic width became {value}"
        );
    }
}
