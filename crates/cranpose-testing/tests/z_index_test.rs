//! `Modifier::z_index` and a placement's z index add up and order siblings
//! as Compose's `zIndex` does: the larger draws on top and takes the tap
//! where they overlap, and equal ones keep the composition order.

use std::{cell::Cell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{
    Box, BoxSpec, Layout, MeasureResult, Modifier, Placement, Size, widgets::Scaffold,
};
use cranpose_ui_layout::{Constraints, Measurable, MeasurePolicy, MeasureScope};

/// Two boxes on one spot; returns which one each tap reached.
fn overlapping(
    front_z: Rc<Cell<Option<MutableState<f32>>>>,
    tapped: Rc<Cell<&'static str>>,
) -> impl FnMut() + 'static {
    move || {
        let z = cranpose_core::rememberMutableStateOf(|| 0.0f32);
        front_z.set(Some(z));
        let tapped = Rc::clone(&tapped);
        Box(Modifier::empty(), BoxSpec::default(), move || {
            for (label, z_index) in [("first", z.get()), ("second", 0.0)] {
                let tapped = Rc::clone(&tapped);
                Box(
                    Modifier::empty()
                        .size(Size::new(100.0, 100.0))
                        .z_index(z_index)
                        .clickable(move |_| tapped.set(label)),
                    BoxSpec::default(),
                    || {},
                );
            }
        });
    }
}

#[test]
fn the_sibling_with_the_larger_z_index_takes_the_tap() {
    let z = Rc::new(Cell::new(None));
    let tapped = Rc::new(Cell::new(""));
    let mut robot =
        create_headless_robot_test(200, 200, overlapping(Rc::clone(&z), Rc::clone(&tapped)));

    assert!(robot.click_at(50.0, 50.0));
    assert_eq!(
        tapped.get(),
        "second",
        "equal z indices keep the composition order"
    );

    z.get().expect("the state is installed").set(1.0);
    robot.wait_for_idle();
    assert!(robot.click_at(50.0, 50.0));
    assert_eq!(
        tapped.get(),
        "first",
        "a larger z index moves the first box on top"
    );

    z.get().expect("the state is installed").set(-1.0);
    robot.wait_for_idle();
    assert!(robot.click_at(50.0, 50.0));
    assert_eq!(
        tapped.get(),
        "second",
        "a smaller z index sends it back under"
    );
}

// Places every child at its top start, the first with `first_z` and the
// others with 0, and takes the largest of their sizes.
#[derive(Clone, PartialEq)]
struct Stack {
    first_z: f32,
}

fn largest(measurables: &[Box<dyn Measurable>], size: impl Fn(&dyn Measurable) -> f32) -> f32 {
    measurables
        .iter()
        .map(|measurable| size(measurable.as_ref()))
        .fold(0.0, f32::max)
}

impl MeasurePolicy for Stack {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let placements = measurables
            .iter()
            .enumerate()
            .map(|(index, measurable)| {
                let placeable = measurable.measure(constraints);
                placeable.place(0.0, 0.0);
                let z_index = if index == 0 { self.first_z } else { 0.0 };
                Placement::new(placeable.node_id(), 0.0, 0.0, z_index)
            })
            .collect();
        MeasureResult::new(
            Size::new(constraints.max_width, constraints.max_height),
            placements,
        )
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        largest(measurables, |child| child.min_intrinsic_width(height))
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        largest(measurables, |child| child.max_intrinsic_width(height))
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        largest(measurables, |child| child.min_intrinsic_height(width))
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        largest(measurables, |child| child.max_intrinsic_height(width))
    }
}

// The z index the stack places its first box with, and the second box's
// `Modifier::z_index`.
#[derive(Clone, Copy)]
struct StackZ {
    placed: MutableState<f32>,
    second_modifier: MutableState<f32>,
}

// Two boxes a [`Stack`] places on one spot; records which one each tap
// reached.
fn placed_overlapping(
    states: Rc<Cell<Option<StackZ>>>,
    tapped: Rc<Cell<&'static str>>,
) -> impl FnMut() + 'static {
    move || {
        let z = cranpose_core::remember(|| StackZ {
            placed: cranpose_core::mutableStateOf(0.0),
            second_modifier: cranpose_core::mutableStateOf(0.0),
        })
        .with(|z| *z);
        states.set(Some(z));
        let tapped = Rc::clone(&tapped);
        Layout(
            Modifier::empty(),
            Stack {
                first_z: z.placed.get(),
            },
            move || {
                for (label, z_index) in [("first", 0.0), ("second", z.second_modifier.get())] {
                    let tapped = Rc::clone(&tapped);
                    Box(
                        Modifier::empty()
                            .size(Size::new(100.0, 100.0))
                            .z_index(z_index)
                            .clickable(move |_| tapped.set(label)),
                        BoxSpec::default(),
                        || {},
                    );
                }
            },
        );
    }
}

#[test]
fn a_placement_z_index_orders_siblings_with_the_modifier_one() {
    let states = Rc::new(Cell::new(None));
    let tapped = Rc::new(Cell::new(""));
    let mut robot = create_headless_robot_test(
        200,
        200,
        placed_overlapping(Rc::clone(&states), Rc::clone(&tapped)),
    );
    let z = states.get().expect("the states are installed");

    assert!(robot.click_at(50.0, 50.0));
    assert_eq!(
        tapped.get(),
        "second",
        "equal z indices keep the composition order"
    );

    z.placed.set(1.0);
    robot.wait_for_idle();
    assert!(robot.click_at(50.0, 50.0));
    assert_eq!(
        tapped.get(),
        "first",
        "the first box placed with a larger z index is on top"
    );

    z.second_modifier.set(2.0);
    robot.wait_for_idle();
    assert!(robot.click_at(50.0, 50.0));
    assert_eq!(
        tapped.get(),
        "second",
        "the second box's modifier z index is larger than the first's placement"
    );

    z.placed.set(0.0);
    z.second_modifier.set(0.0);
    robot.wait_for_idle();
    assert!(robot.click_at(50.0, 50.0));
    assert_eq!(
        tapped.get(),
        "second",
        "back at equal z indices, the composition order"
    );
}

// A scaffold places its top bar after its full-size content and with a
// larger z index: ordered by z index, the bar still takes the tap where the
// two overlap.
#[test]
fn a_scaffold_top_bar_takes_the_tap_over_its_content() {
    let tapped = Rc::new(Cell::new(""));
    let mut robot = create_headless_robot_test(200, 200, {
        let tapped = Rc::clone(&tapped);
        move || {
            let bar = Rc::clone(&tapped);
            let content = Rc::clone(&tapped);
            Scaffold(
                Modifier::empty().fill_max_size(),
                move || {
                    let bar = Rc::clone(&bar);
                    Box(
                        Modifier::empty()
                            .size(Size::new(200.0, 50.0))
                            .clickable(move |_| bar.set("bar")),
                        BoxSpec::default(),
                        || {},
                    );
                },
                || {},
                move |_| {
                    let content = Rc::clone(&content);
                    Box(
                        Modifier::empty()
                            .fill_max_size()
                            .clickable(move |_| content.set("content")),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        }
    });

    assert!(robot.click_at(100.0, 25.0));
    assert_eq!(tapped.get(), "bar", "the bar lies over the content");
    assert!(robot.click_at(100.0, 150.0));
    assert_eq!(tapped.get(), "content", "below the bar, the content");
}
