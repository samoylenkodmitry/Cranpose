use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_foundation::text::TextFieldState;
use cranpose_liquid::prelude::*;
use cranpose_render_common::text_fonts::TextFonts;
use cranpose_testing::create_headless_robot_test_with_fonts;
use cranpose_ui::{Modifier, SemanticsWidgetRole};

use crate::reader::{Robot, node, nodes, tap, tap_node};

fn robot(content: impl FnMut() + 'static) -> Robot {
    let fonts = TextFonts::from_fonts_or_default(&[include_bytes!(
        "../../cranpose-render/common/assets/NotoSansMerged.ttf"
    )]);
    let mut content = content;
    let mut robot = create_headless_robot_test_with_fonts(400, 200, fonts, move || {
        LiquidTheme(LiquidThemeSpec::system(SchemeMode::Light), &mut content);
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    robot
}

type Slot<T> = Rc<Cell<Option<MutableState<T>>>>;

#[test]
fn a_stepper_steps_within_its_range_and_disables_the_bound_it_reached() {
    let slot: Slot<f32> = Rc::default();
    let captured = Rc::clone(&slot);
    let mut robot = robot(move || {
        let value = rememberMutableStateOf(|| 0.8_f32);
        captured.set(Some(value));
        LiquidStepper(
            Modifier::empty(),
            value.get(),
            0.0..=1.0,
            0.1,
            move |next| value.set(next),
        );
    });
    let value = slot.take().expect("the composition remembered the value");
    tap_node(&mut robot, "Increment");
    assert!((value.get() - 0.9).abs() < 1e-4, "{}", value.get());
    tap_node(&mut robot, "Increment");
    assert!((value.get() - 1.0).abs() < 1e-4, "{}", value.get());
    assert!(
        !node(&mut robot, "Increment").enabled,
        "the top bound disables Increment"
    );
    assert!(node(&mut robot, "Decrement").enabled);
    tap_node(&mut robot, "Increment");
    assert!(
        (value.get() - 1.0).abs() < 1e-4,
        "a disabled half changes nothing"
    );
    tap_node(&mut robot, "Decrement");
    assert!((value.get() - 0.9).abs() < 1e-4, "{}", value.get());
}

#[test]
fn progress_bars_and_gauges_read_their_percent() {
    let mut robot = robot(|| {
        LiquidLinearProgressIndicator(Modifier::empty().width(300.0), 0.25);
        LiquidGauge(
            Modifier::empty().width(300.0).offset(0.0, 40.0),
            0.5,
            "Level",
        );
    });
    let bars: Vec<_> = nodes(&mut robot)
        .into_iter()
        .filter(|node| node.widget_role == Some(SemanticsWidgetRole::ProgressBar))
        .map(|node| (node.label, node.state_description))
        .collect();
    assert_eq!(
        bars,
        [
            (None, Some("25%".to_owned())),
            (Some("Level".to_owned()), Some("50%".to_owned())),
        ]
    );
}

#[test]
fn a_spinner_reads_as_a_busy_progress_bar() {
    let mut robot = robot(|| {
        LiquidActivityIndicator(Modifier::empty(), LiquidActivityIndicatorSize::Large, None);
    });
    let spinner = nodes(&mut robot)
        .into_iter()
        .find(|node| node.widget_role == Some(SemanticsWidgetRole::ProgressBar))
        .expect("a spinner node");
    assert_eq!(spinner.layout_bounds.width, 37.0);
    assert_eq!(spinner.layout_bounds.height, 37.0);
}

#[test]
fn a_page_control_turns_one_page_per_tap_toward_the_tapped_side() {
    let slot: Slot<usize> = Rc::default();
    let captured = Rc::clone(&slot);
    let mut robot = robot(move || {
        let page = rememberMutableStateOf(|| 3_usize);
        captured.set(Some(page));
        LiquidPageControl(Modifier::empty(), 5, page.get(), move |next| page.set(next));
    });
    let page = slot.take().expect("the composition remembered the page");
    let control = nodes(&mut robot)
        .into_iter()
        .find(|node| node.state_description.as_deref() == Some("4 / 5"))
        .expect("the page control reads page 4 of 5")
        .layout_bounds;
    let center_y = control.y + control.height * 0.5;
    tap(&mut robot, control.x + control.width - 1.0, center_y);
    assert_eq!(page.get(), 4);
    tap(&mut robot, control.x + control.width - 1.0, center_y);
    assert_eq!(page.get(), 4, "the last page stays");
    tap(&mut robot, control.x + 1.0, center_y);
    assert_eq!(page.get(), 3);
}

#[test]
fn a_reader_sets_the_page_control_to_the_nearest_page() {
    let slot: Slot<usize> = Rc::default();
    let captured = Rc::clone(&slot);
    let mut robot = robot(move || {
        let page = rememberMutableStateOf(|| 0_usize);
        captured.set(Some(page));
        LiquidPageControl(Modifier::empty(), 5, page.get(), move |next| page.set(next));
    });
    let page = slot.take().expect("the composition remembered the page");
    for (requested, expected) in [(2.6, 3), (9.0, 4), (0.4, 0), (-3.0, 0), (1.5, 2)] {
        let node = nodes(&mut robot)
            .into_iter()
            .find(|node| {
                node.state_description
                    .as_deref()
                    .is_some_and(|state| state.ends_with("/ 5"))
            })
            .expect("the page control reads its page")
            .node_id;
        let tree = robot.shell_mut().semantics_tree().expect("semantics");
        let set = cranpose_testing::robot_assertions::find_semantics_node(tree.root(), &|found| {
            found.node_id == node
        })
        .and_then(|found| found.details().set_progress.clone())
        .expect("the page control takes a page");
        assert!(set.invoke(requested));
        robot.wait_for_idle();
        assert_eq!(page.get(), expected, "a request for page {requested}");
    }
}

#[test]
fn a_disclosure_group_shows_its_content_only_while_open() {
    let mut robot = robot(|| {
        let expanded = rememberMutableStateOf(|| false);
        LiquidDisclosureGroup(
            Modifier::empty().width(300.0),
            "Details",
            expanded.get(),
            move |next| expanded.set(next),
            || {
                cranpose_ui::widgets::Text(
                    "More information",
                    Modifier::empty(),
                    liquid_typography().body,
                );
            },
        );
    });
    assert!(!robot.find_by_text("More information").exists());
    tap_node(&mut robot, "Details");
    robot.wait_for_motion();
    assert!(robot.find_by_text("More information").exists());
    tap_node(&mut robot, "Details");
    robot.wait_for_motion();
    assert!(!robot.find_by_text("More information").exists());
}

#[test]
fn a_text_field_shows_its_placeholder_until_it_has_text() {
    let slot: Rc<Cell<Option<TextFieldState>>> = Rc::default();
    let captured = Rc::clone(&slot);
    let mut robot = robot(move || {
        let state = cranpose_core::remember(|| TextFieldState::new("")).with(|state| *state);
        captured.set(Some(state));
        LiquidTextField(Modifier::empty().width(300.0), state, "Name");
    });
    let state = slot.get().expect("the composition remembered the field");
    let reader = node(&mut robot, "Name");
    assert!(
        reader.editable_text,
        "the field reads as editable text named Name"
    );
    assert_eq!(reader.layout_bounds.height, 34.0);
    assert!(robot.find_by_text("Name").exists());
    state.set_text("Ada");
    robot.wait_for_idle();
    assert!(robot.find_by_text("Ada").exists());
    assert!(
        robot.get_all_text().iter().all(|text| text != "Name"),
        "the placeholder leaves once the field has text"
    );
}
