//! A press follows a graphics layer's translation: after the translation
//! changes, a click lands on the moved control and not where it was.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{
    AnimatedVisibility, Box, BoxSpec, Column, ColumnSpec, GraphicsLayer, Modifier, Point, Rect,
    Size, slide_in_vertically, slide_out_vertically,
};

fn clicks_after_translation(in_popup: bool) -> (u32, u32) {
    let clicks = Rc::new(Cell::new(0));
    let counted = Rc::clone(&clicks);
    let offset: Rc<Cell<Option<MutableState<f32>>>> = Rc::default();
    let installed = Rc::clone(&offset);
    let mut robot = create_headless_robot_test(300, 400, move || {
        let state = rememberMutableStateOf(|| 0.0_f32);
        installed.set(Some(state));
        let counted = Rc::clone(&counted);
        let control = move || {
            let counted = Rc::clone(&counted);
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                let counted = Rc::clone(&counted);
                Box(
                    Modifier::empty()
                        .size(Size::new(100.0, 50.0))
                        .graphics_layer(move || GraphicsLayer {
                            translation_y: state.get(),
                            ..GraphicsLayer::default()
                        })
                        .clickable(move |_| counted.set(counted.get() + 1)),
                    BoxSpec::default(),
                    || {},
                );
            });
        };
        if in_popup {
            cranpose_ui::widgets::Popup(
                Rect::from_origin_size(Point::new(0.0, 0.0), Size::new(0.0, 0.0)),
                Point::new(0.0, 0.0),
                control,
            );
        } else {
            control();
        }
    });
    robot.wait_for_idle();
    offset.get().expect("composed").set(200.0);
    robot.wait_for_idle();
    robot.click_at(50.0, 25.0);
    let at_old_place = clicks.get();
    robot.click_at(50.0, 225.0);
    (at_old_place, clicks.get() - at_old_place)
}

#[test]
fn a_click_follows_a_layer_translation() {
    assert_eq!(clicks_after_translation(false), (0, 1));
}

#[test]
fn a_click_follows_a_layer_translation_in_a_popup() {
    assert_eq!(clicks_after_translation(true), (0, 1));
}

// A control found by its tag is clicked and measured where its layer draws
// it, 200 below where layout put it.
#[test]
fn a_tagged_control_is_found_where_its_layer_draws_it() {
    let clicks = Rc::new(Cell::new(0));
    let counted = Rc::clone(&clicks);
    let mut robot = create_headless_robot_test(300, 400, move || {
        let counted = Rc::clone(&counted);
        Box(
            Modifier::empty()
                .size(Size::new(100.0, 50.0))
                .graphics_layer(|| GraphicsLayer {
                    translation_y: 200.0,
                    ..GraphicsLayer::default()
                })
                .clickable(move |_| counted.set(counted.get() + 1))
                .test_tag("moved"),
            BoxSpec::default(),
            || {},
        );
    });
    let bounds = robot.find_by_tag("moved").bounds();
    assert_eq!(
        bounds,
        Some(Rect::from_origin_size(
            Point::new(0.0, 200.0),
            Size::new(100.0, 50.0)
        ))
    );
    assert!(robot.find_by_tag("moved").click());
    assert_eq!(clicks.get(), 1);
}

// Two controls slide in with `AnimatedVisibility`, from half their height
// above their place. Once they rest, a click on each lands on it.
#[test]
fn a_click_reaches_each_control_that_slid_in() {
    let clicks = Rc::new(Cell::new((0u32, 0u32)));
    let counted = Rc::clone(&clicks);
    let shown: Rc<Cell<Option<MutableState<bool>>>> = Rc::default();
    let installed = Rc::clone(&shown);
    let mut robot = create_headless_robot_test(300, 400, move || {
        let state = rememberMutableStateOf(|| false);
        installed.set(Some(state));
        let counted = Rc::clone(&counted);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            Box(
                Modifier::empty().size(Size::new(300.0, 100.0)),
                BoxSpec::default(),
                || {},
            );
            let counted = Rc::clone(&counted);
            AnimatedVisibility(
                state.get(),
                slide_in_vertically(-0.5),
                slide_out_vertically(-0.5),
                move || {
                    let (upper, lower) = (Rc::clone(&counted), Rc::clone(&counted));
                    Column(Modifier::empty(), ColumnSpec::default(), move || {
                        let (upper, lower) = (Rc::clone(&upper), Rc::clone(&lower));
                        Box(
                            Modifier::empty()
                                .size(Size::new(100.0, 50.0))
                                .clickable(move |_| upper.set((upper.get().0 + 1, upper.get().1))),
                            BoxSpec::default(),
                            || {},
                        );
                        Box(
                            Modifier::empty()
                                .size(Size::new(100.0, 50.0))
                                .clickable(move |_| lower.set((lower.get().0, lower.get().1 + 1))),
                            BoxSpec::default(),
                            || {},
                        );
                    });
                },
            );
        });
    });
    robot.wait_for_idle();
    shown.get().expect("composed").set(true);
    robot.wait_for_motion();
    robot.click_at(50.0, 125.0);
    robot.click_at(50.0, 175.0);
    assert_eq!(clicks.get(), (1, 1));
}
