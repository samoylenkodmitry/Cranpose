use cranpose_ui_graphics::Rect;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelDifference {
    pub x: u32,
    pub y: u32,
    pub lhs: [u8; 4],
    pub rhs: [u8; 4],
    pub difference: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageDifferenceStats {
    pub differing_pixels: u32,
    pub max_difference: u32,
    pub first_difference: Option<PixelDifference>,
}

pub fn sample_pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * width + x) * 4) as usize;
    [
        pixels[idx],
        pixels[idx + 1],
        pixels[idx + 2],
        pixels[idx + 3],
    ]
}

pub fn pixel_difference(lhs: [u8; 4], rhs: [u8; 4]) -> u32 {
    lhs.into_iter()
        .zip(rhs)
        .map(|(left, right)| left.abs_diff(right) as u32)
        .sum()
}

pub fn normalize_rgba_region(
    pixels: &[u8],
    width: u32,
    height: u32,
    rect: Rect,
    output_width: u32,
    output_height: u32,
) -> Vec<u8> {
    assert!(output_width > 0, "normalized output_width must be positive");
    assert!(
        output_height > 0,
        "normalized output_height must be positive"
    );

    let mut out = Vec::with_capacity((output_width * output_height * 4) as usize);
    for y in 0..output_height {
        for x in 0..output_width {
            let sample_x = rect.x + ((x as f32 + 0.5) * rect.width / output_width as f32);
            let sample_y = rect.y + ((y as f32 + 0.5) * rect.height / output_height as f32);
            out.extend_from_slice(&sample_pixel_bilinear(
                pixels, width, height, sample_x, sample_y,
            ));
        }
    }
    out
}

pub fn image_difference_stats(
    lhs: &[u8],
    rhs: &[u8],
    width: u32,
    height: u32,
    tolerance: u32,
) -> ImageDifferenceStats {
    assert_eq!(
        lhs.len(),
        rhs.len(),
        "image_difference_stats requires equal buffer lengths"
    );

    let mut differing_pixels = 0;
    let mut max_difference = 0;
    let mut first_difference = None;
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) * 4) as usize;
            let lhs_pixel = [lhs[index], lhs[index + 1], lhs[index + 2], lhs[index + 3]];
            let rhs_pixel = [rhs[index], rhs[index + 1], rhs[index + 2], rhs[index + 3]];
            let difference = pixel_difference(lhs_pixel, rhs_pixel);
            if difference > tolerance {
                differing_pixels += 1;
                max_difference = max_difference.max(difference);
                if first_difference.is_none() {
                    first_difference = Some(PixelDifference {
                        x,
                        y,
                        lhs: lhs_pixel,
                        rhs: rhs_pixel,
                        difference,
                    });
                }
            }
        }
    }

    ImageDifferenceStats {
        differing_pixels,
        max_difference,
        first_difference,
    }
}

fn sample_pixel_bilinear(pixels: &[u8], width: u32, height: u32, x: f32, y: f32) -> [u8; 4] {
    let max_x = width.saturating_sub(1) as f32;
    let max_y = height.saturating_sub(1) as f32;
    let source_x = (x - 0.5).clamp(0.0, max_x);
    let source_y = (y - 0.5).clamp(0.0, max_y);
    let x0 = source_x.floor() as u32;
    let y0 = source_y.floor() as u32;
    let x1 = (x0 + 1).min(width.saturating_sub(1));
    let y1 = (y0 + 1).min(height.saturating_sub(1));
    let tx = source_x - x0 as f32;
    let ty = source_y - y0 as f32;
    let top_left = sample_pixel(pixels, width, x0, y0);
    let top_right = sample_pixel(pixels, width, x1, y0);
    let bottom_left = sample_pixel(pixels, width, x0, y1);
    let bottom_right = sample_pixel(pixels, width, x1, y1);

    let lerp_channel = |index: usize| {
        let top = top_left[index] as f32 * (1.0 - tx) + top_right[index] as f32 * tx;
        let bottom = bottom_left[index] as f32 * (1.0 - tx) + bottom_right[index] as f32 * tx;
        (top * (1.0 - ty) + bottom * ty).round() as u8
    };

    [
        lerp_channel(0),
        lerp_channel(1),
        lerp_channel(2),
        lerp_channel(3),
    ]
}

#[cfg(test)]
mod tests {
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
}
