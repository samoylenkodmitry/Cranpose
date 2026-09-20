use std::borrow::Cow;

use super::{SemanticsNode, SemanticsRole, SemanticsWidgetRole};

impl SemanticsNode {
    /// Whether this node combines its static descendants into one reader stop.
    /// Independently operable descendants remain separate stops.
    pub fn merges_accessibility_descendants(&self) -> bool {
        self.merge_descendants
            || !self.actions.is_empty()
            || self.editable_text
            || self.password
            || self.focusable
            || !self.custom_actions.is_empty()
            || self.set_progress.is_some()
            || self.set_text.is_some()
            || self.set_selection.is_some()
            || self.on_long_click.is_some()
            || self.on_magic_tap.is_some()
            || self.expand.is_some()
            || self.collapse.is_some()
            || self.dismiss.is_some()
    }

    /// Whether an ancestor must leave this node and its content as a separate
    /// reader stop or container instead of including it in its own label.
    pub fn is_accessibility_boundary(&self) -> bool {
        self.merges_accessibility_descendants()
            || self.vertical_scroll.is_some()
            || self.horizontal_scroll.is_some()
            || self.selectable_group
            || self.pane_title.is_some()
            || matches!(
                self.widget_role,
                Some(
                    SemanticsWidgetRole::Dialog
                        | SemanticsWidgetRole::Toolbar
                        | SemanticsWidgetRole::Menu
                        | SemanticsWidgetRole::TabBar
                        | SemanticsWidgetRole::List
                )
            )
    }

    /// Children in screen-reader order. Equal indices retain composition order;
    /// non-finite indices are treated as the default index, zero.
    pub fn accessibility_children(&self) -> Vec<&Self> {
        let mut children: Vec<_> = self.children.iter().collect();
        let index = |node: &&Self| {
            if node.traversal_index.is_finite() {
                node.traversal_index
            } else {
                0.0
            }
        };
        if children.iter().any(|child| index(child) != 0.0) {
            children.sort_by(|left, right| index(left).partial_cmp(&index(right)).unwrap());
        }
        children
    }

    /// The accessible name, including merged static descendants in reading
    /// order. Hidden content and independent controls contribute no text to an
    /// ancestor. Password values never become names. An unnamed editable field
    /// returns an empty name so it remains reachable by assistive technology.
    pub fn accessibility_label(&self) -> Option<Cow<'_, str>> {
        if self.hidden {
            return None;
        }
        let own = self
            .description
            .as_deref()
            .filter(|label| !label.trim().is_empty())
            .or_else(|| match &self.role {
                SemanticsRole::Text { value } if !value.trim().is_empty() => Some(value.as_str()),
                _ => None,
            });
        if self.password {
            return Some(Cow::Borrowed(
                self.description
                    .as_deref()
                    .filter(|label| {
                        !label.trim().is_empty() && Some(*label) != self.text.as_deref()
                    })
                    .unwrap_or("password"),
            ));
        }
        own.map(Cow::Borrowed)
            .or_else(|| {
                if !self.merges_accessibility_descendants() {
                    return None;
                }
                let mut words = Vec::new();
                self.collect_accessibility_words(&mut words);
                (!words.is_empty()).then(|| Cow::Owned(words.join(", ")))
            })
            .or_else(|| self.editable_text.then_some(Cow::Borrowed("")))
    }

    fn collect_accessibility_words<'a>(&'a self, words: &mut Vec<&'a str>) {
        for child in self.accessibility_children() {
            if child.hidden || child.is_accessibility_boundary() {
                continue;
            }
            match child.accessibility_label() {
                Some(Cow::Borrowed(label)) if !label.trim().is_empty() => words.push(label),
                _ => child.collect_accessibility_words(words),
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
}
