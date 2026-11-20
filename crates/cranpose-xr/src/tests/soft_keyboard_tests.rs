use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use cranpose::prelude::*;
use cranpose_core::{CompositionLocalProvider, remember};
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};

use super::*;
use crate::{
    Controller, Hand, PointerMapping, Pose, Ray, SpatialMoveEventType, SpatialPanel,
    SubspaceModifier, XrFrame,
    aim::Aim,
    atlas_input,
    key_layout::{self, KeyKind, Page},
    keyboard_panel::SoftKeyboardPanel,
    pointer_aim::pointer_aim,
    pointers::Pointers,
    scripted_hand::{PinchCycle, ScriptedHand, play},
    space::{
        PanelMove, Ride,
        field_dock::{below, point_on},
        local_space,
    },
    volume_pick::PickMeshes,
};

// The panel of the field is the second one in the atlas, so its part of the atlas does not start
// at the top.
const FIELD_PANEL: [f32; 2] = [400.0, 120.0];

// A headset with a panel of a text field below another panel, and the keyboard that the field
// opens. The window is as tall as the atlas with the keyboard in it. The pointer is a controller
// at the origin, and each frame is 10 ms.
struct Headset {
    robot: RobotTestRule<TestRenderer>,
    space: Space,
    keyboard: SoftKeyboard,
    text: TextFieldState,
    pointer: Pointers,
    frames: u64,
}

fn headset(options: BasicTextFieldOptions, field: SubspaceModifier) -> Headset {
    let held = Rc::new(RefCell::new(None::<(Space, TextFieldState)>));
    let keep = Rc::clone(&held);
    let mut robot = create_headless_robot_test(600, 600, move || {
        let space = remember(Space::new).with(Clone::clone);
        let text = remember(|| TextFieldState::new("")).with(|text| *text);
        keep.replace(Some((space.clone(), text)));
        let (options, field) = (options.clone(), field.clone());
        let keyboard_space = space.clone();
        CompositionLocalProvider([local_space().provides(Some(space))], move || {
            SpatialPanel(
                SubspaceModifier::new()
                    .size(300.0, 100.0)
                    .offset(0.0, 400.0, 0.0),
                || {},
            );
            let options = options.clone();
            SpatialPanel(
                field.clone().size(FIELD_PANEL[0], FIELD_PANEL[1]),
                move || {
                    BasicTextFieldWithOptions(
                        text,
                        Modifier::empty().fill_max_size(),
                        options.clone(),
                    );
                },
            );
            SoftKeyboardPanel(keyboard_space);
        });
    });
    robot.wait_for_idle();
    let (space, text) = held.take().expect("the content composed");
    let keyboard = space.keyboard();
    keyboard.install(robot.shell_mut());
    Headset {
        robot,
        space,
        keyboard,
        text,
        pointer: Pointers::default(),
        frames: 0,
    }
}

fn plain() -> Headset {
    headset(BasicTextFieldOptions::default(), SubspaceModifier::new())
}

impl Headset {
    // One frame of the headset's loop: the pointer's events reach the shell, then the keyboard's.
    fn frame(&mut self, aim: Option<Aim>) {
        self.frames += 1;
        let time = Duration::from_millis(10 * self.frames);
        let panels = self.space.panels();
        self.pointer.update(
            [None, aim],
            time,
            (&panels, &PickMeshes::default()),
            (PointerMapping::default(), true),
        );
        atlas_input::deliver(self.robot.shell_mut(), self.pointer.atlas());
        self.keyboard.pump(self.robot.shell_mut(), &self.space);
        self.robot.wait_for_idle();
    }

    // The controller at the origin points at `target` in the room for 300 ms, which the pointer's
    // filter needs to settle after a turn, then pulls the trigger and lets it go.
    fn press(&mut self, target: [f32; 3]) {
        for _ in 0..30 {
            self.frame(aim_at(target, 0.0));
        }
        for trigger in [1.0, 1.0, 1.0, 0.0, 0.0] {
            self.frame(aim_at(target, trigger));
        }
    }

    fn field_pose(&self) -> Pose {
        self.space.panels()[1].pose
    }

    fn press_field(&mut self) {
        let at = self.field_pose().position;
        self.press(at);
    }

    // Presses the key `kind` of `page`, which the open keyboard shows.
    fn press_key(&mut self, page: Page, kind: KeyKind) {
        let key = key_layout::keys(page)
            .find(|key| key.kind == kind)
            .expect("the page has the key");
        let keyboard = *self.space.panels().last().expect("the keyboard is open");
        let at = point_on(&keyboard.pose, keyboard.size(), key.center());
        self.press(at);
    }

    // Presses the keys that type `text` on the letters page.
    fn type_text(&mut self, text: &str) {
        for ch in text.chars() {
            let (page, key) = key_layout::find(ch).expect("the keyboard has the key");
            assert_eq!(page, Page::Letters, "{ch}");
            self.press_key(page, key.kind);
        }
    }

    fn typed(&self) -> String {
        self.text.text()
    }

    fn has_focus(&mut self) -> bool {
        self.robot.shell_mut().ime_editor_state().is_some()
    }
}

// A controller at the origin that points at `target`, with its trigger pulled by `trigger`.
fn aim_at(target: [f32; 3], trigger: f32) -> Option<Aim> {
    let length = target.iter().map(|v| v * v).sum::<f32>().sqrt();
    let ray = Ray {
        origin: [0.0; 3],
        direction: target.map(|v| v / length),
    };
    Some(Aim::pulling(ray, Pose::IDENTITY, trigger))
}

#[test]
fn a_field_that_takes_focus_opens_the_keyboard_below_its_panel() {
    let mut headset = plain();
    assert!(!headset.keyboard.is_open());
    assert_eq!(headset.space.panels().len(), 2);
    headset.press_field();
    assert!(headset.keyboard.is_open());
    let panels = headset.space.panels();
    assert_eq!(panels.len(), 3);
    let keyboard = panels[2];
    let wanted = below(&panels[1].pose, FIELD_PANEL, key_layout::SIZE);
    assert_eq!(keyboard.pose, wanted);
    assert_eq!(keyboard.ride, Ride::Free);
    assert_eq!(keyboard.size(), key_layout::SIZE);
}

#[test]
fn the_keyboard_closes_when_focus_leaves_the_field() {
    let mut headset = plain();
    headset.press_field();
    assert!(headset.keyboard.is_open());
    headset.robot.shell_mut().clear_text_field_focus();
    headset.robot.wait_for_idle();
    assert!(!headset.keyboard.is_open());
    assert_eq!(headset.space.panels().len(), 2);
}

#[test]
fn the_keyboard_shows_nowhere_while_no_panel_holds_a_focused_field() {
    let mut headset = plain();
    headset.press_field();
    headset.space.set_field_panel(None);
    assert_eq!(headset.space.panels()[2].ride, Ride::Hidden);
}

#[test]
fn keys_type_into_the_focused_field() {
    let mut headset = plain();
    headset.press_field();
    headset.type_text("hi 5");
    assert_eq!(headset.typed(), "hi 5");
}

#[test]
fn a_key_press_does_not_take_the_focus_from_the_field() {
    let mut headset = plain();
    headset.press_field();
    headset.type_text("a");
    assert!(headset.has_focus());
    // A hardware key reaches the same field: the field still holds the focus
    let handled = headset
        .robot
        .shell_mut()
        .on_key_event(&KeyEvent::key_down(Key::B, "b"));
    assert!(handled);
    assert_eq!(headset.typed(), "ab");
    headset.type_text("c");
    assert_eq!(headset.typed(), "abc");
}

#[test]
fn shift_makes_the_next_letter_a_capital_and_two_presses_lock_it() {
    let mut headset = plain();
    headset.press_field();
    headset.press_key(Page::Letters, KeyKind::Shift);
    headset.type_text("hi");
    assert_eq!(headset.typed(), "Hi");
    headset.press_key(Page::Letters, KeyKind::Shift);
    headset.press_key(Page::Letters, KeyKind::Shift);
    headset.type_text("ab");
    assert_eq!(headset.typed(), "HiAB");
    headset.press_key(Page::Letters, KeyKind::Shift);
    headset.type_text("c");
    assert_eq!(headset.typed(), "HiABc");
}

#[test]
fn backspace_deletes_before_the_cursor_and_does_nothing_in_an_empty_field() {
    let mut headset = plain();
    headset.press_field();
    headset.press_key(Page::Letters, KeyKind::Backspace);
    assert_eq!(headset.typed(), "");
    headset.type_text("ab");
    headset.press_key(Page::Letters, KeyKind::Backspace);
    assert_eq!(headset.typed(), "a");
    assert!(headset.has_focus());
}

#[test]
fn enter_starts_a_new_line_in_a_multi_line_field() {
    let options = BasicTextFieldOptions {
        line_limits: TextFieldLineLimits::MultiLine {
            min_lines: 1,
            max_lines: usize::MAX,
        },
        ..BasicTextFieldOptions::default()
    };
    let mut headset = headset(options, SubspaceModifier::new());
    headset.press_field();
    headset.type_text("a");
    headset.press_key(Page::Letters, KeyKind::Enter);
    headset.type_text("b");
    assert_eq!(headset.typed(), "a\nb");
}

#[test]
fn enter_runs_the_keyboard_action_of_a_single_line_field() {
    let runs = Rc::new(Cell::new(0));
    let counted = Rc::clone(&runs);
    let options = BasicTextFieldOptions {
        line_limits: TextFieldLineLimits::SingleLine,
        on_keyboard_action: Some(KeyboardActionHandler::new(move |_| {
            counted.set(counted.get() + 1);
        })),
        ..BasicTextFieldOptions::default()
    };
    let mut headset = headset(options, SubspaceModifier::new());
    headset.press_field();
    headset.type_text("a");
    headset.press_key(Page::Letters, KeyKind::Enter);
    assert_eq!(runs.get(), 1);
    assert_eq!(headset.typed(), "a");
}

#[test]
fn enter_in_a_single_line_field_with_no_action_closes_the_keyboard_and_keeps_the_focus() {
    let options = BasicTextFieldOptions {
        line_limits: TextFieldLineLimits::SingleLine,
        ..BasicTextFieldOptions::default()
    };
    let mut headset = headset(options, SubspaceModifier::new());
    headset.press_field();
    headset.press_key(Page::Letters, KeyKind::Enter);
    assert!(!headset.keyboard.is_open());
    assert!(headset.has_focus());
}

#[test]
fn the_hide_key_closes_the_keyboard_and_a_tap_on_the_field_opens_it_again() {
    let mut headset = plain();
    headset.press_field();
    headset.press_key(Page::Letters, KeyKind::Hide);
    assert!(!headset.keyboard.is_open());
    assert_eq!(headset.space.panels().len(), 2);
    assert!(headset.has_focus());
    headset.press_field();
    assert!(headset.keyboard.is_open());
    headset.type_text("a");
    assert_eq!(headset.typed(), "a");
}

#[test]
fn the_symbols_page_types_symbols_and_the_letters_page_returns() {
    let mut headset = plain();
    headset.press_field();
    headset.press_key(Page::Letters, KeyKind::Page(Page::Symbols));
    for ch in ['@', '(', '?', '='] {
        let (page, key) = key_layout::find(ch).expect("the keyboard has the symbol");
        assert_eq!(page, Page::Symbols);
        headset.press_key(page, key.kind);
    }
    headset.press_key(Page::Symbols, KeyKind::Page(Page::Letters));
    headset.type_text("a");
    assert_eq!(headset.typed(), "@(?=a");
}

#[test]
fn the_keyboard_follows_the_panel_that_the_user_carries() {
    let mut headset = headset(
        BasicTextFieldOptions::default(),
        SubspaceModifier::new().movable(),
    );
    headset.press_field();
    let field = headset.space.panels()[1].id;
    let carried = Pose::at([0.3, 0.2, -1.5]);
    headset.space.carry(&PanelMove {
        panel: field,
        event_type: SpatialMoveEventType::Moving,
        pose: carried,
    });
    let panels = headset.space.panels();
    assert_eq!(panels[1].pose, carried);
    assert_eq!(
        panels[2].pose,
        below(&carried, FIELD_PANEL, key_layout::SIZE)
    );
}

#[test]
fn every_key_lies_on_the_panel_and_no_two_keys_overlap() {
    let [width, height] = key_layout::SIZE;
    for page in [Page::Letters, Page::Symbols] {
        let keys: Vec<_> = key_layout::keys(page).collect();
        for key in &keys {
            let [left, top, w, h] = key.rect;
            assert!(left >= 0.0 && top >= 0.0, "{key:?}");
            assert!(left + w <= width && top + h <= height, "{key:?}");
        }
        for (index, a) in keys.iter().enumerate() {
            for b in &keys[index + 1..] {
                let apart = a.rect[0] + a.rect[2] <= b.rect[0]
                    || b.rect[0] + b.rect[2] <= a.rect[0]
                    || a.rect[1] + a.rect[3] <= b.rect[1]
                    || b.rect[1] + b.rect[3] <= a.rect[1];
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
    }
}

#[test]
fn a_scripted_hand_presses_the_field_and_then_types_the_text_on_the_keyboard() {
    let mut headset = plain();
    let cycle = PinchCycle {
        rest: Duration::from_millis(300),
        closing: Duration::from_millis(100),
        held: Duration::from_millis(200),
        opening: Duration::from_millis(100),
    };
    // The field panel stands a meter ahead of the head, and its field fills it
    let script = ScriptedHand::typing(
        Hand::Right,
        [0.15, -0.25, -0.3],
        Pose::at([0.0, 0.0, -1.0]),
        FIELD_PANEL,
        [FIELD_PANEL[0] / 2.0, FIELD_PANEL[1] / 2.0],
        "Hi 5!",
        cycle,
    );
    for frame in 0..600_u64 {
        let mut xr = XrFrame {
            predicted_display_time: Duration::from_millis(10 * frame),
            ..XrFrame::default()
        };
        play(&script, &mut xr);
        let aim = pointer_aim(&Controller::default(), Some(&xr.right_hand));
        headset.frame(aim);
    }
    assert_eq!(headset.typed(), "Hi 5!");
}
