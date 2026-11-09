use super::*;

#[test]
fn the_frame_overlay_is_drawn_on_the_applications_own_window_only() {
    assert!(dev_overlay_belongs_on(RootId::Primary, true));
    assert!(
        !dev_overlay_belongs_on(RootId::Window(NodeId(7)), true),
        "a window of its own would repeat the application's numbers over content it does not own"
    );
    assert!(!dev_overlay_belongs_on(RootId::Primary, false));
}
