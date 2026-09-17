use crate::{
    accessibility::{AccessibilityElement, AccessibilityRole, CollectionItem, element_ids},
    android_wire_escape::escape_wire_field,
};

const ACTION_SEPARATOR: char = '\u{1f}';

pub(crate) fn encode_elements(
    elements: &[AccessibilityElement],
    changed: &[bool],
    density: f32,
) -> String {
    let density = density.max(f32::EPSILON);
    let ids = element_ids(elements);
    let parents = scroll_parent_ids(elements, &ids);
    elements
        .iter()
        .zip(ids)
        .zip(parents)
        .enumerate()
        .map(|(index, ((element, id), parent))| {
            let role = match element.role {
                AccessibilityRole::Button => 1,
                AccessibilityRole::StaticText => 2,
                AccessibilityRole::TextField => 3,
                AccessibilityRole::Checkbox => 4,
                AccessibilityRole::Switch => 5,
                AccessibilityRole::RadioButton => 6,
                AccessibilityRole::Tab => 7,
                AccessibilityRole::Image => 8,
                AccessibilityRole::Header => 9,
                AccessibilityRole::Dialog => 10,
                AccessibilityRole::DropdownList => 11,
                AccessibilityRole::ValuePicker => 12,
            };
            let (center_x, center_y) = element.bounds.center();
            let actions = element
                .custom_actions
                .iter()
                .map(|label| escape(label))
                .collect::<Vec<_>>()
                .join(&ACTION_SEPARATOR.to_string());
            let progress = element.progress;
            let scroll = element.vertical_scroll.or(element.horizontal_scroll);
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                id,
                role,
                (element.bounds.x * density).round() as i32,
                (element.bounds.y * density).round() as i32,
                ((element.bounds.x + element.bounds.width) * density).round() as i32,
                ((element.bounds.y + element.bounds.height) * density).round() as i32,
                center_x,
                center_y,
                i32::from(element.clickable),
                escape(&element.label),
                escape(element.value.as_deref().unwrap_or("")),
                escape(element.state_description.as_deref().unwrap_or("")),
                escape(element.click_label.as_deref().unwrap_or("")),
                tristate(element.selected),
                tristate(element.toggled),
                i32::from(element.enabled),
                actions,
                i32::from(element.focusable),
                i32::from(element.focused),
                i32::from(element.adjustable),
                progress.map(|p| p.current).unwrap_or(0.0),
                progress.map(|p| p.start).unwrap_or(0.0),
                progress.map(|p| p.end).unwrap_or(0.0),
                i32::from(scroll.is_some()),
                i32::from(scroll.is_some_and(|range| range.can_scroll_forward())),
                i32::from(scroll.is_some_and(|range| range.can_scroll_backward())),
                parent,
                element.collection.map_or(0, |collection| collection.rows),
                element.collection.map_or(0, |collection| collection.columns),
                i32::from(changed.get(index).copied().unwrap_or(false)),
                element.collection_item.map_or(-1, item_row),
                element.collection_item.map_or(-1, item_column),
                escape(element.pane_title.as_deref().unwrap_or("")),
                escape(element.error.as_deref().unwrap_or("")),
                i32::from(element.password),
                tristate(element.expanded),
                escape(element.long_click_label.as_deref().unwrap_or("")),
                i32::from(element.dismissable),
                i32::from(element.scroll_to_index),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The virtual id of the scroll container above each element, or -1, so the
/// host can hang a row under its list.
fn scroll_parent_ids(elements: &[AccessibilityElement], ids: &[i32]) -> Vec<i32> {
    elements
        .iter()
        .map(|element| {
            element
                .scroll_parent
                .and_then(|parent| {
                    elements.iter().position(|candidate| {
                        candidate.node_id == parent && candidate.canvas_key.is_none()
                    })
                })
                .and_then(|index| ids.get(index).copied())
                .unwrap_or(-1)
        })
        .collect()
}

/// The row a control takes inside its group, counted from zero, for the
/// host's collection item info: a group that runs left to right is one row.
fn item_row(item: CollectionItem) -> i32 {
    if item.horizontal {
        0
    } else {
        item.position as i32 - 1
    }
}

/// The column a control takes inside its group, counted from zero.
fn item_column(item: CollectionItem) -> i32 {
    if item.horizontal {
        item.position as i32 - 1
    } else {
        0
    }
}

fn tristate(value: Option<bool>) -> i32 {
    match value {
        None => -1,
        Some(false) => 0,
        Some(true) => 1,
    }
}

fn escape(value: &str) -> String {
    escape_wire_field(value).replace(ACTION_SEPARATOR, "%1F")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::{AccessibilityRect, element_with};

    #[test]
    fn android_accessibility_wire_values_escape_record_delimiters() {
        assert_eq!(
            escape(&format!("A%\tB\nC\r{ACTION_SEPARATOR}")),
            "A%25%09B%0AC%0D%1F"
        );
    }

    #[test]
    fn tristate_distinguishes_unset_from_off() {
        assert_eq!(tristate(None), -1);
        assert_eq!(tristate(Some(false)), 0);
        assert_eq!(tristate(Some(true)), 1);
    }

    #[test]
    fn every_encoded_record_carries_the_fields_java_parses() {
        let elements = vec![
            AccessibilityElement {
                node_id: 4,
                label: "Haptics".into(),
                state_description: Some("On".into()),
                click_label: Some("Toggle".into()),
                bounds: AccessibilityRect::new(1.0, 2.0, 30.0, 40.0),
                role: AccessibilityRole::Switch,
                clickable: true,
                toggled: Some(true),
                custom_actions: vec!["Pause".into(), "Resume".into()],
                ..AccessibilityElement::default()
            },
            element_with(5, Some(1)),
        ];

        let payload = encode_elements(&elements, &[], 2.0);
        let records: Vec<_> = payload.split('\n').collect();
        assert_eq!(records.len(), 2);
        for record in &records {
            assert_eq!(record.split('\t').count(), 39, "record: {record}");
        }

        let fields: Vec<_> = records[0].split('\t').collect();
        assert_eq!(fields[1], "5", "Switch should encode as role 5");
        assert_eq!(&fields[2..6], ["2", "4", "62", "84"]);
        assert_eq!(fields[9], "Haptics");
        assert_eq!(fields[11], "On");
        assert_eq!(fields[12], "Toggle");
        assert_eq!(fields[13], "-1", "selected was never set");
        assert_eq!(fields[14], "1", "toggled on");
        assert_eq!(fields[15], "1", "enabled by default");
        assert_eq!(fields[16], format!("Pause{ACTION_SEPARATOR}Resume"));
        assert_eq!(fields[17], "0", "this element registered no focus target");
        assert_eq!(fields[18], "0", "and focus does not sit on it");
    }

    #[test]
    fn the_record_names_what_a_long_press_does() {
        let elements = vec![
            AccessibilityElement {
                node_id: 4,
                label: "Milk".into(),
                long_click_label: Some("Remove receipt".into()),
                bounds: AccessibilityRect::new(0.0, 0.0, 30.0, 40.0),
                role: AccessibilityRole::Button,
                clickable: true,
                ..AccessibilityElement::default()
            },
            save_button(5),
        ];

        let payload = encode_elements(&elements, &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();
        let with_long_press: Vec<_> = records[0].split('\t').collect();
        let plain: Vec<_> = records[1].split('\t').collect();

        assert_eq!(with_long_press[36], "Remove receipt");
        assert_eq!(plain[36], "", "a control with no long press says nothing");
    }

    #[test]
    fn the_record_carries_whether_focus_can_land_on_a_control_and_whether_it_has() {
        let elements = vec![
            AccessibilityElement {
                node_id: 4,
                label: "Name".into(),
                bounds: AccessibilityRect::new(0.0, 0.0, 30.0, 40.0),
                role: AccessibilityRole::TextField,
                focusable: true,
                focused: true,
                ..AccessibilityElement::default()
            },
            AccessibilityElement {
                node_id: 5,
                label: "Save".into(),
                bounds: AccessibilityRect::new(0.0, 50.0, 30.0, 40.0),
                role: AccessibilityRole::Button,
                clickable: true,
                focusable: true,
                ..AccessibilityElement::default()
            },
        ];

        let payload = encode_elements(&elements, &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();
        let focused: Vec<_> = records[0].split('\t').collect();
        let other: Vec<_> = records[1].split('\t').collect();

        assert_eq!(focused[17], "1");
        assert_eq!(focused[18], "1", "TalkBack follows the app onto this one");
        assert_eq!(other[17], "1", "the button takes focus too");
        assert_eq!(other[18], "0", "but focus does not sit on it");
    }

    fn save_button(node_id: cranpose_core::NodeId) -> AccessibilityElement {
        AccessibilityElement {
            node_id,
            label: "Save".into(),
            bounds: AccessibilityRect::new(0.0, 50.0, 30.0, 40.0),
            role: AccessibilityRole::Button,
            clickable: true,
            ..AccessibilityElement::default()
        }
    }

    #[test]
    fn the_record_names_the_list_above_a_row() {
        let elements = vec![
            AccessibilityElement {
                node_id: 6,
                bounds: AccessibilityRect::new(0.0, 0.0, 400.0, 600.0),
                vertical_scroll: Some(cranpose_ui::ScrollAxisRange::new(0.0, 900.0, false)),
                ..AccessibilityElement::default()
            },
            AccessibilityElement {
                node_id: 9,
                label: "Milk".into(),
                bounds: AccessibilityRect::new(0.0, 10.0, 400.0, 40.0),
                scroll_parent: Some(6),
                ..AccessibilityElement::default()
            },
            save_button(8),
        ];

        let payload = encode_elements(&elements, &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();
        let list: Vec<_> = records[0].split('\t').collect();
        let row: Vec<_> = records[1].split('\t').collect();
        let button: Vec<_> = records[2].split('\t').collect();

        assert_eq!(list[26], "-1", "the list sits under the host");
        assert_eq!(row[26], list[0], "the row names its list by virtual id");
        assert_eq!(
            button[26], "-1",
            "a button outside any list sits under the host"
        );
    }

    #[test]
    fn the_record_says_which_way_a_list_can_still_page() {
        let elements = vec![
            AccessibilityElement {
                node_id: 6,
                bounds: AccessibilityRect::new(0.0, 0.0, 400.0, 600.0),
                vertical_scroll: Some(cranpose_ui::ScrollAxisRange::new(0.0, 900.0, false)),
                ..AccessibilityElement::default()
            },
            AccessibilityElement {
                node_id: 7,
                bounds: AccessibilityRect::new(0.0, 0.0, 400.0, 600.0),
                vertical_scroll: Some(cranpose_ui::ScrollAxisRange::new(900.0, 900.0, false)),
                ..AccessibilityElement::default()
            },
            save_button(8),
        ];

        let payload = encode_elements(&elements, &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();
        let top: Vec<_> = records[0].split('\t').collect();
        let bottom: Vec<_> = records[1].split('\t').collect();
        let button: Vec<_> = records[2].split('\t').collect();

        assert_eq!(top[23], "1", "a list is scrollable");
        assert_eq!(top[24], "1", "at the top it pages forward");
        assert_eq!(top[25], "0", "and not back");
        assert_eq!(bottom[24], "0", "at the end it pages back only");
        assert_eq!(bottom[25], "1");
        assert_eq!(button[23], "0", "a button does not scroll");
    }

    #[test]
    fn the_record_carries_the_range_of_an_adjustable_control() {
        let elements = vec![
            AccessibilityElement {
                node_id: 4,
                label: "Volume".into(),
                bounds: AccessibilityRect::new(0.0, 0.0, 200.0, 40.0),
                progress: Some(cranpose_ui::ProgressBarRangeInfo::new(0.25, 0.0, 1.0, 0)),
                adjustable: true,
                ..AccessibilityElement::default()
            },
            save_button(5),
        ];

        let payload = encode_elements(&elements, &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();
        let slider: Vec<_> = records[0].split('\t').collect();
        let button: Vec<_> = records[1].split('\t').collect();

        assert_eq!(slider[19], "1", "TalkBack may move this one");
        assert_eq!(slider[20], "0.25");
        assert_eq!(slider[21], "0");
        assert_eq!(slider[22], "1");
        assert_eq!(button[19], "0", "a button holds no range");
    }

    #[test]
    fn the_record_says_how_many_rows_a_list_holds() {
        let mut list = AccessibilityElement {
            node_id: 6,
            bounds: AccessibilityRect::new(0.0, 0.0, 400.0, 600.0),
            vertical_scroll: Some(cranpose_ui::ScrollAxisRange::new(0.0, 900.0, false)),
            ..AccessibilityElement::default()
        };
        list.collection = Some(cranpose_ui::CollectionInfo {
            rows: 12,
            columns: 1,
        });

        let payload = encode_elements(&[list, save_button(8)], &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();
        let list: Vec<_> = records[0].split('\t').collect();
        let button: Vec<_> = records[1].split('\t').collect();

        assert_eq!(
            (list[27], list[28]),
            ("12", "1"),
            "the list says its rows and columns"
        );
        assert_eq!((button[27], button[28]), ("0", "0"), "a button is no list");
    }

    #[test]
    fn the_record_flags_a_control_that_says_something_new() {
        let flagged = encode_elements(&[save_button(8)], &[true], 1.0);
        let quiet = encode_elements(&[save_button(8)], &[], 1.0);

        assert_eq!(
            flagged.split('\t').nth(29),
            Some("1"),
            "the button says something new"
        );
        assert_eq!(
            quiet.split('\t').nth(29),
            Some("0"),
            "the button reads as before"
        );
    }

    #[test]
    fn the_record_places_a_tab_in_its_group() {
        let mut tab = save_button(8);
        tab.selected = Some(true);
        tab.collection_item = Some(CollectionItem {
            position: 2,
            count: 5,
            horizontal: true,
        });

        let payload = encode_elements(&[tab, save_button(9)], &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();
        let placed: Vec<_> = records[0].split('\t').collect();
        let loose: Vec<_> = records[1].split('\t').collect();

        assert_eq!(
            (placed[30], placed[31]),
            ("0", "1"),
            "the second tab of a row is column one"
        );
        assert_eq!(
            (loose[30], loose[31]),
            ("-1", "-1"),
            "a button outside a group has no place"
        );
    }

    #[test]
    fn the_record_carries_the_title_of_a_pane() {
        let mut screen = AccessibilityElement {
            node_id: 1,
            bounds: AccessibilityRect::new(0.0, 0.0, 300.0, 600.0),
            ..AccessibilityElement::default()
        };
        screen.pane_title = Some("Library".into());

        let payload = encode_elements(&[screen, save_button(8)], &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();

        assert_eq!(records[0].split('\t').nth(32), Some("Library"));
        assert_eq!(
            records[1].split('\t').nth(32),
            Some(""),
            "a button names no pane"
        );
    }

    #[test]
    fn the_record_says_why_a_field_is_wrong() {
        let mut field = save_button(8);
        field.error = Some("needs a number".into());

        let payload = encode_elements(&[field, save_button(9)], &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();

        assert_eq!(records[0].split('\t').nth(33), Some("needs a number"));
        assert_eq!(
            records[1].split('\t').nth(33),
            Some(""),
            "a sound control names no error"
        );
    }

    #[test]
    fn the_record_marks_a_field_that_holds_a_secret() {
        let mut field = save_button(8);
        field.password = true;

        let payload = encode_elements(&[field, save_button(9)], &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();

        assert_eq!(records[0].split('\t').nth(34), Some("1"));
        assert_eq!(records[1].split('\t').nth(34), Some("0"));
    }

    #[test]
    fn the_record_says_whether_a_control_is_open() {
        let mut open = save_button(8);
        open.expanded = Some(true);
        let mut closed = save_button(9);
        closed.expanded = Some(false);

        let payload = encode_elements(&[open, closed, save_button(10)], &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();

        assert_eq!(records[0].split('\t').nth(35), Some("1"));
        assert_eq!(records[1].split('\t').nth(35), Some("0"));
        assert_eq!(
            records[2].split('\t').nth(35),
            Some("-1"),
            "a plain button opens nothing"
        );
    }

    #[test]
    fn the_record_says_whether_a_control_has_a_way_out() {
        let mut row = save_button(8);
        row.dismissable = true;

        let payload = encode_elements(&[row, save_button(9)], &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();

        assert_eq!(records[0].split('\t').nth(37), Some("1"));
        assert_eq!(
            records[1].split('\t').nth(37),
            Some("0"),
            "a plain button says nothing about a way out"
        );
    }

    #[test]
    fn the_record_says_whether_a_list_takes_a_row_number() {
        let mut list = save_button(8);
        list.scroll_to_index = true;

        let payload = encode_elements(&[list, save_button(9)], &[], 1.0);
        let records: Vec<_> = payload.split('\n').collect();

        assert_eq!(records[0].split('\t').nth(38), Some("1"));
        assert_eq!(
            records[1].split('\t').nth(38),
            Some("0"),
            "a plain button takes no row number"
        );
    }
}
