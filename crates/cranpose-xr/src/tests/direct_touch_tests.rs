// A tracked hand's fingertip touches the panels, as the frame loop of the headset runs it: a scripted
// hand writes the frame's hands, the pointer takes each hand's aim as `HeadsetInput::sample` does, the
// gestures reach a real composition's shell and the surfaces, and the cursors reach the panels.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use cranpose::{
    PointerButton, PointerEventKind, ScrollState,
    prelude::{
        Box as Layer, BoxSpec, Column, ColumnSpec, Modifier, rememberMutableInteractionSource,
    },
};
use cranpose_core::{SideEffect, remember};

use super::two_pointer_tests::{Headset, LEFT, Log, RIGHT, headset, recording, recording_panel};
use crate::{
    Controller, Hand as Side, HandJoint, HandJointType, PointerSettings, Pose, SpatialCurvedRow,
    SpatialExternalSurface, SpatialPanel, StereoMode, SubspaceModifier, XrFrame,
    aim::Aim,
    pointer_aim::pointer_aim,
    scripted_hand::{PinchCycle, ScriptedHand, play},
    space::{PanelSlot, field_dock::point_on},
};

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

// Where the right hand's aim pose stands: right of the head's start, below it and ahead of it
const AIM_FROM: [f32; 3] = [0.15, -0.25, -0.3];
// A place that no panel covers, for an aim that points at nothing
const NOWHERE: [f32; 3] = [6.0, 0.0, -1.0];

// The place `height` meters in front of the point `at` dp from the top left corner of `slot`'s flat
// face, along the face's front.
pub(crate) fn in_front(slot: &PanelSlot, at: [f32; 2], height: f32) -> [f32; 3] {
    let on = point_on(&slot.pose, slot.size(), at);
    let front = slot.pose.rotate([0.0, 0.0, 1.0]);
    [0, 1, 2].map(|axis| on[axis] + front[axis] * height)
}

// A right hand that touches the point `at` of `slot` once with the tip of its index finger, while
// its aim points at `aim_at`. The fingertip rests 10 cm in front of the point for 200 ms, comes to
// 3 cm in front in 100 ms and hovers there for 300 ms, goes 1 cm into the face in 100 ms, holds for
// 100 ms and comes back to 10 cm in 200 ms. The script lasts 1 s and then holds still.
pub(crate) fn touch(slot: &PanelSlot, at: [f32; 2], aim_at: [f32; 3]) -> ScriptedHand {
    let away = in_front(slot, at, 0.10);
    ScriptedHand::touching(Side::Right, AIM_FROM, aim_at, away)
        .holding_fingertip(ms(200))
        .moving_fingertip_to(ms(100), in_front(slot, at, 0.03))
        .holding_fingertip(ms(300))
        .moving_fingertip_to(ms(100), in_front(slot, at, -0.01))
        .holding_fingertip(ms(100))
        .moving_fingertip_to(ms(200), away)
}

// The aims of the hands that `scripts` play at `time`, as the frame loop takes them: the scripts
// write the frame's hands, and each hand with no controller points.
pub(crate) fn aims_at(scripts: &[&ScriptedHand], time: Duration) -> [Option<Aim>; 2] {
    let mut xr = XrFrame {
        predicted_display_time: time,
        ..XrFrame::default()
    };
    for script in scripts {
        play(script, &mut xr);
    }
    Side::BOTH.map(|side| pointer_aim(&Controller::default(), Some(xr.hand(side))))
}

// Frames of 10 ms of the scripted hands, counted from the scripts' start.
struct Hands<'a> {
    scripts: Vec<&'a ScriptedHand>,
    frame: u64,
}

impl<'a> Hands<'a> {
    fn new(scripts: &[&'a ScriptedHand]) -> Self {
        Self {
            scripts: scripts.to_vec(),
            frame: 0,
        }
    }

    // Plays the scripts on `headset` until `millis` after their start. The other side gets `other`,
    // where no script plays it.
    fn until(&mut self, headset: &mut Headset, millis: u64, other: impl Fn() -> Option<Aim>) {
        while self.frame * 10 < millis {
            let mut aims = aims_at(&self.scripts, ms(10 * self.frame));
            for aim in &mut aims {
                if aim.is_none() {
                    *aim = other();
                }
            }
            headset.frame(aims);
            self.frame += 1;
        }
    }
}

// What a box that fills its panel reports: its clicks and whether the hover source that its
// indication reads holds a hover.
#[derive(Clone, Default)]
struct Seen {
    clicks: Rc<Cell<u32>>,
    hovered: Rc<Cell<bool>>,
}

// A clickable box at `place` (left, top, width and height in dp) that reports to `seen`.
fn clickable_box(seen: &Seen, place: [f32; 4]) {
    let source = rememberMutableInteractionSource();
    let hovered = source.collectIsHoveredAsState().get();
    let shown = Rc::clone(&seen.hovered);
    SideEffect(move || shown.set(hovered));
    let clicks = Rc::clone(&seen.clicks);
    let [x, y, width, height] = place;
    Layer(
        Modifier::empty()
            .offset(x, y)
            .size_points(width, height)
            .hoverable(source, true)
            .clickable(move |_| clicks.set(clicks.get() + 1)),
        BoxSpec::default(),
        || {},
    );
}

// A panel of 400 by 300 dp a meter ahead with two boxes side by side, left then right.
fn two_boxes() -> (Headset, [Seen; 2]) {
    let seen = [Seen::default(), Seen::default()];
    let shown = seen.clone();
    let headset = headset(move || {
        let shown = shown.clone();
        SpatialPanel(SubspaceModifier::new().size(400.0, 300.0), move || {
            clickable_box(&shown[0], [20.0, 100.0, 160.0, 100.0]);
            clickable_box(&shown[1], [220.0, 100.0, 160.0, 100.0]);
        });
    });
    (headset, seen)
}

const LEFT_BOX: [f32; 2] = [100.0, 150.0];
const RIGHT_BOX: [f32; 2] = [300.0, 150.0];

#[test]
fn a_fingertip_hovers_near_a_button_and_its_touch_clicks_it_with_no_pinch() {
    let (mut headset, [button, _]) = two_boxes();
    let slot = headset.slot(0);
    let script = touch(&slot, LEFT_BOX, NOWHERE);
    let mut hands = Hands::new(&[&script]);
    hands.until(&mut headset, 250, || None);
    assert!(
        !button.hovered.get(),
        "10 cm away the fingertip is not near"
    );
    assert!(
        headset.beams()[1],
        "the ray points while no panel is near the fingertip"
    );
    hands.until(&mut headset, 550, || None);
    assert!(
        button.hovered.get(),
        "3 cm in front the button shows the hover"
    );
    assert_eq!(button.clicks.get(), 0, "a hover presses nothing");
    assert!(!headset.beams()[1], "the fingertip hides the hand's beam");
    let cursor = headset.cursors()[1].expect("the dot shows where the finger will touch");
    assert!(
        (cursor.x - LEFT_BOX[0]).abs() < 1.0 && (cursor.y - LEFT_BOX[1]).abs() < 1.0,
        "{cursor:?}"
    );
    assert!(!cursor.pressed);
    hands.until(&mut headset, 750, || None);
    let cursor = headset.cursors()[1].expect("the dot shows the touch");
    assert!(cursor.pressed, "the dot shows the press");
    assert_eq!(button.clicks.get(), 0, "the click comes at the release");
    hands.until(&mut headset, 1100, || None);
    assert_eq!(button.clicks.get(), 1, "the touch clicked the button once");
    assert!(!button.hovered.get(), "the hover ends when the finger goes");
}

#[test]
fn the_ray_of_a_hand_whose_fingertip_is_near_a_panel_neither_hovers_nor_presses() {
    let (mut headset, [touched, pointed]) = two_boxes();
    let slot = headset.slot(0);
    // The aim points at the right box and pinches in each cycle of 3 s, at 2.1 s and 5.1 s. The
    // fingertip hovers 3 cm in front of the left box through the first pinch, then goes 30 cm away.
    let script = ScriptedHand::pointing(
        Side::Right,
        AIM_FROM,
        point_on(&slot.pose, slot.size(), RIGHT_BOX),
        PinchCycle::default(),
    )
    .moving_fingertip_to(ms(1), in_front(&slot, LEFT_BOX, 0.03))
    .holding_fingertip(ms(3100))
    .moving_fingertip_to(ms(100), in_front(&slot, LEFT_BOX, 0.30));
    let mut hands = Hands::new(&[&script]);
    hands.until(&mut headset, 1500, || None);
    assert!(
        touched.hovered.get(),
        "the fingertip hovers over the left box"
    );
    assert!(
        !pointed.hovered.get(),
        "the ray rests on the right box, and it does not hover there"
    );
    hands.until(&mut headset, 3100, || None);
    assert_eq!(
        (touched.clicks.get(), pointed.clicks.get()),
        (0, 0),
        "the pinch pressed nothing while the fingertip was near"
    );
    hands.until(&mut headset, 6000, || None);
    assert!(
        pointed.hovered.get(),
        "the ray hovers again once the finger left"
    );
    assert_eq!(
        (touched.clicks.get(), pointed.clicks.get()),
        (0, 1),
        "the next pinch presses the right box with the ray"
    );
}

#[test]
fn a_finger_that_pushes_through_a_panel_presses_nothing_behind_it() {
    let front = Log::default();
    let behind = Seen::default();
    let (shown_front, shown_behind) = (front.clone(), behind.clone());
    let mut headset = headset(move || {
        let (shown_front, shown_behind) = (shown_front.clone(), shown_behind.clone());
        SpatialPanel(SubspaceModifier::new().size(400.0, 300.0), move || {
            recording(&shown_front);
        });
        // The same place, 4 cm behind
        SpatialPanel(
            SubspaceModifier::new()
                .size(400.0, 300.0)
                .offset(0.0, 0.0, -40.0),
            move || clickable_box(&shown_behind, [0.0, 0.0, 400.0, 300.0]),
        );
    });
    let slot = headset.slot(0);
    let at = [200.0, 150.0];
    let away = in_front(&slot, at, 0.10);
    // The fingertip goes 6 cm into the front panel, 2 cm past the face of the one behind
    let script = ScriptedHand::touching(Side::Right, AIM_FROM, NOWHERE, away)
        .moving_fingertip_to(ms(300), in_front(&slot, at, -0.06))
        .holding_fingertip(ms(200))
        .moving_fingertip_to(ms(300), away);
    Hands::new(&[&script]).until(&mut headset, 1000, || None);
    assert_eq!(behind.clicks.get(), 0, "the panel behind took no touch");
    assert!(!behind.hovered.get());
    assert_eq!(front.pointers_of(PointerEventKind::Down), [RIGHT]);
    assert_eq!(front.pointers_of(PointerEventKind::Up), [RIGHT]);

    // A fingertip that the cameras first see between the panels presses neither, also when it moves
    // on into the panel behind
    headset.frame([None, None]);
    let between = in_front(&slot, at, -0.02);
    let script = ScriptedHand::touching(Side::Right, AIM_FROM, NOWHERE, between)
        .holding_fingertip(ms(100))
        .moving_fingertip_to(ms(200), in_front(&slot, at, -0.06));
    Hands::new(&[&script]).until(&mut headset, 500, || None);
    assert_eq!(behind.clicks.get(), 0);
    assert!(!behind.hovered.get());
    assert_eq!(
        front.pointers_of(PointerEventKind::Down),
        [RIGHT],
        "no second press"
    );
}

#[test]
fn the_other_hand_keeps_its_own_pointer_while_one_hand_touches() {
    let logs = [Log::default(), Log::default()];
    let shown = logs.clone();
    let mut headset = headset(move || {
        for (index, x) in [-250.0, 250.0].into_iter().enumerate() {
            let log = shown[index].clone();
            SpatialPanel(
                SubspaceModifier::new()
                    .size(400.0, 300.0)
                    .offset(x, 0.0, 0.0),
                move || recording(&log),
            );
        }
    });
    let [left, right] = [headset.slot(0), headset.slot(1)];
    let script = touch(&right, [200.0, 150.0], NOWHERE);
    // The left controller's ray rests on the left panel and pulls its trigger from 300 to 900 ms,
    // which spans the right fingertip's touch from about 620 to 820 ms
    let mut hands = Hands::new(&[&script]);
    let mut time = 0;
    while time < 1100 {
        time += 100;
        let trigger = if (300..900).contains(&time) { 1.0 } else { 0.0 };
        let ray = super::tests::aim_at(
            [-0.2, 0.0, 0.0],
            point_on(&left.pose, left.size(), [200.0, 150.0]),
            trigger,
        );
        hands.until(&mut headset, time, || ray);
    }
    assert_eq!(logs[0].pointers_of(PointerEventKind::Down), [LEFT]);
    assert_eq!(logs[0].pointers_of(PointerEventKind::Up), [LEFT]);
    assert_eq!(logs[1].pointers_of(PointerEventKind::Down), [RIGHT]);
    assert_eq!(logs[1].pointers_of(PointerEventKind::Up), [RIGHT]);
    assert!(
        logs[0].events().iter().all(|seen| seen.id == LEFT),
        "the left panel heard only the left pointer"
    );
}

#[test]
fn a_touch_on_a_surface_is_a_click_at_the_touched_point() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let shown = Rc::clone(&seen);
    let mut headset = headset(move || {
        let shown = Rc::clone(&shown);
        SpatialExternalSurface(
            StereoMode::Mono,
            SubspaceModifier::new().size(960.0, 540.0),
            move |scope| {
                let shown = Rc::clone(&shown);
                scope.on_pointer_event(move |event| {
                    let primary = event.buttons.contains(PointerButton::Primary);
                    shown.borrow_mut().push((
                        event.kind,
                        [event.position.x, event.position.y],
                        primary,
                    ));
                });
            },
        );
    });
    let slot = headset.slot(0);
    let at = [300.0, 200.0];
    let script = touch(&slot, at, NOWHERE);
    Hands::new(&[&script]).until(&mut headset, 1100, || None);
    let seen = seen.take();
    let down = seen
        .iter()
        .find(|(kind, ..)| *kind == PointerEventKind::Down)
        .expect("the touch went down on the surface");
    assert!(down.2, "with the primary button");
    assert!(
        (down.1[0] - at[0]).abs() < 1.0 && (down.1[1] - at[1]).abs() < 1.0,
        "at the touched point: {down:?}"
    );
    let up = seen
        .iter()
        .position(|(kind, ..)| *kind == PointerEventKind::Up)
        .expect("the touch came up");
    let place = seen[up].1;
    assert!(
        (place[0] - at[0]).abs() < 1.0 && (place[1] - at[1]).abs() < 1.0,
        "a click: the press held its point, {seen:?}"
    );
}

#[test]
fn a_touch_that_slides_along_the_face_scrolls_a_list() {
    let scroll = Rc::new(RefCell::new(None));
    let kept = Rc::clone(&scroll);
    let mut headset = headset(move || {
        let state = remember(|| ScrollState::new(0.0)).with(ScrollState::clone);
        kept.replace(Some(state));
        SpatialPanel(SubspaceModifier::new().size(300.0, 300.0), move || {
            Column(
                Modifier::empty()
                    .fill_max_size()
                    .vertical_scroll(state, false),
                ColumnSpec::default(),
                || {
                    for _ in 0..20 {
                        Layer(
                            Modifier::empty().size_points(300.0, 100.0),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
        });
    });
    let list = scroll.take().expect("the list composed");
    let slot = headset.slot(0);
    let (low, high) = ([150.0, 250.0], [150.0, 150.0]);
    // The fingertip touches low on the list and slides 10 cm up along the face
    let script = ScriptedHand::touching(Side::Right, AIM_FROM, NOWHERE, in_front(&slot, low, 0.05))
        .moving_fingertip_to(ms(100), in_front(&slot, low, -0.01))
        .moving_fingertip_to(ms(500), in_front(&slot, high, -0.01))
        .moving_fingertip_to(ms(100), in_front(&slot, high, 0.05));
    Hands::new(&[&script]).until(&mut headset, 800, || None);
    assert!(
        list.value() > 60.0,
        "the list followed the finger: {}",
        list.value()
    );
}

#[test]
fn a_hand_that_the_cameras_lose_during_a_touch_releases_its_press() {
    let (mut headset, log) = recording_panel();
    let slot = headset.slot(0);
    let at = [200.0, 150.0];
    let script = ScriptedHand::touching(Side::Right, AIM_FROM, NOWHERE, in_front(&slot, at, 0.05))
        .moving_fingertip_to(ms(100), in_front(&slot, at, -0.01));
    let mut hands = Hands::new(&[&script]);
    hands.until(&mut headset, 300, || None);
    assert_eq!(log.pointers_of(PointerEventKind::Down), [RIGHT]);
    assert!(log.pointers_of(PointerEventKind::Up).is_empty());
    // The joint of the fingertip goes invalid, as when the cameras lose the finger
    let mut xr = XrFrame::default();
    play(&script, &mut xr);
    xr.right_hand.joints[HandJointType::IndexTip.index()] = HandJoint::default();
    let aim = pointer_aim(&Controller::default(), Some(&xr.right_hand));
    headset.frame([None, aim]);
    assert_eq!(
        log.pointers_of(PointerEventKind::Up),
        [RIGHT],
        "the press ends with a release"
    );
}

#[test]
fn an_app_that_turns_the_direct_touch_off_keeps_the_ray() {
    let (mut headset, [button, _]) = two_boxes();
    let settings = PointerSettings::new();
    settings.set_direct_touch(false);
    headset.choice = settings.choice();
    let slot = headset.slot(0);
    let script = touch(&slot, LEFT_BOX, NOWHERE);
    let mut hands = Hands::new(&[&script]);
    hands.until(&mut headset, 750, || None);
    assert!(headset.beams()[1], "the beam shows by the fingertip");
    assert!(!button.hovered.get());
    hands.until(&mut headset, 1100, || None);
    assert_eq!(button.clicks.get(), 0, "the fingertip pressed nothing");
}

#[test]
fn a_fingertip_touches_a_curved_panel_on_its_curve() {
    let log = Log::default();
    let shown = log.clone();
    let mut headset = headset(move || {
        let shown = shown.clone();
        SpatialCurvedRow(SubspaceModifier::new(), 400.0, move || {
            let shown = shown.clone();
            SpatialPanel(SubspaceModifier::new().size(400.0, 300.0), move || {
                recording(&shown);
            });
        });
    });
    headset.space.set_curves_shown(true);
    let slot = headset.slot(0);
    let crate::space::Shape::Curved { radius } = slot.shape else {
        panic!("the panel stands on a curve: {:?}", slot.shape);
    };
    // The point 150 dp right of the middle along the curve, and the curve's front there, toward
    // its axis
    let turn = 0.150 / radius;
    let on_curve = |height: f32| {
        let along = radius - height;
        slot.pose
            .then(&Pose::at([
                along * turn.sin(),
                0.0,
                radius - along * turn.cos(),
            ]))
            .position
    };
    let script = ScriptedHand::touching(Side::Right, AIM_FROM, NOWHERE, on_curve(0.05))
        .moving_fingertip_to(ms(200), on_curve(-0.01))
        .moving_fingertip_to(ms(200), on_curve(0.05));
    Hands::new(&[&script]).until(&mut headset, 500, || None);
    let down = log
        .events()
        .into_iter()
        .find(|seen| seen.kind == PointerEventKind::Down)
        .expect("the touch pressed the curved panel");
    // The box's events come in its own dp
    assert!(
        (down.position.x - 350.0).abs() < 1.0 && (down.position.y - 150.0).abs() < 1.0,
        "{down:?}"
    );
}
