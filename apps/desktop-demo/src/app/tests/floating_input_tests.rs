use super::*;

#[test]
fn a_press_that_lets_go_where_it_began_is_a_click_and_a_drag_is_not() {
    let here = Some(Point::new(160.0, 140.0));
    let there = Some(Point::new(320.0, 230.0));
    assert_eq!(clicks_after_press(2, here, here), 3);
    assert_eq!(clicks_after_press(2, here, there), 2);
    assert_eq!(
        clicks_after_press(0, None, None),
        1,
        "a window not yet placed still takes a click"
    );
}
