use super::uses_present_thread;

#[test]
fn a_phone_with_idle_cores_overlaps_by_default() {
    assert!(uses_present_thread(None, 8));
    assert!(uses_present_thread(None, 6));
}

#[test]
fn devices_with_fewer_than_four_cores_stay_synchronous() {
    assert!(!uses_present_thread(None, 3));
    assert!(!uses_present_thread(None, 2));
    assert!(!uses_present_thread(None, 1));
}

#[test]
fn the_override_wins_in_both_directions_on_any_core_count() {
    assert!(uses_present_thread(Some("1"), 4));
    assert!(uses_present_thread(Some("on"), 1));
    assert!(!uses_present_thread(Some("0"), 8));
    assert!(!uses_present_thread(Some(" off "), 8));
}

#[test]
fn an_unparsable_override_falls_back_to_the_core_default() {
    assert!(uses_present_thread(Some("maybe"), 8));
    assert!(uses_present_thread(Some(""), 4));
}

#[test]
fn four_cores_allow_ui_preparation_and_rendering_to_overlap() {
    assert!(uses_present_thread(None, 4));
}
