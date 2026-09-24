use super::*;

#[test]
fn the_ring_stays_inside_the_control() {
    let [(outer, outer_width), (inner, inner_width)] = focus_ring_lines(Size {
        width: 48.0,
        height: 20.0,
    });
    assert_eq!((outer_width, inner_width), (2.0, 1.0));
    assert_eq!(
        (outer.x, outer.y, outer.width, outer.height),
        (1.0, 1.0, 46.0, 18.0)
    );
    assert_eq!(
        (inner.x, inner.y, inner.width, inner.height),
        (2.5, 2.5, 43.0, 15.0)
    );
}

#[test]
fn a_ring_around_nothing_has_no_width() {
    let [(outer, _), (inner, _)] = focus_ring_lines(Size {
        width: 1.0,
        height: 1.0,
    });
    assert_eq!(
        (outer.width, outer.height, inner.width, inner.height),
        (0.0, 0.0, 0.0, 0.0)
    );
}
