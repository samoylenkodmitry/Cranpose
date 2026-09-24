use super::*;

fn gaussian(tap: u32, radius: f32) -> f32 {
    let sigma = radius * 0.5;
    (-((tap * tap) as f32) / (2.0 * sigma * sigma)).exp()
}

#[test]
fn a_kernel_pairs_its_taps_where_their_weights_meet() {
    let kernel = BlurKernel::of_radius(5.5);
    assert_eq!(
        kernel.pair_count, 3,
        "a radius of 5.5 takes six taps a side"
    );
    let mut total = 1.0;
    for (k, pair) in kernel.pairs[..3].iter().enumerate() {
        let (i, j) = (2 * k as u32 + 1, 2 * k as u32 + 2);
        assert!((pair.inner - gaussian(i, 5.5)).abs() <= 1e-6);
        assert!((pair.outer - gaussian(j, 5.5)).abs() <= 1e-6);
        assert_eq!(pair.weight, pair.inner + pair.outer);
        assert!(pair.offset > i as f32 && pair.offset < j as f32);
        total += 2.0 * (pair.inner + pair.outer);
    }
    assert_eq!(kernel.total_weight, total);
    assert_eq!(kernel.pairs[3], BlurTapPair::default());
}

#[test]
fn an_odd_tap_count_leaves_its_last_fetch_on_the_inner_tap() {
    let kernel = BlurKernel::of_radius(7.0);
    assert_eq!(kernel.pair_count, 4);
    let tail = kernel.pairs[3];
    assert_eq!(tail.outer, 0.0);
    assert_eq!(tail.offset, 7.0);
    assert_eq!(tail.weight, tail.inner);
}

#[test]
fn a_kernel_stops_at_the_tap_cap_and_a_zero_radius_has_no_pairs() {
    let capped = BlurKernel::of_radius(100.0);
    assert_eq!(capped.pair_count, BLUR_TAP_PAIRS as u32);
    assert!(capped.pairs[BLUR_TAP_PAIRS - 1].outer > 0.0);
    let none = BlurKernel::of_radius(0.0);
    assert_eq!((none.pair_count, none.total_weight), (0, 1.0));
}
