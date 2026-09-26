use super::{all_finite, at_least, within};

const SAMPLES: [f32; 9] = [
    f32::NAN,
    f32::NEG_INFINITY,
    -3.5,
    -0.0,
    0.0,
    f32::MIN_POSITIVE,
    2.25,
    f32::MAX,
    f32::INFINITY,
];

fn same(a: f32, b: f32) -> bool {
    (a.is_nan() && b.is_nan()) || a == b
}

#[test]
fn at_least_is_max_for_every_floor_that_is_not_nan() {
    for value in SAMPLES {
        for floor in SAMPLES.into_iter().filter(|floor| !floor.is_nan()) {
            assert!(
                same(at_least(value, floor), value.max(floor)),
                "at_least({value}, {floor})"
            );
        }
    }
}

#[test]
fn a_nan_value_gives_the_floor() {
    assert_eq!(at_least(f32::NAN, 0.0), 0.0);
    assert_eq!(at_least(f32::NAN, -1.0), -1.0);
}

#[test]
fn within_is_clamp_for_ordered_bounds_that_are_not_nan() {
    for value in SAMPLES {
        for low in SAMPLES.into_iter().filter(|low| !low.is_nan()) {
            for high in SAMPLES
                .into_iter()
                .filter(|high| !high.is_nan() && *high >= low)
            {
                let clamped = within(value, low, high);
                assert!(
                    same(clamped, value.clamp(low, high)),
                    "within({value}, {low}, {high})"
                );
                assert_eq!(
                    clamped.is_sign_negative(),
                    value.clamp(low, high).is_sign_negative(),
                    "within({value}, {low}, {high}) keeps the sign of zero"
                );
            }
        }
    }
}

#[test]
fn all_finite_rejects_any_nan_or_infinity() {
    for a in SAMPLES {
        for b in SAMPLES {
            assert_eq!(
                all_finite([a, 1.0, b]),
                a.is_finite() && b.is_finite(),
                "all_finite([{a}, 1, {b}])"
            );
        }
    }
    assert!(all_finite([f32::MAX, f32::MAX, -f32::MAX]));
    assert!(all_finite::<0>([]));
}
