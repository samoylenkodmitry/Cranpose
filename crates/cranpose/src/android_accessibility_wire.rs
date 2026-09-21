use crate::{
    accessibility::{
        AccessibilityElement, AccessibilitySnapshot, CollectionItem, checked_state, utf16_offset,
    },
    android_wire_escape::escape_wire_field,
};

const ACTION_SEPARATOR: char = '\u{1f}';

pub(crate) fn encode_elements(
    snapshot: &AccessibilitySnapshot,
    changed: &[bool],
    density: f32,
) -> String {
    let elements = &snapshot.elements;
    let density = density.max(f32::EPSILON);
    let ids = &snapshot.ids;
    let parents = scroll_parent_ids(elements, ids);
    elements
        .iter()
        .zip(ids)
        .zip(parents)
        .enumerate()
        .map(|(index, ((element, id), parent))| {
            let role = element.role.android_code();
            let (center_x, center_y) = element.bounds.center();
            let actions = element
                .custom_actions
                .iter()
                .map(|label| escape(label))
                .collect::<Vec<_>>()
                .join(&ACTION_SEPARATOR.to_string());
            let progress = element.progress;
            let scroll = element.vertical_scroll.or(element.horizontal_scroll);
            let (selection_start, selection_end) = selection_in_utf16(element);
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
                tristate(checked_state(element)),
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
                selection_start,
                selection_end,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The two ends of a field's selection as Android counts text, in UTF-16
/// units, the anchor first; -1 and -1 for a control with no caret.
fn selection_in_utf16(element: &AccessibilityElement) -> (i32, i32) {
    match (&element.value, element.text_selection) {
        (Some(value), Some((anchor, focus))) => (
            utf16_offset(value, anchor) as i32,
            utf16_offset(value, focus) as i32,
        ),
        _ => (-1, -1),
    }
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
#[path = "tests/android_accessibility_wire.rs"]
mod tests;
