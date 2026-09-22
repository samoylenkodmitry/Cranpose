use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_ui::{
    CanvasSemanticsNode, SemanticsAction, SemanticsCallback, SemanticsCustomAction, SemanticsNode,
    SemanticsRole, SemanticsWidgetRole,
};

use super::*;

#[test]
fn voiceover_content_updates_preserve_navigation_order() {
    let current = AccessibilityElement {
        node_id: 7,
        label: "Receipt".into(),
        value: Some("Preparing".into()),
        role: AccessibilityRole::Button,
        clickable: true,
        ..Default::default()
    };
    let mut next = current.clone();
    next.bounds.x = 24.0;
    assert!(voiceover_same_structure(
        std::slice::from_ref(&current),
        std::slice::from_ref(&next)
    ));
    next.label = "Receipt ready".into();
    next.value = Some("One page".into());
    next.selected = Some(true);
    assert!(voiceover_same_structure(&[current], &[next]));
}

#[test]
fn voiceover_reorders_only_when_native_elements_change() {
    let first = element_with(1, None);
    let second = element_with(2, None);
    assert!(!voiceover_same_structure(
        &[first.clone(), second.clone()],
        &[second, first.clone()]
    ));
    assert!(!voiceover_same_structure(std::slice::from_ref(&first), &[]));
    let mut canvas = first.clone();
    canvas.canvas_key = Some(3);
    assert!(!voiceover_same_structure(
        std::slice::from_ref(&first),
        &[canvas]
    ));
    let mut text = first.clone();
    text.role = AccessibilityRole::TextField;
    assert!(!voiceover_same_structure(&[first], &[text.clone()]));
    let mut focused = text.clone();
    focused.focused = true;
    focused.text_selection = Some((0, 0));
    assert!(!voiceover_same_structure(&[text], &[focused]));
}

#[test]
fn colliding_canvas_ids_survive_reordering_and_removal() {
    let first = element_with(42, Some(0));
    let second = element_with(42, Some(1 << 31));
    let mut snapshot = AccessibilitySnapshot::default();
    snapshot
        .update(vec![first.clone(), second.clone()])
        .expect("unique elements");
    let original = snapshot.ids.clone();
    snapshot
        .update(vec![second.clone(), first])
        .expect("unique elements");
    let reordered = snapshot.ids.clone();
    assert_eq!(original[0], reordered[1]);
    assert_eq!(original[1], reordered[0]);
    snapshot.update(vec![second]).expect("unique element");
    assert_eq!(snapshot.ids, vec![original[1]]);
}

#[test]
fn a_pane_title_preserves_dialog_and_control_roles_on_the_web() {
    for (role, expected) in [
        (AccessibilityRole::Dialog, "dialog"),
        (AccessibilityRole::Alert, "alert"),
        (AccessibilityRole::Button, "button"),
        (AccessibilityRole::Toolbar, "toolbar"),
        (AccessibilityRole::StaticText, "region"),
    ] {
        let element = AccessibilityElement {
            role,
            pane_title: Some("Preferences".into()),
            ..Default::default()
        };
        assert_eq!(web_role(&element), expected);
    }
}

#[test]
fn opening_a_dialog_reports_its_identity_once() {
    let mut dialog = element_with(7, None);
    dialog.role = AccessibilityRole::Dialog;
    assert_eq!(opened_dialog(&[], &[dialog.clone()]), Some(7));
    assert_eq!(opened_dialog(&[dialog.clone()], &[dialog]), None);
    assert_eq!(opened_dialog(&[], &[element_with(8, None)]), None);
}

#[test]
fn a_named_adjustable_range_preserves_its_web_control_role() {
    let mut element = AccessibilityElement {
        pane_title: Some("Volume".into()),
        progress: Some(cranpose_ui::ProgressBarRangeInfo::new(40.0, 0.0, 100.0, 0)),
        adjustable: true,
        ..Default::default()
    };
    assert_eq!(web_role(&element), "slider");
    element.role = AccessibilityRole::ValuePicker;
    assert_eq!(web_role(&element), "spinbutton");
}

#[test]
fn voiceover_values_include_control_state_without_repeating_the_label() {
    let mut element = AccessibilityElement {
        role: AccessibilityRole::Switch,
        label: "Notifications".into(),
        value: Some("Notifications".into()),
        toggled: Some(true),
        error: Some("Network unavailable".into()),
        ..Default::default()
    };
    assert_eq!(
        voiceover_value(&element).as_deref(),
        Some("on, invalid, Network unavailable")
    );
    element.toggled = Some(false);
    assert_eq!(
        voiceover_value(&element).as_deref(),
        Some("off, invalid, Network unavailable")
    );
    element.state_description = Some("Paused".into());
    assert_eq!(
        voiceover_value(&element).as_deref(),
        Some("Paused, invalid, Network unavailable")
    );
}

#[test]
fn voiceover_values_keep_text_state_and_collection_position_together() {
    let element = AccessibilityElement {
        role: AccessibilityRole::TextField,
        label: "Name".into(),
        value: Some("Ada".into()),
        state_description: Some("Required".into()),
        collection_item: Some(CollectionItem {
            position: 2,
            count: 3,
            horizontal: false,
        }),
        ..Default::default()
    };
    assert_eq!(
        voiceover_value(&element).as_deref(),
        Some("Ada, Required, 2 of 3")
    );
}

#[test]
fn voiceover_passwords_never_read_values_and_radios_read_checked_state() {
    let mut element = AccessibilityElement {
        role: AccessibilityRole::RadioButton,
        selected: Some(true),
        ..Default::default()
    };
    assert_eq!(voiceover_value(&element).as_deref(), Some("checked"));
    element.selected = Some(false);
    assert_eq!(voiceover_value(&element).as_deref(), Some("not checked"));
    element = AccessibilityElement {
        role: AccessibilityRole::TextField,
        password: true,
        value: Some("private".into()),
        ..Default::default()
    };
    assert_eq!(voiceover_value(&element).as_deref(), Some("password"));
}

#[test]
fn reader_focus_rejects_missing_hidden_disabled_and_nonfocusable_targets() {
    let mut root = SemanticsNode {
        node_id: 1,
        ..Default::default()
    };
    assert!(!focus_node(&root, 2));
    assert!(!focus_node(&root, 1));
    root.focusable = true;
    root.hidden = true;
    assert!(!focus_node(&root, 1));
    root.hidden = false;
    root.enabled = false;
    assert!(!focus_node(&root, 1));
}

#[test]
fn inspector_uses_sanitized_projection_and_reports_actions_and_state() {
    let mut button = element_with(42, Some(7));
    button.role = AccessibilityRole::Button;
    button.label = "Account, Account".into();
    button.enabled = true;
    button.focused = true;
    button.clickable = true;
    button.custom_actions = vec!["Archive".into()];
    let inspected = inspector_node(button);
    assert_eq!(inspected.node_id, 42);
    assert_eq!(inspected.canvas_key, Some(7));
    assert!(inspected.focused);
    assert!(!inspected.issue);
    assert!(inspected.label.contains("Account, Account"));
    assert!(inspected.details.contains("Activate, Archive"));
    assert!(inspected.details.contains("Enabled: true  Focused: true"));
    let mut password = element_with(43, None);
    password.password = true;
    password.value = Some("private password".into());
    let inspected = inspector_node(password);
    assert!(inspected.details.contains("[protected]"));
    assert!(!inspected.details.contains("private password"));
    let mut unnamed = element_with(44, None);
    unnamed.label.clear();
    unnamed.clickable = true;
    assert!(inspector_node(unnamed).issue);
}

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
        ..SemanticsNode::default()
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> cranpose_ui::Rect {
    cranpose_ui::Rect {
        x,
        y,
        width,
        height,
    }
}

fn project_test_tree(root: &SemanticsNode) -> Vec<AccessibilityElement> {
    fn bounds_for(node: &SemanticsNode, bounds: &mut HashMap<NodeId, AccessibilityRect>) {
        bounds.insert(node.node_id, AccessibilityRect::new(0.0, 0.0, 300.0, 48.0));
        for child in &node.children {
            bounds_for(child, bounds);
        }
    }
    let mut bounds = HashMap::default();
    bounds_for(root, &mut bounds);
    project_semantics(root, &bounds)
}

fn merged_test_row(children: Vec<SemanticsNode>) -> SemanticsNode {
    SemanticsNode {
        node_id: 1,
        merge_descendants: true,
        children,
        ..SemanticsNode::default()
    }
}

#[test]
fn merged_names_exclude_independent_controls_and_passwords() {
    let mut button = text_node(3, "Delete");
    button.actions.push(SemanticsAction::Click {
        handler: SemanticsCallback::new(3),
    });
    let mut password = text_node(4, "secret passphrase");
    password.editable_text = true;
    password.password = true;
    password.text = Some("secret passphrase".into());
    let tree = merged_test_row(vec![text_node(2, "Account"), button, password]);
    let projected = project_test_tree(&tree);
    assert_eq!(
        projected
            .iter()
            .map(|element| element.label.as_str())
            .collect::<Vec<_>>(),
        ["Account", "Delete", "password"]
    );
    assert!(!format!("{projected:?}").contains("secret passphrase"));
}

#[test]
fn a_merge_does_not_swallow_slider_or_custom_action_controls() {
    let mut slider = text_node(3, "Volume");
    slider.set_progress = Some(cranpose_ui::SemanticsSetProgress::new(|_| true));
    let mut action = text_node(4, "Attachment");
    action
        .custom_actions
        .push(SemanticsCustomAction::new("Download", || {}));
    let tree = merged_test_row(vec![text_node(2, "Player"), slider, action]);
    let projected = project_test_tree(&tree);
    assert_eq!(
        projected
            .iter()
            .map(|element| element.label.as_str())
            .collect::<Vec<_>>(),
        ["Player", "Volume", "Attachment"]
    );
}

#[test]
fn merged_names_follow_traversal_order_and_preserve_repeated_words() {
    let mut first = text_node(4, "Very");
    first.traversal_index = -1.0;
    let tree = merged_test_row(vec![
        text_node(2, "very"),
        text_node(3, "good"),
        first,
        text_node(5, "good"),
    ]);
    let projected = project_test_tree(&tree);
    assert_eq!(projected[0].label, "Very, very, good, good");
}

#[test]
fn a_blank_description_does_not_hide_descendant_text() {
    let mut tree = merged_test_row(vec![text_node(2, "Save")]);
    tree.description = Some("  ".into());
    assert_eq!(project_test_tree(&tree)[0].label, "Save");
}

fn action_test_node() -> SemanticsNode {
    SemanticsNode {
        node_id: 2,
        text: Some("abc".into()),
        set_progress: Some(cranpose_ui::SemanticsSetProgress::new(|_| true)),
        set_text: Some(cranpose_ui::SemanticsSetText::new(|_| true)),
        set_selection: Some(cranpose_ui::SemanticsSetSelection::new(|_, _| true)),
        expand: Some(cranpose_ui::SemanticsExpand::new(|| true)),
        collapse: Some(cranpose_ui::SemanticsExpand::new(|| true)),
        dismiss: Some(cranpose_ui::SemanticsDismiss::new(|| true)),
        on_long_click: Some(cranpose_ui::SemanticsLongClick::new(|| true)),
        on_magic_tap: Some(cranpose_ui::SemanticsMagicTap::new(|| true)),
        scroll_by: Some(cranpose_ui::SemanticsScrollBy::new(|_, _| true)),
        scroll_to_index: Some(cranpose_ui::SemanticsScrollToIndex::new(|_| true)),
        custom_actions: vec![SemanticsCustomAction::new("Run", || {})],
        ..SemanticsNode::default()
    }
}

fn assert_reader_actions(root: &SemanticsNode, accepts: bool) {
    let results = [
        set_progress(root, 2, 0.5),
        set_text(root, 2, "new"),
        set_text_selection(root, 2, 0, 1),
        set_text_selection_utf16(root, 2, 0, 1),
        set_text_selection_chars(root, 2, 0, 1),
        set_expanded(root, 2, true),
        set_expanded(root, 2, false),
        dismiss(root, 2),
        long_click(root, 2),
        magic_tap(root, 2),
        scroll_by(root, 2, 0.0, 10.0),
        scroll_to_index(root, 2, 1),
        perform_custom_action(root, 2, None, 0),
        perform_custom_action(root, 2, None, 1),
        perform_custom_action(root, 2, None, 2),
        perform_listed_action(root, 2, None, 3, 3),
    ];
    assert!(
        results.iter().all(|result| *result == accepts),
        "expected {accepts}: {results:?}"
    );
}

#[test]
fn reader_actions_reject_disabled_and_hidden_targets() {
    let mut control = action_test_node();
    assert_reader_actions(&control, true);
    control.enabled = false;
    assert_reader_actions(&control, false);
    control.enabled = true;
    control.hidden = true;
    assert_reader_actions(&control, false);
}

#[test]
fn reader_actions_cannot_reach_a_hidden_subtree() {
    let mut root = merged_test_row(vec![action_test_node()]);
    assert_reader_actions(&root, true);
    root.hidden = true;
    assert_reader_actions(&root, false);
}

#[test]
fn disabled_canvas_actions_do_not_invoke_the_callback() {
    let calls = Rc::new(Cell::new(0));
    let count = Rc::clone(&calls);
    let mut root = action_test_node();
    root.canvas_children.push(
        CanvasSemanticsNode::control(9, rect(0.0, 0.0, 48.0, 48.0), "Play").with_custom_action(
            SemanticsCustomAction::new("Pause", move || count.set(count.get() + 1)),
        ),
    );
    assert!(perform_custom_action(&root, 2, Some(9), 0));
    root.canvas_children[0].enabled = false;
    assert!(!perform_custom_action(&root, 2, Some(9), 0));
    assert_eq!(calls.get(), 1);
}

#[test]
fn drawn_controls_get_distinct_ids_that_do_not_move_with_list_position() {
    let rows: Vec<_> = (0..24).map(|key| element_with(7, Some(key))).collect();
    let mut snapshot = AccessibilitySnapshot::default();
    snapshot.update(rows.clone()).expect("unique rows");
    let ids = snapshot.ids.clone();

    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "ids collided: {ids:?}");
    assert!(ids.iter().all(|id| *id > 0));

    snapshot.update(rows[1..].to_vec()).expect("unique rows");
    assert_eq!(snapshot.ids, ids[1..]);
    snapshot
        .update(vec![element_with(7, None)])
        .expect("layout node");
    assert!(!ids.contains(&snapshot.ids[0]));
    snapshot
        .update(vec![element_with(7, Some(3)), element_with(8, Some(3))])
        .expect("distinct canvases");
    assert_ne!(snapshot.ids[0], snapshot.ids[1]);
}

#[test]
fn an_element_id_resolves_back_to_the_element_that_published_it() {
    let elements = vec![
        element_with(7, None),
        element_with(7, Some(3)),
        element_with(9, Some(3)),
    ];
    let mut snapshot = AccessibilitySnapshot::default();
    snapshot.update(elements).expect("unique elements");
    assert_eq!(snapshot.identity(snapshot.ids[1]), Some((7, Some(3))));
    assert_eq!(snapshot.identity(snapshot.ids[2]), Some((9, Some(3))));
    assert_eq!(snapshot.identity(snapshot.ids[0]), Some((7, None)));
    assert_eq!(snapshot.identity(-12), None);
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

#[test]
fn drawn_controls_become_elements_positioned_inside_their_canvas() {
    let canvas_id = 7;
    let mut root = node(
        canvas_id,
        SemanticsRole::Layout,
        Vec::new(),
        None,
        Vec::new(),
    );
    root.canvas_children = vec![
        CanvasSemanticsNode::text(1, rect(0.0, 0.0, 200.0, 30.0), "SETTINGS")
            .with_role(SemanticsWidgetRole::Header),
        CanvasSemanticsNode::control(2, rect(0.0, 40.0, 200.0, 52.0), "Haptics")
            .with_role(SemanticsWidgetRole::Switch)
            .with_toggled(true)
            .with_state_description("On"),
        CanvasSemanticsNode::control(3, rect(0.0, 100.0, 200.0, 52.0), "Reset progress")
            .with_click_label("Reset")
            .with_enabled(false),
    ];
    root.node_generation = 9;
    let bounds =
        HashMap::from_iter([(canvas_id, AccessibilityRect::new(20.0, 100.0, 200.0, 300.0))]);

    let projected = project_semantics(&root, &bounds);

    assert_eq!(projected.len(), 3);
    assert!(projected.iter().all(|element| element.node_id == canvas_id));
    assert!(projected.iter().all(|element| element.node_generation == 9));
    assert_eq!(
        projected
            .iter()
            .map(|element| element.canvas_key)
            .collect::<Vec<_>>(),
        vec![Some(1), Some(2), Some(3)]
    );

    assert_eq!(projected[0].role, AccessibilityRole::Header);
    assert!(!projected[0].clickable);

    assert_eq!(projected[1].label, "Haptics");
    assert_eq!(projected[1].role, AccessibilityRole::Switch);
    assert_eq!(projected[1].toggled, Some(true));
    assert_eq!(projected[1].state_description.as_deref(), Some("On"));
    assert!(projected[1].clickable);
    assert_eq!(projected[1].bounds.center(), (120.0, 166.0));

    assert_eq!(projected[2].click_label.as_deref(), Some("Reset"));
    assert!(!projected[2].enabled);
}

fn click(node_id: NodeId) -> SemanticsAction {
    SemanticsAction::Click {
        handler: SemanticsCallback::new(node_id),
    }
}

fn tab(node_id: NodeId, label: &str, picked: bool) -> SemanticsNode {
    let mut tab = node(
        node_id,
        SemanticsRole::Layout,
        vec![click(node_id)],
        Some(label),
        Vec::new(),
    );
    tab.selected = Some(picked);
    tab
}

fn tab_group(node_id: NodeId, tabs: Vec<SemanticsNode>) -> SemanticsNode {
    let mut group = node(node_id, SemanticsRole::Layout, Vec::new(), None, tabs);
    group.selectable_group = true;
    group
}

#[test]
fn tabs_in_a_group_know_their_place() {
    let root = tab_group(
        1,
        vec![
            tab(2, "Home", true),
            tab(3, "Library", false),
            tab(4, "Settings", false),
        ],
    );
    let bounds = HashMap::from_iter([
        (1, AccessibilityRect::new(0.0, 0.0, 300.0, 60.0)),
        (2, AccessibilityRect::new(0.0, 0.0, 100.0, 60.0)),
        (3, AccessibilityRect::new(100.0, 0.0, 100.0, 60.0)),
        (4, AccessibilityRect::new(200.0, 0.0, 100.0, 60.0)),
    ]);

    let projected = project_semantics(&root, &bounds);

    assert_eq!(
        projected[0].collection,
        Some(CollectionInfo {
            rows: 1,
            columns: 3
        })
    );
    let places: Vec<_> = projected[1..]
        .iter()
        .map(|tab| tab.collection_item.expect("a tab knows its place"))
        .collect();
    assert_eq!(
        places,
        vec![
            CollectionItem {
                position: 1,
                count: 3,
                horizontal: true
            },
            CollectionItem {
                position: 2,
                count: 3,
                horizontal: true
            },
            CollectionItem {
                position: 3,
                count: 3,
                horizontal: true
            },
        ]
    );
}

#[test]
fn a_tab_pages_the_list_around_its_group() {
    let mut list = node(
        1,
        SemanticsRole::Layout,
        Vec::new(),
        None,
        vec![tab_group(2, vec![tab(3, "Home", true)])],
    );
    list.vertical_scroll = Some(cranpose_ui::ScrollAxisRange::new(0.0, 900.0, false));
    let bounds = HashMap::from_iter([
        (1, AccessibilityRect::new(0.0, 0.0, 300.0, 600.0)),
        (2, AccessibilityRect::new(0.0, 0.0, 300.0, 60.0)),
        (3, AccessibilityRect::new(0.0, 0.0, 100.0, 60.0)),
    ]);

    let projected = project_semantics(&root_of(list), &bounds);
    let tab = projected.last().expect("the tab is published");

    assert_eq!(tab.scroll_parent, Some(2), "the tab sits under its group");
    assert_eq!(
        scroll_container_for(&projected, tab).map(|list| list.node_id),
        Some(1),
        "a page from the tab reaches the list above the group"
    );
}

fn root_of(node: SemanticsNode) -> SemanticsNode {
    node
}

fn text_node(node_id: NodeId, label: &str) -> SemanticsNode {
    node(
        node_id,
        SemanticsRole::Text {
            value: label.to_owned(),
        },
        Vec::new(),
        None,
        Vec::new(),
    )
}

fn one_row_tree(row: SemanticsNode) -> (SemanticsNode, HashMap<NodeId, AccessibilityRect>) {
    let root = node(1, SemanticsRole::Layout, Vec::new(), None, vec![row]);
    let bounds = HashMap::from_iter([
        (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
        (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
    ]);
    (root, bounds)
}

#[test]
fn a_pane_is_published_with_its_title_and_no_label() {
    let mut screen = node(
        1,
        SemanticsRole::Layout,
        Vec::new(),
        None,
        vec![node(
            2,
            SemanticsRole::Text {
                value: "Milk".into(),
            },
            Vec::new(),
            None,
            Vec::new(),
        )],
    );
    screen.pane_title = Some("Library".into());
    let bounds = HashMap::from_iter([
        (1, AccessibilityRect::new(0.0, 0.0, 300.0, 600.0)),
        (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
    ]);

    let projected = project_semantics(&screen, &bounds);

    assert_eq!(projected.len(), 2, "the pane and its text: {projected:?}");
    assert_eq!(projected[0].label, "");
    assert_eq!(projected[0].pane_title.as_deref(), Some("Library"));
    assert_eq!(projected[1].label, "Milk");
    assert_eq!(
        projected[1].scroll_parent,
        Some(1),
        "pane content belongs to its landmark"
    );
}

#[test]
fn a_merged_row_is_one_stop() {
    let mut row = node(
        2,
        SemanticsRole::Layout,
        Vec::new(),
        None,
        vec![
            node(
                3,
                SemanticsRole::Text {
                    value: "Milk".into(),
                },
                Vec::new(),
                None,
                Vec::new(),
            ),
            node(
                4,
                SemanticsRole::Text { value: "2".into() },
                Vec::new(),
                None,
                Vec::new(),
            ),
            node(
                5,
                SemanticsRole::Text {
                    value: "3.40".into(),
                },
                Vec::new(),
                None,
                Vec::new(),
            ),
        ],
    );
    row.merge_descendants = true;
    let root = node(1, SemanticsRole::Layout, Vec::new(), None, vec![row]);
    let bounds =
        HashMap::from_iter((1..=5).map(|id| (id, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0))));

    let projected = project_semantics(&root, &bounds);

    assert_eq!(projected.len(), 1, "the row is one stop: {projected:?}");
    assert_eq!(projected[0].label, "Milk, 2, 3.40");
    assert_eq!(projected[0].role, AccessibilityRole::StaticText);
    assert!(!projected[0].clickable);
}

#[test]
fn a_hidden_node_and_everything_under_it_stay_out() {
    let mut placeholder = node(
        2,
        SemanticsRole::Layout,
        Vec::new(),
        Some("Item"),
        Vec::new(),
    );
    placeholder.hidden = true;
    let mut root = node(
        1,
        SemanticsRole::Layout,
        Vec::new(),
        None,
        vec![placeholder],
    );
    root.children[0].children.push(node(
        3,
        SemanticsRole::Layout,
        Vec::new(),
        Some("Under it"),
        Vec::new(),
    ));
    let bounds = HashMap::from_iter([
        (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
        (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
        (3, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
    ]);

    let projected = project_semantics(&root, &bounds);

    assert!(
        projected.is_empty(),
        "a hidden node is not published: {projected:?}"
    );
}

fn projected_field(name: Option<&str>, text: &str, password: bool) -> Vec<AccessibilityElement> {
    let mut field = node(2, SemanticsRole::Layout, Vec::new(), name, Vec::new());
    field.editable_text = true;
    field.password = password;
    field.text = Some(text.to_owned());
    let root = node(1, SemanticsRole::Layout, Vec::new(), None, vec![field]);
    let bounds = HashMap::from_iter([
        (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
        (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
    ]);
    project_semantics(&root, &bounds)
}

#[test]
fn an_empty_text_field_is_still_a_stop() {
    let projected = projected_field(Some(""), "", false);

    assert_eq!(
        projected.len(),
        1,
        "the field is published with nothing to read"
    );
    assert_eq!(projected[0].role, AccessibilityRole::TextField);
    assert_eq!(projected[0].label, "");
    assert_eq!(projected[0].value.as_deref(), Some(""));
}

#[test]
fn a_named_text_field_keeps_its_name_and_carries_its_text() {
    let projected = projected_field(Some("Folder name"), "Milk", false);

    assert_eq!(projected[0].label, "Folder name");
    assert_eq!(projected[0].value.as_deref(), Some("Milk"));
}

#[test]
fn drawn_controls_without_a_label_or_a_size_are_not_published() {
    let canvas_id = 4;
    let mut root = node(
        canvas_id,
        SemanticsRole::Layout,
        Vec::new(),
        None,
        Vec::new(),
    );
    root.canvas_children = vec![
        CanvasSemanticsNode::control(1, rect(0.0, 0.0, 100.0, 40.0), "   "),
        CanvasSemanticsNode::control(2, rect(0.0, 40.0, 100.0, 0.0), "Off screen"),
        CanvasSemanticsNode::control(3, rect(0.0, 60.0, 100.0, 40.0), "Visible"),
    ];
    let bounds = HashMap::from_iter([(canvas_id, AccessibilityRect::new(0.0, 0.0, 100.0, 200.0))]);

    let projected = project_semantics(&root, &bounds);

    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].label, "Visible");
}

#[test]
fn a_labelled_canvas_keeps_its_own_element_ahead_of_its_drawn_controls() {
    let canvas_id = 9;
    let mut root = node(
        canvas_id,
        SemanticsRole::Layout,
        vec![SemanticsAction::Click {
            handler: SemanticsCallback::new(canvas_id),
        }],
        Some("Orbit Breaker. CAMPAIGN. Turn the crown to change the choice."),
        Vec::new(),
    );
    root.widget_role = Some(SemanticsWidgetRole::RadioButton);
    root.state_description = Some("CAMPAIGN".into());
    root.on_click_label = Some("CAMPAIGN".into());
    root.selected = Some(true);
    root.canvas_children = vec![
        CanvasSemanticsNode::control(1, rect(60.0, 10.0, 80.0, 40.0), "CAMPAIGN")
            .with_role(SemanticsWidgetRole::RadioButton)
            .with_selected(true),
        CanvasSemanticsNode::control(2, rect(60.0, 150.0, 80.0, 40.0), "DAILY")
            .with_role(SemanticsWidgetRole::RadioButton)
            .with_selected(false),
    ];
    let bounds = HashMap::from_iter([(canvas_id, AccessibilityRect::new(0.0, 0.0, 200.0, 200.0))]);

    let projected = project_semantics(&root, &bounds);

    assert_eq!(projected.len(), 3);
    assert_eq!(projected[0].canvas_key, None);
    assert_eq!(projected[0].role, AccessibilityRole::RadioButton);
    assert_eq!(projected[0].state_description.as_deref(), Some("CAMPAIGN"));
    assert_eq!(projected[0].click_label.as_deref(), Some("CAMPAIGN"));
    assert_eq!(projected[1].label, "CAMPAIGN");
    assert_eq!(projected[1].selected, Some(true));
    assert_eq!(projected[2].label, "DAILY");
    assert_eq!(projected[2].selected, Some(false));
}

#[test]
fn custom_action_labels_reach_the_platform_in_publication_order() {
    let arena_id = 3;
    let mut root = node(
        arena_id,
        SemanticsRole::Layout,
        Vec::new(),
        Some("Level 4. Score 120."),
        Vec::new(),
    );
    root.custom_actions = vec![
        SemanticsCustomAction::new("Pause", || {}),
        SemanticsCustomAction::new("Restart", || {}),
    ];
    let bounds = HashMap::from_iter([(arena_id, AccessibilityRect::new(0.0, 0.0, 200.0, 200.0))]);

    let projected = project_semantics(&root, &bounds);

    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].custom_actions, vec!["Pause", "Restart"]);
}

#[test]
fn a_custom_action_runs_the_handler_the_tree_currently_holds() {
    let arena_id = 3;
    let canvas_id = 5;
    let fired = Rc::new(RefCell::new(Vec::new()));

    let mut root = node(
        arena_id,
        SemanticsRole::Layout,
        Vec::new(),
        None,
        Vec::new(),
    );
    root.custom_actions = vec![SemanticsCustomAction::new("Pause", {
        let fired = Rc::clone(&fired);
        move || fired.borrow_mut().push("pause")
    })];
    let mut child = node(
        canvas_id,
        SemanticsRole::Layout,
        Vec::new(),
        None,
        Vec::new(),
    );
    child.canvas_children = vec![
        CanvasSemanticsNode::control(42, rect(0.0, 0.0, 40.0, 40.0), "Haptics").with_custom_action(
            SemanticsCustomAction::new("Toggle", {
                let fired = Rc::clone(&fired);
                move || fired.borrow_mut().push("toggle")
            }),
        ),
    ];
    root.children = vec![child];

    assert!(perform_custom_action(&root, arena_id, None, 0));
    assert!(perform_custom_action(&root, canvas_id, Some(42), 0));

    assert!(!perform_custom_action(&root, 999, None, 0));
    assert!(!perform_custom_action(&root, canvas_id, Some(43), 0));
    assert!(!perform_custom_action(&root, arena_id, None, 1));

    assert_eq!(*fired.borrow(), vec!["pause", "toggle"]);
}

#[test]
fn rebuilding_a_custom_action_handler_is_not_a_published_change() {
    let arena_id = 3;
    let bounds = HashMap::from_iter([(arena_id, AccessibilityRect::new(0.0, 0.0, 200.0, 200.0))]);
    let project = |handler: fn()| {
        let mut root = node(
            arena_id,
            SemanticsRole::Layout,
            Vec::new(),
            Some("Level 4."),
            Vec::new(),
        );
        root.custom_actions = vec![SemanticsCustomAction::new("Pause", handler)];
        project_semantics(&root, &bounds)
    };

    assert_eq!(project(|| {}), project(|| panic!("must not run")));
}

fn live_text(node_id: NodeId, text: &str) -> AccessibilityElement {
    AccessibilityElement {
        node_id,
        label: text.into(),
        bounds: AccessibilityRect::new(0.0, 0.0, 10.0, 10.0),
        live_region: Some(cranpose_ui::LiveRegionMode::Polite),
        ..AccessibilityElement::default()
    }
}

#[test]
fn a_live_region_with_new_text_is_read_out() {
    let before = vec![live_text(1, "3 receipts")];
    let after = vec![live_text(1, "4 receipts")];
    let announcements = live_region_announcements(&before, &after);
    assert_eq!(announcements.len(), 1);
    assert_eq!(announcements[0].text, "4 receipts");
}

#[test]
fn a_live_region_that_kept_its_text_stays_quiet() {
    let before = vec![live_text(1, "3 receipts")];
    let after = vec![live_text(1, "3 receipts")];
    assert!(live_region_announcements(&before, &after).is_empty());
}

fn pane(node_id: NodeId, title: &str) -> AccessibilityElement {
    AccessibilityElement {
        node_id,
        bounds: AccessibilityRect::new(0.0, 0.0, 300.0, 600.0),
        pane_title: Some(title.into()),
        ..AccessibilityElement::default()
    }
}

#[test]
fn a_new_pane_title_is_read_out() {
    let before = vec![pane(1, "Library")];
    let after = vec![pane(1, "Receipt")];
    let announcements = pane_title_announcements(&before, &after);
    assert_eq!(announcements.len(), 1);
    assert_eq!(announcements[0].text, "Receipt");
    assert!(
        pane_title_announcements(&after, &after).is_empty(),
        "the same title stays quiet"
    );
}

#[test]
fn the_first_publish_keeps_pane_titles_quiet() {
    assert!(pane_title_announcements(&[], &[pane(1, "Library")]).is_empty());
}

#[test]
fn a_toggle_that_flipped_is_a_spoken_change() {
    let mut before = live_text(1, "Dark theme");
    before.toggled = Some(false);
    let mut after = before.clone();
    after.toggled = Some(true);
    assert_eq!(spoken_changes(&[before], &[after]), vec![true]);
}

#[test]
fn an_element_that_kept_its_words_is_not_a_spoken_change() {
    let before = live_text(1, "Dark theme");
    let after = live_text(1, "Dark theme");
    let fresh = live_text(2, "Fresh");
    assert_eq!(
        spoken_changes(&[before], &[after, fresh]),
        vec![false, false]
    );
}

#[test]
fn a_new_error_is_a_spoken_change() {
    let before = live_text(1, "Amount");
    let mut after = live_text(1, "Amount");
    after.error = Some("needs a number".into());
    assert_eq!(spoken_changes(&[before], &[after.clone()]), vec![true]);
    assert_eq!(
        state_with_error(&after).as_deref(),
        Some("invalid, needs a number")
    );
}

#[test]
fn a_password_field_never_reads_its_text_out() {
    let named = projected_field(Some("Passphrase"), "hunter2", true);
    assert_eq!(named[0].label, "Passphrase");
    assert_eq!(named[0].value, None, "the text stays unspoken");
    assert!(named[0].password);

    let unnamed = projected_field(None, "hunter2", true);
    assert_eq!(unnamed[0].label, "password", "no name reads no secret");
    assert_eq!(unnamed[0].value, None);
}

#[test]
fn a_traversal_index_moves_a_node_in_the_reading_order() {
    let mut search = node(
        2,
        SemanticsRole::Text {
            value: "Search".into(),
        },
        Vec::new(),
        None,
        Vec::new(),
    );
    search.traversal_index = -1.0;
    let title = node(
        3,
        SemanticsRole::Text {
            value: "Receipts".into(),
        },
        Vec::new(),
        None,
        Vec::new(),
    );
    let root = node(
        1,
        SemanticsRole::Layout,
        Vec::new(),
        None,
        vec![title, search],
    );
    let bounds = HashMap::from_iter([
        (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
        (2, AccessibilityRect::new(0.0, 60.0, 300.0, 40.0)),
        (3, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
    ]);

    let projected = project_semantics(&root, &bounds);
    let labels: Vec<&str> = projected
        .iter()
        .map(|element| element.label.as_str())
        .collect();

    assert_eq!(
        labels,
        vec!["Search", "Receipts"],
        "the search field is read first"
    );
}

#[test]
fn a_control_that_opens_reads_as_closed_and_back() {
    let mut row = node(
        2,
        SemanticsRole::Text {
            value: "Details".into(),
        },
        Vec::new(),
        None,
        Vec::new(),
    );
    row.expand = Some(cranpose_ui::SemanticsExpand::new(|| true));
    let root = node(
        1,
        SemanticsRole::Layout,
        Vec::new(),
        None,
        vec![row.clone()],
    );
    let bounds = HashMap::from_iter([
        (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
        (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
    ]);

    let closed = project_semantics(&root, &bounds);
    assert_eq!(
        closed[0].expanded,
        Some(false),
        "a control that opens is closed"
    );
    assert_eq!(expansion_word(&closed[0]), Some("collapsed"));
    assert!(set_expanded(&root, 2, true), "the control takes the ask");
    assert!(!set_expanded(&root, 2, false), "it has no way to close yet");

    let mut open = row;
    open.expand = None;
    open.collapse = Some(cranpose_ui::SemanticsExpand::new(|| true));
    let root = node(1, SemanticsRole::Layout, Vec::new(), None, vec![open]);
    let projected = project_semantics(&root, &bounds);
    assert_eq!(projected[0].expanded, Some(true));
    assert_eq!(expansion_word(&projected[0]), Some("expanded"));
    assert!(set_expanded(&root, 2, false));
}

#[test]
fn a_control_says_what_it_does_when_a_reader_sends_it_away() {
    let ran = Rc::new(Cell::new(0));
    let mut row = text_node(2, "Milk");
    row.dismiss = Some(cranpose_ui::SemanticsDismiss::new({
        let ran = Rc::clone(&ran);
        move || {
            ran.set(ran.get() + 1);
            true
        }
    }));
    let quiet = text_node(3, "Bread");
    let root = node(1, SemanticsRole::Layout, Vec::new(), None, vec![row, quiet]);
    let bounds = HashMap::from_iter([
        (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
        (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
        (3, AccessibilityRect::new(0.0, 40.0, 300.0, 40.0)),
    ]);

    let projected = project_semantics(&root, &bounds);
    assert!(projected[0].dismissable, "the row says it has a way out");
    assert!(!projected[1].dismissable, "the other row says nothing");
    assert_eq!(listed_actions(&projected[0]), vec![DISMISS_LABEL]);
    assert!(listed_actions(&projected[1]).is_empty());

    assert!(dismiss(&root, 2), "the row takes the ask");
    assert_eq!(ran.get(), 1);
    assert!(!dismiss(&root, 3), "the other row has no way out");
}

#[test]
fn the_way_out_sits_after_the_actions_the_app_named() {
    let ran = Rc::new(Cell::new(String::new()));
    let mut row = text_node(2, "Milk");
    row.custom_actions = vec![cranpose_ui::SemanticsCustomAction::new("Pin", {
        let ran = Rc::clone(&ran);
        move || ran.set("Pin".into())
    })];
    row.dismiss = Some(cranpose_ui::SemanticsDismiss::new({
        let ran = Rc::clone(&ran);
        move || {
            ran.set("Dismiss".into());
            true
        }
    }));
    let (root, bounds) = one_row_tree(row);

    let projected = project_semantics(&root, &bounds);
    assert_eq!(listed_actions(&projected[0]), vec!["Pin", DISMISS_LABEL]);

    assert!(perform_listed_action(&root, 2, None, 1, 0));
    assert_eq!(ran.take(), "Pin");
    assert!(perform_listed_action(&root, 2, None, 1, 1));
    assert_eq!(ran.take(), "Dismiss");
}

#[test]
fn a_long_press_is_named_and_sits_after_the_custom_actions() {
    let ran = Rc::new(Cell::new(0));
    let mut row = text_node(2, "Milk");
    row.custom_actions = vec![cranpose_ui::SemanticsCustomAction::new("Pause", || {})];
    row.on_long_click_label = Some("Remove receipt".into());
    row.on_long_click = Some(cranpose_ui::SemanticsLongClick::new({
        let ran = Rc::clone(&ran);
        move || {
            ran.set(ran.get() + 1);
            true
        }
    }));
    let (root, bounds) = one_row_tree(row);

    let projected = project_semantics(&root, &bounds);
    assert_eq!(
        projected[0].long_click_label.as_deref(),
        Some("Remove receipt")
    );
    assert_eq!(
        reader_actions(&projected[0]),
        vec!["Pause".to_owned(), "Remove receipt".to_owned()],
        "the long press is the last action a reader lists"
    );

    assert!(perform_custom_action(&root, 2, None, 1));
    assert_eq!(ran.get(), 1, "the trailing action is the long press");
    assert!(long_click(&root, 2));
    assert_eq!(ran.get(), 2);
    assert!(!perform_custom_action(&root, 2, None, 2));
}

#[test]
fn a_control_with_no_long_press_offers_none_and_a_nameless_one_is_still_read() {
    let mut plain = text_node(2, "Milk");
    let (root, bounds) = one_row_tree(plain.clone());
    assert_eq!(project_semantics(&root, &bounds)[0].long_click_label, None);
    assert!(!long_click(&root, 2), "there is nothing to run");

    plain.on_long_click = Some(cranpose_ui::SemanticsLongClick::new(|| true));
    plain.on_long_click_label = Some("   ".into());
    let (root, bounds) = one_row_tree(plain);
    assert_eq!(
        project_semantics(&root, &bounds)[0]
            .long_click_label
            .as_deref(),
        Some("long press"),
        "a control that takes a long press with no phrase still reads as one"
    );
}

#[test]
fn a_dropdown_and_a_picker_keep_their_own_roles() {
    let mut dropdown = node(
        2,
        SemanticsRole::Text {
            value: "Sort by".into(),
        },
        Vec::new(),
        None,
        Vec::new(),
    );
    dropdown.widget_role = Some(cranpose_ui::SemanticsWidgetRole::DropdownList);
    let mut picker = node(
        3,
        SemanticsRole::Text {
            value: "Copies".into(),
        },
        Vec::new(),
        None,
        Vec::new(),
    );
    picker.widget_role = Some(cranpose_ui::SemanticsWidgetRole::ValuePicker);
    let root = node(
        1,
        SemanticsRole::Layout,
        Vec::new(),
        None,
        vec![dropdown, picker],
    );
    let bounds = HashMap::from_iter([
        (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
        (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
        (3, AccessibilityRect::new(0.0, 40.0, 300.0, 40.0)),
    ]);

    let projected = project_semantics(&root, &bounds);

    assert_eq!(projected[0].role, AccessibilityRole::DropdownList);
    assert_eq!(projected[1].role, AccessibilityRole::ValuePicker);
}

#[test]
fn a_live_region_that_just_appeared_is_read_out() {
    let before = vec![live_text(1, "3 receipts")];
    let after = vec![live_text(1, "3 receipts"), live_text(2, "Import failed")];
    let announcements = live_region_announcements(&before, &after);
    assert_eq!(announcements.len(), 1);
    assert_eq!(announcements[0].text, "Import failed");
}

#[test]
fn the_first_screen_is_not_read_out_as_a_change() {
    assert!(live_region_announcements(&[], &[live_text(1, "3 receipts")]).is_empty());
}

#[test]
fn a_node_without_a_live_region_is_never_read_out_on_a_change() {
    let before = vec![element_with(1, None)];
    let mut after = element_with(1, None);
    after.label = "Row 2".into();
    assert!(live_region_announcements(&before, &[after]).is_empty());
}

#[test]
fn a_live_region_reaches_every_control_under_it() {
    let bounds = HashMap::from_iter([
        (1, AccessibilityRect::new(0.0, 0.0, 200.0, 50.0)),
        (2, AccessibilityRect::new(0.0, 0.0, 200.0, 50.0)),
    ]);
    let mut root = node(
        1,
        SemanticsRole::Layout,
        Vec::new(),
        None,
        vec![node(
            2,
            SemanticsRole::Text {
                value: "2 left".into(),
            },
            Vec::new(),
            None,
            Vec::new(),
        )],
    );
    root.live_region = Some(cranpose_ui::LiveRegionMode::Assertive);
    let elements = project_semantics(&root, &bounds);
    assert_eq!(elements.len(), 1);
    assert_eq!(
        elements[0].live_region,
        Some(cranpose_ui::LiveRegionMode::Assertive)
    );
}

#[test]
fn an_adjustable_control_publishes_its_range_and_takes_a_new_value() {
    let taken = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&taken);
    let bounds = HashMap::from_iter([(7, AccessibilityRect::new(0.0, 0.0, 200.0, 40.0))]);
    let mut root = node(
        7,
        SemanticsRole::Layout,
        Vec::new(),
        Some("Volume"),
        Vec::new(),
    );
    root.progress = Some(cranpose_ui::ProgressBarRangeInfo::new(0.4, 0.0, 1.0, 0));
    root.set_progress = Some(cranpose_ui::SemanticsSetProgress::new(move |value| {
        seen.borrow_mut().push(value);
        true
    }));

    let elements = project_semantics(&root, &bounds);
    assert_eq!(elements.len(), 1);
    let published = elements[0]
        .progress
        .expect("the range reaches the platform");
    assert_eq!(published.current, 0.4);
    assert_eq!(published.end, 1.0);
    assert!(elements[0].adjustable);

    assert!(set_progress(&root, 7, 0.6));
    assert_eq!(*taken.borrow(), vec![0.6]);
    assert!(!set_progress(&root, 99, 0.6), "no such control");
}

#[test]
fn a_reader_hands_a_field_its_text() {
    let taken = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&taken);
    let mut root = node(7, SemanticsRole::Layout, Vec::new(), Some(""), Vec::new());
    root.editable_text = true;
    root.set_text = Some(cranpose_ui::SemanticsSetText::new(move |text| {
        seen.borrow_mut().push(text.to_owned());
        true
    }));

    assert!(set_text(&root, 7, "Milk"));
    assert_eq!(*taken.borrow(), vec!["Milk".to_owned()]);
    assert!(!set_text(&root, 99, "Milk"), "no such field");
}

#[test]
fn every_role_is_named_once_on_every_platform() {
    let once = |count: usize, what: &str, role: AccessibilityRole| {
        assert_eq!(count, 1, "{role:?} should be in the {what} table once");
    };
    for role in AccessibilityRole::ALL {
        let widget = WIDGET_ROLES.iter().filter(|(_, own)| *own == role).count();
        if role != AccessibilityRole::StaticText && role != AccessibilityRole::TextField {
            once(widget, "widget role", role);
        }
        once(
            ARIA_ROLES
                .iter()
                .filter(|(named, _)| *named == role)
                .count(),
            "ARIA",
            role,
        );
        once(
            ANDROID_ROLE_CODES
                .iter()
                .filter(|(named, _)| *named == role)
                .count(),
            "Android",
            role,
        );
    }
    let mut codes: Vec<i32> = ANDROID_ROLE_CODES.iter().map(|(_, code)| *code).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(
        codes.len(),
        ANDROID_ROLE_CODES.len(),
        "every Android code is its own"
    );
    assert_eq!(
        AccessibilityRole::from_widget_role(SemanticsWidgetRole::SearchField),
        AccessibilityRole::SearchField
    );
    assert_eq!(AccessibilityRole::SearchField.aria_name(), "searchbox");
    assert_eq!(AccessibilityRole::ListItem.android_code(), 23);
}

#[test]
fn a_magic_tap_is_listed_after_the_long_press_and_runs_from_the_list() {
    let taps = Rc::new(Cell::new(0));
    let presses = Rc::new(Cell::new(0));
    let mut root = node(
        7,
        SemanticsRole::Layout,
        Vec::new(),
        Some("Shutter"),
        Vec::new(),
    );
    root.custom_actions = vec![SemanticsCustomAction::new("Flash", || {})];
    root.on_long_click_label = Some("Hold to focus".into());
    root.on_long_click = Some(cranpose_ui::SemanticsLongClick::new({
        let presses = Rc::clone(&presses);
        move || {
            presses.set(presses.get() + 1);
            true
        }
    }));
    root.on_magic_tap_label = Some("Take the photo".into());
    root.on_magic_tap = Some(cranpose_ui::SemanticsMagicTap::new({
        let taps = Rc::clone(&taps);
        move || {
            taps.set(taps.get() + 1);
            true
        }
    }));
    let bounds = HashMap::from_iter([(7, AccessibilityRect::new(0.0, 0.0, 80.0, 44.0))]);
    let projected = project_semantics(&root, &bounds);

    assert_eq!(
        reader_actions(&projected[0]),
        vec!["Flash", "Hold to focus", "Take the photo"]
    );
    assert!(perform_custom_action(&root, 7, None, 1));
    assert!(perform_custom_action(&root, 7, None, 2));
    assert!(!perform_custom_action(&root, 7, None, 3));
    assert!(magic_tap(&root, 7));
    assert!(!magic_tap(&root, 99));
    assert_eq!((presses.get(), taps.get()), (1, 2));

    root.on_long_click = None;
    root.on_long_click_label = None;
    let projected = project_semantics(&root, &bounds);
    assert_eq!(
        reader_actions(&projected[0]),
        vec!["Flash", "Take the photo"]
    );
    assert!(perform_custom_action(&root, 7, None, 1));
    assert_eq!(taps.get(), 3);
}

#[test]
fn voice_control_names_and_a_language_reach_the_element() {
    let mut root = node(
        7,
        SemanticsRole::Layout,
        Vec::new(),
        Some("Importieren"),
        Vec::new(),
    );
    root.input_labels = vec!["Import".into()];
    root.language = Some("de".into());
    let bounds = HashMap::from_iter([(7, AccessibilityRect::new(0.0, 0.0, 80.0, 44.0))]);

    let projected = project_semantics(&root, &bounds);

    assert_eq!(projected[0].input_labels, vec!["Import".to_owned()]);
    assert_eq!(projected[0].language.as_deref(), Some("de"));
    assert_eq!(projected[0].magic_tap_label, None);
}

#[test]
fn a_reader_moves_the_caret_of_a_field() {
    let taken = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&taken);
    let mut root = node(7, SemanticsRole::Layout, Vec::new(), Some(""), Vec::new());
    root.editable_text = true;
    root.text = Some("añb😀c".to_owned());
    root.set_selection = Some(cranpose_ui::SemanticsSetSelection::new(
        move |anchor, focus| {
            seen.borrow_mut().push((anchor, focus));
            true
        },
    ));

    assert!(set_text_selection(&root, 7, 1, 3));
    assert!(set_text_selection_utf16(&root, 7, 2, 5));
    assert!(set_text_selection_chars(&root, 7, 4, 2));
    assert_eq!(*taken.borrow(), vec![(1, 3), (3, 8), (8, 3)]);
    assert!(!set_text_selection(&root, 99, 0, 0), "no such field");
}

#[test]
fn text_offsets_convert_between_bytes_utf16_units_and_characters() {
    let text = "añb😀c";
    assert_eq!(utf16_offset(text, 0), 0);
    assert_eq!(utf16_offset(text, 3), 2);
    assert_eq!(utf16_offset(text, 8), 5);
    assert_eq!(
        utf16_offset(text, 2),
        1,
        "inside ñ rounds down to its start"
    );
    assert_eq!(utf16_offset(text, 99), 6);
    assert_eq!(byte_offset_for_utf16(text, 2), 3);
    assert_eq!(
        byte_offset_for_utf16(text, 4),
        8,
        "inside the emoji lands after it"
    );
    assert_eq!(byte_offset_for_utf16(text, 99), 9);
    assert_eq!(char_offset(text, 8), 4);
    assert_eq!(char_offset(text, 99), 5);
    assert_eq!(byte_offset_for_chars(text, 3), 4);
    assert_eq!(byte_offset_for_chars(text, 4), 8);
    assert_eq!(byte_offset_for_chars(text, 99), 9);
}

#[test]
fn an_editable_field_publishes_where_its_caret_is_and_a_password_does_not() {
    for (password, expected) in [(false, Some((1, 3))), (true, None)] {
        let mut field = node(
            2,
            SemanticsRole::Layout,
            Vec::new(),
            Some("Name"),
            Vec::new(),
        );
        field.editable_text = true;
        field.password = password;
        field.multiline = true;
        field.text = Some("Milk".to_owned());
        field.text_selection = Some(cranpose_ui::TextRange::new(1, 3));
        let root = node(1, SemanticsRole::Layout, Vec::new(), None, vec![field]);
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
            (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
        ]);

        let projected = project_semantics(&root, &bounds);

        assert_eq!(projected[0].text_selection, expected);
        assert!(projected[0].multiline);
    }
}

#[test]
fn a_step_moves_one_stop_and_stops_at_the_ends() {
    let ten = cranpose_ui::ProgressBarRangeInfo::new(0.5, 0.0, 1.0, 0);
    assert!((stepped_value(&ten, true) - 0.6).abs() < 1e-6);
    assert!((stepped_value(&ten, false) - 0.4).abs() < 1e-6);

    let top = cranpose_ui::ProgressBarRangeInfo::new(1.0, 0.0, 1.0, 0);
    assert_eq!(stepped_value(&top, true), 1.0, "a step up stays at the end");

    let four_stops = cranpose_ui::ProgressBarRangeInfo::new(0.0, 0.0, 1.0, 4);
    assert!((four_stops.step() - 0.2).abs() < 1e-6);
}

fn scroll_box(node_id: NodeId, scroll_parent: Option<NodeId>, height: f32) -> AccessibilityElement {
    AccessibilityElement {
        node_id,
        bounds: AccessibilityRect::new(0.0, 0.0, 400.0, height),
        vertical_scroll: Some(cranpose_ui::ScrollAxisRange::new(0.0, 900.0, false)),
        scroll_parent,
        ..AccessibilityElement::default()
    }
}

fn row_in(node_id: NodeId, label: &str, scroll_parent: Option<NodeId>) -> AccessibilityElement {
    AccessibilityElement {
        node_id,
        label: label.into(),
        bounds: AccessibilityRect::new(0.0, -300.0, 400.0, 40.0),
        scroll_parent,
        ..AccessibilityElement::default()
    }
}

#[test]
fn a_row_pages_the_list_above_it_even_once_it_left_the_screen() {
    let outer = scroll_box(1, None, 800.0);
    let inner = scroll_box(2, Some(1), 300.0);
    let row = row_in(3, "Milk", Some(2));
    let footer = row_in(4, "Total", Some(1));
    let elements = vec![outer, inner, row.clone(), footer.clone()];

    let around_row = scroll_container_for(&elements, &row).expect("the row sits in a list");
    let around_footer =
        scroll_container_for(&elements, &footer).expect("the footer sits in the outer list");
    let page = page_delta(around_row, true);

    assert_eq!(around_row.node_id, 2, "the list right above the row wins");
    assert_eq!(around_footer.node_id, 1);
    assert_eq!(
        page,
        (0.0, 270.0),
        "one page is nine tenths of the list height"
    );
    assert_eq!(page_delta(around_row, false), (0.0, -270.0));
}

#[test]
fn a_list_publishes_its_rows_and_takes_a_row_number() {
    let taken = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&taken);
    let bounds = HashMap::from_iter([(7, AccessibilityRect::new(0.0, 0.0, 400.0, 600.0))]);
    let mut root = node(7, SemanticsRole::Layout, Vec::new(), Some(""), Vec::new());
    root.vertical_scroll = Some(cranpose_ui::ScrollAxisRange::new(0.0, 1.0, false));
    root.collection = Some(cranpose_ui::CollectionInfo {
        rows: 500,
        columns: 1,
    });
    root.scroll_to_index = Some(cranpose_ui::SemanticsScrollToIndex::new(move |index| {
        seen.borrow_mut().push(index);
        true
    }));

    let elements = project_semantics(&root, &bounds);
    assert_eq!(elements.len(), 1);
    assert!(elements[0].scroll_to_index);
    assert_eq!(row_count(&elements[0]), 500, "the last row is 499");

    assert!(scroll_to_index(&root, 7, 300));
    assert_eq!(*taken.borrow(), vec![300]);
    assert!(!scroll_to_index(&root, 99, 300), "no such list");
}

#[test]
fn a_plain_scroll_view_takes_no_row_number() {
    let list = scroll_box(1, None, 300.0);

    assert!(!list.scroll_to_index);
    assert_eq!(
        row_count(&list),
        0,
        "a container that answers no row number names no last row"
    );
}

#[test]
fn an_element_outside_every_list_pages_nothing() {
    let list = scroll_box(1, None, 300.0);
    let button = row_in(2, "Pay", None);

    assert!(scroll_container_for(&[list], &button).is_none());
}

#[test]
fn the_projection_names_the_list_above_each_row() {
    let bounds = HashMap::from_iter([
        (1, AccessibilityRect::new(0.0, 0.0, 400.0, 600.0)),
        (2, AccessibilityRect::new(0.0, 0.0, 400.0, 40.0)),
        (3, AccessibilityRect::new(0.0, 700.0, 400.0, 40.0)),
    ]);
    let mut list = node(
        1,
        SemanticsRole::Layout,
        vec![],
        None,
        vec![node(2, SemanticsRole::Button, vec![], Some("Milk"), vec![])],
    );
    list.vertical_scroll = Some(cranpose_ui::ScrollAxisRange::new(0.0, 900.0, false));
    let root = node(
        0,
        SemanticsRole::Layout,
        vec![],
        None,
        vec![
            list,
            node(3, SemanticsRole::Button, vec![], Some("Pay"), vec![]),
        ],
    );

    let elements = project_semantics(&root, &bounds);
    let by_id = |id: NodeId| {
        elements
            .iter()
            .find(|e| e.node_id == id)
            .expect("projected")
    };

    assert_eq!(
        by_id(1).scroll_parent,
        None,
        "the list itself sits under no list"
    );
    assert_eq!(
        by_id(2).scroll_parent,
        Some(1),
        "the row names the list above it"
    );
    assert_eq!(
        by_id(3).scroll_parent,
        None,
        "the button outside names none"
    );
}

#[test]
fn a_control_without_a_range_is_never_adjustable() {
    let bounds = HashMap::from_iter([(7, AccessibilityRect::new(0.0, 0.0, 200.0, 40.0))]);
    let root = node(
        7,
        SemanticsRole::Layout,
        Vec::new(),
        Some("Volume"),
        Vec::new(),
    );
    let elements = project_semantics(&root, &bounds);
    assert!(elements[0].progress.is_none());
    assert!(!elements[0].adjustable);
}

#[test]
fn a_spoken_line_says_the_name_the_role_the_state_and_the_actions() {
    let mut flash = element_with(1, None);
    flash.label = "Flash".to_string();
    flash.role = AccessibilityRole::Switch;
    flash.toggled = Some(true);
    flash.custom_actions = vec!["Reset".to_string()];
    assert_eq!(spoken_line(&flash), "Flash, switch, on, actions: Reset");

    let mut loading = element_with(2, None);
    loading.label = "Loading".to_string();
    loading.role = AccessibilityRole::ProgressBar;
    loading.progress = Some(ProgressBarRangeInfo::new(0.4, 0.0, 1.0, 0));
    loading.enabled = false;
    assert_eq!(
        spoken_line(&loading),
        "Loading, progress bar, 40 percent, dimmed"
    );

    let mut library = element_with(3, None);
    library.label = String::new();
    library.pane_title = Some("Library".to_string());
    assert_eq!(spoken_line(&library), "Library, pane");

    let mut plain = element_with(4, None);
    plain.label = "Milk".to_string();
    plain.focused = true;
    assert_eq!(spoken_line(&plain), "Milk, focused");
}
