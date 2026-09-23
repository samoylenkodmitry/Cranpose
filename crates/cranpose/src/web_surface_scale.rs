pub(crate) fn web_canvas_buffer_scale(device_pixel_ratio: f64) -> f64 {
    if device_pixel_ratio.is_finite() && device_pixel_ratio > 0.0 {
        device_pixel_ratio
    } else {
        1.0
    }
}

/// The canvas's size in device pixels: what its buffer has to be for the page
/// to show it pixel for pixel rather than stretch it.
///
/// `measured` is the browser's own count of the canvas's device pixels, from a
/// `device-pixel-content-box` observation, where the browser reports one. It
/// is exact, and it is taken while it agrees with the CSS box to within a
/// pixel; between a resize and the observation after it, the CSS box is the
/// newer of the two. Otherwise the fractional CSS size times the ratio is
/// rounded. Truncating instead is what blurs a page at a fractional ratio: at
/// 1.5 a canvas 1280 device pixels wide is 853.33 CSS pixels, and a buffer of
/// 853 * 1.5 = 1279 pixels is stretched across 1280 by the page.
pub(crate) fn web_canvas_device_size(
    css_width: f64,
    css_height: f64,
    device_pixel_ratio: f64,
    measured: Option<(u32, u32)>,
) -> (u32, u32) {
    let scale = web_canvas_buffer_scale(device_pixel_ratio);
    let device = |css: f64| (css * scale).round().max(1.0) as u32;
    let estimated = (device(css_width), device(css_height));
    match measured {
        Some((width, height))
            if width.abs_diff(estimated.0) <= 1 && height.abs_diff(estimated.1) <= 1 =>
        {
            (width.max(1), height.max(1))
        }
        _ => estimated,
    }
}

#[cfg(test)]
mod tests {
    use super::{web_canvas_buffer_scale, web_canvas_device_size};

    #[test]
    fn hidpi_ratios_size_the_buffer_to_physical_pixels() {
        assert_eq!(web_canvas_buffer_scale(2.0), 2.0);
        assert!((web_canvas_buffer_scale(1.354) - 1.354).abs() < f64::EPSILON);
        assert!(web_canvas_buffer_scale(3.0) > 1.0);
    }

    #[test]
    fn standard_density_leaves_the_buffer_at_css_resolution() {
        assert_eq!(web_canvas_buffer_scale(1.0), 1.0);
        assert_eq!(web_canvas_device_size(800.0, 600.0, 1.0, None), (800, 600));
    }

    #[test]
    fn degenerate_ratios_fall_back_to_css_resolution() {
        assert_eq!(web_canvas_buffer_scale(0.0), 1.0);
        assert_eq!(web_canvas_buffer_scale(-2.0), 1.0);
        assert_eq!(web_canvas_buffer_scale(f64::NAN), 1.0);
        assert_eq!(web_canvas_buffer_scale(f64::INFINITY), 1.0);
    }

    #[test]
    fn buffer_dimensions_scale_both_axes_to_the_device_pixel_ratio() {
        assert_eq!(
            web_canvas_device_size(800.0, 600.0, 2.0, None),
            (1600, 1200)
        );
        assert_eq!(
            web_canvas_device_size(390.0, 844.0, 3.0, None),
            (1170, 2532)
        );
    }

    #[test]
    fn a_fractional_ratio_fills_every_device_pixel_of_the_canvas() {
        // A 1280x900 window at a ratio of 1.5, as a GNOME text scale of 1.5
        // gives Chromium on Linux.
        let css = (1280.0 / 1.5, 900.0 / 1.5);
        assert_eq!(web_canvas_device_size(css.0, css.1, 1.5, None), (1280, 900));
        assert_eq!(
            web_canvas_device_size(853.0, 600.0, 1.25, None),
            (1066, 750)
        );
    }

    #[test]
    fn the_browsers_own_device_pixel_count_wins_while_it_agrees_with_the_box() {
        assert_eq!(
            web_canvas_device_size(853.33, 600.0, 1.5, Some((1279, 900))),
            (1279, 900),
            "the browser snaps the box to device pixels, and knows how"
        );
        assert_eq!(
            web_canvas_device_size(1000.0, 600.0, 1.5, Some((1280, 900))),
            (1500, 900),
            "an observation from before a resize is stale"
        );
        assert_eq!(web_canvas_device_size(0.0, 0.0, 1.5, Some((0, 0))), (1, 1));
    }
}
