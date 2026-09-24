use super::*;

struct Packaged;

impl AppInfo for Packaged {
    fn version_name(&self) -> Option<String> {
        Some("1.4.2-debug".to_string())
    }

    fn build_version(&self) -> Option<String> {
        Some("17.2.1".to_string())
    }
}

#[test]
fn an_unpackaged_binary_has_no_version_to_report() {
    clear_platform_app_info();
    assert_eq!(version_name(), None);
    assert_eq!(build_version(), None);
}

#[test]
fn the_platform_answer_wins_and_carries_what_packaging_added() {
    clear_platform_app_info();
    set_platform_app_info(Rc::new(Packaged));
    assert_eq!(version_name().as_deref(), Some("1.4.2-debug"));
    assert_eq!(build_version().as_deref(), Some("17.2.1"));
    clear_platform_app_info();
    assert_eq!(version_name(), None);
}
