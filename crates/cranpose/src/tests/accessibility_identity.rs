use super::*;
use crate::accessibility::element_with;

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
        snapshot.update(vec![element_with(8, None)]),
        Err(AccessibilityIdentityError::Exhausted)
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
        snapshot.update(vec![element_with(8, Some(3)), element_with(8, Some(3))]),
        Err(AccessibilityIdentityError::Duplicate(8, Some(3)))
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
