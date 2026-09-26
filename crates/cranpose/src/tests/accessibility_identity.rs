use super::*;
use crate::accessibility::element_with;

#[test]
fn recycled_node_generations_never_retain_accessibility_ids() {
    for canvas_key in [None, Some(5)] {
        let mut snapshot = AccessibilitySnapshot::default();
        let original = element_with(7, canvas_key);
        snapshot
            .update(vec![original.clone()])
            .expect("initial control");
        let old_id = snapshot.ids[0];
        let replacement = AccessibilityElement {
            node_generation: 1,
            ..original
        };
        snapshot
            .update(vec![replacement])
            .expect("recycled control");
        assert_ne!(snapshot.ids[0], old_id);
        assert!(snapshot.element(old_id).is_none());
    }
}

#[test]
fn recycled_generation_updates_structure_and_announcements() {
    use crate::accessibility::{
        AccessibilityRole, live_region_announcements, opened_dialog, pane_title_announcements,
        spoken_changes, voiceover_same_structure,
    };
    let original = AccessibilityElement {
        node_id: 7,
        label: "Receipt".into(),
        role: AccessibilityRole::Dialog,
        live_region: Some(cranpose_ui::LiveRegionMode::Polite),
        pane_title: Some("Receipt".into()),
        ..Default::default()
    };
    let replacement = AccessibilityElement {
        node_generation: 1,
        ..original.clone()
    };
    let previous = [original];
    let current = [replacement];
    assert!(!voiceover_same_structure(&previous, &current));
    assert_eq!(opened_dialog(&previous, &current), Some(7));
    assert_eq!(live_region_announcements(&previous, &current).len(), 1);
    assert_eq!(pane_title_announcements(&previous, &current).len(), 1);
    assert_eq!(spoken_changes(&previous, &current), vec![false]);
}

#[test]
fn removed_reader_cursor_starts_at_the_first_named_visible_control() {
    use crate::accessibility::{AccessibilityRect, voiceover_replacement_focus};
    let unnamed = AccessibilityElement {
        label: String::new(),
        ..element_with(1, None)
    };
    let hidden = AccessibilityElement {
        label: "Hidden".into(),
        bounds: AccessibilityRect::default(),
        ..element_with(2, None)
    };
    let back = AccessibilityElement {
        label: "Back".into(),
        bounds: AccessibilityRect {
            x: 0.0,
            y: 0.0,
            width: 44.0,
            height: 44.0,
        },
        ..element_with(3, None)
    };
    let elements = [unnamed, hidden, back];
    assert_eq!(
        voiceover_replacement_focus(&elements, &[10, 11, 12], Some(9)),
        Some(12)
    );
    assert_eq!(
        voiceover_replacement_focus(&elements, &[10, 11, 12], Some(12)),
        None
    );
    assert_eq!(
        voiceover_replacement_focus(&elements, &[10, 11, 12], None),
        None
    );
}

#[test]
fn removed_ids_never_target_replacements_or_reappearing_controls() {
    let mut snapshot = AccessibilitySnapshot::default();
    snapshot
        .update(vec![element_with(0, None), element_with(1, None)])
        .expect("unique identities");
    let removed = snapshot.ids[0];
    let retained = snapshot.ids[1];
    snapshot
        .update(vec![element_with(1, None), element_with(2, None)])
        .expect("unique identities");
    assert!(snapshot.element(removed).is_none());
    assert_eq!(snapshot.ids[0], retained);
    assert_ne!(snapshot.ids[1], removed);
    snapshot.update(vec![]).expect("empty tree");
    assert!(snapshot.element(retained).is_none());
    snapshot
        .update(vec![element_with(0, None)])
        .expect("reappearing control");
    assert_ne!(snapshot.ids[0], removed);
}

#[test]
fn exhaustion_preserves_the_published_snapshot_and_rejects_id_reuse() {
    let mut snapshot = AccessibilitySnapshot {
        last_id: i32::MAX - 1,
        ..Default::default()
    };
    snapshot
        .update(vec![element_with(7, None)])
        .expect("last available ID");
    assert_eq!(snapshot.ids, vec![i32::MAX]);
    assert_eq!(
        snapshot.update(vec![element_with(8, None)]).err(),
        Some(AccessibilityIdentityError::Exhausted)
    );
    assert_eq!(snapshot.identity(i32::MAX), Some((7, None)));
    snapshot
        .update(vec![element_with(7, None)])
        .expect("retained ID needs no allocation");
}

#[test]
fn duplicate_identities_are_rejected_without_changing_the_published_tree() {
    let mut snapshot = AccessibilitySnapshot::default();
    snapshot
        .update(vec![element_with(7, None)])
        .expect("unique identity");
    let id = snapshot.ids[0];
    assert_eq!(
        snapshot
            .update(vec![element_with(8, Some(3)), element_with(8, Some(3))])
            .err(),
        Some(AccessibilityIdentityError::Duplicate(8, Some(3)))
    );
    assert_eq!(snapshot.identity(id), Some((7, None)));
    assert_eq!(snapshot.last_id, id);
}

#[test]
fn snapshot_identity_is_independent_of_state_and_reading_order() {
    let mut snapshot = AccessibilitySnapshot::default();
    snapshot
        .update(vec![element_with(1, None), element_with(2, None)])
        .expect("unique identities");
    let ids = snapshot.ids.clone();
    let mut second = element_with(2, None);
    second.label = "Updated".into();
    second.focused = true;
    snapshot
        .update(vec![second, element_with(1, None)])
        .expect("updated state");
    assert_eq!(snapshot.ids, vec![ids[1], ids[0]]);
    assert!(
        snapshot
            .element(ids[1])
            .is_some_and(|element| element.focused && element.label == "Updated")
    );
}

#[test]
fn an_update_hands_back_the_snapshot_it_replaced() {
    let mut snapshot = AccessibilitySnapshot::default();
    snapshot
        .update(vec![element_with(7, None)])
        .expect("unique identity");
    let first = snapshot.ids.clone();
    let replaced = snapshot
        .update(vec![element_with(8, None)])
        .expect("unique identity");
    assert_eq!(replaced.ids, first);
    assert_eq!(replaced.elements, vec![element_with(7, None)]);
    assert_ne!(snapshot.ids, first);
}
