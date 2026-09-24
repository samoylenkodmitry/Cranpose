use super::*;

#[test]
fn test_approx_eq() {
    assert_approx_eq(100.0, 100.0, 0.1, "exact match");
    assert_approx_eq(100.05, 100.0, 0.1, "within tolerance");
}

#[test]
#[should_panic]
fn test_approx_eq_fails() {
    assert_approx_eq(100.5, 100.0, 0.1, "should fail");
}

#[test]
fn test_rect_approx_eq() {
    let rect1 = Rect {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 50.0,
    };
    let rect2 = Rect {
        x: 10.05,
        y: 20.05,
        width: 100.05,
        height: 50.05,
    };
    assert_rect_approx_eq(rect1, rect2, 0.1, "nearly equal rects");
}

#[test]
fn test_rect_contains_point() {
    let rect = Rect {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 50.0,
    };
    assert_rect_contains_point(rect, 50.0, 30.0, "center point");
    assert_rect_contains_point(rect, 10.0, 20.0, "top-left corner");
    assert_rect_contains_point(rect, 110.0, 70.0, "bottom-right corner");
}

#[test]
fn test_contains_text() {
    let texts = vec!["Hello".to_string(), "World".to_string()];
    assert_contains_text(&texts, "Hello", "exact match");
    assert_contains_text(&texts, "Wor", "partial match");
    assert_not_contains_text(&texts, "Goodbye", "not present");
}

#[test]
fn test_count() {
    let items = vec![1, 2, 3];
    assert_count(&items, 3, "correct count");
}

#[test]
fn test_bounds_center() {
    let bounds = Bounds {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 50.0,
    };
    let (cx, cy) = bounds.center();
    assert_eq!(cx, 60.0);
    assert_eq!(cy, 45.0);
}
