use super::*;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-5
}

#[test]
fn auto_falls_as_the_inverse_of_the_distance_from_one_meter() {
    let auto = DistanceAttenuation::Auto;
    assert!(close(auto.gain(0.0), 1.0));
    assert!(close(auto.gain(1.0), 1.0));
    assert!(close(auto.gain(2.0), 0.5));
    assert!(close(auto.gain(10.0), 0.1));
}

#[test]
fn none_keeps_the_level_at_any_distance() {
    assert!(close(DistanceAttenuation::None.gain(500.0), 1.0));
}

#[test]
fn natural_starts_at_its_min_distance_and_keeps_a_floor_for_the_min() {
    let natural = DistanceAttenuation::Natural { min_distance: 2.0 };
    assert!(close(natural.gain(1.0), 1.0));
    assert!(close(natural.gain(4.0), 0.5));
    // A min distance below 0.1 counts as 0.1, so a source at the head is not infinitely loud
    let tiny = DistanceAttenuation::Natural { min_distance: 0.0 };
    assert!(close(tiny.gain(0.0), 1.0));
    assert!(close(tiny.gain(1.0), 0.1));
}

#[test]
fn linear_falls_in_a_straight_line_and_holds_its_floor() {
    let linear = DistanceAttenuation::Linear {
        min_distance: 1.0,
        max_distance: 5.0,
        gain_at_max_distance: 0.2,
    };
    assert!(close(linear.gain(0.5), 1.0));
    assert!(close(linear.gain(1.0), 1.0));
    assert!(close(linear.gain(3.0), 0.6));
    assert!(close(linear.gain(5.0), 0.2));
    assert!(close(linear.gain(500.0), 0.2));
}

#[test]
fn a_curve_with_nonsense_numbers_still_gives_a_gain_between_zero_and_one() {
    let linear = DistanceAttenuation::Linear {
        min_distance: f32::NAN,
        max_distance: 0.0,
        gain_at_max_distance: 7.0,
    };
    for distance in [0.0, 0.05, 1.0, f32::INFINITY, f32::NAN] {
        let gain = linear.gain(distance);
        assert!((0.0..=1.0).contains(&gain), "{distance} gave {gain}");
    }
}

#[test]
fn a_curve_survives_its_words() {
    for curve in [
        DistanceAttenuation::Auto,
        DistanceAttenuation::None,
        DistanceAttenuation::Natural { min_distance: 3.0 },
        DistanceAttenuation::Linear {
            min_distance: 0.5,
            max_distance: 9.0,
            gain_at_max_distance: 0.25,
        },
    ] {
        assert_eq!(DistanceAttenuation::from_words(curve.to_words()), curve);
    }
}
