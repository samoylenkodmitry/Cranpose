//! `Modifier::test_tag` names a node for tests, as Compose's `testTag` does:
//! a test finds the node by the tag when its text changes, and the name a
//! screen reader speaks stays the node's own.

use std::{cell::Cell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_testing::{ComposeTestRule, PlacedSemanticsNode, spoken_name};
use cranpose_ui::{Column, ColumnSpec, Modifier, Size, Text, TextStyle};

fn placed(rule: &mut ComposeTestRule) -> PlacedSemanticsNode {
    rule.placed_semantics(Size::new(300.0, 300.0))
        .expect("the content lays out")
        .expect("the content places something")
}

#[test]
fn a_test_finds_a_node_by_its_tag_after_its_text_changes() {
    let mut rule = ComposeTestRule::new();
    let label: Rc<Cell<Option<MutableState<&'static str>>>> = Rc::new(Cell::new(None));
    let installed = Rc::clone(&label);
    rule.set_content(move || {
        let text = cranpose_core::rememberMutableStateOf(|| "Pay");
        installed.set(Some(text));
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            Text(
                text.get(),
                Modifier::empty().test_tag("checkout"),
                TextStyle::default(),
            );
            Text("Total", Modifier::empty(), TextStyle::default());
        });
    })
    .expect("the content composes");

    let root = placed(&mut rule);
    let checkout = root
        .find_by_tag("checkout")
        .expect("the tagged text is found");
    assert_eq!(spoken_name(checkout), "Pay");
    assert!(root.find_by_tag("missing").is_none());

    label.get().expect("the state is installed").set("Pay now");
    rule.pump_until_idle().expect("the change recomposes");
    let root = placed(&mut rule);
    let checkout = root
        .find_by_tag("checkout")
        .expect("the tag still finds the text after its copy changed");
    assert_eq!(
        spoken_name(checkout),
        "Pay now",
        "a screen reader speaks the text, never the tag"
    );
}

#[test]
fn a_headless_robot_clicks_a_control_by_its_tag() {
    let count = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&count);
    let mut robot = cranpose_testing::create_headless_robot_test(300, 200, move || {
        let recorded = Rc::clone(&recorded);
        cranpose_ui::Row(
            Modifier::empty(),
            cranpose_ui::RowSpec::default(),
            move || {
                let recorded = Rc::clone(&recorded);
                cranpose_ui::Box(
                    Modifier::empty()
                        .size(Size::new(100.0, 48.0))
                        .content_description("Add")
                        .test_tag("add"),
                    cranpose_ui::BoxSpec::default(),
                    || {},
                );
                cranpose_ui::Box(
                    Modifier::empty()
                        .size(Size::new(100.0, 48.0))
                        .content_description("Add")
                        .test_tag("add-twice")
                        .clickable(move |_| recorded.set(recorded.get() + 2)),
                    cranpose_ui::BoxSpec::default(),
                    || {},
                );
            },
        );
    });
    robot.find_by_tag("add-twice").assert_exists();
    assert!(!robot.find_by_tag("missing").exists());
    let bounds = robot
        .find_by_tag("add-twice")
        .bounds()
        .expect("tagged bounds");
    assert_eq!((bounds.x, bounds.width), (100.0, 100.0));
    assert!(robot.find_by_tag("add-twice").click());
    assert_eq!(
        count.get(),
        2,
        "the tag picks one of two controls with one name"
    );
}
