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
        list_position: None,
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
fn an_independent_child_cannot_name_its_parent() {
    let mut parent = button(None, rect(0.0, 0.0, 120.0, 48.0));
    parent
        .children
        .push(button(Some("Delete"), rect(0.0, 0.0, 48.0, 48.0)));
    assert_eq!(spoken_name(&parent), "");
    assert!(kinds(&screen(vec![parent])).contains(&AccessibilityIssueKind::NoName));
}

#[test]
fn an_explicit_empty_name_is_not_replaced_with_descendant_words() {
    let mut parent = button(Some(""), rect(0.0, 0.0, 120.0, 48.0));
    parent
        .children
        .push(node(Some("Independent action"), rect(0.0, 0.0, 48.0, 48.0)));
    assert_eq!(spoken_name(&parent), "");
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

#[test]
fn a_new_issue_and_a_listed_issue_that_went_away_both_fail_the_comparison() {
    let known: &[KnownIssue] = &[
        (
            "library",
            "SmallTarget: control \"Pill\"",
            "sits over a tab",
        ),
        (
            "*",
            "SameName: tab \"Apps\"",
            "two bars show one set of tabs",
        ),
    ];
    let issues = vec![
        "SmallTarget: control \"Pill\" is 79x17 points".to_string(),
        "SameName: tab \"Apps\" appears 2 times".to_string(),
    ];
    assert_eq!(audit_changes("library", &issues, known), Ok(()));
    assert_eq!(
        audit_changes("settings", &issues[1..], known),
        Ok(()),
        "a * entry matches on every screen"
    );
    let with_new = [
        issues.clone(),
        vec!["NoName: control \"\" is 40x40 points at (1, 2)".to_string()],
    ]
    .concat();
    let report = audit_changes("library", &with_new, known).unwrap_err();
    assert!(report.contains("new accessibility issues on library"));
    assert!(report.contains("NoName"));
    let report = audit_changes("library", &issues[1..], known).unwrap_err();
    assert!(report.contains("went away"));
    assert!(report.contains("SmallTarget"));
    assert_eq!(
        audit_changes("settings", &[], known),
        Ok(()),
        "a * entry is never reported as gone"
    );
}

#[test]
fn rows_of_a_list_with_one_name_are_apart_when_each_has_its_place() {
    let mut list = screen(vec![
        button(Some("Scan"), rect(0.0, 0.0, 200.0, 40.0)),
        button(Some("Scan"), rect(0.0, 50.0, 200.0, 40.0)),
    ]);
    let same_names = |root: &PlacedSemanticsNode| {
        audit_accessibility(root)
            .iter()
            .any(|issue| issue.kind == AccessibilityIssueKind::SameName)
    };
    assert!(same_names(&list), "two buttons with one name and no place");
    for (row, position) in list.children.iter_mut().zip(1..) {
        row.list_position = Some(position);
    }
    assert!(!same_names(&list), "the same two as rows 1 and 2 of a list");
}

#[test]
fn a_row_wrapped_by_its_list_still_gets_its_place() {
    let wrapped = |y: f32| PlacedSemanticsNode {
        children: vec![button(Some("Scan"), rect(0.0, y, 200.0, 40.0))],
        ..node(None, rect(0.0, y, 200.0, 40.0))
    };
    let mut list = screen(vec![wrapped(0.0), wrapped(50.0)]);
    for (row, position) in list.children.iter_mut().zip(1..) {
        crate::placed_semantics::place_row(row, position);
    }
    let issues = audit_accessibility(&list);
    assert!(
        !issues
            .iter()
            .any(|issue| issue.kind == AccessibilityIssueKind::SameName),
        "the buttons under the wrappers carry places 1 and 2: {issues:?}"
    );
}

#[test]
fn a_control_inside_a_row_carries_the_place_of_the_row() {
    let row = |y: f32| PlacedSemanticsNode {
        children: vec![
            button(Some("page thumbnail"), rect(0.0, y, 40.0, 40.0)),
            button(Some("More"), rect(160.0, y, 40.0, 40.0)),
        ],
        ..node(None, rect(0.0, y, 200.0, 40.0))
    };
    let mut list = screen(vec![row(0.0), row(50.0)]);
    for (row, position) in list.children.iter_mut().zip(1..) {
        crate::placed_semantics::place_row(row, position);
    }
    let issues = audit_accessibility(&list);
    assert!(
        !issues
            .iter()
            .any(|issue| issue.kind == AccessibilityIssueKind::SameName),
        "the thumbnails and the More buttons sit in rows 1 and 2: {issues:?}"
    );
}
