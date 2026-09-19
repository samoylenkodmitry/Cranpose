use super::surface_present_required;

#[test]
fn present_is_required_until_surface_is_clean() {
    assert!(surface_present_required(true, false, false));
    assert!(surface_present_required(false, true, false));
    assert!(surface_present_required(false, false, true));
    assert!(!surface_present_required(false, false, false));
}
