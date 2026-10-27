//! The coverage of a pixel on a shape's edge, `1 - t^2 (3 - 2t)`, with the
//! bits that f64 steps give it, from f32 steps.
//!
//! A target without a double precision unit, such as the ESP32, runs f64
//! steps in software: on the ES3C35P they took 2.2% of the benchmarks'
//! dashboard frame. The f32 steps here carry each product and sum as a pair
//! of floats whose sum holds about 44 bits of the value. The f64 steps land
//! within 2^-53 of the exact value; where the pair lies farther than both
//! errors from the halfway point between two f32 values, both round to the
//! same one. Elsewhere the f64 steps decide. Every f32 `t` in (0, 1) gives
//! the f64 steps' bits, 2,036 of them through those steps.

// Below it the low parts of the pairs near the f32's subnormal range.
const SMALLEST_PAIR_T: f32 = 1.0 / 1_048_576.0;

// How far the pair's sum and the f64 steps' value can lie from the exact
// value, together: 2^-53 for the f64 steps, 2^-44 of the value for the pair.
const ABSOLUTE_MARGIN: f32 = 2.3e-16;
const RELATIVE_MARGIN: f32 = 6.0e-14;

// `1 - t^2 (3 - 2t)` for `t` in (0, 1), as f64 steps round it.
pub(super) fn edge_coverage(t: f32) -> f32 {
    if t >= SMALLEST_PAIR_T
        && let Some(coverage) = pair_coverage(t)
    {
        return coverage;
    }
    f64_coverage(t)
}

#[cold]
#[inline(never)]
fn f64_coverage(t: f32) -> f32 {
    let t = f64::from(t);
    let smooth = t * t * (3.0 - 2.0 * t);
    (1.0 - smooth) as f32
}

// The f32 the f64 steps give, or `None` where the pair cannot tell it.
fn pair_coverage(t: f32) -> Option<f32> {
    let (high, low) = if t >= 0.5 {
        // 1 - t^2 (3 - 2t) is u^2 (3 - 2u) with u = 1 - t, which is exact
        // from t = 0.5 up, and keeps the small values' low bits.
        let u = 1.0 - t;
        let (square, square_low) = two_product(u, u);
        let (factor, factor_low) = two_sum(3.0, -2.0 * u);
        let (product, product_low) = two_product(square, factor);
        two_sum(
            product,
            product_low + (square * factor_low + square_low * factor),
        )
    } else {
        let (square, square_low) = two_product(t, t);
        let (factor, factor_low) = two_sum(3.0, -2.0 * t);
        let (product, product_low) = two_product(square, factor);
        let product_low = product_low + (square * factor_low + square_low * factor);
        let (rest, rest_low) = two_sum(1.0, -product);
        two_sum(rest, rest_low - product_low)
    };
    let nearest = high + low;
    let (off, off_low) = two_sum(high - nearest, low);
    // Half the step to the f32 below, the shorter one at a power of two.
    let below = f32::from_bits(nearest.to_bits().checked_sub(1)?);
    let half_step = (nearest - below) * 0.5;
    let margin = ABSOLUTE_MARGIN + nearest * RELATIVE_MARGIN;
    ((off + off_low).abs() < half_step - margin).then_some(nearest)
}

// `a` in two halves of 12 bits each, Dekker's split.
fn split(a: f32) -> (f32, f32) {
    let scaled = 4097.0 * a;
    let high = scaled - (scaled - a);
    (high, a - high)
}

// `a * b` as a rounded product and its exact error.
fn two_product(a: f32, b: f32) -> (f32, f32) {
    let product = a * b;
    let (a_high, a_low) = split(a);
    let (b_high, b_low) = split(b);
    let error = ((a_high * b_high - product) + a_high * b_low + a_low * b_high) + a_low * b_low;
    (product, error)
}

// `a + b` as a rounded sum and its exact error.
fn two_sum(a: f32, b: f32) -> (f32, f32) {
    let sum = a + b;
    let b_part = sum - a;
    let error = (a - (sum - b_part)) + (b - b_part);
    (sum, error)
}
