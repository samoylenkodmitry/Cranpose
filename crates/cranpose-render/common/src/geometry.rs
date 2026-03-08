use cranpose_ui_graphics::Rect;

pub const BLUR_EXTENT_MULTIPLIER: f32 = 3.0;

pub fn union_rect(lhs: Option<Rect>, rhs: Rect) -> Option<Rect> {
    if rhs.width <= 0.0 || rhs.height <= 0.0 {
        return lhs;
    }

    Some(match lhs {
        Some(current) => current.union(rhs),
        None => rhs,
    })
}

pub fn blur_extent_margin(blur_radius: f32) -> f32 {
    (blur_radius.max(0.0) * BLUR_EXTENT_MULTIPLIER).max(1.0)
}

pub fn expand_blurred_rect(mut rect: Rect, blur_radius: f32, clip: Option<Rect>) -> Option<Rect> {
    let blur_margin = blur_extent_margin(blur_radius);
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
    fn blur_extent_margin_has_minimum_one_pixel() {
        assert_eq!(blur_extent_margin(0.0), 1.0);
        assert_eq!(blur_extent_margin(-5.0), 1.0);
        assert_eq!(blur_extent_margin(2.0), 6.0);
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
