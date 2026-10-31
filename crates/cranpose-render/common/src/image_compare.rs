use cranpose_ui_graphics::{Rect, whole};

/// One pixel that differs between two RGBA images.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelDifference {
    /// Column of the pixel, counted from the left.
    pub x: u32,
    /// Row of the pixel, counted from the top.
    pub y: u32,
    /// RGBA bytes of the pixel in the left image.
    pub lhs: [u8; 4],
    /// RGBA bytes of the pixel in the right image.
    pub rhs: [u8; 4],
    /// Sum of the absolute per-channel differences, from 0 to 1020.
    pub difference: u32,
}

/// Summary of how two RGBA images differ.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageDifferenceStats {
    /// Number of pixels whose difference exceeds the tolerance.
    pub differing_pixels: u32,
    /// Largest difference among the differing pixels.
    pub max_difference: u32,
    /// The first differing pixel in row-major order, or `None` when the images match.
    pub first_difference: Option<PixelDifference>,
}

/// Reads the RGBA bytes of pixel (`x`, `y`) from a tightly packed RGBA8 buffer that is `width`
/// pixels wide. Panics when the pixel lies outside the buffer.
pub fn sample_pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * width + x) * 4) as usize;
    [
        pixels[idx],
        pixels[idx + 1],
        pixels[idx + 2],
        pixels[idx + 3],
    ]
}

/// Sums the absolute per-channel differences between two RGBA pixels, from 0 to 1020.
pub fn pixel_difference(lhs: [u8; 4], rhs: [u8; 4]) -> u32 {
    lhs.into_iter()
        .zip(rhs)
        .map(|(left, right)| left.abs_diff(right) as u32)
        .sum()
}

/// Resamples the `rect` region, in source pixels, of an RGBA8 image of `width` by `height`
/// pixels to `output_width` by `output_height` pixels with bilinear sampling. Panics when an
/// output dimension is zero.
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

/// Compares two RGBA8 buffers of `width` by `height` pixels and counts the pixels whose
/// difference exceeds `tolerance`. Panics when the buffers differ in length.
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
    let x0 = whole::floor(source_x) as u32;
    let y0 = whole::floor(source_y) as u32;
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
        whole::round(top * (1.0 - ty) + bottom * ty) as u8
    };

    [
        lerp_channel(0),
        lerp_channel(1),
        lerp_channel(2),
        lerp_channel(3),
    ]
}

#[cfg(test)]
#[path = "tests/image_compare_tests.rs"]
mod tests;
