use super::*;

#[test]
fn normalize_rgba_region_preserves_pixel_grid_at_native_size() {
    let pixels = vec![
        10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
    ];
    let normalized = normalize_rgba_region(
        &pixels,
        2,
        2,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 2.0,
            height: 2.0,
        },
        2,
        2,
    );
    assert_eq!(normalized, pixels);
}

#[test]
fn image_difference_stats_reports_first_difference() {
    let lhs = vec![0, 0, 0, 255, 8, 8, 8, 255];
    let rhs = vec![0, 0, 0, 255, 18, 12, 10, 255];
    let stats = image_difference_stats(&lhs, &rhs, 2, 1, 3);

    assert_eq!(stats.differing_pixels, 1);
    assert_eq!(stats.max_difference, 16);
    assert_eq!(
        stats.first_difference,
        Some(PixelDifference {
            x: 1,
            y: 0,
            lhs: [8, 8, 8, 255],
            rhs: [18, 12, 10, 255],
            difference: 16,
        })
    );
}
