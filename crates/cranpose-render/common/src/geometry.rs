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
#[path = "tests/geometry_tests.rs"]
mod tests;

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
#[path = "tests/geometry_blur_kernel_tests.rs"]
mod blur_kernel_tests;
