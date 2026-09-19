use super::*;

#[test]
fn a_torn_tool_remembers_where_it_left_and_docks_back() {
    let mut places = ToolPlaces::default();
    places.tear(2, Point::new(10.0, 20.0));
    assert_eq!(places.torn_origin(2), Some(Point::new(10.0, 20.0)));
    assert_eq!(places.torn_origin(1), None);
    assert_eq!(places.torn_tools(), vec![2]);
    places.tear(2, Point::new(30.0, 40.0));
    assert_eq!(
        places.torn_tools(),
        vec![2],
        "tearing again moves, it does not duplicate"
    );
    places.dock(2);
    assert_eq!(places.torn_origin(2), None);
    assert!(places.torn_tools().is_empty());
}

#[test]
fn a_press_that_moves_past_the_tear_distance_puts_the_pane_under_the_pointer() {
    let pressed = Point::new(10.0, 5.0);
    let mut event = PointerEvent::new(
        PointerEventKind::Move,
        Point::new(40.0, 5.0),
        Point::new(40.0, 5.0),
    );
    assert_eq!(
        tear_origin(pressed, &event),
        None,
        "without a screen position the pane cannot be placed"
    );
    event = event.with_screen_position(Some(Point::new(240.0, 105.0)));
    assert_eq!(tear_origin(pressed, &event), Some(Point::new(200.0, 100.0)));
    let short = PointerEvent::new(
        PointerEventKind::Move,
        Point::new(15.0, 5.0),
        Point::new(15.0, 5.0),
    )
    .with_screen_position(Some(Point::new(215.0, 105.0)));
    assert_eq!(tear_origin(pressed, &short), None);
}
