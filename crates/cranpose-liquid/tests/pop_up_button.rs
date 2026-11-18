//! `LiquidPopUpButton` and `LiquidColorWell` through the headless robot: the
//! pop-up button reads its option, opens a menu that checks the selected
//! option and selects the one chosen; the color well reads as a button and
//! calls its action.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_liquid::prelude::*;
use cranpose_testing::{
    PlacedSemanticsNode, RobotTestRule, TestRenderer, create_headless_robot_test,
    placed_semantics_from_shell,
};
use cranpose_ui::{Modifier, SemanticsWidgetRole};
use cranpose_ui_graphics::Color;

type Robot = RobotTestRule<TestRenderer>;

const FRUITS: [&str; 3] = ["Apple", "Banana", "Cherry"];

fn node(robot: &mut Robot, label: &str) -> PlacedSemanticsNode {
    let tree = placed_semantics_from_shell(robot.shell_mut()).expect("placed controls");
    tree.flatten()
        .into_iter()
        .find(|node| !node.hidden && node.label.as_deref() == Some(label))
        .cloned()
        .unwrap_or_else(|| panic!("no reader node named {label}"))
}

fn activate(robot: &mut Robot, label: &str) {
    let target = node(robot, label);
    assert!(target.clickable, "{label} advertises activation");
    assert!(
        robot
            .shell_mut()
            .accessibility_activate(target.node_id, None),
        "{label} takes the activation"
    );
    robot.wait_for_motion();
}

#[test]
fn a_pop_up_button_selects_the_option_chosen_from_its_menu() {
    let slot: Rc<Cell<Option<MutableState<usize>>>> = Rc::default();
    let captured = Rc::clone(&slot);
    let mut robot = create_headless_robot_test(400, 600, move || {
        let captured = Rc::clone(&captured);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let selected = rememberMutableStateOf(|| 0_usize);
            captured.set(Some(selected));
            LiquidPopUpButton(
                Modifier::empty(),
                "Fruit",
                &FRUITS,
                selected.get(),
                move |next| selected.set(next),
            );
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let selected = slot
        .take()
        .expect("the composition remembered the selection");

    let button = node(&mut robot, "Fruit");
    assert_eq!(button.widget_role, Some(SemanticsWidgetRole::DropdownList));
    assert_eq!(button.state_description.as_deref(), Some("Apple"));

    activate(&mut robot, "Fruit");
    let apple = node(&mut robot, "Apple");
    assert_eq!(
        apple.toggled,
        Some(true.into()),
        "the menu checks the selection"
    );
    activate(&mut robot, "Cherry");
    assert_eq!(selected.get(), 2);
    assert_eq!(
        node(&mut robot, "Fruit").state_description.as_deref(),
        Some("Cherry")
    );
}

#[test]
fn a_color_well_reads_as_a_button_and_calls_its_action() {
    let taps = Rc::new(Cell::new(0));
    let counter = Rc::clone(&taps);
    let mut robot = create_headless_robot_test(400, 200, move || {
        let counter = Rc::clone(&counter);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let counter = Rc::clone(&counter);
            LiquidColorWell(
                Modifier::empty(),
                "Tint",
                Color::from_rgb_u8(0, 136, 255),
                move || counter.set(counter.get() + 1),
            );
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let well = node(&mut robot, "Tint");
    assert_eq!(well.widget_role, Some(SemanticsWidgetRole::Button));
    assert_eq!(well.layout_bounds.width, 28.0);
    activate(&mut robot, "Tint");
    assert_eq!(taps.get(), 1);
}
