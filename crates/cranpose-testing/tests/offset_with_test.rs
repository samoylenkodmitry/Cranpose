// `Modifier::offset_with` moves content by what its closure reads, as
// Compose's `Modifier.offset { }`: a click lands where the content moved,
// the content does not compose again, a right-to-left layout mirrors `x`,
// and `absolute_offset_with` does not.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};
use cranpose_ui::{Box, BoxSpec, LayoutDirection, Modifier, Point, ProvideLayoutDirection, Size};

const WIDTH: u32 = 300;

#[derive(Clone, Copy)]
enum Offset {
    Relative,
    Absolute,
}

struct Moved {
    robot: RobotTestRule<TestRenderer>,
    shift: MutableState<f32>,
    clicks: Rc<Cell<u32>>,
    compositions: Rc<Cell<u32>>,
}

// A 100 × 50 clickable box at the start of a 300 wide window, moved along
// x by a state that starts at `x`.
fn moved(direction: LayoutDirection, offset: Offset, x: f32) -> Moved {
    let clicks = Rc::new(Cell::new(0));
    let compositions = Rc::new(Cell::new(0));
    let shift: Rc<Cell<Option<MutableState<f32>>>> = Rc::default();
    let (counted, composed, installed) = (
        Rc::clone(&clicks),
        Rc::clone(&compositions),
        Rc::clone(&shift),
    );
    let robot = create_headless_robot_test(WIDTH, 200, move || {
        let state = rememberMutableStateOf(|| x);
        installed.set(Some(state));
        let (counted, composed) = (Rc::clone(&counted), Rc::clone(&composed));
        ProvideLayoutDirection(direction, move || {
            composed.set(composed.get() + 1);
            let at = move || Point::new(state.get(), 0.0);
            let modifier = Modifier::empty().size(Size::new(100.0, 50.0));
            let modifier = match offset {
                Offset::Relative => modifier.offset_with(at),
                Offset::Absolute => modifier.absolute_offset_with(at),
            };
            let counted = Rc::clone(&counted);
            // A full-width parent puts the box at the start edge.
            Box(
                Modifier::empty().fill_max_width(),
                BoxSpec::default(),
                move || {
                    let counted = Rc::clone(&counted);
                    Box(
                        modifier
                            .clone()
                            .clickable(move |_| counted.set(counted.get() + 1)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        });
    });
    Moved {
        robot,
        shift: shift.get().expect("composed"),
        clicks,
        compositions,
    }
}

impl Moved {
    // Whether a click at `x`, halfway down the box, reaches it.
    fn hits(&mut self, x: f32) -> bool {
        let before = self.clicks.get();
        self.robot.click_at(x, 25.0);
        self.clicks.get() > before
    }
}

#[test]
fn the_content_moves_with_the_state_without_composing_again() {
    let mut box_ = moved(LayoutDirection::Ltr, Offset::Relative, 0.0);
    assert!(box_.hits(50.0));
    let compositions = box_.compositions.get();

    box_.shift.set(150.0);
    box_.robot.wait_for_idle();
    assert!(!box_.hits(50.0), "the box left its first place");
    assert!(box_.hits(200.0), "a click lands where it moved");
    assert_eq!(
        box_.compositions.get(),
        compositions,
        "the move reads the state where the scene is built"
    );
}

#[test]
fn a_right_to_left_layout_mirrors_x() {
    // The box starts at the right edge, from 200 to 300.
    let mut box_ = moved(LayoutDirection::Rtl, Offset::Relative, 100.0);
    assert!(box_.hits(150.0), "moved left by 100");
    assert!(!box_.hits(250.0));
}

#[test]
fn an_absolute_offset_moves_right_in_either_direction() {
    let mut box_ = moved(LayoutDirection::Rtl, Offset::Absolute, -100.0);
    assert!(box_.hits(150.0), "moved left by -100");
    assert!(!box_.hits(250.0));
}

#[test]
fn a_change_of_direction_mirrors_the_offset_again() {
    let clicks = Rc::new(Cell::new(0));
    let counted = Rc::clone(&clicks);
    let direction: Rc<Cell<Option<MutableState<LayoutDirection>>>> = Rc::default();
    let installed = Rc::clone(&direction);
    let mut robot = create_headless_robot_test(WIDTH, 200, move || {
        let state = rememberMutableStateOf(|| LayoutDirection::Ltr);
        installed.set(Some(state));
        let counted = Rc::clone(&counted);
        ProvideLayoutDirection(state.get(), move || {
            let counted = Rc::clone(&counted);
            Box(
                Modifier::empty().fill_max_width(),
                BoxSpec::default(),
                move || {
                    let counted = Rc::clone(&counted);
                    Box(
                        Modifier::empty()
                            .size(Size::new(100.0, 50.0))
                            .offset_with(|| Point::new(100.0, 0.0))
                            .clickable(move |_| counted.set(counted.get() + 1)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        });
    });
    let mut hits = |x: f32| {
        let before = clicks.get();
        robot.click_at(x, 25.0);
        clicks.get() > before
    };
    assert!(hits(150.0), "left to right, from 100 to 200");
    direction.get().expect("composed").set(LayoutDirection::Rtl);
    assert!(hits(150.0), "right to left, from 100 to 200 again");
    assert!(!hits(250.0), "not at the right edge, where the box starts");
}
