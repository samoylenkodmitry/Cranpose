use cranpose_ui_graphics::Rect;

/// The most taps a blur pass takes on one side of a pixel; a kernel wider
/// than this in scratch texels truncates there.
pub const BLUR_MAX_TAPS: u32 = 32;

/// The block of device pixels one scratch texel of a blur stands for: a
/// wide blur runs at a coarser grid, its kernel scaled with it.
pub fn blur_scratch_block(radius_px: f32) -> u32 {
    if radius_px < 6.0 {
        1
    } else if radius_px < 16.0 {
        2
    } else {
        4
    }
}

pub fn union_rect(lhs: Option<Rect>, rhs: Rect) -> Option<Rect> {
    if rhs.width <= 0.0 || rhs.height <= 0.0 {
        return lhs;
    }

    Some(match lhs {
        Some(current) => current.union(rhs),
        None => rhs,
    })
}

/// How far, in device pixels, a blur of `radius_px` carries a source pixel:
/// the kernel's taps at the scratch grid, the block each scratch texel
/// averages on the way down and interpolates on the way back, and the
/// source's own antialiased pixel, rounded up to whole blocks so the
/// scratch grid sits on the source the same way whatever the margin. Past
/// this distance the blur is exactly zero, so nothing reads or draws
/// beyond it.
pub fn blur_reach_px(radius_px: f32) -> f32 {
    if radius_px.is_nan() || radius_px <= 0.0 {
        return 1.0;
    }
    let block = blur_scratch_block(radius_px) as f32;
    let reach = radius_px.min(BLUR_MAX_TAPS as f32 * block) + 3.0 * block + 1.0;
    (reach / block).ceil() * block
}

/// [`blur_reach_px`] in logical pixels for a blur of `blur_radius` logical
/// pixels drawn at `scale` device pixels per logical pixel.
pub fn blur_reach(blur_radius: f32, scale: f32) -> f32 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    blur_reach_px(blur_radius.max(0.0) * scale) / scale
}

pub fn expand_blurred_rect(
    mut rect: Rect,
    blur_radius: f32,
    scale: f32,
    clip: Option<Rect>,
) -> Option<Rect> {
    let blur_margin = blur_reach(blur_radius, scale);
    rect.x -= blur_margin;
    rect.y -= blur_margin;
    rect.width += blur_margin * 2.0;
    rect.height += blur_margin * 2.0;
    if let Some(clip) = clip {
        rect = rect.intersect(clip)?;
    }
    Some(rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_rect_ignores_empty_rhs() {
        let lhs = Some(Rect {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        });
        let rhs = Rect {
            x: 5.0,
            y: 6.0,
            width: 0.0,
            height: 7.0,
        };

        assert_eq!(union_rect(lhs, rhs), lhs);
    }

    #[test]
    fn union_rect_merges_extents() {
        let lhs = Some(Rect {
            x: 8.0,
            y: 4.0,
            width: 3.0,
            height: 5.0,
        });
        let rhs = Rect {
            x: 2.0,
            y: 7.0,
            width: 12.0,
            height: 4.0,
        };

        assert_eq!(
            union_rect(lhs, rhs),
            Some(Rect {
                x: 2.0,
                y: 4.0,
                width: 12.0,
                height: 7.0,
            })
        );
    }

    #[test]
    fn a_blur_reaches_its_kernel_and_its_scratch_blocks_past_the_source() {
        assert_eq!(blur_reach_px(0.0), 1.0);
        assert_eq!(blur_reach_px(-5.0), 1.0);
        assert_eq!(blur_reach_px(2.0), 6.0);
        assert_eq!(blur_reach_px(10.0), 18.0);
        assert_eq!(blur_reach_px(44.0), 60.0);
        assert_eq!(blur_reach_px(200.0), 144.0);
    }

    #[test]
    fn the_logical_reach_follows_the_device_scale() {
        assert_eq!(blur_reach(2.0, 1.0), 6.0);
        assert!((blur_reach(20.0, 2.25) - 60.0 / 2.25).abs() < 1e-5);
        assert_eq!(blur_reach(2.0, 0.0), 6.0);
        assert_eq!(blur_reach(2.0, f32::NAN), 6.0);
    }

    #[test]
    fn expand_blurred_rect_applies_margin_and_clip() {
        let expanded = expand_blurred_rect(
            Rect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0,
            },
            2.0,
            1.0,
            Some(Rect {
                x: 8.0,
                y: 18.0,
                width: 20.0,
                height: 20.0,
            }),
        );

        assert_eq!(
            expanded,
            Some(Rect {
                x: 8.0,
                y: 18.0,
                width: 20.0,
                height: 20.0,
            })
        );
    }
}

/// The tap pairs of a kernel of `BLUR_MAX_TAPS` taps: the taps at i and
/// i + 1 on one side share one bilinear fetch.
pub const BLUR_TAP_PAIRS: usize = (BLUR_MAX_TAPS / 2) as usize;

/// One pair of kernel taps on one side of the pixel: the Gaussian weights of
/// the inner and outer tap, and the one bilinear fetch that stands for both,
/// its `offset` in taps from the pixel and its `weight` their sum. The outer
/// weight is zero past an odd tap count, which leaves the fetch on the inner
/// tap alone.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlurTapPair {
    pub inner: f32,
    pub outer: f32,
    pub offset: f32,
    pub weight: f32,
}

/// The separable Gaussian kernel of a blur of `radius` source texels as the
/// blur pass samples it: `pair_count` pairs on each side, `total_weight` the
/// kernel's sum with the centre tap's one, computed once per draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurKernel {
    pub pairs: [BlurTapPair; BLUR_TAP_PAIRS],
    pub pair_count: u32,
    pub total_weight: f32,
}

impl BlurKernel {
    /// The kernel of a blur of `radius` texels: sigma is half the radius,
    /// the taps on one side its ceiling, at most `BLUR_MAX_TAPS`.
    pub fn of_radius(radius: f32) -> Self {
        let radius = radius.max(0.0);
        let sigma = (radius * 0.5).max(0.001);
        let tap_count = (radius.ceil() as u32).min(BLUR_MAX_TAPS);
        let inv_2sigma2 = 1.0 / (2.0 * sigma * sigma);
        let mut pairs = [BlurTapPair::default(); BLUR_TAP_PAIRS];
        let mut total_weight = 1.0f32;
        let mut pair_count = 0;
        for i in (1..=tap_count).step_by(2) {
            let fi = i as f32;
            let fj = fi + 1.0;
            let inner = (-(fi * fi) * inv_2sigma2).exp();
            let outer = if i < tap_count {
                (-(fj * fj) * inv_2sigma2).exp()
            } else {
                0.0
            };
            total_weight += 2.0 * (inner + outer);
            let weight = inner + outer;
            let offset = if weight > 0.0 {
                (fi * inner + fj * outer) / weight
            } else {
                0.0
            };
            pairs[pair_count] = BlurTapPair {
                inner,
                outer,
                offset,
                weight,
            };
            pair_count += 1;
        }
        Self {
            pairs,
            pair_count: pair_count as u32,
            total_weight,
        }
    }
}

#[cfg(test)]
mod blur_kernel_tests {
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
}
