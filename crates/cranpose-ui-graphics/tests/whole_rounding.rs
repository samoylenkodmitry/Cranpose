use cranpose_ui_graphics::whole;

/// Values where rounding goes wrong first: signed zeros, halves either side
/// of zero, the float just below a half, whole numbers near 2^23 and past
/// it, the extremes, infinities and NaN.
fn edge_values() -> Vec<f32> {
    let mut values = vec![
        0.0,
        -0.0,
        0.5,
        -0.5,
        0.499_999_97,
        -0.499_999_97,
        1.5,
        -1.5,
        2.5,
        -2.5,
        1.0 - f32::EPSILON / 2.0,
        -(1.0 - f32::EPSILON / 2.0),
        8_388_607.0,
        8_388_607.5,
        -8_388_607.5,
        8_388_608.0,
        16_777_217.0,
        f32::MIN_POSITIVE,
        -f32::MIN_POSITIVE,
        f32::MAX,
        f32::MIN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    values.extend(values.clone().into_iter().map(f32::next_up));
    values.extend(values.clone().into_iter().map(f32::next_down));
    values
}

/// Every float a thousandth apart across the range layout and pixels use,
/// both signs.
fn sweep() -> impl Iterator<Item = f32> {
    (-2_000_000..=2_000_000).map(|step| step as f32 / 1000.0)
}

fn assert_same(name: &str, ours: fn(f32) -> f32, theirs: fn(f32) -> f32) {
    for value in edge_values().into_iter().chain(sweep()) {
        let (ours, theirs) = (ours(value), theirs(value));
        assert!(
            ours.to_bits() == theirs.to_bits() || (ours.is_nan() && theirs.is_nan()),
            "{name}({value:?}) gave {ours:?}, f32::{name} gives {theirs:?}",
        );
    }
}

#[test]
fn floor_matches_f32_floor() {
    assert_same("floor", whole::floor, f32::floor);
}

#[test]
fn ceil_matches_f32_ceil() {
    assert_same("ceil", whole::ceil, f32::ceil);
}

#[test]
fn round_matches_f32_round() {
    assert_same("round", whole::round, f32::round);
}

#[test]
fn trunc_matches_f32_trunc() {
    assert_same("trunc", whole::trunc, f32::trunc);
}

#[test]
fn fract_matches_f32_fract() {
    assert_same("fract", whole::fract, f32::fract);
}

// Halves either side of zero, the double just below a half, the ends of
// `i32` and of exact whole doubles, the extremes, infinities, and the
// sixteenths the device snap rounds, each with its neighbours.
#[test]
fn round_f64_matches_f64_round() {
    let mut values = vec![
        0.0,
        -0.0,
        0.5,
        -0.5,
        1.5,
        -1.5,
        2.5,
        -2.5,
        0.499_999_999_999_999_94,
        -0.499_999_999_999_999_94,
        2_147_483_647.5,
        -2_147_483_648.5,
        2_147_483_648.0,
        4_503_599_627_370_495.5,
        -4_503_599_627_370_495.5,
        9_007_199_254_740_993.0,
        f64::MAX,
        f64::MIN,
        f64::MIN_POSITIVE,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ];
    values.extend((-64_000..=64_000).map(|step| f64::from(step) / 16.0));
    values.extend(values.clone().into_iter().map(f64::next_up));
    values.extend(values.clone().into_iter().map(f64::next_down));
    for value in values {
        let (ours, theirs) = (whole::round_f64(value), value.round());
        assert!(
            ours.to_bits() == theirs.to_bits() || (ours.is_nan() && theirs.is_nan()),
            "round_f64({value:e}) gave {ours:e}, f64::round gives {theirs:e}",
        );
    }
}
