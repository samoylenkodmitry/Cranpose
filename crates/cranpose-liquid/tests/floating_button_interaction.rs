use std::{cell::Cell, rc::Rc};

use cranpose_liquid::prelude::*;
use cranpose_testing::{
    RobotTestRule, TestRenderer, create_headless_robot_test, placed_semantics_from_shell,
};
use cranpose_ui::{KeyCode, KeyEvent, Modifier};

fn button() -> (RobotTestRule<TestRenderer>, Rc<Cell<usize>>) {
    let clicks = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&clicks);
    let mut robot = create_headless_robot_test(400, 300, move || {
        let recorded = Rc::clone(&recorded);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            GlassButton(
                Modifier::empty()
                    .offset(100.0, 100.0)
                    .size_points(100.0, 44.0),
                GlassButtonSpec::glass(),
                move || recorded.set(recorded.get() + 1),
                || GlassButtonLabel("Continue".into(), GlassButtonSpec::glass()),
            );
        });
    });
    robot.wait_for_idle();
    (robot, clicks)
}

#[test]
fn floating_release_tolerance_stays_relative_to_the_resting_target() {
    for hold_frames in [0, 36] {
        for (beyond_edge, expected) in [(69.0, 1), (70.0, 0)] {
            let (mut robot, clicks) = button();
            robot.mouse_move(150.0, 122.0);
            robot.mouse_down();
            for _ in 0..hold_frames {
                robot.advance_time(16_666_667);
            }
            robot.mouse_move(200.0 + beyond_edge, 122.0);
            robot.mouse_up();
            assert_eq!(
                clicks.get(),
                expected,
                "hold={hold_frames}, edge={beyond_edge}"
            );
        }
    }
}

#[test]
fn another_finger_cannot_release_the_active_floating_button() {
    let (mut robot, clicks) = button();
    robot.mouse_move(150.0, 122.0);
    robot.mouse_down();
    robot
        .shell_mut()
        .secondary_pointer_pressed(7, 150.0, 122.0, None);
    robot
        .shell_mut()
        .secondary_pointer_released(7, 150.0, 122.0, None);
    robot.wait_for_idle();
    assert_eq!(clicks.get(), 0);
    robot.mouse_up();
    assert_eq!(clicks.get(), 1);
}

#[test]
fn chip_selection_is_immediate_and_keeps_one_accessible_label_during_transition() {
    let selected = Rc::new(Cell::new(false));
    let observed = Rc::clone(&selected);
    let mut robot = create_headless_robot_test(400, 300, move || {
        let observed = Rc::clone(&observed);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let state = cranpose_core::rememberMutableStateOf(|| false);
            let observed = Rc::clone(&observed);
            LiquidChip(
                Modifier::empty().size_points(100.0, 28.0),
                state.get(),
                move || {
                    state.set(!state.get());
                    observed.set(state.get());
                },
                "Unread".into(),
            );
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    for expected in [true, false, true] {
        robot.click_at(50.0, 14.0);
        assert_eq!(selected.get(), expected);
        robot.advance_time(50_000_000);
        let tree = placed_semantics_from_shell(robot.shell_mut()).expect("chip semantics");
        let actions: Vec<_> = tree
            .flatten()
            .into_iter()
            .filter(|node| node.clickable && !node.hidden)
            .collect();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].label.as_deref(), Some("Unread"));
    }
}

#[test]
fn floating_button_can_be_reached_and_activated_from_the_keyboard() {
    let (mut robot, clicks) = button();
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let tree = placed_semantics_from_shell(robot.shell_mut()).expect("button semantics");
    let buttons: Vec<_> = tree
        .flatten()
        .into_iter()
        .filter(|node| node.clickable && !node.hidden)
        .collect();
    assert_eq!(buttons.len(), 1, "one control, one accessible action");
    assert_eq!(buttons[0].label.as_deref(), Some("Continue"));
    assert_eq!(buttons[0].role, cranpose_ui::layout::SemanticsRole::Button);
    robot
        .shell_mut()
        .on_key_event(&KeyEvent::key_down(KeyCode::Tab, "\t"));
    robot.wait_for_idle();
    robot
        .shell_mut()
        .on_key_event(&KeyEvent::key_down(KeyCode::Enter, "\r"));
    robot.wait_for_idle();
    assert_eq!(clicks.get(), 1);
}

#[test]
fn pointer_activation_uses_the_callback_from_the_latest_composition() {
    let observed = Rc::new(Cell::new(usize::MAX));
    let recorded = Rc::clone(&observed);
    let mut robot = create_headless_robot_test(400, 300, move || {
        let recorded = Rc::clone(&recorded);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let generation = cranpose_core::rememberMutableStateOf(|| 0usize);
            let current = generation.get();
            GlassButton(
                Modifier::empty().size_points(100.0, 44.0),
                GlassButtonSpec::glass(),
                move || {
                    recorded.set(current);
                    generation.set(current + 1);
                },
                || GlassButtonLabel("Continue".into(), GlassButtonSpec::glass()),
            );
        });
    });
    robot.wait_for_idle();
    for expected in 0..3 {
        robot.click_at(50.0, 22.0);
        robot.wait_for_idle();
        assert_eq!(observed.get(), expected);
    }
}

#[test]
fn scrolling_from_a_floating_button_cancels_its_activation() {
    let clicks = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&clicks);
    let mut robot = create_headless_robot_test(400, 300, move || {
        let recorded = Rc::clone(&recorded);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let scroll = cranpose_ui::rememberScrollState!(0.0);
            cranpose_ui::Column(
                Modifier::empty()
                    .fill_max_size()
                    .vertical_scroll(scroll, false),
                cranpose_ui::ColumnSpec::default(),
                move || {
                    cranpose_ui::Spacer(Modifier::empty().height(200.0));
                    let recorded = Rc::clone(&recorded);
                    GlassButton(
                        Modifier::empty().size_points(100.0, 44.0),
                        GlassButtonSpec::glass(),
                        move || recorded.set(recorded.get() + 1),
                        || GlassButtonLabel("Continue".into(), GlassButtonSpec::glass()),
                    );
                    cranpose_ui::Spacer(Modifier::empty().height(600.0));
                },
            );
        });
    });
    robot.wait_for_idle();
    let before = robot
        .find_by_text("Continue")
        .center()
        .expect("button position");
    robot.drag(before.x, before.y, before.x, before.y - 50.0);
    let after = robot
        .find_by_text("Continue")
        .center()
        .expect("scrolled button position");
    assert!(after.y < before.y - 20.0, "the parent must actually scroll");
    assert_eq!(clicks.get(), 0);
}
