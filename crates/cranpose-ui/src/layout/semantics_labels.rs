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
                        | SemanticsWidgetRole::RadioGroup
                )
            )
    }

    /// Children in screen-reader order. Equal indices retain composition order;
    /// non-finite indices are treated as the default index, zero. Children
    /// that all keep the default index come in composition order without
    /// being collected first.
    pub fn accessibility_children(&self) -> impl Iterator<Item = &Self> + '_ {
        let index = |node: &Self| {
            if node.traversal_index.is_finite() {
                node.traversal_index
            } else {
                0.0
            }
        };
        if !self.children.iter().any(|child| index(child) != 0.0) {
            return ReaderOrder::Composed(self.children.iter());
        }
        let mut children: Vec<_> = self.children.iter().collect();
        children.sort_by(|left, right| {
            index(left)
                .partial_cmp(&index(right))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        ReaderOrder::Sorted(children.into_iter())
    }

    /// The accessible name, including merged static descendants in reading
    /// order. Hidden content and independent controls contribute no text to an
    /// ancestor. Password values never become names. An unnamed editable field
    /// returns an empty name so it remains reachable by assistive technology.
    pub fn accessibility_label(&self) -> Option<Cow<'_, str>> {
        if self.hidden {
            return None;
        }
        if let Some(own) = self.own_accessibility_label() {
            return Some(Cow::Borrowed(own));
        }
        let mut words = String::new();
        if self.merges_accessibility_descendants() && self.write_accessibility_words(&mut words) {
            return Some(Cow::Owned(words));
        }
        self.editable_text.then_some(Cow::Borrowed(""))
    }

    /// Appends the name [`SemanticsNode::accessibility_label`] gives to
    /// `out`, without collecting a merged name first, and answers whether
    /// the node has a name at all; an unnamed editable field has an empty one.
    pub fn write_accessibility_label(&self, out: &mut String) -> bool {
        if self.hidden {
            return false;
        }
        if let Some(own) = self.own_accessibility_label() {
            out.push_str(own);
            return true;
        }
        (self.merges_accessibility_descendants() && self.write_accessibility_words(out))
            || self.editable_text
    }

    fn own_accessibility_label(&self) -> Option<&str> {
        if self.password {
            return Some(
                self.description
                    .as_deref()
                    .filter(|label| {
                        !label.trim().is_empty() && Some(*label) != self.text.as_deref()
                    })
                    .unwrap_or("password"),
            );
        }
        self.description
            .as_deref()
            .filter(|label| !label.trim().is_empty())
            .or_else(|| match &self.role {
                SemanticsRole::Text { value } if !value.as_str().trim().is_empty() => {
                    Some(value.as_str())
                }
                _ => None,
            })
    }

    fn write_accessibility_words(&self, out: &mut String) -> bool {
        let mut wrote = false;
        self.append_accessibility_words(out, &mut wrote);
        wrote
    }

    fn append_accessibility_words(&self, out: &mut String, wrote: &mut bool) {
        for child in self.accessibility_children() {
            if child.hidden || child.is_accessibility_boundary() {
                continue;
            }
            match child.own_accessibility_label() {
                Some(label) if !label.trim().is_empty() => {
                    if *wrote {
                        out.push_str(", ");
                    }
                    out.push_str(label);
                    *wrote = true;
                }
                _ => child.append_accessibility_words(out, wrote),
            }
        }
    }
}

enum ReaderOrder<'a> {
    Composed(std::slice::Iter<'a, SemanticsNode>),
    Sorted(std::vec::IntoIter<&'a SemanticsNode>),
}

impl<'a> Iterator for ReaderOrder<'a> {
    type Item = &'a SemanticsNode;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Composed(children) => children.next(),
            Self::Sorted(children) => children.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Composed(children) => children.size_hint(),
            Self::Sorted(children) => children.size_hint(),
        }
    }
}

#[cfg(test)]
#[path = "tests/semantics_labels.rs"]
mod tests;
