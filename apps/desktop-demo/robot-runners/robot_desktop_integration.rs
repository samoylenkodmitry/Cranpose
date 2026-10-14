//! A desktop integration sees the primary window's raw handle as the window
//! opens, and writes state from the shell's main thread before the event
//! loop waits; the screen then shows what it wrote.

use std::cell::RefCell;

use cranpose::{
    DesktopIntegration, DesktopIntegrationContext, DesktopWindow, DesktopWindowId,
    raw_window_handle::HasWindowHandle,
};
use cranpose_core::{MutableState, mutableStateOf, remember};
use cranpose_testing::{find_in_semantics, find_text};
use cranpose_ui::{Modifier, Text, TextStyle};

use crate::{robot_exit, robot_launch, text_input_robot_helpers};

const READY: &str = "integration saw the primary window";

thread_local! {
    /// The status text the screen shows, installed by its composition.
    static STATUS: RefCell<Option<MutableState<String>>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct Probe {
    /// Whether the primary window opened with a raw handle.
    primary_handle: bool,
    written: bool,
}

impl DesktopIntegration for Probe {
    fn window_opened(&mut self, window: &DesktopWindow<'_>, _: &mut DesktopIntegrationContext<'_>) {
        self.primary_handle |=
            window.id() == DesktopWindowId::Primary && window.window_handle().is_ok();
    }

    fn about_to_wait(&mut self, context: &mut DesktopIntegrationContext<'_>) {
        let Some(status) = STATUS.with(|status| *status.borrow()) else {
            return;
        };
        if self.primary_handle && !self.written {
            self.written = context.update(|| status.set(READY.to_owned())).is_ok();
        }
    }
}

pub(crate) fn main() {
    env_logger::init();
    robot_launch::launch("Desktop Integration", 400, 200)
        .with_desktop_integration(Probe::default())
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(60);
            let shown = text_input_robot_helpers::wait_for_in_semantics(&robot, |robot| {
                find_in_semantics(robot, |element| find_text(element, READY))
            });
            if shown.is_none() {
                robot_exit::fail(&robot, "the integration's state never reached the screen");
            }
            println!("PASS: {READY}");
            let _ = robot.exit();
        })
        .run(|| {
            let status = remember(|| mutableStateOf("waiting".to_owned())).with(|status| *status);
            STATUS.with(|cell| cell.replace(Some(status)));
            Text(status.get(), Modifier::empty(), TextStyle::default());
        });
}
