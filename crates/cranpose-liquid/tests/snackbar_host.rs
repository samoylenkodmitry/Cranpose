//! `LiquidSnackbarHost` through the headless robot: a snackbar shows its
//! message, times out as dismissed, ends as performed when its action is
//! tapped, waits its turn in the queue, and stays for a screen reader.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use cranpose_core::{CoroutineScope, rememberCoroutineScope};
use cranpose_liquid::prelude::*;
use cranpose_services::{AccessibilityState, ProvideAccessibilityState};
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::Modifier;

struct Host {
    robot: RobotTestRule<TestRenderer>,
    state: LiquidSnackbarHostState,
    scope: CoroutineScope,
}

fn host(screen_reader_on: bool) -> Host {
    let captured: Rc<RefCell<Option<(LiquidSnackbarHostState, CoroutineScope)>>> = Rc::default();
    let installed = Rc::clone(&captured);
    let robot = create_headless_robot_test(360, 200, move || {
        let installed = Rc::clone(&installed);
        ProvideAccessibilityState(AccessibilityState { screen_reader_on }, move || {
            LiquidTheme(LiquidThemeSpec::default(), {
                let installed = Rc::clone(&installed);
                move || {
                    let state = rememberLiquidSnackbarHostState();
                    *installed.borrow_mut() = Some((state.clone(), rememberCoroutineScope()));
                    LiquidSnackbarHost(state, Modifier::empty().fill_max_width());
                }
            });
        });
    });
    let (state, scope) = captured.borrow().clone().expect("the host is composed");
    Host {
        robot,
        state,
        scope,
    }
}

impl Host {
    /// Shows `visuals` from a coroutine; the cell holds the result once it
    /// ends.
    fn show(&mut self, visuals: SnackbarVisuals) -> Rc<Cell<Option<SnackbarResult>>> {
        let result = Rc::new(Cell::new(None));
        let (state, ended) = (self.state.clone(), Rc::clone(&result));
        self.scope.launch(async move {
            ended.set(Some(state.show(visuals).await));
        });
        self.robot.wait_for_idle();
        result
    }

    fn settle_until(&mut self, done: impl Fn(&Self) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done(self) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            self.robot.wait_for_idle();
        }
        done(self)
    }

    fn shows(&mut self, text: &str) -> bool {
        self.robot.find_by_text(text).exists()
    }
}

fn quick(message: &str) -> SnackbarVisuals {
    SnackbarVisuals::new(message).duration(SnackbarDuration::Exactly(Duration::from_millis(40)))
}

#[test]
fn a_snackbar_shows_its_message_and_times_out_as_dismissed() {
    let mut host = host(false);
    let result = host.show(quick("Photo saved"));
    assert!(host.shows("Photo saved"));
    assert!(host.settle_until(|_| result.get().is_some()));
    assert_eq!(result.get(), Some(SnackbarResult::Dismissed));
    assert!(host.state.current().is_none());
}

#[test]
fn tapping_the_action_ends_it_as_performed() {
    let mut host = host(false);
    let result = host.show(
        SnackbarVisuals::new("Message deleted")
            .action("Undo")
            .duration(SnackbarDuration::Indefinite),
    );
    assert!(host.robot.find_by_text("Undo").click());
    host.robot.wait_for_idle();
    assert_eq!(result.get(), Some(SnackbarResult::ActionPerformed));
}

#[test]
fn snackbars_wait_their_turn_in_the_queue() {
    let mut host = host(false);
    let first = host.show(quick("First"));
    let second = host.show(quick("Second"));
    assert_eq!(
        host.state.current().map(|shown| shown.message),
        Some("First".into())
    );
    assert!(second.get().is_none());
    assert!(host.settle_until(|_| first.get().is_some()));
    assert_eq!(
        host.state.current().map(|shown| shown.message),
        Some("Second".into())
    );
    assert!(host.settle_until(|_| second.get().is_some()));
}

#[test]
fn a_snackbar_with_an_action_stays_while_a_screen_reader_is_on() {
    let mut host = host(true);
    let result = host.show(quick("Message deleted").action("Undo"));
    std::thread::sleep(Duration::from_millis(120));
    host.robot.wait_for_idle();
    assert!(result.get().is_none(), "it timed out under a screen reader");
    host.state.dismiss();
    host.robot.wait_for_idle();
    assert_eq!(result.get(), Some(SnackbarResult::Dismissed));
}
