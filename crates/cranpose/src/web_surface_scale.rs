pub(crate) fn web_canvas_buffer_scale(device_pixel_ratio: f64) -> f64 {
    if device_pixel_ratio.is_finite() && device_pixel_ratio > 0.0 {
        device_pixel_ratio
    } else {
        1.0
    }
}

pub(crate) fn web_canvas_buffer_dimensions(
    css_width: u32,
    css_height: u32,
    device_pixel_ratio: f64,
) -> (u32, u32) {
    let scale = web_canvas_buffer_scale(device_pixel_ratio);
    (
        (css_width as f64 * scale) as u32,
        (css_height as f64 * scale) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::{web_canvas_buffer_dimensions, web_canvas_buffer_scale};

    #[test]
    fn hidpi_ratios_size_the_buffer_to_physical_pixels() {
        assert_eq!(web_canvas_buffer_scale(2.0), 2.0);
        assert!((web_canvas_buffer_scale(1.354) - 1.354).abs() < f64::EPSILON);
        assert!(web_canvas_buffer_scale(3.0) > 1.0);
    }

    #[test]
    fn standard_density_leaves_the_buffer_at_css_resolution() {
        assert_eq!(web_canvas_buffer_scale(1.0), 1.0);
        assert_eq!(web_canvas_buffer_dimensions(800, 600, 1.0), (800, 600));
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
        assert_eq!(web_canvas_buffer_dimensions(800, 600, 2.0), (1600, 1200));
        assert_ne!(web_canvas_buffer_dimensions(800, 600, 2.0), (800, 600));
        assert_eq!(web_canvas_buffer_dimensions(390, 844, 3.0), (1170, 2532));
    }
}
