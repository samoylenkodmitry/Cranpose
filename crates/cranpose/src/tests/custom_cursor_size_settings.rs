use super::*;

#[test]
fn custom_cursors_follow_the_system_pointer_unless_the_app_says_otherwise() {
    assert_eq!(
        AppSettings::default().custom_cursor_size,
        CustomCursorSize::FollowSystem,
        "the person enlarged the pointer to see it"
    );
    assert_eq!(
        AppLauncher::new().settings.custom_cursor_size,
        CustomCursorSize::FollowSystem
    );
}

#[test]
fn an_app_can_keep_its_cursors_at_the_size_it_drew_them() {
    let settings = AppLauncher::new()
        .with_custom_cursor_size(CustomCursorSize::AsDrawn)
        .settings;

    assert_eq!(settings.custom_cursor_size, CustomCursorSize::AsDrawn);
}
