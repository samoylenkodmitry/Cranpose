use super::*;

#[test]
fn inspector_defaults_to_debug_builds_and_can_be_overridden() {
    assert_eq!(
        AppSettings::default().developer_inspector,
        cfg!(debug_assertions)
    );
    assert!(
        AppLauncher::new()
            .with_developer_inspector(true)
            .settings
            .developer_inspector
    );
    assert!(
        !AppLauncher::new()
            .with_developer_inspector(false)
            .settings
            .developer_inspector
    );
}
