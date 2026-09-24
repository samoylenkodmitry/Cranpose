use super::*;

#[test]
fn test_initial_range() {
    let state = NearestRangeState::new(0);
    assert_eq!(state.range(), 0..130);
}

#[test]
fn test_range_after_small_scroll() {
    let mut state = NearestRangeState::new(0);
    state.update(5);
    assert_eq!(state.range(), 0..130);
}

#[test]
fn test_range_after_crossing_window() {
    let mut state = NearestRangeState::new(0);
    state.update(35);
    assert_eq!(state.range(), 0..160);
}

#[test]
fn test_range_far_scroll() {
    let mut state = NearestRangeState::new(0);
    state.update(1000);
    assert_eq!(state.range(), 890..1120);
}
