use cranpose_core::collections::map::HashMap;

use crate::accessibility::{
    AccessibilityElement, AccessibilityIdentityError, AccessibilityRect, AccessibilitySnapshot,
    CollectionItem, checked_state, spoken_changes, utf16_offset,
};

/// What the host needs to show a new snapshot: every virtual id in order,
/// full records for the controls it does not hold as they are now, and new
/// pixel bounds for the controls that only moved, as `id, left, top, right,
/// bottom` runs.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct AccessibilityUpdate {
    pub(crate) order: Vec<i32>,
    /// The records back to back, as [`RecordWriter`] writes them.
    pub(crate) records: Vec<u8>,
    pub(crate) moves: Vec<i32>,
    reordered: bool,
}

impl AccessibilityUpdate {
    /// Whether the host already shows everything this update says.
    pub(crate) fn is_empty(&self) -> bool {
        !self.reordered && self.records.is_empty() && self.moves.is_empty()
    }
}

/// What the host was last sent: the density its pixel bounds were made at,
/// and the list above each control, in the published snapshot's order.
#[derive(Default)]
pub(crate) struct AccessibilityWire {
    density: Option<u32>,
    parents: Vec<i32>,
}

impl AccessibilityWire {
    /// Forgets what the host holds, so the next update sends every control.
    pub(crate) fn forget(&mut self) {
        self.density = None;
    }

    /// Moves `snapshot` on to `elements` and says what the host needs to
    /// catch up, flagging the controls that now say something else.
    pub(crate) fn publish(
        &mut self,
        snapshot: &mut AccessibilitySnapshot,
        elements: Vec<AccessibilityElement>,
        density: f32,
    ) -> Result<AccessibilityUpdate, AccessibilityIdentityError> {
        let density = density.max(f32::EPSILON);
        let mut replaced = snapshot.update(elements)?;
        let changed = spoken_changes(&replaced.elements, &snapshot.elements, &replaced.was);
        let parents = scroll_parent_ids(&snapshot.elements, &snapshot.ids);
        let known = self.density == Some(density.to_bits());
        let mut records = Vec::new();
        let mut moves = Vec::new();
        for (((element, id), parent), (spoken_change, was)) in snapshot
            .elements
            .iter()
            .zip(&snapshot.ids)
            .zip(&parents)
            .zip(changed.into_iter().zip(&replaced.was))
        {
            let published = match *was {
                Some(old) if known && !spoken_change && self.parents.get(old) == Some(parent) => {
                    replaced.elements.get_mut(old)
                }
                _ => None,
            };
            let kept =
                published.and_then(|was| same_but_bounds(was, element).then_some(was.bounds));
            match kept {
                Some(was) => {
                    let bounds = pixel_bounds(element.bounds, density);
                    if pixel_bounds(was, density) != bounds {
                        moves.push(*id);
                        moves.extend(bounds);
                    }
                }
                None => {
                    encode_record(&mut records, element, *id, *parent, spoken_change, density);
                }
            }
        }
        let reordered = !known || snapshot.ids != replaced.ids;
        snapshot.recycle(replaced.elements);
        self.density = Some(density.to_bits());
        self.parents = parents;
        Ok(AccessibilityUpdate {
            order: snapshot.ids.clone(),
            records,
            moves,
            reordered,
        })
    }
}

/// Whether `was` and `now` differ in nothing but their bounds.
fn same_but_bounds(was: &mut AccessibilityElement, now: &AccessibilityElement) -> bool {
    let bounds = std::mem::replace(&mut was.bounds, now.bounds);
    let same = *was == *now;
    was.bounds = bounds;
    same
}

/// A control's bounds in pixels, as the host's `Rect` holds them.
fn pixel_bounds(bounds: AccessibilityRect, density: f32) -> [i32; 4] {
    [
        (bounds.x * density).round() as i32,
        (bounds.y * density).round() as i32,
        ((bounds.x + bounds.width) * density).round() as i32,
        ((bounds.y + bounds.height) * density).round() as i32,
    ]
}

/// Appends one control's record to `out`: its fields in the order Java
/// reads them.
fn encode_record(
    out: &mut Vec<u8>,
    element: &AccessibilityElement,
    id: i32,
    parent: i32,
    changed: bool,
    density: f32,
) {
    let [left, top, right, bottom] = pixel_bounds(element.bounds, density);
    let (center_x, center_y) = element.bounds.center();
    let progress = element.progress;
    let scroll = element.vertical_scroll.or(element.horizontal_scroll);
    let (selection_start, selection_end) = selection_in_utf16(element);
    let mut record = RecordWriter(out);
    record.number(id);
    record.number(element.role.android_code());
    record.number(left);
    record.number(top);
    record.number(right);
    record.number(bottom);
    record.float(center_x);
    record.float(center_y);
    record.number(i32::from(element.clickable));
    record.text(&element.label);
    record.text(element.value.as_deref().unwrap_or(""));
    record.text(element.state_description.as_deref().unwrap_or(""));
    record.text(element.click_label.as_deref().unwrap_or(""));
    record.number(tristate(element.selected));
    record.number(tristate(checked_state(element)));
    record.number(i32::from(element.enabled));
    record.actions(&element.custom_actions);
    record.number(i32::from(element.focusable));
    record.number(i32::from(element.focused));
    record.number(i32::from(element.adjustable));
    record.float(progress.map_or(0.0, |p| p.current));
    record.float(progress.map_or(0.0, |p| p.start));
    record.float(progress.map_or(0.0, |p| p.end));
    record.number(i32::from(scroll.is_some()));
    record.number(i32::from(
        scroll.is_some_and(|range| range.can_scroll_forward()),
    ));
    record.number(i32::from(
        scroll.is_some_and(|range| range.can_scroll_backward()),
    ));
    record.number(parent);
    record.number(
        element
            .collection
            .map_or(0, |collection| count(collection.rows)),
    );
    record.number(
        element
            .collection
            .map_or(0, |collection| count(collection.columns)),
    );
    record.number(i32::from(changed));
    record.number(element.collection_item.map_or(-1, item_row));
    record.number(element.collection_item.map_or(-1, item_column));
    record.text(element.pane_title.as_deref().unwrap_or(""));
    record.text(element.error.as_deref().unwrap_or(""));
    record.number(i32::from(element.password));
    record.number(tristate(element.expanded));
    record.text(element.long_click_label.as_deref().unwrap_or(""));
    record.number(i32::from(element.dismissable));
    record.number(i32::from(element.scroll_to_index));
    record.number(selection_start);
    record.number(selection_end);
}

/// Writes a record's fields straight into the update, in the order the host
/// reads them: numbers and flags as little-endian `i32`, fractions as
/// little-endian `f32`, text as its UTF-8 length, a little-endian `u32`,
/// then its bytes, and a list of labels as their count, then each label.
/// The host reads them off a buffer; nothing is escaped or parsed.
struct RecordWriter<'a>(&'a mut Vec<u8>);

impl RecordWriter<'_> {
    fn number(&mut self, value: i32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn float(&mut self, value: f32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn length(&mut self, length: usize) {
        let length = u32::try_from(length).unwrap_or(u32::MAX);
        self.0.extend_from_slice(&length.to_le_bytes());
    }

    fn text(&mut self, value: &str) {
        self.length(value.len());
        self.0.extend_from_slice(value.as_bytes());
    }

    /// The custom actions' labels.
    fn actions(&mut self, labels: &[String]) {
        self.length(labels.len());
        for label in labels {
            self.text(label);
        }
    }
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
    let mut nodes: HashMap<cranpose_core::NodeId, i32> = HashMap::default();
    for (element, id) in elements.iter().zip(ids) {
        if element.canvas_key.is_none() {
            nodes.entry(element.node_id).or_insert(*id);
        }
    }
    elements
        .iter()
        .map(|element| {
            element
                .scroll_parent
                .and_then(|parent| nodes.get(&parent).copied())
                .unwrap_or(-1)
        })
        .collect()
}

/// The row a control takes inside its group, counted from zero, for the
/// host's collection item info: a group that runs left to right is one row.
/// A collection's size as the host's `int` holds it.
fn count(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

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

#[cfg(test)]
#[path = "tests/android_accessibility_wire.rs"]
mod tests;
