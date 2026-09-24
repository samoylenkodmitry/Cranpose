use super::*;

#[test]
fn a_pointer_event_carries_the_logical_position_as_both_of_its_points() {
    let platform = DesktopWinitPlatform::new(2.0);
    let event = platform.pointer_event(
        PointerEventKind::Move,
        PhysicalPosition { x: 100.0, y: 50.0 },
    );
    assert_eq!(event.position, Point { x: 50.0, y: 25.0 });
    assert_eq!(event.position, event.global_position);
    assert_eq!(event.kind, PointerEventKind::Move);
}

#[test]
fn line_scroll_delta_is_scaled_to_pixels() {
    let platform = DesktopWinitPlatform::new(1.0);
    let delta = platform.scroll_delta(MouseScrollDelta::LineDelta(1.0, -2.0));
    assert_eq!(delta, Some(Point { x: 40.0, y: -80.0 }));
}

#[test]
fn pixel_scroll_delta_is_converted_to_logical_space() {
    let platform = DesktopWinitPlatform::new(2.0);
    let delta = platform.scroll_delta(MouseScrollDelta::PixelDelta(PhysicalPosition {
        x: 24.0,
        y: -10.0,
    }));
    assert_eq!(delta, Some(Point { x: 12.0, y: -5.0 }));
}
