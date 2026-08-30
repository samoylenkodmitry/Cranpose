use crate::{
    accessibility::{AccessibilityElement, AccessibilityRole, element_ids},
    android_wire_escape::escape_wire_field,
};

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
                AccessibilityRole::Dialog => 10,
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
            element_with(5, Some(1)),
        ];

        let payload = encode_elements(&elements, 2.0);
        let records: Vec<_> = payload.split('\n').collect();
        assert_eq!(records.len(), 2);
        for record in &records {
            assert_eq!(record.split('\t').count(), 17, "record: {record}");
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
    }
}
