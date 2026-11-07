use super::*;

#[test]
fn target_rects_intersect_to_the_pixels_both_keep() {
    assert_eq!(
        intersect_target_rects((0, 0, 10, 10), (5, 2, 10, 4)),
        Some((5, 2, 5, 4))
    );
    assert_eq!(intersect_target_rects((0, 0, 4, 4), (4, 0, 4, 4)), None);
}
