use super::*;

#[test]
fn test_normal_viewport() {
    let handler = ViewportHandler::new(500.0, 50.0, 0.0);
    assert_eq!(handler.effective_size(), 500.0);
    assert!(!handler.is_infinite());
}

#[test]
fn test_infinite_viewport() {
    let handler = ViewportHandler::new(f32::INFINITY, 50.0, 8.0);
    assert!(handler.is_infinite());
    assert_eq!(handler.effective_size(), 1160.0);
}

#[test]
fn test_huge_viewport_treated_as_infinite() {
    let handler = ViewportHandler::new(200_000.0, 50.0, 0.0);
    assert!(handler.is_infinite());
    assert!(handler.effective_size() < 100_000.0);
}

#[test]
fn test_uses_default_estimate_when_average_is_zero() {
    let handler = ViewportHandler::new(f32::INFINITY, 0.0, 0.0);
    assert!(handler.is_infinite());
    assert_eq!(handler.effective_size(), DEFAULT_ITEM_SIZE_ESTIMATE * 20.0);
}
