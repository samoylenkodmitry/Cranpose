//! `Modifier::z_index` orders siblings as Compose's `zIndex` does: the one
//! with the larger z index draws on top and takes the tap where they overlap,
//! and equal ones keep the composition order.

use std::{cell::Cell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{Box, BoxSpec, Modifier, Size};

/// Two boxes on one spot; returns which one each tap reached.
fn overlapping(
    front_z: Rc<Cell<Option<MutableState<f32>>>>,
    tapped: Rc<Cell<&'static str>>,
) -> impl FnMut() + 'static {
    move || {
        let z = cranpose_core::rememberMutableStateOf(|| 0.0f32);
        front_z.set(Some(z));
        let tapped = Rc::clone(&tapped);
        Box(Modifier::empty(), BoxSpec::default(), move || {
            for (label, z_index) in [("first", z.get()), ("second", 0.0)] {
                let tapped = Rc::clone(&tapped);
                Box(
                    Modifier::empty()
                        .size(Size::new(100.0, 100.0))
                        .z_index(z_index)
                        .clickable(move |_| tapped.set(label)),
                    BoxSpec::default(),
                    || {},
                );
            }
        });
    }
}

#[test]
fn the_sibling_with_the_larger_z_index_takes_the_tap() {
    let z = Rc::new(Cell::new(None));
    let tapped = Rc::new(Cell::new(""));
    let mut robot =
        create_headless_robot_test(200, 200, overlapping(Rc::clone(&z), Rc::clone(&tapped)));

    assert!(robot.click_at(50.0, 50.0));
    assert_eq!(
        tapped.get(),
        "second",
        "equal z indices keep the composition order"
    );

    z.get().expect("the state is installed").set(1.0);
    robot.wait_for_idle();
    assert!(robot.click_at(50.0, 50.0));
    assert_eq!(
        tapped.get(),
        "first",
        "a larger z index moves the first box on top"
    );

    z.get().expect("the state is installed").set(-1.0);
    robot.wait_for_idle();
    assert!(robot.click_at(50.0, 50.0));
    assert_eq!(
        tapped.get(),
        "second",
        "a smaller z index sends it back under"
    );
}
