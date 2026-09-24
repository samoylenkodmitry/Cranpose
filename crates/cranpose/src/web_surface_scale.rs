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
#[path = "tests/web_surface_scale_tests.rs"]
mod tests;
