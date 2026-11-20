use super::*;

const NATURAL: DistanceAttenuation = DistanceAttenuation::Natural { min_distance: 1.0 };

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-3
}

fn head() -> AudioListener {
    AudioListener::AT_ORIGIN
}

// The head turned by `degrees` to its left about the vertical axis
fn turned(degrees: f32) -> AudioListener {
    let (sin, cos) = (degrees.to_radians() / 2.0).sin_cos();
    AudioListener {
        orientation: [0.0, sin, 0.0, cos],
        ..AudioListener::AT_ORIGIN
    }
}

// The level of the far ear at the head's axis: 6 dB below the near ear
const FAR_AT_AXIS: f32 = 0.501_187;
// The Woodworth delay at the head's axis, in seconds: (0.0875 / 343) * (pi / 2 + 1)
const DELAY_AT_AXIS: f32 = 0.000_655_8;

#[test]
fn a_source_ahead_sounds_the_same_in_both_ears() {
    let placed = place(&head(), [0.0, 0.0, -1.0], NATURAL);
    assert!(close(placed.gain[0], 1.0) && close(placed.gain[1], 1.0));
    assert_eq!(placed.delay, [0.0, 0.0]);
    assert!(close(placed.shadow[0], placed.shadow[1]));
}

#[test]
fn a_source_to_the_left_is_louder_and_earlier_in_the_left_ear() {
    let placed = place(&head(), [-1.0, 0.0, 0.0], NATURAL);
    assert!(close(placed.gain[0], 1.0));
    assert!(close(placed.gain[1], FAR_AT_AXIS));
    assert_eq!(placed.delay[0], 0.0);
    assert!((placed.delay[1] - DELAY_AT_AXIS).abs() < 1.0e-6);
    // The near ear's shadow lifts the high frequencies by 6 dB, as the filter's alpha of 2 says
    assert!(close(placed.shadow[0], 2.0));
}

#[test]
fn a_source_to_the_right_is_the_mirror_of_one_to_the_left() {
    let left = place(&head(), [-1.0, 0.0, 0.0], NATURAL);
    let right = place(&head(), [1.0, 0.0, 0.0], NATURAL);
    assert_eq!(right.gain, [left.gain[1], left.gain[0]]);
    assert_eq!(right.delay, [left.delay[1], left.delay[0]]);
    assert!(close(right.shadow[0], left.shadow[1]) && close(right.shadow[1], left.shadow[0]));
}

#[test]
fn a_source_behind_sounds_as_one_ahead_does_because_the_model_has_no_front_and_back_cue() {
    let ahead = place(&head(), [0.0, 0.0, -1.0], NATURAL);
    let behind = place(&head(), [0.0, 0.0, 1.0], NATURAL);
    assert_eq!(ahead, behind);
}

#[test]
fn a_source_above_the_head_has_no_difference_between_the_ears() {
    let above = place(&head(), [0.0, 3.0, 0.0], DistanceAttenuation::None);
    assert_eq!(above.gain, [1.0, 1.0]);
    assert_eq!(above.delay, [0.0, 0.0]);
}

#[test]
fn a_source_at_thirty_degrees_has_half_the_sideways_part() {
    // The sine of 30 degrees is one half
    let placed = place(&head(), [0.5, 0.0, -0.866_025_4], NATURAL);
    let far = 10.0_f32.powf(-6.0 * 0.5 / 20.0);
    assert!(close(placed.gain[0], far) && close(placed.gain[1], 1.0));
    // (0.0875 / 343) * (asin(0.5) + 0.5) = 261 microseconds
    assert!((placed.delay[0] - 0.000_261).abs() < 1.0e-6);
}

#[test]
fn a_far_source_is_quieter_in_both_ears_by_its_curve() {
    let far = place(&head(), [0.0, 0.0, -10.0], NATURAL);
    assert!(close(far.gain[0], 0.1) && close(far.gain[1], 0.1));
    let none = place(&head(), [0.0, 0.0, -10.0], DistanceAttenuation::None);
    assert_eq!(none.gain, [1.0, 1.0]);
    // The distance gain scales the level difference. The delay stays as it is
    let far_left = place(&head(), [-10.0, 0.0, 0.0], NATURAL);
    assert!(close(far_left.gain[0], 0.1));
    assert!(close(far_left.gain[1], 0.1 * FAR_AT_AXIS));
    assert!((far_left.delay[1] - DELAY_AT_AXIS).abs() < 1.0e-6);
}

#[test]
fn a_source_in_the_head_sounds_in_the_middle_with_a_finite_gain() {
    let inside = place(&head(), [0.0; 3], NATURAL);
    assert_eq!(inside.gain, [1.0, 1.0]);
    assert_eq!(inside.delay, [0.0, 0.0]);
    assert!(inside.shadow.iter().all(|factor| factor.is_finite()));
}

#[test]
fn a_head_that_turns_left_moves_a_source_ahead_to_its_right() {
    // Turned 90 degrees to the left, the head looks along -x. What stood at -z is at its right
    let placed = place(&turned(90.0), [0.0, 0.0, -1.0], NATURAL);
    assert!(close(placed.gain[1], 1.0));
    assert!(close(placed.gain[0], FAR_AT_AXIS));
    assert!(placed.delay[0] > 0.0 && placed.delay[1] == 0.0);
    // Turned 90 degrees to the right, the same source is at the head's left
    let placed = place(&turned(-90.0), [0.0, 0.0, -1.0], NATURAL);
    assert!(close(placed.gain[0], 1.0));
    assert!(placed.delay[1] > 0.0 && placed.delay[0] == 0.0);
    // Turned all the way round, the source is behind the head. It sounds as one ahead does
    let placed = place(&turned(180.0), [0.0, 0.0, -1.0], NATURAL);
    assert!(close(placed.gain[0], 1.0) && close(placed.gain[1], 1.0));
}

#[test]
fn a_head_that_moves_changes_the_distance_and_the_side() {
    let listener = AudioListener {
        position: [2.0, 0.0, 0.0],
        ..AudioListener::AT_ORIGIN
    };
    // The source at the origin is 2 m to the left of a head that stands at x = 2
    let placed = place(&listener, [0.0, 0.0, 0.0], NATURAL);
    assert!(close(placed.gain[0], 0.5));
    assert!(close(placed.gain[1], 0.5 * FAR_AT_AXIS));
}

#[test]
fn the_shadow_is_lowest_for_an_ear_that_is_150_degrees_from_the_source() {
    // The right ear's axis is +x. A source 150 degrees from it has the cosine -0.866 there.
    let placed = place(&head(), [-0.866_025_4, 0.0, -0.5], NATURAL);
    assert!(close(placed.shadow[1], SHADOW_MIN_ALPHA));
    assert!(placed.shadow[1] < placed.shadow[0]);
}

#[test]
fn the_shadow_filter_keeps_low_frequencies_and_scales_high_ones_by_its_factor() {
    for factor in [0.1, 0.756, 1.0, 2.0] {
        let [b0, b1, a1] = shadow_coefficients(factor, 48_000.0);
        // At zero frequency z is 1, at half the sample rate it is -1
        let at_zero = (b0 + b1) / (1.0 + a1);
        let at_nyquist = (b0 - b1) / (1.0 - a1);
        assert!(close(at_zero, 1.0), "{factor}: {at_zero}");
        assert!(close(at_nyquist, factor), "{factor}: {at_nyquist}");
        assert!(a1.abs() < 1.0, "the filter is stable");
    }
}
