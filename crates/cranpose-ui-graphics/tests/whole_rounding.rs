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
