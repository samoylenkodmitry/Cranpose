use super::*;

fn bounds() -> Constraints {
    Constraints {
        min_width: 20.0,
        max_width: 200.0,
        min_height: 10.0,
        max_height: 100.0,
    }
}

#[test]
fn a_constraint_reports_which_of_its_axes_are_bounded() {
    assert!(bounds().has_bounded_width());
    assert!(bounds().has_bounded_height());

    let scrolling = bounds().copy_with_height(0.0, f32::INFINITY);
    assert!(scrolling.has_bounded_width());
    assert!(!scrolling.has_bounded_height());
    assert!(!scrolling.is_bounded());
}

#[test]
fn a_constraint_reports_which_of_its_axes_are_tight() {
    let loose = Constraints::loose(200.0, 100.0);
    assert!(!loose.has_tight_width());
    assert!(!loose.has_tight_height());
    assert!(!loose.is_tight());

    let tight = Constraints::tight(200.0, 100.0);
    assert!(tight.has_tight_width());
    assert!(tight.has_tight_height());
    assert!(tight.is_tight());

    let row_child = loose.tighten_height(100.0);
    assert!(!row_child.has_tight_width());
    assert!(row_child.has_tight_height());
    assert!(!row_child.is_tight());
}

#[test]
fn tightening_one_axis_leaves_the_other_alone() {
    let tightened = bounds().tighten_width(64.0);
    assert_eq!(tightened.min_width, 64.0);
    assert_eq!(tightened.max_width, 64.0);
    assert_eq!(tightened.min_height, 10.0);
    assert_eq!(tightened.max_height, 100.0);

    let tightened = bounds().tighten_height(64.0);
    assert_eq!(tightened.min_height, 64.0);
    assert_eq!(tightened.max_height, 64.0);
    assert_eq!(tightened.min_width, 20.0);
    assert_eq!(tightened.max_width, 200.0);
}

#[test]
fn replacing_one_axis_leaves_the_other_alone() {
    let replaced = bounds().copy_with_width(0.0, 50.0);
    assert_eq!((replaced.min_width, replaced.max_width), (0.0, 50.0));
    assert_eq!((replaced.min_height, replaced.max_height), (10.0, 100.0));

    let replaced = bounds().copy_with_height(5.0, 50.0);
    assert_eq!((replaced.min_height, replaced.max_height), (5.0, 50.0));
    assert_eq!((replaced.min_width, replaced.max_width), (20.0, 200.0));
}

#[test]
fn deflating_takes_padding_off_both_ends_and_stops_at_zero() {
    let inner = bounds().deflate(16.0, 8.0);
    assert_eq!(inner.min_width, 4.0);
    assert_eq!(inner.max_width, 184.0);
    assert_eq!(inner.min_height, 2.0);
    assert_eq!(inner.max_height, 92.0);

    let squeezed = Constraints::tight(10.0, 10.0).deflate(40.0, 40.0);
    assert_eq!(squeezed.min_width, 0.0);
    assert_eq!(squeezed.max_width, 0.0);
    assert_eq!(squeezed.min_height, 0.0);
    assert_eq!(squeezed.max_height, 0.0);
}

#[test]
fn loosening_drops_the_minimums_and_keeps_the_maximums() {
    let loosened = Constraints::tight(200.0, 100.0).loosen();
    assert_eq!(loosened.min_width, 0.0);
    assert_eq!(loosened.min_height, 0.0);
    assert_eq!(loosened.max_width, 200.0);
    assert_eq!(loosened.max_height, 100.0);
    assert!(!loosened.is_tight());
}
