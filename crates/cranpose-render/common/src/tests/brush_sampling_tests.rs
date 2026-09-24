use cranpose_ui_graphics::Point;

use super::*;

fn sample_rect() -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 40.0,
    }
}

#[test]
fn empty_gradient_samples_transparent_instead_of_panicking() {
    let brush =
        Brush::linear_gradient_range(Vec::new(), Point::new(0.0, 0.0), Point::new(100.0, 0.0));
    assert_eq!(
        sample_brush_rgba(&brush, sample_rect(), 50.0, 10.0, Point::default()),
        [0.0, 0.0, 0.0, 0.0]
    );
}

#[test]
fn clamped_gradient_samples_last_color_at_end() {
    let brush = Brush::linear_gradient_range(
        vec![Color::RED, Color::BLUE],
        Point::new(0.0, 0.0),
        Point::new(100.0, 0.0),
    );
    for y in 0..4 {
        for x in 0..4 {
            let sampled = sample_brush_rgba(
                &brush,
                sample_rect(),
                120.0 + x as f32,
                y as f32,
                Point::default(),
            );
            let bytes: Vec<u8> = sampled.iter().map(|c| (c * 255.0).round() as u8).collect();
            assert_eq!(bytes, vec![0, 0, 255, 255], "cell ({x}, {y})");
        }
    }
}

#[test]
fn mirror_tile_mode_normalizes_across_repeated_segments() {
    assert_eq!(normalize_gradient_t(1.25, TileMode::Mirror), Some(0.75));
    assert_eq!(normalize_gradient_t(1.75, TileMode::Mirror), Some(0.25));
}

const BAYER_4X4: [[u32; 4]; 4] = [[0, 4, 1, 5], [8, 12, 9, 13], [2, 6, 3, 7], [10, 14, 11, 15]];

#[test]
fn the_dither_lays_out_skias_bayer_matrix() {
    for y in 0..4u32 {
        for x in 0..4u32 {
            let expected = BAYER_4X4[y as usize][x as usize] as f32 / 16.0 - 15.0 / 32.0;
            assert_eq!(
                gradient_dither_offset(x as f32 - 1.0 + 4.0, y as f32 - 1.0 + 4.0),
                expected,
                "cell ({x}, {y})"
            );
        }
    }
}

#[test]
fn the_dither_is_a_pixel_ahead_of_the_fragment() {
    assert_eq!(
        gradient_dither_offset(4.0, 4.0),
        gradient_dither_offset(5.0 - 1.0, 5.0 - 1.0),
    );
    assert_eq!(
        gradient_dither_offset(3.0, 3.0),
        BAYER_4X4[0][0] as f32 / 16.0 - 15.0 / 32.0,
    );
}

#[test]
fn the_dither_repeats_every_four_pixels_and_never_moves_a_whole_level() {
    for y in 0..16u32 {
        for x in 0..16u32 {
            assert_eq!(
                gradient_dither_offset(x as f32, y as f32),
                gradient_dither_offset((x % 4) as f32, (y % 4) as f32),
            );
        }
    }
    let offsets: Vec<f32> = (0..4)
        .flat_map(|y| (0..4).map(move |x| gradient_dither_offset(x as f32, y as f32)))
        .collect();
    assert!(offsets.iter().all(|offset| offset.abs() < 0.5));
    let mean = offsets.iter().sum::<f32>() / offsets.len() as f32;
    assert!(mean.abs() < 1e-6, "mean offset {mean}");
}

#[test]
fn a_solid_brush_is_left_alone() {
    let brush = Brush::Solid(Color(0.25, 0.5, 0.75, 1.0));
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(
                sample_brush_rgba(&brush, sample_rect(), x as f32, y as f32, Point::default()),
                [0.25, 0.5, 0.75, 1.0],
            );
        }
    }
}

#[test]
fn the_dither_moves_a_flat_gradient_off_one_value_onto_two() {
    let grey = 100.4 / 255.0;
    let brush = Brush::linear_gradient_range(
        vec![Color(grey, grey, grey, 1.0), Color(grey, grey, grey, 1.0)],
        Point::new(0.0, 0.0),
        Point::new(100.0, 0.0),
    );
    let mut levels = std::collections::BTreeSet::new();
    for y in 0..4 {
        for x in 0..4 {
            let sampled =
                sample_brush_rgba(&brush, sample_rect(), x as f32, y as f32, Point::default());
            levels.insert((sampled[0] * 255.0).round() as u8);
        }
    }
    assert_eq!(levels.into_iter().collect::<Vec<_>>(), vec![100, 101]);
}
