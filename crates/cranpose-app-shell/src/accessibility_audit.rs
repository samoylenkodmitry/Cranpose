//! The checks a screen reader user fails a screen on, run over a placed
//! semantics tree so a test catches them before a person does.

use std::fmt;

use cranpose_foundation::SemanticsWidgetRole;

use crate::placed_semantics::PlacedSemanticsNode;

/// The smallest target WCAG 2.5.8 accepts, in points. Material asks for 48
/// and Apple for 44; `Modifier::minimum_interactive_component_size()` gives
/// a small control the 48.
pub const MINIMUM_TARGET_SIZE: f32 = 24.0;

/// One kind of thing a reader user trips over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccessibilityIssueKind {
    /// A control a reader can press or type into has nothing to say.
    NoName,
    /// Two controls of one role say the same name, so a reader user cannot
    /// tell them apart.
    SameName,
    /// A control's target is under [`MINIMUM_TARGET_SIZE`] on a side.
    SmallTarget,
    /// A control sits fully above the one a reader visits before it, with
    /// no `traversal_index` to say so on purpose.
    OutOfOrder,
    /// No node names the screen, so a reader says nothing on arrival.
    NoPaneTitle,
    /// A picture a reader stops on with no words for it.
    UnnamedImage,
}

impl AccessibilityIssueKind {
    /// What fixes the issue.
    pub fn advice(self) -> &'static str {
        match self {
            Self::NoName => "give it Modifier::content_description(...), or put a Text inside it",
            Self::SameName => {
                "say what each one acts on: \"Delete Milk\" and \"Delete Bread\", not \"Delete\" twice"
            }
            Self::SmallTarget => {
                "add Modifier::minimum_interactive_component_size(), or make the control larger"
            }
            Self::OutOfOrder => {
                "lay the controls out in reading order, or set Modifier::traversal_index(...)"
            }
            Self::NoPaneTitle => "put Modifier::pane_title(\"...\") on the screen's root",
            Self::UnnamedImage => {
                "give the image a content description, or pass None so a reader skips it"
            }
        }
    }
}

/// One thing a reader user trips over on a screen.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibilityIssue {
    pub kind: AccessibilityIssueKind,
    /// The control the issue is about: its role and its spoken name.
    pub control: String,
    /// What is wrong with it, when the kind alone does not say.
    pub detail: String,
}

impl fmt::Display for AccessibilityIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}{}", self.kind, self.control, self.detail)
    }
}

/// Every issue on the screen, in reading order.
pub fn audit_accessibility(root: &PlacedSemanticsNode) -> Vec<AccessibilityIssue> {
    let mut visible = Vec::new();
    collect_visible(root, &mut visible);

    let mut issues = Vec::new();
    for node in &visible {
        let name = spoken_name(node);
        if is_control(node) {
            check_control(node, &name, &mut issues);
        } else if node.widget_role == Some(SemanticsWidgetRole::Image) && name.is_empty() {
            issues.push(issue(
                AccessibilityIssueKind::UnnamedImage,
                node,
                &name,
                &where_it_is(node),
            ));
        }
        check_order(node, &mut issues);
    }
    check_same_names(&visible, &mut issues);
    if !visible.iter().any(|node| node.pane_title.is_some()) {
        issues.push(AccessibilityIssue {
            kind: AccessibilityIssueKind::NoPaneTitle,
            control: "the screen".to_string(),
            detail: String::new(),
        });
    }
    issues
}

/// Fails the test with every issue on the screen and what fixes each kind.
pub fn assert_accessible(root: &PlacedSemanticsNode) {
    let issues = audit_accessibility(root);
    if issues.is_empty() {
        return;
    }
    let mut report = format!("{} accessibility issue(s):\n", issues.len());
    for issue in &issues {
        report.push_str(&format!("  {issue}\n"));
    }
    let mut kinds: Vec<AccessibilityIssueKind> = issues.iter().map(|issue| issue.kind).collect();
    kinds.dedup();
    kinds.sort_by_key(|kind| *kind as u8);
    kinds.dedup();
    for kind in kinds {
        report.push_str(&format!("  {kind:?}: {}\n", kind.advice()));
    }
    panic!("{report}");
}

/// The name a reader speaks for a node: its own label, else the title it
/// gives the screen, else the words under it.
pub fn spoken_name(node: &PlacedSemanticsNode) -> String {
    if let Some(label) = node
        .label
        .as_deref()
        .filter(|label| !label.trim().is_empty())
    {
        return label.trim().to_string();
    }
    if let Some(title) = node
        .pane_title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
    {
        return title.trim().to_string();
    }
    let mut words = Vec::new();
    for child in &node.children {
        if child.hidden {
            continue;
        }
        let name = spoken_name(child);
        if !name.is_empty() {
            words.push(name);
        }
    }
    words.join(" ")
}

fn collect_visible<'a>(node: &'a PlacedSemanticsNode, out: &mut Vec<&'a PlacedSemanticsNode>) {
    if node.hidden {
        return;
    }
    out.push(node);
    for child in &node.children {
        collect_visible(child, out);
    }
}

fn is_control(node: &PlacedSemanticsNode) -> bool {
    node.clickable || node.editable_text || node.focusable
}

fn holds_control(node: &PlacedSemanticsNode) -> bool {
    !node.hidden && (is_control(node) || node.children.iter().any(holds_control))
}

fn check_control(node: &PlacedSemanticsNode, name: &str, issues: &mut Vec<AccessibilityIssue>) {
    if name.is_empty() {
        issues.push(issue(
            AccessibilityIssueKind::NoName,
            node,
            name,
            &where_it_is(node),
        ));
    }
    let target = node.target_bounds();
    if target.width > 0.0
        && target.height > 0.0
        && (target.width < MINIMUM_TARGET_SIZE || target.height < MINIMUM_TARGET_SIZE)
    {
        let detail = format!(
            " is {}x{} points",
            target.width.round(),
            target.height.round()
        );
        issues.push(issue(
            AccessibilityIssueKind::SmallTarget,
            node,
            name,
            &detail,
        ));
    }
}

fn check_order(parent: &PlacedSemanticsNode, issues: &mut Vec<AccessibilityIssue>) {
    let in_order: Vec<&PlacedSemanticsNode> = parent
        .children
        .iter()
        .filter(|child| {
            holds_control(child)
                && child.traversal_index == 0.0
                && child.layout_bounds.width > 0.0
                && child.layout_bounds.height > 0.0
        })
        .collect();
    for pair in in_order.windows(2) {
        let (before, after) = (pair[0], pair[1]);
        let after_bottom = after.layout_bounds.y + after.layout_bounds.height;
        if after_bottom <= before.layout_bounds.y {
            let detail = format!(" sits above {:?}", spoken_name(before));
            issues.push(issue(
                AccessibilityIssueKind::OutOfOrder,
                after,
                &spoken_name(after),
                &detail,
            ));
        }
    }
}

fn check_same_names(visible: &[&PlacedSemanticsNode], issues: &mut Vec<AccessibilityIssue>) {
    let mut seen: Vec<(String, Option<SemanticsWidgetRole>, usize)> = Vec::new();
    for node in visible.iter().filter(|node| is_control(node)) {
        let name = spoken_name(node);
        if name.is_empty() {
            continue;
        }
        match seen
            .iter_mut()
            .find(|(seen_name, role, _)| *seen_name == name && *role == node.widget_role)
        {
            Some(entry) => entry.2 += 1,
            None => seen.push((name, node.widget_role, 1)),
        }
    }
    for (name, role, count) in seen.into_iter().filter(|(_, _, count)| *count > 1) {
        issues.push(AccessibilityIssue {
            kind: AccessibilityIssueKind::SameName,
            control: format!("{} {name:?}", role_word(role)),
            detail: format!(" appears {count} times"),
        });
    }
}

fn issue(
    kind: AccessibilityIssueKind,
    node: &PlacedSemanticsNode,
    name: &str,
    detail: &str,
) -> AccessibilityIssue {
    AccessibilityIssue {
        kind,
        control: format!("{} {name:?}", role_word(node.widget_role)),
        detail: detail.to_string(),
    }
}

fn where_it_is(node: &PlacedSemanticsNode) -> String {
    let bounds = node.layout_bounds;
    format!(
        " is {}x{} points at ({}, {})",
        bounds.width.round(),
        bounds.height.round(),
        bounds.x.round(),
        bounds.y.round()
    )
}

fn role_word(role: Option<SemanticsWidgetRole>) -> String {
    match role {
        Some(role) => format!("{role:?}").to_lowercase(),
        None => "control".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use cranpose_ui::{Rect, SemanticsRole};

    use super::*;

    fn node(label: Option<&str>, rect: Rect) -> PlacedSemanticsNode {
        PlacedSemanticsNode {
            node_id: 0,
            role: SemanticsRole::Unknown,
            widget_role: None,
            label: label.map(str::to_string),
            state_description: None,
            clickable: false,
            interactive: false,
            toggled: None,
            selected: None,
            enabled: true,
            editable_text: false,
            focusable: false,
            hidden: false,
            pane_title: None,
            traversal_index: 0.0,
            layout_bounds: rect,
            touch_bounds: None,
            children: Vec::new(),
        }
    }

    fn button(label: Option<&str>, rect: Rect) -> PlacedSemanticsNode {
        PlacedSemanticsNode {
            widget_role: Some(SemanticsWidgetRole::Button),
            clickable: true,
            focusable: true,
            ..node(label, rect)
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn screen(children: Vec<PlacedSemanticsNode>) -> PlacedSemanticsNode {
        PlacedSemanticsNode {
            pane_title: Some("Library".to_string()),
            children,
            ..node(None, rect(0.0, 0.0, 400.0, 800.0))
        }
    }

    fn kinds(root: &PlacedSemanticsNode) -> Vec<AccessibilityIssueKind> {
        audit_accessibility(root)
            .into_iter()
            .map(|issue| issue.kind)
            .collect()
    }

    #[test]
    fn a_named_screen_with_named_large_controls_in_order_passes() {
        let root = screen(vec![
            button(Some("Send"), rect(0.0, 0.0, 120.0, 48.0)),
            button(Some("Cancel"), rect(0.0, 60.0, 120.0, 48.0)),
        ]);
        assert!(audit_accessibility(&root).is_empty());
        assert_accessible(&root);
    }

    #[test]
    fn a_control_with_nothing_to_say_is_named_as_such() {
        let root = screen(vec![button(None, rect(0.0, 0.0, 48.0, 48.0))]);
        let issues = audit_accessibility(&root);
        assert_eq!(issues.len(), 1);
        assert_eq!(
            issues[0].to_string(),
            "NoName: button \"\" is 48x48 points at (0, 0)"
        );
    }

    #[test]
    fn the_words_inside_a_control_are_its_name() {
        let mut send = button(None, rect(0.0, 0.0, 120.0, 48.0));
        send.children.push(PlacedSemanticsNode {
            role: SemanticsRole::Text {
                value: "Send".to_string(),
            },
            ..node(Some("Send"), rect(8.0, 8.0, 40.0, 20.0))
        });
        assert!(audit_accessibility(&screen(vec![send])).is_empty());
    }

    #[test]
    fn two_controls_with_one_name_and_one_role_are_reported_once() {
        let root = screen(vec![
            button(Some("Delete"), rect(0.0, 0.0, 120.0, 48.0)),
            button(Some("Delete"), rect(0.0, 60.0, 120.0, 48.0)),
        ]);
        let issues = audit_accessibility(&root);
        assert_eq!(issues.len(), 1);
        assert_eq!(
            issues[0].to_string(),
            "SameName: button \"Delete\" appears 2 times"
        );
    }

    #[test]
    fn a_small_target_says_its_size() {
        let root = screen(vec![button(Some("Close"), rect(0.0, 0.0, 16.0, 16.0))]);
        let issues = audit_accessibility(&root);
        assert_eq!(
            issues[0].to_string(),
            "SmallTarget: button \"Close\" is 16x16 points"
        );
    }

    #[test]
    fn a_touch_target_counts_over_the_drawn_size() {
        let mut close = button(Some("Close"), rect(0.0, 0.0, 16.0, 16.0));
        close.touch_bounds = Some(rect(-16.0, -16.0, 48.0, 48.0));
        assert!(audit_accessibility(&screen(vec![close])).is_empty());
    }

    #[test]
    fn a_control_placed_above_the_one_before_it_is_out_of_order() {
        let root = screen(vec![
            button(Some("Second"), rect(0.0, 100.0, 120.0, 48.0)),
            button(Some("First"), rect(0.0, 0.0, 120.0, 48.0)),
        ]);
        let issues = audit_accessibility(&root);
        assert_eq!(kinds(&root), vec![AccessibilityIssueKind::OutOfOrder]);
        assert_eq!(
            issues[0].to_string(),
            "OutOfOrder: button \"First\" sits above \"Second\""
        );
        let mut on_purpose = root.clone();
        on_purpose.children[1].traversal_index = -1.0;
        assert!(audit_accessibility(&on_purpose).is_empty());
    }

    #[test]
    fn a_screen_without_a_title_and_a_picture_without_words_are_reported() {
        let mut root = node(None, rect(0.0, 0.0, 400.0, 800.0));
        root.children.push(PlacedSemanticsNode {
            widget_role: Some(SemanticsWidgetRole::Image),
            ..node(None, rect(0.0, 0.0, 64.0, 64.0))
        });
        assert_eq!(
            kinds(&root),
            vec![
                AccessibilityIssueKind::UnnamedImage,
                AccessibilityIssueKind::NoPaneTitle
            ]
        );
    }

    #[test]
    fn a_hidden_control_is_left_alone() {
        let root = screen(vec![PlacedSemanticsNode {
            hidden: true,
            ..button(None, rect(0.0, 0.0, 10.0, 10.0))
        }]);
        assert!(audit_accessibility(&root).is_empty());
    }

    #[test]
    #[should_panic(expected = "NoName: button \"\"")]
    fn the_assertion_names_every_issue_and_the_fix() {
        assert_accessible(&screen(vec![button(None, rect(0.0, 0.0, 48.0, 48.0))]));
    }
}
