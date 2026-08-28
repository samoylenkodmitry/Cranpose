use std::fmt::Debug;

use cranpose_app_shell::AppShell;
use cranpose_core::MutableState;
use cranpose_render_common::Renderer;
use cranpose_testing::{
    robot::{RobotTestRule, TestRenderer},
    ComposeTestRule,
};
use desktop_app::app::{DemoTab, TEST_ACTIVE_TAB_STATE, TEST_RECURSIVE_LAYOUT_DEPTH_STATE};

pub fn with_active_tab<F>(f: F)
where
    F: FnOnce(&MutableState<DemoTab>),
{
    TEST_ACTIVE_TAB_STATE.with(|cell| {
        let state = *cell.borrow().as_ref().expect("active tab state registered");
        f(&state);
    });
}

pub fn active_tab_state() -> MutableState<DemoTab> {
    TEST_ACTIVE_TAB_STATE.with(|cell| *cell.borrow().as_ref().expect("active tab state registered"))
}

pub fn set_active_tab(tab: DemoTab) {
    with_active_tab(|state| state.set(tab));
}

pub fn wait_for_active_tab_registration_robot(robot: &mut RobotTestRule<TestRenderer>) {
    for _ in 0..20 {
        robot.wait_for_idle();
        let registered = TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow().is_some());
        if registered {
            return;
        }
    }
    panic!("active tab state was not registered");
}

pub fn with_recursive_depth<F>(f: F)
where
    F: FnOnce(&MutableState<usize>),
{
    TEST_RECURSIVE_LAYOUT_DEPTH_STATE.with(|cell| {
        let state = *cell
            .borrow()
            .as_ref()
            .expect("recursive layout depth state registered");
        f(&state);
    });
}

pub fn set_recursive_depth(depth: usize) {
    with_recursive_depth(|state| state.set(depth));
}

pub fn wait_for_recursive_depth_registration(rule: &mut ComposeTestRule) {
    for _ in 0..20 {
        let registered = TEST_RECURSIVE_LAYOUT_DEPTH_STATE.with(|cell| cell.borrow().is_some());
        if registered {
            return;
        }
        rule.pump_until_idle()
            .expect("pump while waiting for recursive layout depth state registration");
    }
    panic!(
        "recursive layout depth state not registered. tree:\n{}",
        rule.dump_tree(),
    );
}

pub fn pump_shell_until_stable<R>(shell: &mut AppShell<R>)
where
    R: Renderer,
    R::Error: Debug,
{
    for _ in 0..80 {
        if !(shell.needs_redraw() || shell.has_active_animations()) {
            break;
        }
        shell.update();
    }
}
