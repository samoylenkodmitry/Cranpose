use super::*;

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
            .iter()
            .map(|child| child.node_id)
            .collect::<Vec<_>>(),
        [4, 0, 1, 2, 3]
    );
}

#[test]
fn accessible_labels_hide_secrets_and_preserve_an_empty_field() {
    let mut node = SemanticsNode {
        editable_text: true,
        ..SemanticsNode::default()
    };
    assert_eq!(node.accessibility_label().as_deref(), Some(""));
    node.password = true;
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
        password: true,
        role: SemanticsRole::Text {
            value: "secret".into(),
        },
        ..SemanticsNode::default()
    };
    assert_eq!(node.accessibility_label().as_deref(), Some("password"));
}
