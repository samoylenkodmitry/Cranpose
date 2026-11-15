// A shell that no platform runner shows in a window keeps what it copies to
// itself: tests that run side by side never meet in the system clipboard, and
// a test run leaves the developer's clipboard alone.

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    Modifier, Text, TextStyle,
    clipboard_session::{clipboard_read_text, clipboard_write_text, has_platform_clipboard},
};

fn headless_shell() -> AppShell<TestRenderer> {
    AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {
            Text("hello", Modifier::empty(), TextStyle::default());
        },
    )
}

#[test]
fn a_copy_in_one_headless_shell_never_reaches_another() {
    let copier = headless_shell();
    let reader = headless_shell();
    copier
        .app_context()
        .enter(|| clipboard_write_text("copied by another test"));
    assert_eq!(reader.app_context().enter(clipboard_read_text), None);
    assert_eq!(
        copier.app_context().enter(clipboard_read_text).as_deref(),
        Some("copied by another test"),
        "a shell still pastes what it copied"
    );
    assert!(!copier.app_context().enter(has_platform_clipboard));
}
