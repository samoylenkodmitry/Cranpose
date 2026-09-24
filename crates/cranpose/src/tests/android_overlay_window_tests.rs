use super::*;

#[test]
fn logical_to_physical_rounds_by_density() {
    assert_eq!(logical_to_physical_px(10.4, 2.0), Ok(21));
    assert_eq!(logical_to_physical_px(-3.0, 3.0), Ok(-9));
}

#[test]
fn logical_to_physical_rejects_non_finite() {
    assert!(logical_to_physical_px(f32::NAN, 2.0).is_err());
    assert!(logical_to_physical_px(12.0, f32::INFINITY).is_err());
    assert!(logical_to_physical_px(12.0, 0.0).is_err());
}

#[test]
fn logical_dimension_to_physical_clamps_to_visible_pixel() {
    assert_eq!(logical_dimension_to_physical_px(0.1, 1.0), Ok(1));
}

#[test]
fn overlay_options_validate_positive_size() {
    assert!(AndroidOverlayWindowOptions::new(100, 50).is_valid());
    assert!(!AndroidOverlayWindowOptions::new(0, 50).is_valid());
    assert!(!AndroidOverlayWindowOptions::new(100, 0).is_valid());
}

#[test]
fn overlay_options_to_physical_bounds_uses_initial_position_and_size() {
    let options = AndroidOverlayWindowOptions::new(100, 50).with_position(-4, 8);
    let bounds = overlay_options_to_physical_bounds(options, 2.0).unwrap();

    assert_eq!(
        bounds,
        AndroidOverlayWindowBounds {
            width_px: 200,
            height_px: 100,
            x_px: -8,
            y_px: 16,
        }
    );
}

#[test]
fn overlay_bounds_to_physical_uses_runtime_position_and_size() {
    let bounds =
        overlay_bounds_to_physical(Point::new(12.25, -4.5), Size::new(200.0, 80.0), 2.0).unwrap();

    assert_eq!(
        bounds,
        AndroidOverlayWindowBounds {
            width_px: 400,
            height_px: 160,
            x_px: 25,
            y_px: -9,
        }
    );
}

#[test]
fn overlay_events_are_isolated_by_queue() {
    let first = std::sync::Arc::new(AndroidOverlayEventQueue::default());
    let second = std::sync::Arc::new(AndroidOverlayEventQueue::default());

    first.push(AndroidOverlayWindowEvent::CreateFailed("first".to_string()));
    second.push(AndroidOverlayWindowEvent::CreateFailed(
        "second".to_string(),
    ));

    assert_create_failed_events(drain_android_overlay_window_events(&first), &["first"]);
    assert_create_failed_events(drain_android_overlay_window_events(&second), &["second"]);
    assert!(drain_android_overlay_window_events(&first).is_empty());
    assert!(drain_android_overlay_window_events(&second).is_empty());
}

fn assert_create_failed_events(events: Vec<AndroidOverlayWindowEvent>, expected: &[&str]) {
    let messages: Vec<String> = events
        .into_iter()
        .map(|event| match event {
            AndroidOverlayWindowEvent::CreateFailed(message) => message,
            other => panic!("expected create failure event, got {other:?}"),
        })
        .collect();
    assert_eq!(messages, expected);
}
