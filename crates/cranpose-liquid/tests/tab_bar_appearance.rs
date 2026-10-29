use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_liquid::prelude::*;
use cranpose_testing::create_headless_robot_test;
use cranpose_ui::Modifier;

// A tab bar that appears in a running app, as on a switch to the tab that
// holds it, settles at once. Its first frame drew the lens at a bar width
// nothing had measured yet, and the lens strained from that jump for about
// three seconds, drawing every frame of them.
#[test]
fn a_tab_bar_shown_in_a_running_app_settles_at_once() {
    let shown: Rc<Cell<Option<MutableState<bool>>>> = Rc::new(Cell::new(None));
    let handle = Rc::clone(&shown);
    let mut robot = create_headless_robot_test(400, 120, move || {
        let state = rememberMutableStateOf(|| false);
        handle.set(Some(state));
        if state.get() {
            LiquidTheme(LiquidThemeSpec::default(), || {
                LiquidTabBar(
                    Modifier::empty().width(390.0),
                    LiquidTabBarSpec::default(),
                    1,
                    |_| {},
                    |tabs| {
                        for label in ["One", "Two", "Three"] {
                            tabs.tab(icons::SEARCH, label);
                        }
                    },
                );
            });
        }
    });
    robot.wait_for_idle();
    robot.advance_time(1_000_000_000);
    shown.get().expect("the content composed").set(true);
    robot.wait_for_motion();
}
