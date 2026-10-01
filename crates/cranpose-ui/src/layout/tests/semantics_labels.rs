use super::*;
use crate::layout::SemanticsDetails;

#[test]
fn controls_and_containers_define_merge_boundaries() {
    let mut node = SemanticsNode::default();
    assert!(!node.merges_accessibility_descendants());
    assert!(!node.is_accessibility_boundary());
    node.widget_role = Some(SemanticsWidgetRole::List);
    assert!(node.is_accessibility_boundary());
    assert!(!node.merges_accessibility_descendants());
    node.focusable = true;
    assert!(node.merges_accessibility_descendants());
}

#[test]
fn invalid_and_signed_zero_indices_keep_stable_order() {
    let node = SemanticsNode {
        children: [f32::NAN, -0.0, f32::INFINITY, 0.0, -1.0]
            .into_iter()
            .enumerate()
            .map(|(node_id, traversal_index)| SemanticsNode {
                node_id,
                traversal_index,
                ..SemanticsNode::default()
            })
            .collect(),
        ..SemanticsNode::default()
    };
    assert_eq!(
        node.accessibility_children()
            .map(|child| child.node_id)
            .collect::<Vec<_>>(),
        [4, 0, 1, 2, 3]
    );
}

#[test]
fn children_without_traversal_indices_come_in_composition_order() {
    let node = SemanticsNode {
        children: (0..3)
            .map(|node_id| SemanticsNode {
                node_id,
                ..SemanticsNode::default()
            })
            .collect(),
        ..SemanticsNode::default()
    };
    let children = node.accessibility_children();
    assert_eq!(children.size_hint(), (3, Some(3)));
    assert_eq!(
        children.map(|child| child.node_id).collect::<Vec<_>>(),
        [0, 1, 2]
    );
}

#[test]
fn a_written_label_is_the_label() {
    let text = |node_id, value: &str| SemanticsNode {
        node_id,
        role: SemanticsRole::Text {
            value: value.into(),
        },
        ..SemanticsNode::default()
    };
    let group = SemanticsNode {
        children: vec![text(2, "  "), text(3, "Inner")],
        ..SemanticsNode::default()
    };
    let button = SemanticsNode {
        merge_descendants: true,
        children: vec![
            text(1, "Save"),
            group,
            SemanticsNode {
                hidden: true,
                ..text(4, "Hidden")
            },
            SemanticsNode {
                focusable: true,
                ..text(5, "Separate")
            },
        ],
        ..SemanticsNode::default()
    };
    let field = SemanticsNode {
        details: Some(Box::new(SemanticsDetails {
            editable_text: true,
            ..SemanticsDetails::NONE
        })),
        ..SemanticsNode::default()
    };
    let hidden = SemanticsNode {
        hidden: true,
        description: Some("Gone".into()),
        ..SemanticsNode::default()
    };
    for node in [&button, &field, &hidden, &text(6, "Plain")] {
        let mut written = String::from("kept: ");
        let named = node.write_accessibility_label(&mut written);
        let label = node.accessibility_label();
        assert_eq!(named, label.is_some());
        assert_eq!(written, format!("kept: {}", label.unwrap_or_default()));
    }
    assert_eq!(
        button.accessibility_label().as_deref(),
        Some("Save, Inner"),
        "a merged name reads the static text below it in order"
    );
}

#[test]
fn accessible_labels_hide_secrets_and_preserve_an_empty_field() {
    let mut node = SemanticsNode {
        details: Some(Box::new(SemanticsDetails {
            editable_text: true,
            ..SemanticsDetails::NONE
        })),
        ..SemanticsNode::default()
    };
    assert_eq!(node.accessibility_label().as_deref(), Some(""));
    node.update_details(|details| details.password = true);
    node.description = Some("secret".into());
    node.text = node.description.clone();
    assert_eq!(node.accessibility_label().as_deref(), Some("password"));
    node.description = Some("Passphrase".into());
    assert_eq!(node.accessibility_label().as_deref(), Some("Passphrase"));
    node.hidden = true;
    assert_eq!(node.accessibility_label(), None);
}

#[test]
fn password_semantics_never_use_the_displayed_text_as_a_name() {
    let node = SemanticsNode {
        details: Some(Box::new(SemanticsDetails {
            password: true,
            ..SemanticsDetails::NONE
        })),
        role: SemanticsRole::Text {
            value: "secret".into(),
        },
        ..SemanticsNode::default()
    };
    assert_eq!(node.accessibility_label().as_deref(), Some("password"));
}
