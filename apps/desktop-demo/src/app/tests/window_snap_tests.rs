use std::collections::HashMap;

use cranpose_ui::{Point, Size};

use super::{carried_by, lined_up_with_neighbours, touching, Pane};

fn pane(x: f32, y: f32, width: f32, height: f32) -> Pane {
    Pane {
        origin: Point::new(x, y),
        size: Size::new(width, height),
    }
}

#[test]
fn a_window_let_go_near_an_edge_lines_up_with_it() {
    let above = pane(100.0, 100.0, 275.0, 116.0);
    let dropped = pane(104.0, 220.0, 275.0, 232.0);
    assert_eq!(
        lined_up_with_neighbours(dropped, &[above]),
        Point::new(100.0, 216.0),
        "a window put down within reach of the one above meets its lower edge and its side"
    );
}

#[test]
fn a_window_let_go_well_clear_of_everything_stays_where_it_was_put() {
    let above = pane(100.0, 100.0, 275.0, 116.0);
    let dropped = pane(600.0, 600.0, 275.0, 232.0);
    assert_eq!(
        lined_up_with_neighbours(dropped, &[above]),
        Point::new(600.0, 600.0),
        "reaching for an edge that is nowhere near would move a window the user placed"
    );
}

#[test]
fn windows_that_only_touch_at_a_corner_are_not_attached() {
    let first = pane(0.0, 0.0, 100.0, 100.0);
    let corner = pane(100.0, 100.0, 100.0, 100.0);
    assert!(
        !touching(first, corner),
        "a corner is not an edge, and dragging one must not carry the other"
    );
    assert!(touching(first, pane(100.0, 40.0, 100.0, 100.0)));
    assert!(touching(first, pane(0.0, 100.0, 100.0, 100.0)));
}

#[test]
fn a_window_carries_the_ones_attached_to_it_and_what_is_attached_to_those() {
    let mut panes = HashMap::new();
    panes.insert(1, pane(0.0, 0.0, 100.0, 100.0));
    panes.insert(2, pane(0.0, 100.0, 100.0, 100.0));
    panes.insert(3, pane(0.0, 200.0, 100.0, 100.0));
    panes.insert(4, pane(500.0, 500.0, 100.0, 100.0));

    let mut carried = carried_by(1, &panes);
    carried.sort_unstable();
    assert_eq!(
        carried,
        vec![2, 3],
        "the third window is attached through the second, and travels with the first"
    );
    assert!(
        carried_by(4, &panes).is_empty(),
        "a window off on its own carries nobody"
    );
}
