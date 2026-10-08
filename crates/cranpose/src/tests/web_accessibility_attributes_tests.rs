use super::*;

/// A field that was disabled and said it was required comes back enabled
/// and with no state: its node removes what the field lost, writes the one
/// value that changed, and leaves the attributes the mirror's listeners and
/// placement own alone.
#[test]
fn attributes_a_control_loses_are_removed_and_listener_attributes_stay() {
    let mut was = AccessibilityElement {
        node_id: 3,
        label: "Name".into(),
        value: Some("Ada".into()),
        role: AccessibilityRole::TextField,
        enabled: false,
        focusable: true,
        tab_stop: true,
        ..Default::default()
    };
    was.update_details(|details| {
        details.text_selection = Some((3, 3));
        details.state_description = Some("Required".into());
    });
    let mut now = was.clone();
    now.enabled = true;
    now.update_details(|details| details.state_description = None);
    let mut shown = MirrorAttributes::default();
    let mut next = MirrorAttributes::default();
    shown.describe(5, &was, None);
    next.describe(5, &now, None);

    let mut dropped: Vec<_> = next.dropped_from(&shown).collect();
    dropped.sort_unstable();
    assert_eq!(dropped, ["aria-description", "aria-disabled", "disabled"]);
    assert_eq!(
        next.written_over(&shown).collect::<Vec<_>>(),
        [("tabindex", "0")]
    );
    for owned in [
        "style",
        "data-cranpose-selection",
        "data-cranpose-composition",
    ] {
        assert!(
            shown
                .iter()
                .chain(next.iter())
                .all(|(name, _)| name != owned)
        );
    }
}
