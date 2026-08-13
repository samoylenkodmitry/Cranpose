//! The wire format that carries Cranpose's semantics into
//! `CranposeActivity`'s `AccessibilityNodeProvider`.
//!
//! The whole visible tree crosses JNI as one `String` on the frames it
//! changes, rather than one call per node, for the same reason the Play
//! Billing snapshot does: a screen reader must never observe a half-built
//! tree. Encoding lives here, in safe Rust, and is unit-tested on the host —
//! the format is the contract between the two sides, and the Java parser
//! rejects any record whose field count does not match.
//!
//! ```text
//! <id>\t<role>\t<left>\t<top>\t<right>\t<bottom>\t<centerX>\t<centerY>
//!      \t<clickable>\t<label>\t<value>\t<stateDescription>\t<clickLabel>
//!      \t<selected>\t<toggled>\t<enabled>\t<customActions>
//! ```
//!
//! Bounds are physical pixels (Java draws them into `Rect`s); the two centre
//! coordinates stay logical because they are fed back through the pointer
//! pipeline as a synthetic tap. `selected` and `toggled` are `-1` when the app
//! said nothing, so "not a checkable control" is distinguishable from "off".
//! Text uses the same `%09`/`%0A`/`%0D`/`%25` escaping as the launch-argument
//! payload, plus `%1F` for the separator that packs custom action labels into
//! one field, because every one of these strings is app-authored.

use crate::accessibility::{element_ids, AccessibilityElement, AccessibilityRole};

/// Separator between the custom action labels packed into one wire field.
/// ASCII unit separator, escaped like every other delimiter so a label
/// containing one cannot split the field.
const ACTION_SEPARATOR: char = '\u{1f}';

pub(crate) fn encode_elements(elements: &[AccessibilityElement], density: f32) -> String {
    let density = density.max(f32::EPSILON);
    let ids = element_ids(elements);
    elements
        .iter()
        .zip(ids)
        .map(|(element, id)| {
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
            };
            let (center_x, center_y) = element.bounds.center();
            let actions = element
                .custom_actions
                .iter()
                .map(|label| escape(label))
                .collect::<Vec<_>>()
                .join(&ACTION_SEPARATOR.to_string());
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `-1` when the app said nothing about the state, so Java can tell "this is
/// not a checkable control" from "this switch is off".
fn tristate(value: Option<bool>) -> i32 {
    match value {
        None => -1,
        Some(false) => 0,
        Some(true) => 1,
    }
}

fn escape(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\t', "%09")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
        .replace(ACTION_SEPARATOR, "%1F")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::AccessibilityRect;

    fn element(node_id: usize, canvas_key: Option<u64>) -> AccessibilityElement {
        AccessibilityElement {
            node_id,
            canvas_key,
            label: "Row".into(),
            bounds: AccessibilityRect::new(0.0, 0.0, 10.0, 10.0),
            ..AccessibilityElement::default()
        }
    }

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

    /// The Java parser rejects a record whose field count is wrong, so the
    /// count is part of the contract and is asserted here rather than left to
    /// be discovered on a watch.
    #[test]
    fn every_encoded_record_carries_the_seventeen_fields_java_parses() {
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
            element(5, Some(1)),
        ];

        let payload = encode_elements(&elements, 2.0);
        let records: Vec<_> = payload.split('\n').collect();
        assert_eq!(records.len(), 2);
        for record in &records {
            assert_eq!(record.split('\t').count(), 17, "record: {record}");
        }

        let fields: Vec<_> = records[0].split('\t').collect();
        assert_eq!(fields[1], "5", "Switch should encode as role 5");
        // Density 2.0 turns logical 1,2 .. 31,42 into physical pixels.
        assert_eq!(&fields[2..6], ["2", "4", "62", "84"]);
        assert_eq!(fields[9], "Haptics");
        assert_eq!(fields[11], "On");
        assert_eq!(fields[12], "Toggle");
        assert_eq!(fields[13], "-1", "selected was never set");
        assert_eq!(fields[14], "1", "toggled on");
        assert_eq!(fields[15], "1", "enabled by default");
        assert_eq!(fields[16], format!("Pause{ACTION_SEPARATOR}Resume"));
    }
}
