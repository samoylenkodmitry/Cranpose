use std::rc::Rc;

use cranpose_services::{
    AppInfo, app_info, build_version, clear_platform_app_info, set_platform_app_info, version_name,
};

struct PackagedIdentity;

impl AppInfo for PackagedIdentity {
    fn version_name(&self) -> Option<String> {
        Some("2.4.0-debug".to_string())
    }

    fn build_version(&self) -> Option<String> {
        Some("42.3.1".to_string())
    }
}

#[test]
fn public_api_preserves_packaged_version_strings() {
    clear_platform_app_info();
    set_platform_app_info(Rc::new(PackagedIdentity));

    assert_eq!(version_name().as_deref(), Some("2.4.0-debug"));
    assert_eq!(build_version().as_deref(), Some("42.3.1"));
    assert_eq!(app_info().build_version().as_deref(), Some("42.3.1"));

    clear_platform_app_info();
}
