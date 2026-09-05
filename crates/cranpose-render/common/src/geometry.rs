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
