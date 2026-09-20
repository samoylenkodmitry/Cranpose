use super::*;

#[test]
fn inspector_defaults_to_debug_builds_and_can_be_overridden() {
    assert_eq!(AppSettings::default().developer_inspector, None);
    assert_eq!(
        AppLauncher::new()
            .with_developer_inspector(true)
            .settings
            .developer_inspector,
        Some(true)
    );
    assert_eq!(
        AppLauncher::new()
            .with_developer_inspector(false)
            .settings
            .developer_inspector,
        Some(false)
    );
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    feature = "robot"
))]
#[test]
fn robot_driver_defaults_inspector_off_and_preserves_explicit_choices() {
    assert_eq!(
        AppLauncher::new()
            .with_test_driver(|_| {})
            .settings
            .developer_inspector,
        Some(false)
    );
    for enabled in [false, true] {
        let before = AppLauncher::new()
            .with_developer_inspector(enabled)
            .with_test_driver(|_| {});
        let after = AppLauncher::new()
            .with_test_driver(|_| {})
            .with_developer_inspector(enabled);
        assert_eq!(before.settings.developer_inspector, Some(enabled));
        assert_eq!(after.settings.developer_inspector, Some(enabled));
    }
}
