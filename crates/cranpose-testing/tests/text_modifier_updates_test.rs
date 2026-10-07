//! A text node whose string stays the same takes every other change to its
//! modifier, such as a semantics element that updates on every set.

use cranpose_core::MutableState;
use cranpose_testing::ComposeTestRule;
use cranpose_ui::{Modifier, Size, Text, TextStyle};

fn labels(rule: &mut ComposeTestRule) -> Vec<String> {
    rule.placed_semantics(Size::new(200.0, 100.0))
        .expect("the content lays out")
        .expect("the content places something")
        .flatten()
        .into_iter()
        .filter_map(|node| node.label.clone())
        .collect()
}

#[test]
fn a_text_with_the_same_string_takes_its_new_semantics() {
    let mut rule = ComposeTestRule::new();
    let label = MutableState::with_runtime("first", rule.runtime_handle());
    rule.set_content(move || {
        let value = label.get();
        Text(
            "price",
            Modifier::empty().semantics(move |config| {
                config.content_description = Some(value.to_string());
            }),
            TextStyle::default(),
        );
    })
    .expect("the content composes");
    assert!(labels(&mut rule).iter().any(|label| label == "first"));
    label.set("second");
    let after = labels(&mut rule);
    assert!(
        after.iter().any(|label| label == "second"),
        "the node reports its new semantics: {after:?}"
    );
}
