// Two hands at once: each hand's pointer hovers, presses and drags with its own state, and reaches
// the app's composition as a pointer of its own. The tests run the frame loop of the headset: the
// two aims go to the pointers, the gestures go to a real composition's shell, and the panels follow
// the carries.

use std::{cell::RefCell, rc::Rc, time::Duration};

use cranpose::{
    PointerButton, PointerEventKind, PointerId, ScrollState,
    prelude::{Box as Layer, BoxSpec, Button, ButtonSpec, Column, ColumnSpec, Modifier},
};
use cranpose_core::{CompositionLocalProvider, remember};
use cranpose_testing::{
    PointerLog, create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};

use super::tests::{Frames, MOVABLE, PANEL, aim, aim_at, hand_aim_of, handle_aim, hovered};
use crate::{
    ControllerInput, Hand as Side, HandAim, HandState, Pinch, PointerMapping, SpatialPanel,
    SubspaceModifier, TrackingState,
    aim::{Aim, Buttons},
    atlas_input,
    handle::HandleKind,
    pointers::Pointers,
    scripted_hand::aim_pose,
    space::{PanelSlot, Space, field_dock::point_on, local_space},
    volume_pick::PickMeshes,
};

const LEFT: PointerId = PointerId(1);
const RIGHT: PointerId = PointerId(2);

type Log = PointerLog;

// A box that fills its panel and records every pointer event that reaches it.
fn recording(log: &Log) {
    Layer(
        Modifier::empty().fill_max_size().then(log.recorder()),
        BoxSpec::default(),
        || {},
    );
}

// The pointers that made an event of `kind` in `log`.
fn of_kind(log: &Log, kind: PointerEventKind) -> Vec<PointerId> {
    log.pointers_of(kind)
}

// A headset: the app's panels in a space, the two hands' pointers and the shell of the content.
struct Headset {
    robot: RobotTestRule<TestRenderer>,
    space: Space,
    pointers: Pointers,
    picks: PickMeshes,
    mapping: PointerMapping,
    frame: u64,
}

// A headset whose content `compose` composes.
fn headset(compose: impl Fn() + 'static) -> Headset {
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let compose = Rc::new(compose);
    let mut robot = create_headless_robot_test(500, 450, move || {
        let space = remember(Space::new).with(Clone::clone);
        keep.replace(Some(space.clone()));
        let compose = Rc::clone(&compose);
        CompositionLocalProvider([local_space().provides(Some(space))], move || compose());
    });
    robot.wait_for_idle();
    let space = held.take().expect("the content composed");
    Headset {
        robot,
        space,
        pointers: Pointers::default(),
        picks: PickMeshes::default(),
        mapping: PointerMapping::default(),
        frame: 0,
    }
}

impl Headset {
    // One frame of the headset's loop, 10 ms after the one before.
    fn frame(&mut self, aims: [Option<Aim>; 2]) {
        self.frame += 1;
        let panels = self.space.panels();
        self.pointers.update(
            aims,
            Duration::from_millis(10 * self.frame),
            (&panels, &self.picks),
            (self.mapping, true),
        );
        atlas_input::deliver(self.robot.shell_mut(), self.pointers.atlas());
        for step in self.pointers.moves() {
            self.space.carry(step);
        }
        for step in self.pointers.resizes() {
            self.space.resize(step);
        }
        self.robot.wait_for_idle();
    }

    // `frames` frames, with the aims that `aims` gives for the number of the frame.
    fn run(&mut self, frames: u32, aims: impl Fn(u32) -> [Option<Aim>; 2]) {
        for frame in 0..frames {
            self.frame(aims(frame));
        }
    }

    fn slot(&self, index: usize) -> PanelSlot {
        self.space.panels()[index]
    }
}

// A controller at `from` that points at the place `at` dp from the top left corner of `slot`'s
// panel, with its trigger pulled by `trigger`.
fn on(slot: &PanelSlot, from: [f32; 3], at: [f32; 2], trigger: f32) -> Option<Aim> {
    aim_at(from, point_on(&slot.pose, slot.size(), at), trigger)
}

fn with_buttons(aim: Option<Aim>, buttons: Buttons) -> Option<Aim> {
    aim.map(|aim| Aim { buttons, ..aim })
}

fn leaning(aim: Option<Aim>, stick: [f32; 2]) -> Option<Aim> {
    aim.map(|aim| Aim { stick, ..aim })
}

const LEFT_FROM: [f32; 3] = [-0.15, 0.0, 0.0];
const RIGHT_FROM: [f32; 3] = [0.15, 0.0, 0.0];

// Two panels of 200 by 200 dp side by side, each with a list of 20 rows of 100 dp. The scroll state
// of each list is in the result, left then right.
fn lists() -> (Headset, [ScrollState; 2]) {
    let made = Rc::new(RefCell::new(Vec::<ScrollState>::new()));
    let kept = Rc::clone(&made);
    let headset = headset(move || {
        for (index, x) in [-150.0, 150.0].into_iter().enumerate() {
            let scroll = remember(|| ScrollState::new(0.0)).with(ScrollState::clone);
            if kept.borrow().len() <= index {
                kept.borrow_mut().push(scroll);
            }
            SpatialPanel(
                SubspaceModifier::new()
                    .size(200.0, 200.0)
                    .offset(x, 0.0, 0.0),
                move || {
                    Column(
                        Modifier::empty()
                            .fill_max_size()
                            .vertical_scroll(scroll, false),
                        ColumnSpec::default(),
                        || {
                            for _ in 0..20 {
                                Layer(
                                    Modifier::empty().size_points(200.0, 100.0),
                                    BoxSpec::default(),
                                    || {},
                                );
                            }
                        },
                    );
                },
            );
        }
    });
    let scrolls = made.borrow();
    let pair = [scrolls[0], scrolls[1]];
    (headset, pair)
}

#[test]
fn two_hands_drag_two_panels_at_once() {
    let (mut headset, [left_list, right_list]) = lists();
    let [left, right] = [headset.slot(0), headset.slot(1)];
    // Both hands rest, pull and hold their triggers, and then drag up by 3 dp in each frame. The
    // press holds its point for 350 ms, so the drag starts when the ray has turned past 4 degrees.
    headset.run(130, |frame| {
        let (trigger, y) = match frame {
            0..30 => (0.0, 170.0),
            30..80 => (1.0, 170.0),
            _ => (1.0, 170.0 - 3.0 * (frame - 80) as f32),
        };
        [
            on(&left, LEFT_FROM, [100.0, y], trigger),
            on(&right, RIGHT_FROM, [100.0, y], trigger),
        ]
    });
    assert!(
        left_list.value() > 20.0 && right_list.value() > 20.0,
        "both lists follow their hands: {} and {}",
        left_list.value(),
        right_list.value()
    );
}

#[test]
fn a_hand_that_drags_a_list_does_not_hold_back_the_other_hand_on_the_other_list() {
    let (mut headset, [left_list, right_list]) = lists();
    let [left, right] = [headset.slot(0), headset.slot(1)];
    // The left hand drags for 2 s while the right hand only points
    headset.run(230, |frame| {
        let (trigger, y) = match frame {
            0..30 => (0.0, 170.0),
            30..80 => (1.0, 170.0),
            _ => (1.0, 170.0 - 3.0 * (frame - 80).min(50) as f32),
        };
        [
            on(&left, LEFT_FROM, [100.0, y], trigger),
            on(&right, RIGHT_FROM, [100.0, 100.0], 0.0),
        ]
    });
    assert!(left_list.value() > 20.0);
    assert_eq!(right_list.value(), 0.0, "the right hand did not press");
    // The right hand drags now, with the left one still pulling its trigger
    headset.run(130, |frame| {
        let y = 170.0 - 3.0 * (frame.max(80) - 80) as f32;
        let trigger = if frame < 30 { 0.0 } else { 1.0 };
        [
            on(&left, LEFT_FROM, [100.0, 20.0], 1.0),
            on(&right, RIGHT_FROM, [100.0, y.max(20.0)], trigger),
        ]
    });
    assert!(right_list.value() > 20.0);
}

#[test]
fn two_hands_pressing_one_panel_arrive_as_two_pointers() {
    let log = Log::default();
    let shown = log.clone();
    let mut headset = headset(move || {
        let shown = shown.clone();
        SpatialPanel(SubspaceModifier::new().size(400.0, 300.0), move || {
            recording(&shown);
        });
    });
    let slot = headset.slot(0);
    headset.run(60, |frame| {
        let trigger = if frame < 30 { 0.0 } else { 1.0 };
        let right_trigger = if frame < 40 { 0.0 } else { 1.0 };
        [
            on(&slot, LEFT_FROM, [100.0, 150.0], trigger),
            on(&slot, RIGHT_FROM, [300.0, 150.0], right_trigger),
        ]
    });
    headset.run(5, |_| {
        [
            on(&slot, LEFT_FROM, [100.0, 150.0], 0.0),
            on(&slot, RIGHT_FROM, [300.0, 150.0], 0.0),
        ]
    });
    assert_eq!(of_kind(&log, PointerEventKind::Down), [LEFT, RIGHT]);
    assert_eq!(of_kind(&log, PointerEventKind::Up), [LEFT, RIGHT]);
    let downs: Vec<f32> = log
        .events()
        .iter()
        .filter(|seen| seen.kind == PointerEventKind::Down)
        .map(|seen| seen.position.x)
        .collect();
    assert!(
        (downs[0] - 100.0).abs() < 1.0 && (downs[1] - 300.0).abs() < 1.0,
        "{downs:?}"
    );
}

#[test]
fn a_handle_that_one_hand_carries_does_not_stop_the_other_hand_from_pressing() {
    let clicks = Rc::new(RefCell::new(0_u32));
    let counted = Rc::clone(&clicks);
    let mut headset = headset(move || {
        let counted = Rc::clone(&counted);
        SpatialPanel(
            SubspaceModifier::new()
                .size(200.0, 200.0)
                .offset(-250.0, 0.0, 0.0)
                .movable(),
            || {},
        );
        SpatialPanel(
            SubspaceModifier::new()
                .size(200.0, 200.0)
                .offset(250.0, 0.0, 0.0),
            move || {
                let counted = Rc::clone(&counted);
                Button(
                    Modifier::empty().fill_max_size(),
                    ButtonSpec::default(),
                    move || *counted.borrow_mut() += 1,
                    || {},
                );
            },
        );
    });
    let [movable, button] = [headset.slot(0), headset.slot(1)];
    let start = movable.pose.position;
    // The left hand takes the handle under the first panel and carries the panel 20 cm up, while the
    // right hand clicks the button of the second panel
    headset.run(120, |frame| {
        let lift = 0.2 * (frame.saturating_sub(40) as f32 / 60.0).min(1.0);
        let left = aim_at(
            [-0.4, lift, 0.0],
            crate::carry::handle_center(&movable),
            if frame < 20 { 0.0 } else { 1.0 },
        );
        let click = (60..70).contains(&frame);
        [
            left,
            on(
                &button,
                RIGHT_FROM,
                [100.0, 100.0],
                if click { 1.0 } else { 0.0 },
            ),
        ]
    });
    let moved = headset.slot(0).pose.position;
    assert!(
        moved[1] - start[1] > 0.1,
        "the carry went on while the other hand clicked: from {start:?} to {moved:?}"
    );
    assert_eq!(*clicks.borrow(), 1, "the other hand's press clicked once");
}

#[test]
fn the_other_hand_cannot_grab_the_handle_that_a_hand_holds() {
    let mut frames = Frames::default();
    let at_handle = |x: f32, trigger: f32| handle_aim(&MOVABLE, x, trigger);
    frames.update([at_handle(-0.1, 0.0), at_handle(0.1, 0.0)], &[MOVABLE]);
    // The left hand grabs. The right hand pulls on the same handle in the next frame.
    frames.update([at_handle(-0.1, 1.0), at_handle(0.1, 0.0)], &[MOVABLE]);
    frames.update([at_handle(-0.1, 1.0), at_handle(0.1, 1.0)], &[MOVABLE]);
    assert_eq!(
        frames.pointer.handles()[0],
        Some(crate::handle::HandleFocus {
            pressed: true,
            ..hovered(1, HandleKind::Move)
        })
    );
    assert_eq!(
        frames.pointer.handles()[1],
        None,
        "the held panel has no handle for the right hand"
    );
    frames.update([at_handle(-0.1, 1.0), at_handle(0.1, 1.0)], &[MOVABLE]);
    assert!(
        frames
            .pointer
            .moves()
            .iter()
            .all(|step| step.event_type != crate::SpatialMoveEventType::Start),
        "only the left hand grabbed"
    );
    // The right hand's pull did not press anything either
    assert!(frames.pointer.atlas().is_empty());
}

// A panel of 400 by 300 dp with a recording box, and the log.
fn recording_panel() -> (Headset, Log) {
    let log = Log::default();
    let shown = log.clone();
    let headset = headset(move || {
        let shown = shown.clone();
        SpatialPanel(SubspaceModifier::new().size(400.0, 300.0), move || {
            recording(&shown);
        });
    });
    (headset, log)
}

// The right controller points at the panel's middle for 30 frames, and then has `buttons` down for
// 10 frames and up for 5.
fn press(headset: &mut Headset, buttons: Buttons) {
    let slot = headset.slot(0);
    let at = |buttons: Buttons| with_buttons(on(&slot, RIGHT_FROM, [200.0, 150.0], 0.0), buttons);
    headset.run(30, |_| [None, at(Buttons::default())]);
    headset.run(10, |_| [None, at(buttons)]);
    headset.run(5, |_| [None, at(Buttons::default())]);
}

#[test]
fn the_first_face_button_is_a_right_click() {
    let (mut headset, log) = recording_panel();
    press(
        &mut headset,
        Buttons {
            primary: true,
            ..Buttons::default()
        },
    );
    let seen = log.events();
    let down = seen
        .iter()
        .find(|seen| seen.kind == PointerEventKind::ButtonDown(PointerButton::Secondary))
        .expect("a secondary press reaches the panel");
    assert_eq!(down.id, RIGHT);
    assert!(down.buttons.contains(PointerButton::Secondary));
    assert!((down.position.x - 200.0).abs() < 1.0 && (down.position.y - 150.0).abs() < 1.0);
    assert!(
        seen.iter()
            .any(|seen| seen.kind == PointerEventKind::ButtonUp(PointerButton::Secondary)),
        "and its release"
    );
    assert!(
        seen.iter().all(|seen| seen.kind != PointerEventKind::Down),
        "the trigger stayed up, so no primary press happened"
    );
}

#[test]
fn the_thumbstick_click_is_the_middle_button_and_the_second_face_button_presses_nothing() {
    let (mut headset, log) = recording_panel();
    press(
        &mut headset,
        Buttons {
            stick_click: true,
            secondary: true,
            menu: true,
            squeeze: 1.0,
            ..Buttons::default()
        },
    );
    let kinds: Vec<_> = log
        .events()
        .iter()
        .map(|seen| seen.kind)
        .filter(|kind| matches!(kind, PointerEventKind::ButtonDown(_)))
        .collect();
    assert_eq!(
        kinds,
        [PointerEventKind::ButtonDown(PointerButton::Middle)],
        "the grip, the second face button and the menu button press nothing by default"
    );
}

#[test]
fn the_thumbstick_scrolls_the_panel_under_the_pointer_as_a_wheel_does() {
    let (mut headset, log) = recording_panel();
    let slot = headset.slot(0);
    // The stick leans back and to the right for 20 frames
    headset.run(10, |_| [None, on(&slot, RIGHT_FROM, [200.0, 150.0], 0.0)]);
    headset.run(20, |_| {
        [
            None,
            leaning(on(&slot, RIGHT_FROM, [200.0, 150.0], 0.0), [1.0, -1.0]),
        ]
    });
    let seen = log.events();
    let scrolls: Vec<_> = seen
        .iter()
        .filter(|seen| seen.kind == PointerEventKind::Scroll)
        .collect();
    assert!(!scrolls.is_empty(), "the stick scrolls");
    for scroll in &scrolls {
        assert_eq!(scroll.id, RIGHT);
        // A wheel turned toward the user scrolls down with a negative y. Content moves left with a
        // negative x, which shows what lies to the right.
        assert!(
            scroll.scroll_delta.y < 0.0 && scroll.scroll_delta.x < 0.0,
            "{scroll:?}"
        );
    }
    // 800 dp each second at full lean, in frames of 10 ms
    let total: f32 = scrolls.iter().map(|scroll| scroll.scroll_delta.y).sum();
    assert!(
        (total + 800.0 * 0.01 * scrolls.len() as f32).abs() < 1.0,
        "{total}"
    );
}

#[test]
fn the_thumbstick_scrolls_a_list_down() {
    let (mut headset, [left_list, _]) = lists();
    let slot = headset.slot(0);
    headset.run(40, |frame| {
        let aim = on(&slot, LEFT_FROM, [100.0, 100.0], 0.0);
        [
            if frame < 10 {
                aim
            } else {
                leaning(aim, [0.0, -1.0])
            },
            None,
        ]
    });
    assert!(
        left_list.value() > 100.0,
        "leaning back scrolls a list toward its end: {}",
        left_list.value()
    );
}

#[test]
fn a_stick_that_carries_a_panel_does_not_scroll() {
    let mut frames = Frames::default();
    let at_handle = |trigger: f32, stick: [f32; 2]| {
        handle_aim(&MOVABLE, 0.0, trigger).map(|aim| Aim { stick, ..aim })
    };
    frames.update([None, at_handle(0.0, [0.0; 2])], &[MOVABLE]);
    frames.update([None, at_handle(1.0, [0.0; 2])], &[MOVABLE]);
    for _ in 0..5 {
        frames.update([None, at_handle(1.0, [0.5, 1.0])], &[MOVABLE]);
        assert!(
            frames.pointer.atlas().is_empty(),
            "the stick pushes the panel"
        );
    }
}

#[test]
fn an_app_remaps_the_buttons_and_the_scroll() {
    use PointerButton::{Middle, Secondary};
    let (mut headset, log) = recording_panel();
    headset.mapping = PointerMapping::new()
        .with_button(ControllerInput::Trigger, Some(Middle))
        .with_button(ControllerInput::Squeeze, Some(Secondary))
        .with_button(ControllerInput::Primary, None)
        .with_scroll_speed(0.0);
    let slot = headset.slot(0);
    let at = |trigger: f32, buttons: Buttons| {
        leaning(
            with_buttons(on(&slot, RIGHT_FROM, [200.0, 150.0], trigger), buttons),
            [0.0, 1.0],
        )
    };
    headset.run(30, |_| [None, at(0.0, Buttons::default())]);
    headset.run(10, |_| {
        [
            None,
            at(
                1.0,
                Buttons {
                    squeeze: 1.0,
                    primary: true,
                    ..Buttons::default()
                },
            ),
        ]
    });
    headset.run(5, |_| [None, at(0.0, Buttons::default())]);
    let kinds: Vec<PointerEventKind> = log
        .events()
        .iter()
        .map(|seen| seen.kind)
        .filter(|kind| {
            !matches!(
                kind,
                PointerEventKind::Move | PointerEventKind::Enter | PointerEventKind::Exit
            )
        })
        .collect();
    use PointerEventKind::{ButtonDown, ButtonUp};
    assert_eq!(
        kinds,
        [
            ButtonDown(Middle),
            ButtonDown(Secondary),
            ButtonUp(Middle),
            ButtonUp(Secondary)
        ],
        "the trigger is a middle press, the grip a right press, the first face button nothing, and the stick no longer scrolls"
    );
    // The press of the trigger as a middle button holds a primary press nowhere
    assert!(of_kind(&log, PointerEventKind::Down).is_empty());
}

// A tracked hand at `from` that points at the place `at` dp from the top left corner of `slot`'s
// panel, with its index finger pinching by `strength`, as the pointer takes it.
fn hand_on(slot: &PanelSlot, from: [f32; 3], at: [f32; 2], strength: f32) -> Option<Aim> {
    hand_aim_of(&HandState {
        tracking_state: TrackingState::Tracking,
        aim: Some(HandAim {
            pose: aim_pose(from, point_on(&slot.pose, slot.size(), at)),
            index: Pinch {
                strength,
                pinching: strength > 0.8,
            },
            ..HandAim::default()
        }),
        ..HandState::default()
    })
}

#[test]
fn a_hand_and_a_controller_press_at_once() {
    let (mut headset, log) = recording_panel();
    let slot = headset.slot(0);
    // The left hand is a tracked hand that pinches at frame 30, and the right controller pulls its
    // trigger at frame 40. Each lets go ten frames later.
    headset.run(60, |frame| {
        let pinch = if (30..40).contains(&frame) { 1.0 } else { 0.0 };
        let pull = if (40..50).contains(&frame) { 1.0 } else { 0.0 };
        [
            hand_on(&slot, LEFT_FROM, [100.0, 150.0], pinch),
            on(&slot, RIGHT_FROM, [300.0, 150.0], pull),
        ]
    });
    assert_eq!(of_kind(&log, PointerEventKind::Down), [LEFT, RIGHT]);
    assert_eq!(of_kind(&log, PointerEventKind::Up), [LEFT, RIGHT]);
    // A hand holds no controller, so its pinch is the primary button and has no other
    assert!(
        log.events()
            .iter()
            .all(|seen| !matches!(seen.kind, PointerEventKind::ButtonDown(_)))
    );
}

#[test]
fn a_hand_that_pinches_has_no_secondary_button_and_does_not_scroll() {
    let (mut headset, log) = recording_panel();
    let slot = headset.slot(0);
    headset.run(40, |frame| {
        let pinch = if frame < 20 { 0.0 } else { 1.0 };
        [hand_on(&slot, LEFT_FROM, [100.0, 150.0], pinch), None]
    });
    assert!(of_kind(&log, PointerEventKind::Scroll).is_empty());
    assert_eq!(of_kind(&log, PointerEventKind::Down), [LEFT]);
}

// A panel that stands in the way of nothing shows no beam, a hidden one shows none, and an app that
// hides the beams has none, while the pointer still presses.
#[test]
fn a_beam_shows_only_while_a_panel_is_in_reach_and_the_app_lets_it() {
    let mut frames = Frames::default();
    let hidden = PanelSlot {
        ride: crate::space::Ride::Hidden,
        ..PANEL
    };
    let ridden_by_right = PanelSlot {
        ride: crate::space::Ride::On(Side::Right),
        ..PANEL
    };
    let both = || [aim(-0.1, 0.0, 0.0), aim(0.1, 0.0, 0.0)];

    frames.update(both(), &[]);
    assert_eq!(
        frames.pointer.beams(),
        [None, None],
        "an empty Subspace has no beams"
    );
    frames.update(both(), &[hidden]);
    assert_eq!(
        frames.pointer.beams(),
        [None, None],
        "a panel that shows nowhere has none"
    );
    frames.update(both(), &[ridden_by_right]);
    let [left, right] = frames.pointer.beams();
    assert!(
        left.is_some(),
        "the left ray reaches the ball of the right controller"
    );
    assert_eq!(
        right, None,
        "the right ray cannot reach the panel that rides the right hand"
    );
    frames.update(both(), &[PANEL]);
    assert!(frames.pointer.beams().iter().all(Option::is_some));

    // The app hides the beams: the pointers go on
    frames.step(both(), &[PANEL]);
    frames.pointer.update(
        [aim(-0.1, 0.0, 0.0), aim(0.1, 0.0, 0.0)],
        Duration::from_millis(500),
        (&[PANEL], &PickMeshes::default()),
        (PointerMapping::default(), false),
    );
    assert_eq!(frames.pointer.beams(), [None, None]);
    assert!(!frames.pointer.cursors().iter().all(Option::is_none));
}
