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
