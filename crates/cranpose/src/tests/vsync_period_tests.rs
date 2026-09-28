use super::*;

const VSYNC: i64 = 16_666_667;

#[test]
fn a_gap_of_several_vsyncs_keeps_the_period() {
    assert_eq!(
        refine_vsync_period(VSYNC, 2 * VSYNC + 50_000),
        VSYNC + 25_000
    );
    assert_eq!(refine_vsync_period(VSYNC, 3 * VSYNC), VSYNC);
    assert_eq!(refine_vsync_period(VSYNC, VSYNC + 100_000), VSYNC + 100_000);
}

#[test]
fn a_first_gap_spanning_two_vsyncs_is_corrected_by_the_next_single_one() {
    let first = refine_vsync_period(0, 2 * VSYNC);
    assert_eq!(first, 2 * VSYNC);
    assert_eq!(refine_vsync_period(first, VSYNC), VSYNC);
}

#[test]
fn a_display_that_changes_rate_replaces_the_period() {
    let hz_90 = 11_111_111;
    let hz_120 = 8_333_333;
    assert_eq!(refine_vsync_period(VSYNC, hz_90), hz_90);
    assert_eq!(refine_vsync_period(hz_90, VSYNC), VSYNC);
    assert_eq!(refine_vsync_period(VSYNC, hz_120), hz_120);
}

#[test]
fn an_implausible_gap_leaves_the_period_alone() {
    assert_eq!(refine_vsync_period(0, 1_000_000), 0);
    assert_eq!(refine_vsync_period(0, 200_000_000), 0);
    assert_eq!(refine_vsync_period(VSYNC, 1_000_000), VSYNC);
    assert_eq!(refine_vsync_period(VSYNC, 10 * VSYNC), VSYNC);
}

#[test]
fn the_next_vsync_is_the_first_on_the_grid_after_the_time_asked() {
    let vsync = 1_000 * VSYNC;
    assert_eq!(next_vsync_ns(vsync, vsync, VSYNC), Some(vsync + VSYNC));
    assert_eq!(next_vsync_ns(vsync + 1, vsync, VSYNC), Some(vsync + VSYNC));
    assert_eq!(
        next_vsync_ns(vsync + 5 * VSYNC / 2, vsync, VSYNC),
        Some(vsync + 3 * VSYNC),
        "a stale vsync still fixes the grid"
    );
    assert_eq!(
        next_vsync_ns(vsync - VSYNC / 2, vsync, VSYNC),
        Some(vsync),
        "a time before the vsync reaches it"
    );
}

#[test]
fn no_next_vsync_is_known_without_a_vsync_or_a_period() {
    assert_eq!(next_vsync_ns(VSYNC, 0, VSYNC), None);
    assert_eq!(next_vsync_ns(VSYNC, VSYNC, 0), None);
}
