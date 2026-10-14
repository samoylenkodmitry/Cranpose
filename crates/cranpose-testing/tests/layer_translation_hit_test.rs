//! A press follows a graphics layer's translation: after the translation
//! changes, a click lands on the moved control and not where it was.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, GraphicsLayer, Modifier, Point, Rect, Size};

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
