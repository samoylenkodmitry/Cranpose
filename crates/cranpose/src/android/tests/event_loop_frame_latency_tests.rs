use super::android_frame_latency;

#[test]
fn the_platform_depth_is_what_an_app_gets_without_asking() {
    assert_eq!(android_frame_latency(None), 3);
}

#[test]
fn the_switch_takes_any_depth_the_surface_can_hold() {
    for (spelling, depth) in [("1", 1), (" 2 ", 2), ("3", 3)] {
        assert_eq!(android_frame_latency(Some(spelling)), depth);
    }
}

#[test]
fn a_depth_the_surface_cannot_hold_still_starts_the_app() {
    for mistyped in ["0", "4", "two", ""] {
        assert_eq!(android_frame_latency(Some(mistyped)), 3, "{mistyped:?}");
    }
}
