use std::{cell::RefCell, rc::Rc, time::Duration};

use cranpose_core::{CompositionLocalProvider, rememberMutableStateOf};
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};

use super::*;
use crate::frame_effects::FrameEffects;

// What the effects saw: the effect's name and the frame's time in milliseconds.
type Seen = Rc<RefCell<Vec<(&'static str, u64)>>>;

// A headset's composition with two effects. The first one leaves while `shown` is off.
struct Headset {
    robot: RobotTestRule<TestRenderer>,
    effects: FrameEffects,
    shown: MutableState<bool>,
    seen: Seen,
}

#[composable]
fn Named(name: &'static str, seen: Seen) {
    SpatialFrameEffect(move |frame| {
        seen.borrow_mut()
            .push((name, frame.predicted_display_time.as_millis() as u64));
    });
}

fn headset(provided: bool) -> Headset {
    let seen = Seen::default();
    let held = Rc::new(RefCell::new(None));
    let (keep, effects_seen) = (Rc::clone(&held), Rc::clone(&seen));
    let robot = create_headless_robot_test(200, 200, move || {
        let effects = remember(FrameEffects::default).with(Clone::clone);
        let shown = rememberMutableStateOf(|| true);
        keep.replace(Some((effects.clone(), shown)));
        let seen = Rc::clone(&effects_seen);
        let content = move || {
            if shown.get() {
                Named("first", Rc::clone(&seen));
            }
            Named("second", Rc::clone(&seen));
        };
        if provided {
            CompositionLocalProvider([local_frame_effects().provides(Some(effects))], content);
        } else {
            content();
        }
    });
    let (effects, shown) = held.take().expect("the content composed");
    Headset {
        robot,
        effects,
        shown,
        seen,
    }
}

impl Headset {
    // Runs one frame at `millis` and returns what the effects saw in it.
    fn frame(&self, millis: u64) -> Vec<(&'static str, u64)> {
        let frame = XrFrame {
            predicted_display_time: Duration::from_millis(millis),
            ..XrFrame::default()
        };
        self.effects.run(&frame);
        self.seen.take()
    }
}

#[test]
fn each_effect_runs_once_in_each_frame_in_the_order_of_composition() {
    let mut headset = headset(true);
    headset.robot.wait_for_idle();
    assert_eq!(headset.frame(8), [("first", 8), ("second", 8)]);
    assert_eq!(headset.frame(16), [("first", 16), ("second", 16)]);
}

#[test]
fn an_effect_stops_when_its_composable_leaves_and_runs_again_when_it_returns() {
    let mut headset = headset(true);
    headset.robot.wait_for_idle();
    headset.shown.set(false);
    headset.robot.wait_for_idle();
    assert_eq!(headset.frame(8), [("second", 8)]);
    headset.shown.set(true);
    headset.robot.wait_for_idle();
    assert_eq!(headset.frame(16), [("second", 16), ("first", 16)]);
}

#[test]
fn outside_a_headset_the_effect_composes_and_never_runs() {
    let mut headset = headset(false);
    headset.robot.wait_for_idle();
    assert_eq!(headset.frame(8), []);
}
