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
    if let Some(label) = node.label.as_deref() {
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
        if child.hidden || is_control(child) {
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

/// Two controls of one role with one name. Rows of a list at different
/// places are apart: a reader speaks their place.
fn check_same_names(visible: &[&PlacedSemanticsNode], issues: &mut Vec<AccessibilityIssue>) {
    let mut seen: Vec<(String, Option<SemanticsWidgetRole>, Option<usize>, usize)> = Vec::new();
    for node in visible.iter().filter(|node| is_control(node)) {
        let name = spoken_name(node);
        if name.is_empty() {
            continue;
        }
        match seen.iter_mut().find(|(seen_name, role, position, _)| {
            *seen_name == name && *role == node.widget_role && *position == node.list_position
        }) {
            Some(entry) => entry.3 += 1,
            None => seen.push((name, node.widget_role, node.list_position, 1)),
        }
    }
    for (name, role, _, count) in seen.into_iter().filter(|(_, _, _, count)| *count > 1) {
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

/// An issue a test leaves as it is: the screen it is on, `*` for every
/// screen; the start of the issue line; the reason it stays.
pub type KnownIssue = (&'static str, &'static str, &'static str);

/// Compares the issues found on `screen` with the list a test keeps and
/// reports what changed: the lines that are new, and the listed issues that
/// went away. A test fails on either, so the list only shrinks. An issue
/// matches a listed one when its line starts with the listed text; a `*`
/// entry matches on every screen and is never reported as gone.
pub fn audit_changes(screen: &str, issues: &[String], known: &[KnownIssue]) -> Result<(), String> {
    let listed = |issue: &str| {
        known
            .iter()
            .any(|(on, text, _)| (*on == "*" || *on == screen) && issue.starts_with(text))
    };
    let new: Vec<&str> = issues
        .iter()
        .map(String::as_str)
        .filter(|issue| !listed(issue))
        .collect();
    let gone: Vec<&str> = known
        .iter()
        .filter(|(on, text, _)| {
            *on == screen && !issues.iter().any(|issue| issue.starts_with(text))
        })
        .map(|(_, text, _)| *text)
        .collect();
    if new.is_empty() && gone.is_empty() {
        return Ok(());
    }
    let mut report = String::new();
    if !new.is_empty() {
        report.push_str(&format!(
            "new accessibility issues on {screen}:\n  {}\n",
            new.join("\n  ")
        ));
    }
    if !gone.is_empty() {
        report.push_str(&format!(
            "issues listed for {screen} went away; take them off the list:\n  {}\n",
            gone.join("\n  ")
        ));
    }
    Err(report)
}

#[cfg(test)]
#[path = "tests/accessibility_audit.rs"]
mod tests;
