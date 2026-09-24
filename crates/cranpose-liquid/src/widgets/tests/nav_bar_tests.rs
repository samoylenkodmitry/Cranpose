use super::*;

#[test]
fn expanded_height_covers_the_inline_bar_and_its_large_title() {
    assert_eq!(
        liquid_nav_bar_expanded_height(),
        BAR_HEIGHT + LARGE_TITLE_HEIGHT,
        "content's top padding has to clear both rows"
    );
    assert!(liquid_nav_bar_expanded_height() > BAR_HEIGHT);
}

#[test]
fn a_rest_inside_the_collapse_band_snaps_to_the_nearer_edge() {
    let range = 52.0_f32;
    let settle = large_title_settle_policy(range);
    assert_eq!(settle(1.0, 0.0), 0.0, "just past expanded unfolds again");
    assert_eq!(
        settle(range - 1.0, 0.0),
        range,
        "nearly collapsed finishes collapsing"
    );
    assert_eq!(
        settle(range * 0.5, 0.0),
        range,
        "the midpoint commits to collapsed"
    );
    assert_eq!(
        settle(range * 0.5 - 0.1, 0.0),
        0.0,
        "a hair under it unfolds"
    );
}

#[test]
fn a_rest_outside_the_collapse_band_is_left_alone() {
    let range = 52.0_f32;
    let settle = large_title_settle_policy(range);
    assert_eq!(settle(0.0, 0.0), 0.0);
    assert_eq!(
        settle(-30.0, 0.0),
        -30.0,
        "an overscrolled top keeps its bounce"
    );
    assert_eq!(settle(range, 0.0), range);
    assert_eq!(settle(900.0, 0.0), 900.0, "a deep scroll is never rewound");
}

#[test]
fn a_degenerate_collapse_range_still_yields_a_usable_policy() {
    let settle = large_title_settle_policy(0.0);
    assert_eq!(
        settle(0.5, 0.0),
        1.0,
        "the range floors at one dp, not zero"
    );
    assert_eq!(settle(0.0, 0.0), 0.0);
}
