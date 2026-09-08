pub(crate) fn android_uses_present_thread(requested: Option<&str>, available_cores: usize) -> bool {
    match requested.map(str::trim) {
        Some("1") | Some("true") | Some("on") => true,
        Some("0") | Some("false") | Some("off") => false,
        _ => available_cores >= 4,
    }
}

#[cfg(test)]
mod tests {
    use super::android_uses_present_thread;

    #[test]
    fn a_phone_with_idle_cores_overlaps_by_default() {
        assert!(android_uses_present_thread(None, 8));
        assert!(android_uses_present_thread(None, 6));
    }

    #[test]
    fn devices_with_fewer_than_four_cores_stay_synchronous() {
        assert!(!android_uses_present_thread(None, 3));
        assert!(!android_uses_present_thread(None, 2));
        assert!(!android_uses_present_thread(None, 1));
    }

    #[test]
    fn the_override_wins_in_both_directions_on_any_core_count() {
        assert!(android_uses_present_thread(Some("1"), 4));
        assert!(android_uses_present_thread(Some("on"), 1));
        assert!(!android_uses_present_thread(Some("0"), 8));
        assert!(!android_uses_present_thread(Some(" off "), 8));
    }

    #[test]
    fn an_unparsable_override_falls_back_to_the_core_default() {
        assert!(android_uses_present_thread(Some("maybe"), 8));
        assert!(android_uses_present_thread(Some(""), 4));
    }

    #[test]
    fn four_cores_allow_ui_preparation_and_rendering_to_overlap() {
        assert!(android_uses_present_thread(None, 4));
    }
}
