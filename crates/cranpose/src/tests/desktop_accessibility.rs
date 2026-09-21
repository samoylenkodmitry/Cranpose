use super::*;
use crate::accessibility::AccessibilityRect;

#[test]
fn collection_positions_use_accesskits_zero_based_index() {
    for position in 1..=3 {
        let element = AccessibilityElement {
            role: AccessibilityRole::Tab,
            collection_item: Some(accessibility::CollectionItem {
                position,
                count: 3,
                horizontal: true,
            }),
            ..AccessibilityElement::default()
        };
        let node = accesskit_node(&element);
        assert_eq!(node.position_in_set(), Some(position - 1));
        assert_eq!(node.size_of_set(), Some(3));
    }
}

#[test]
fn radio_selection_is_a_native_checked_state() {
    for selected in [false, true] {
        let element = AccessibilityElement {
            role: AccessibilityRole::RadioButton,
            selected: Some(selected),
            ..AccessibilityElement::default()
        };
        let node = accesskit_node(&element);
        assert_eq!(node.toggled(), Some(Toggled::from(selected)));
        assert_eq!(node.is_selected(), None);
    }
}
#[test]
fn the_tree_points_at_the_focused_control_and_offers_focus_on_the_others() {
    let elements = vec![
        AccessibilityElement {
            node_id: 7,
            label: "Name".into(),
            bounds: AccessibilityRect::new(0.0, 0.0, 80.0, 44.0),
            role: AccessibilityRole::TextField,
            focusable: true,
            ..AccessibilityElement::default()
        },
        AccessibilityElement {
            node_id: 8,
            label: "Save".into(),
            bounds: AccessibilityRect::new(0.0, 50.0, 80.0, 44.0),
            role: AccessibilityRole::Button,
            clickable: true,
            focusable: true,
            focused: true,
            ..AccessibilityElement::default()
        },
    ];

    let update = tree_update(&elements, None, false);
    let ids = accessibility::element_ids(&elements);

    assert_eq!(
        update.focus,
        NodeId(ids[1] as u64),
        "a screen reader reads focus from the tree, and it sits on Save"
    );
    for (id, _) in ids.iter().zip(&elements) {
        let node = update
            .nodes
            .iter()
            .find(|(node_id, _)| *node_id == NodeId(*id as u64))
            .map(|(_, node)| node)
            .expect("every element is in the tree");
        assert!(
            node.supports_action(Action::Focus),
            "a control focus can land on offers the Focus action"
        );
    }
}

#[test]
fn every_role_has_an_accesskit_role_of_its_own() {
    for role in AccessibilityRole::ALL {
        assert_eq!(
            ACCESSKIT_ROLES
                .iter()
                .filter(|(named, _)| *named == role)
                .count(),
            1,
            "{role:?} should be in the accesskit table once"
        );
        assert_ne!(accesskit_role(role), Role::Unknown);
    }
}

#[test]
fn progress_indicators_keep_their_role_and_offer_no_adjustment() {
    let mut element = AccessibilityElement {
        role: AccessibilityRole::ProgressBar,
        progress: Some(cranpose_ui::ProgressBarRangeInfo::new(0.4, 0.0, 1.0, 0)),
        ..AccessibilityElement::default()
    };
    let node = accesskit_node(&element);
    assert_eq!(node.role(), Role::ProgressIndicator);
    assert!(node.numeric_value().is_some());
    assert!(!node.supports_action(Action::SetValue));
    element.adjustable = true;
    let node = accesskit_node(&element);
    assert_eq!(node.role(), Role::Slider);
    assert!(node.supports_action(Action::SetValue));
}

#[test]
fn disabled_controls_keep_their_state_without_offering_actions() {
    let element = AccessibilityElement {
        label: "Save".into(),
        state_description: Some("Disabled".into()),
        role: AccessibilityRole::Button,
        enabled: false,
        clickable: true,
        adjustable: true,
        expanded: Some(true),
        custom_actions: vec!["Delete".into()],
        ..AccessibilityElement::default()
    };
    let node = accesskit_node(&element);
    assert!(node.is_disabled());
    assert_eq!(node.label(), Some("Save"));
    assert_eq!(node.description(), Some("Disabled"));
    assert_eq!(node.is_expanded(), Some(true));
    for action in [
        Action::Click,
        Action::CustomAction,
        Action::Collapse,
        Action::SetValue,
        Action::Increment,
    ] {
        assert!(!node.supports_action(action), "{action:?}");
    }
}

#[test]
fn a_tree_with_nothing_focused_leaves_focus_on_the_window() {
    let elements = vec![AccessibilityElement {
        node_id: 7,
        label: "Name".into(),
        bounds: AccessibilityRect::new(0.0, 0.0, 80.0, 44.0),
        role: AccessibilityRole::TextField,
        focusable: true,
        ..AccessibilityElement::default()
    }];

    assert_eq!(tree_update(&elements, None, false).focus, ROOT_ID);
}

#[test]
fn desktop_tree_maps_controls_to_native_roles_and_click_actions() {
    let elements = vec![
        AccessibilityElement {
            node_id: 7,
            label: "Items".into(),
            bounds: AccessibilityRect::new(10.0, 20.0, 80.0, 44.0),
            role: AccessibilityRole::Button,
            clickable: true,
            ..AccessibilityElement::default()
        },
        AccessibilityElement {
            node_id: 8,
            label: "Receipts".into(),
            bounds: AccessibilityRect::new(10.0, 70.0, 120.0, 24.0),
            role: AccessibilityRole::StaticText,
            ..AccessibilityElement::default()
        },
    ];

    let update = tree_update(&elements, None, false);
    assert_eq!(update.tree.as_ref().map(|tree| tree.root), Some(ROOT_ID));
    let button = &update.nodes[1].1;
    assert_eq!(button.role(), Role::Button);
    assert_eq!(button.label(), Some("Items"));
    assert!(button.supports_action(Action::Click));
    let label = &update.nodes[2].1;
    assert_eq!(label.role(), Role::Label);
    assert_eq!(label.value(), Some("Receipts"));
}

fn field(value: &str, anchor: usize, focus: usize) -> AccessibilityElement {
    AccessibilityElement {
        node_id: 7,
        label: "Note".into(),
        value: Some(value.into()),
        text_selection: Some((anchor, focus)),
        bounds: AccessibilityRect::new(0.0, 0.0, 200.0, 44.0),
        role: AccessibilityRole::TextField,
        focusable: true,
        ..AccessibilityElement::default()
    }
}

#[test]
fn a_text_field_carries_its_text_as_runs_with_characters_words_and_a_caret() {
    let update = tree_update(&[field("Milk añd eggs", 5, 8)], None, false);

    let input = &update.nodes[1].1;
    assert_eq!(input.role(), Role::TextInput);
    assert!(input.supports_action(Action::SetTextSelection));
    let run_id = text_run_id(7, 0);
    assert_eq!(input.children(), &[run_id]);
    let selection = input.text_selection().expect("the caret is published");
    assert_eq!(
        selection.anchor,
        TextPosition {
            node: run_id,
            character_index: 5
        }
    );
    assert_eq!(
        selection.focus,
        TextPosition {
            node: run_id,
            character_index: 7
        }
    );

    let (id, run) = &update.nodes[2];
    assert_eq!(*id, run_id);
    assert_eq!(run.role(), Role::TextRun);
    assert_eq!(run.value(), Some("Milk añd eggs"));
    assert_eq!(
        run.character_lengths(),
        &[1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 1]
    );
    assert_eq!(run.word_starts(), &[5, 9]);
}

#[test]
fn a_field_with_lines_is_multiline_and_puts_the_caret_after_a_break_on_the_next_line() {
    let update = tree_update(&[field("one\ntwo", 4, 4)], None, false);

    let input = &update.nodes[1].1;
    assert_eq!(input.role(), Role::MultilineTextInput);
    assert_eq!(input.children(), &[text_run_id(7, 0), text_run_id(7, 1)]);
    let caret = input
        .text_selection()
        .expect("the caret is published")
        .focus;
    assert_eq!(
        caret,
        TextPosition {
            node: text_run_id(7, 1),
            character_index: 0
        }
    );
    assert_eq!(update.nodes[2].1.value(), Some("one\n"));
    assert_eq!(update.nodes[3].1.value(), Some("two"));
}

#[test]
fn a_password_field_and_a_button_carry_no_text_runs() {
    let mut secret = field("hunter2", 0, 0);
    secret.password = true;
    secret.text_selection = None;
    let elements = vec![
        secret,
        AccessibilityElement {
            node_id: 8,
            label: "Save".into(),
            bounds: AccessibilityRect::new(0.0, 50.0, 80.0, 44.0),
            role: AccessibilityRole::Button,
            clickable: true,
            ..AccessibilityElement::default()
        },
    ];

    let update = tree_update(&elements, None, false);

    assert_eq!(update.nodes.len(), 3);
    assert!(update.nodes[1].1.children().is_empty());
    assert!(update.nodes[1].1.text_selection().is_none());
}

#[test]
fn a_long_line_breaks_into_runs_at_a_space_before_the_word_limit() {
    let word = "abcdefghi ";
    let text = word.repeat(50);
    let runs = text_runs(&text);

    assert!(runs.len() > 1);
    assert!(runs.iter().all(|run| {
        let chars = text[run.start_byte..run.end_byte].chars().count();
        chars <= TEXT_RUN_CHARS && chars > 0
    }));
    assert!(
        runs.iter().all(|run| text[..run.end_byte].ends_with(' ')),
        "every run ends where a word ends: {runs:?}"
    );
    assert_eq!(runs[0].start_char, 0);
    assert_eq!(runs.last().map(|run| run.end_byte), Some(text.len()));
    assert_eq!(
        text_runs(""),
        vec![TextRunSpan {
            start_byte: 0,
            end_byte: 0,
            start_char: 0
        }]
    );
}

#[test]
fn a_selection_a_reader_set_on_a_run_comes_back_in_characters_of_the_whole_text() {
    let elements = vec![field("one\ntwo", 0, 0)];
    let selection = TextSelection {
        anchor: TextPosition {
            node: text_run_id(7, 1),
            character_index: 1,
        },
        focus: TextPosition {
            node: text_run_id(7, 0),
            character_index: 2,
        },
    };

    assert_eq!(
        selection_chars(&elements, NodeId(7), &selection),
        Some((7, 5, 2))
    );
    let stray = TextSelection {
        anchor: TextPosition {
            node: text_run_id(9, 0),
            character_index: 1,
        },
        focus: TextPosition {
            node: text_run_id(9, 0),
            character_index: 1,
        },
    };
    assert_eq!(selection_chars(&elements, NodeId(9), &stray), None);
}

#[test]
fn drawn_controls_sharing_a_layout_node_become_separate_accesskit_nodes() {
    let elements = vec![
        AccessibilityElement {
            node_id: 4,
            canvas_key: Some(1),
            label: "Haptics".into(),
            state_description: Some("On".into()),
            bounds: AccessibilityRect::new(0.0, 0.0, 100.0, 50.0),
            role: AccessibilityRole::Switch,
            clickable: true,
            toggled: Some(true),
            ..AccessibilityElement::default()
        },
        AccessibilityElement {
            node_id: 4,
            canvas_key: Some(2),
            label: "Sound effects".into(),
            bounds: AccessibilityRect::new(0.0, 60.0, 100.0, 50.0),
            role: AccessibilityRole::Switch,
            clickable: true,
            toggled: Some(false),
            enabled: false,
            ..AccessibilityElement::default()
        },
    ];

    let update = tree_update(&elements, None, false);
    let ids: Vec<_> = update.nodes.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids.len(), 3, "root plus one node per drawn control");
    assert_ne!(ids[1], ids[2]);
    assert_eq!(update.nodes[0].1.children(), &ids[1..]);

    let haptics = &update.nodes[1].1;
    assert_eq!(haptics.role(), Role::Switch);
    assert_eq!(haptics.toggled(), Some(Toggled::True));
    assert_eq!(haptics.description(), Some("On"));
    assert!(!haptics.is_disabled());

    let sound = &update.nodes[2].1;
    assert_eq!(sound.toggled(), Some(Toggled::False));
    assert!(sound.is_disabled());
}

#[test]
fn an_announcement_rides_along_as_a_live_node_under_the_window() {
    let elements = vec![AccessibilityElement {
        node_id: 1,
        label: "Import".into(),
        bounds: AccessibilityRect::new(0.0, 0.0, 100.0, 40.0),
        role: AccessibilityRole::Button,
        clickable: true,
        ..AccessibilityElement::default()
    }];
    let announcement = Announcement {
        text: "Seven receipts imported".into(),
        mode: LiveRegionMode::Assertive,
    };

    let update = tree_update(&elements, Some(&announcement), false);
    assert_eq!(update.nodes.len(), 3, "root, the button, the announcement");
    let (id, spoken) = &update.nodes[2];
    assert_eq!(*id, ANNOUNCEMENT_ID);
    assert_eq!(spoken.value(), Some("Seven receipts imported"));
    assert_eq!(spoken.live(), Some(Live::Assertive));
    assert!(update.nodes[0].1.children().contains(&ANNOUNCEMENT_ID));

    let again = tree_update(&elements, Some(&announcement), true);
    assert_eq!(again.nodes[2].1.value(), Some("Seven receipts imported "));
}

#[test]
fn a_live_control_tells_accesskit_how_urgent_it_is() {
    let elements = vec![AccessibilityElement {
        node_id: 1,
        label: "3 receipts left".into(),
        bounds: AccessibilityRect::new(0.0, 0.0, 100.0, 40.0),
        live_region: Some(LiveRegionMode::Polite),
        ..AccessibilityElement::default()
    }];

    let update = tree_update(&elements, None, false);
    assert_eq!(update.nodes[1].1.live(), Some(Live::Polite));
}

#[test]
fn an_adjustable_control_reads_as_a_slider_with_a_value_and_a_way_to_move_it() {
    let elements = vec![AccessibilityElement {
        node_id: 1,
        label: "Volume".into(),
        state_description: Some("40 %".into()),
        bounds: AccessibilityRect::new(0.0, 0.0, 200.0, 40.0),
        progress: Some(cranpose_ui::ProgressBarRangeInfo::new(0.4, 0.0, 1.0, 0)),
        adjustable: true,
        ..AccessibilityElement::default()
    }];

    let update = tree_update(&elements, None, false);
    let slider = &update.nodes[1].1;
    assert_eq!(slider.role(), Role::Slider);
    let near =
        |value: Option<f64>, want: f64| value.is_some_and(|value| (value - want).abs() < 1e-6);
    assert!(near(slider.numeric_value(), 0.4));
    assert!(near(slider.min_numeric_value(), 0.0));
    assert!(near(slider.max_numeric_value(), 1.0));
    assert!(near(slider.numeric_value_step(), 0.1));
    assert!(slider.supports_action(Action::SetValue));
    assert!(slider.supports_action(Action::Increment));
    assert!(slider.supports_action(Action::Decrement));
}

#[test]
fn several_announcements_in_one_frame_become_one_line() {
    let joined = join_announcements(vec![
        Announcement {
            text: "Import done".into(),
            mode: LiveRegionMode::Polite,
        },
        Announcement {
            text: "Two receipts failed".into(),
            mode: LiveRegionMode::Assertive,
        },
    ])
    .expect("two announcements make one");
    assert_eq!(joined.text, "Import done. Two receipts failed");
    assert_eq!(joined.mode, LiveRegionMode::Assertive);
    assert!(join_announcements(Vec::new()).is_none());
}

#[test]
fn rows_sit_under_their_list_in_the_desktop_tree() {
    let list = AccessibilityElement {
        node_id: 6,
        bounds: AccessibilityRect::new(0.0, 0.0, 400.0, 600.0),
        vertical_scroll: Some(cranpose_ui::ScrollAxisRange::new(0.0, 900.0, false)),
        ..AccessibilityElement::default()
    };
    let row = AccessibilityElement {
        node_id: 9,
        label: "Milk".into(),
        bounds: AccessibilityRect::new(0.0, 10.0, 400.0, 40.0),
        scroll_parent: Some(6),
        ..AccessibilityElement::default()
    };

    let update = tree_update(&[list, row], None, false);
    let ids: Vec<_> = update.nodes.iter().map(|(id, _)| *id).collect();

    assert_eq!(
        update.nodes[0].1.children(),
        &ids[1..2],
        "the root holds the list alone"
    );
    assert_eq!(
        update.nodes[1].1.children(),
        &ids[2..],
        "the list holds its row"
    );
}
