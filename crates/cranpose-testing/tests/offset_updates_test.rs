//! A node's offset moves it on the next layout whenever the offset or the
//! layout direction an offset follows changes, though nothing else about
//! the node's modifiers did.

use cranpose_core::MutableState;
use cranpose_testing::{ComposeTestRule, PlacedSemanticsNode};
use cranpose_ui::{Box, BoxSpec, LayoutDirection, Modifier, ProvideLayoutDirection, Size};

fn tagged(modifier: Modifier) -> Modifier {
    modifier.semantics(|config| {
        config.content_description = Some("moved".to_string());
    })
}

fn left_of_moved(rule: &mut ComposeTestRule) -> f32 {
    let root: PlacedSemanticsNode = rule
        .placed_semantics(Size::new(200.0, 100.0))
        .expect("the content lays out")
        .expect("the content places something");
    root.flatten()
        .into_iter()
        .find(|node| node.label.as_deref() == Some("moved"))
        .expect("the moved box is placed")
        .layout_bounds
        .x
}

#[test]
fn a_changed_offset_moves_the_node() {
    let mut rule = ComposeTestRule::new();
    let x = MutableState::with_runtime(10.0f32, rule.runtime_handle());
    rule.set_content(move || {
        Box(
            Modifier::empty().size(Size::new(200.0, 100.0)),
            BoxSpec::default(),
            move || {
                Box(
                    tagged(
                        Modifier::empty()
                            .offset(x.get(), 0.0)
                            .size(Size::new(20.0, 20.0)),
                    ),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    })
    .expect("the content composes");
    assert_eq!(left_of_moved(&mut rule), 10.0);
    x.set(35.0);
    assert_eq!(
        left_of_moved(&mut rule),
        35.0,
        "the node follows its new offset"
    );
}

#[test]
fn an_offset_follows_a_changed_layout_direction() {
    let mut rule = ComposeTestRule::new();
    let direction = MutableState::with_runtime(LayoutDirection::Ltr, rule.runtime_handle());
    rule.set_content(move || {
        ProvideLayoutDirection(direction.get(), || {
            Box(
                Modifier::empty().size(Size::new(200.0, 100.0)),
                BoxSpec::default(),
                || {
                    Box(
                        tagged(
                            Modifier::empty()
                                .offset(10.0, 0.0)
                                .size(Size::new(20.0, 20.0)),
                        ),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        });
    })
    .expect("the content composes");
    let ltr = left_of_moved(&mut rule);
    direction.set(LayoutDirection::Rtl);
    let rtl = left_of_moved(&mut rule);
    assert_eq!(
        ltr, 10.0,
        "left to right, the offset moves the box right of the start"
    );
    assert_eq!(
        rtl,
        200.0 - 20.0 - 10.0,
        "right to left, it moves it left of the end"
    );
}
