//! Platform-neutral projection of Cranpose semantics into accessibility elements.

use cranpose_core::collections::map::HashMap;
use cranpose_core::NodeId;
use cranpose_ui::{SemanticsAction, SemanticsNode, SemanticsRole};
use std::borrow::Cow;

use cranpose_app_shell::AppShell;
use cranpose_render_common::Renderer;
use cranpose_ui::LayoutBox;
use std::fmt::Debug;

/// Logical bounds for a platform accessibility element.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct AccessibilityRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl AccessibilityRect {
    pub(crate) const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub(crate) fn center(self) -> (f32, f32) {
        (self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    fn is_visible(self) -> bool {
        self.width > 0.0
            && self.height > 0.0
            && self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
    }
}

/// Role understood by native accessibility backends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccessibilityRole {
    Button,
    StaticText,
    TextField,
}

/// A flattened native accessibility element with stable Cranpose identity.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccessibilityElement {
    pub(crate) node_id: NodeId,
    pub(crate) label: String,
    pub(crate) value: Option<String>,
    pub(crate) bounds: AccessibilityRect,
    pub(crate) role: AccessibilityRole,
    pub(crate) clickable: bool,
}

/// Extracts the current semantics and layout snapshots from an app shell.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn snapshot<R>(shell: &mut AppShell<R>) -> Vec<AccessibilityElement>
where
    R: Renderer,
    R::Error: Debug,
{
    // Borrowed rather than cloned: this runs on every frame that updates, and
    // deep-copying the layout and semantics trees to read them dominated the
    // accessibility cost.
    let mut bounds = HashMap::new();
    let has_layout = shell.with_layout_tree(|layout_tree| match layout_tree {
        Some(layout_tree) => {
            collect_bounds(layout_tree.root(), &mut bounds);
            true
        }
        None => false,
    });
    if !has_layout {
        return Vec::new();
    }
    let Some(semantics_tree) = shell.semantics_tree() else {
        return Vec::new();
    };
    project_semantics(semantics_tree.root(), &bounds)
}

#[cfg_attr(test, allow(dead_code))]
fn collect_bounds(root: &LayoutBox, bounds: &mut HashMap<NodeId, AccessibilityRect>) {
    bounds.insert(
        root.node_id,
        AccessibilityRect::new(root.rect.x, root.rect.y, root.rect.width, root.rect.height),
    );
    for child in &root.children {
        collect_bounds(child, bounds);
    }
}

fn project_semantics(
    root: &SemanticsNode,
    bounds: &HashMap<NodeId, AccessibilityRect>,
) -> Vec<AccessibilityElement> {
    let mut elements = Vec::new();
    project_node(root, bounds, false, &mut elements);
    elements
}

fn project_node(
    node: &SemanticsNode,
    bounds: &HashMap<NodeId, AccessibilityRect>,
    suppress_static_text: bool,
    elements: &mut Vec<AccessibilityElement>,
) {
    let clickable = node
        .actions
        .iter()
        .any(|action| matches!(action, SemanticsAction::Click { .. }));
    let actionable = clickable || node.editable_text;
    let own_label = node_label(node).map(Cow::Borrowed);
    let label = if actionable {
        own_label.or_else(|| descendant_label(node).map(Cow::Owned))
    } else {
        own_label
    };
    let rect = bounds.get(&node.node_id).copied().unwrap_or_default();

    if let Some(label) = label.filter(|label| !label.trim().is_empty()) {
        if rect.is_visible() && (actionable || !suppress_static_text) {
            let role = if node.editable_text {
                AccessibilityRole::TextField
            } else if clickable || matches!(node.role, SemanticsRole::Button) {
                AccessibilityRole::Button
            } else {
                AccessibilityRole::StaticText
            };
            let label = label.into_owned();
            elements.push(AccessibilityElement {
                node_id: node.node_id,
                value: node.editable_text.then(|| label.clone()),
                label,
                bounds: rect,
                role,
                clickable,
            });
        }
    }

    let suppress_children = suppress_static_text || actionable;
    for child in &node.children {
        project_node(child, bounds, suppress_children, elements);
    }
}

/// Borrows rather than clones: this runs for every semantics node on every
/// updating frame, and only the few nodes that become elements need an owned
/// label.
fn node_label(node: &SemanticsNode) -> Option<&str> {
    node.description.as_deref().or(match &node.role {
        SemanticsRole::Text { value } => Some(value.as_str()),
        _ => None,
    })
}

fn descendant_label(node: &SemanticsNode) -> Option<String> {
    let mut labels = Vec::new();
    collect_descendant_labels(node, &mut labels);
    (!labels.is_empty()).then(|| labels.join(", "))
}

fn collect_descendant_labels<'a>(node: &'a SemanticsNode, labels: &mut Vec<&'a str>) {
    for child in &node.children {
        if let Some(label) = node_label(child) {
            if !label.trim().is_empty() && !labels.contains(&label) {
                labels.push(label);
            }
        } else {
            collect_descendant_labels(child, labels);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_core::NodeId;
    use cranpose_ui::{SemanticsAction, SemanticsCallback, SemanticsNode, SemanticsRole};

    fn node(
        node_id: NodeId,
        role: SemanticsRole,
        actions: Vec<SemanticsAction>,
        description: Option<&str>,
        children: Vec<SemanticsNode>,
    ) -> SemanticsNode {
        SemanticsNode {
            node_id,
            role,
            actions,
            children,
            description: description.map(str::to_owned),
            editable_text: false,
            text_selection: None,
        }
    }

    #[test]
    fn actionable_parent_uses_descendant_text_without_duplicate_leaf() {
        let button_id = 2;
        let root = node(
            1,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            vec![
                node(
                    button_id,
                    SemanticsRole::Button,
                    vec![SemanticsAction::Click {
                        handler: SemanticsCallback::new(button_id),
                    }],
                    None,
                    vec![node(
                        3,
                        SemanticsRole::Text {
                            value: "Library".into(),
                        },
                        Vec::new(),
                        None,
                        Vec::new(),
                    )],
                ),
                node(
                    4,
                    SemanticsRole::Text {
                        value: "Receipts".into(),
                    },
                    Vec::new(),
                    None,
                    Vec::new(),
                ),
            ],
        );
        let bounds = HashMap::from_iter([
            (button_id, AccessibilityRect::new(8.0, 700.0, 80.0, 64.0)),
            (3, AccessibilityRect::new(20.0, 712.0, 50.0, 20.0)),
            (4, AccessibilityRect::new(16.0, 80.0, 100.0, 28.0)),
        ]);

        let projected = project_semantics(&root, &bounds);

        assert_eq!(projected.len(), 2);
        assert_eq!(projected[0].node_id, button_id);
        assert_eq!(projected[0].label, "Library");
        assert_eq!(projected[0].role, AccessibilityRole::Button);
        assert_eq!(projected[0].bounds.center(), (48.0, 732.0));
        assert_eq!(projected[1].label, "Receipts");
        assert_eq!(projected[1].role, AccessibilityRole::StaticText);
    }
}
