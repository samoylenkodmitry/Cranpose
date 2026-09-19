use super::*;

const DROP: Point = Point { x: 300.0, y: 200.0 };

fn two_pages() -> TabWindows {
    let mut windows = TabWindows::new(1);
    windows.add_page(1, 2);
    windows
}

#[test]
fn tearing_a_page_opens_a_window_where_it_was_dropped() {
    let mut windows = two_pages();
    assert_eq!(windows.tear_page(2, DROP), Some(2));
    assert_eq!(windows.window_of(2), Some(2));
    assert_eq!(windows.windows()[0].pages, vec![1]);
    assert_eq!(windows.windows()[1].origin, DROP);
    assert_eq!(windows.windows()[1].active, 2);
}

#[test]
fn the_only_page_of_a_window_stays_when_dropped_outside() {
    let mut windows = TabWindows::new(1);
    assert_eq!(windows.tear_page(1, DROP), None);
    assert_eq!(windows.windows().len(), 1);
}

#[test]
fn moving_the_last_page_out_closes_its_window() {
    let mut windows = two_pages();
    windows.tear_page(2, DROP);
    assert!(windows.move_page(1, 2));
    assert_eq!(windows.windows().len(), 1);
    assert_eq!(windows.windows()[0].id, 2);
    assert_eq!(windows.windows()[0].pages, vec![2, 1]);
    assert_eq!(windows.windows()[0].active, 1);
}

#[test]
fn a_page_dropped_on_its_own_strip_is_accepted_and_stays() {
    let mut windows = two_pages();
    assert!(windows.move_page(2, 1));
    assert_eq!(windows.windows()[0].pages, vec![1, 2]);
    assert!(!windows.move_page(2, 9), "no such window");
}

#[test]
fn closing_the_active_page_activates_a_neighbour_and_the_last_close_removes_the_window() {
    let mut windows = two_pages();
    windows.activate(2);
    windows.close_page(2);
    assert_eq!(windows.windows()[0].active, 1);
    windows.close_page(1);
    assert!(windows.windows().is_empty());
}
