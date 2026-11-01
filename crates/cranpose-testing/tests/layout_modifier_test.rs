// `Modifier::layout` measures and places the content it wraps, as Compose's
// `Modifier.layout { measurable, constraints -> }`: its size is what its
// parent lays out, a state it reads measures it again, and intrinsic sizes
// run it on the content's intrinsic sizes.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, IntrinsicSize, LayoutModifierMeasureResult, Modifier, Row,
    RowSpec, Size,
};

// A modifier that adds `extra` around its content's height, half above.
fn taller(modifier: Modifier, extra: impl Fn() -> f32 + 'static) -> Modifier {
    modifier.layout(move |measurable, constraints| {
        let extra = extra();
        let placeable = measurable.measure(constraints);
        LayoutModifierMeasureResult::new(
            Size::new(placeable.width(), placeable.height() + extra),
            0.0,
            extra / 2.0,
        )
    })
}

fn tagged_box(tag: &'static str, modifier: Modifier) {
    Box(modifier.test_tag(tag), BoxSpec::default(), || {});
}

fn top(robot: &mut RobotTestRule<TestRenderer>, tag: &str) -> Option<f32> {
    robot.find_by_tag(tag).bounds().map(|bounds| bounds.y)
}

#[test]
fn the_parent_lays_out_the_size_the_closure_gives_and_a_read_state_measures_it_again() {
    let extra: Rc<Cell<Option<MutableState<f32>>>> = Rc::default();
    let installed = Rc::clone(&extra);
    let mut robot = create_headless_robot_test(200, 300, move || {
        let state = rememberMutableStateOf(|| 20.0_f32);
        installed.set(Some(state));
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            tagged_box(
                "content",
                taller(Modifier::empty(), move || state.get()).size(Size::new(40.0, 30.0)),
            );
            tagged_box("below", Modifier::empty().size(Size::new(40.0, 10.0)));
        });
    });
    assert_eq!(top(&mut robot, "below"), Some(50.0), "30 high and 20 more");

    extra.get().expect("composed").set(60.0);
    robot.wait_for_idle();
    assert_eq!(top(&mut robot, "below"), Some(90.0), "the new extra counts");
}

#[test]
fn an_intrinsic_height_runs_the_closure_on_the_content_intrinsic_height() {
    let mut robot = create_headless_robot_test(200, 300, || {
        Row(
            Modifier::empty().height_intrinsic(IntrinsicSize::Max),
            RowSpec::default(),
            || {
                tagged_box(
                    "content",
                    taller(Modifier::empty(), || 10.0).size(Size::new(20.0, 30.0)),
                );
                tagged_box("fill", Modifier::empty().width(10.0).fill_max_height());
            },
        );
    });
    assert_eq!(
        robot
            .find_by_tag("fill")
            .bounds()
            .map(|bounds| bounds.height),
        Some(40.0),
        "the row is as high as the content and the 10 the closure adds"
    );
}
