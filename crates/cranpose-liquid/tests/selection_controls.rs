//! `LiquidCheckbox` and `LiquidRadioButton` through the headless robot: a tap
//! and a screen reader's activation change them, readers hear their role and
//! state, and each one offers a touch target of the minimum interactive size.

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_liquid::prelude::*;
use cranpose_testing::{
    MINIMUM_TARGET_SIZE, PlacedSemanticsNode, RobotTestRule, TestRenderer,
    create_headless_robot_test, placed_semantics_from_shell,
};
use cranpose_ui::{Column, ColumnSpec, Modifier, SemanticsWidgetRole};

fn placed(robot: &mut RobotTestRule<TestRenderer>) -> PlacedSemanticsNode {
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    placed_semantics_from_shell(robot.shell_mut()).expect("the controls are placed")
}

fn tagged<'a>(root: &'a PlacedSemanticsNode, tag: &str) -> &'a PlacedSemanticsNode {
    root.find_by_tag(tag)
        .unwrap_or_else(|| panic!("{tag} is placed"))
}

#[test]
fn a_checkbox_toggles_on_a_tap_and_on_a_reader_activation() {
    let checked: std::rc::Rc<std::cell::Cell<Option<MutableState<bool>>>> = Default::default();
    let installed = std::rc::Rc::clone(&checked);
    let mut robot = create_headless_robot_test(300, 200, move || {
        let state = rememberMutableStateOf(|| false);
        installed.set(Some(state));
        LiquidTheme(LiquidThemeSpec::default(), move || {
            LiquidCheckbox(
                Modifier::empty()
                    .content_description("Remember me")
                    .test_tag("remember"),
                state.get(),
                move |next| state.set(next),
            );
        });
    });
    let state = checked.get().expect("the state is installed");

    let root = placed(&mut robot);
    let checkbox = tagged(&root, "remember");
    assert_eq!(checkbox.widget_role, Some(SemanticsWidgetRole::Checkbox));
    assert_eq!(checkbox.toggled, Some(false));
    let target = checkbox.target_bounds();
    assert!(
        target.width >= MINIMUM_TARGET_SIZE && target.height >= MINIMUM_TARGET_SIZE,
        "touch target {target:?}"
    );

    let (x, y) = (
        target.x + target.width / 2.0,
        target.y + target.height / 2.0,
    );
    assert!(robot.click_at(x, y));
    robot.wait_for_idle();
    assert!(state.get(), "a tap checks it");
    let root = placed(&mut robot);
    assert_eq!(tagged(&root, "remember").toggled, Some(true));

    let node = tagged(&root, "remember").node_id;
    assert!(robot.shell_mut().accessibility_activate(node, None));
    robot.wait_for_idle();
    assert!(!state.get(), "a reader's activation unchecks it");
}

#[test]
fn a_radio_group_picks_one_option() {
    let picked: std::rc::Rc<std::cell::Cell<Option<MutableState<usize>>>> = Default::default();
    let installed = std::rc::Rc::clone(&picked);
    let mut robot = create_headless_robot_test(300, 300, move || {
        let state = rememberMutableStateOf(|| 0usize);
        installed.set(Some(state));
        LiquidTheme(LiquidThemeSpec::default(), move || {
            Column(
                Modifier::empty().selectable_group(),
                ColumnSpec::default(),
                move || {
                    for (index, name) in
                        ["Standard", "Express", "Overnight"].into_iter().enumerate()
                    {
                        LiquidRadioButton(
                            Modifier::empty().content_description(name).test_tag(name),
                            state.get() == index,
                            move || state.set(index),
                        );
                    }
                },
            );
        });
    });
    let state = picked.get().expect("the state is installed");

    let root = placed(&mut robot);
    for (index, name) in ["Standard", "Express", "Overnight"].into_iter().enumerate() {
        let option = tagged(&root, name);
        assert_eq!(option.widget_role, Some(SemanticsWidgetRole::RadioButton));
        assert_eq!(option.selected, Some(index == 0), "{name}");
    }

    let express = tagged(&root, "Express").target_bounds();
    assert!(robot.click_at(
        express.x + express.width / 2.0,
        express.y + express.height / 2.0
    ));
    robot.wait_for_idle();
    assert_eq!(state.get(), 1);
    let root = placed(&mut robot);
    assert_eq!(tagged(&root, "Standard").selected, Some(false));
    assert_eq!(tagged(&root, "Express").selected, Some(true));
}
