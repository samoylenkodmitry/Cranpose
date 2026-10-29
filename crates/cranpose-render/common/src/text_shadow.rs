//! A text's shadow: each glyph's mask blurred, moved and drawn under it.

use cranpose_ui::text::Shadow;
use cranpose_ui_graphics::whole;

use crate::{
    brush_sampling::color_to_rgba,
    text_canvas::{PlacedMask, blend_src_over},
    text_mask_gamma::TextLuminance,
};

const SHADOW_SIGMA_SCALE: f32 = 0.57735;
const SHADOW_SIGMA_BIAS: f32 = 0.5;
const MAX_GAUSSIAN_KERNEL_HALF: i32 = 128;

fn blend_shadow_pixel(
    canvas: &mut [[f32; 4]],
    width: u32,
    height: u32,
    px: i32,
    py: i32,
    color: [f32; 4],
    alpha: f32,
) {
    let (Ok(px), Ok(py)) = (u32::try_from(px), u32::try_from(py)) else {
        return;
    };
    if px >= width || py >= height || alpha <= 0.0 {
        return;
    }
    let idx = (py * width + px) as usize;
    blend_src_over(
        &mut canvas[idx],
        [color[0], color[1], color[2], alpha.clamp(0.0, 1.0)],
    );
}

fn blend_shadow_pixel_subpixel(
    canvas: &mut [[f32; 4]],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    color: [f32; 4],
    alpha: f32,
) {
    if alpha <= 0.0 {
        return;
    }

    let base_x = whole::floor(x);
    let base_y = whole::floor(y);
    let frac_x = x - base_x;
    let frac_y = y - base_y;
    let base_x_i32 = whole::to_i32(base_x);
    let base_y_i32 = whole::to_i32(base_y);
    let weights = [
        ((1.0 - frac_x) * (1.0 - frac_y), 0i32, 0i32),
        (frac_x * (1.0 - frac_y), 1, 0),
        ((1.0 - frac_x) * frac_y, 0, 1),
        (frac_x * frac_y, 1, 1),
    ];

    for (weight, dx, dy) in weights {
        if weight <= 0.0 {
            continue;
        }
        blend_shadow_pixel(
            canvas,
            width,
            height,
            base_x_i32 + dx,
            base_y_i32 + dy,
            color,
            alpha * weight,
        );
    }
}

pub(crate) fn shadow_blur_sigma(blur_radius: f32) -> f32 {
    if blur_radius <= 0.0 {
        0.0
    } else {
        (blur_radius * SHADOW_SIGMA_SCALE + SHADOW_SIGMA_BIAS).max(0.5)
    }
}

fn gaussian_blur_alpha(src: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let kernel = gaussian_kernel_1d(sigma);
    if kernel.len() == 1 {
        return src.to_vec();
    }
    let half = (kernel.len() / 2) as i32;

    let mut horizontal = vec![0.0f32; src.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0f32;
            for (index, weight) in kernel.iter().enumerate() {
                let offset = index as i32 - half;
                let sample_x = (x as i32 + offset).clamp(0, width as i32 - 1) as usize;
                sum += src[y * width + sample_x] * *weight;
            }
            horizontal[y * width + x] = sum;
        }
    }

    let mut output = vec![0.0f32; src.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0f32;
            for (index, weight) in kernel.iter().enumerate() {
                let offset = index as i32 - half;
                let sample_y = (y as i32 + offset).clamp(0, height as i32 - 1) as usize;
                sum += horizontal[sample_y * width + x] * *weight;
            }
            output[y * width + x] = sum;
        }
    }

    output
}

fn gaussian_kernel_1d(sigma: f32) -> Vec<f32> {
    let half = whole::to_i32(whole::ceil(sigma * 3.0)).clamp(1, MAX_GAUSSIAN_KERNEL_HALF);
    if half <= 0 {
        return vec![1.0];
    }

    let mut kernel = Vec::with_capacity((half * 2 + 1) as usize);
    let mut sum = 0.0f32;
    for offset in -half..=half {
        let distance = offset as f32;
        let weight = (-0.5 * (distance / sigma).powi(2)).exp();
        kernel.push(weight);
        sum += weight;
    }

    if sum > f32::EPSILON {
        for weight in &mut kernel {
            *weight /= sum;
        }
    }

    kernel
}

/// Draws the shadow of the glyph `placed` on a canvas of `width` by `height`
/// float pixels, moved and blurred as `shadow` says at `scale` device pixels
/// a layout pixel; on whole pixels for a static text.
pub(crate) fn draw_shadow_mask(
    canvas: &mut [[f32; 4]],
    (width, height): (u32, u32),
    placed: PlacedMask<'_>,
    shadow: Shadow,
    (scale, static_motion): (f32, bool),
) {
    let mask = placed.mask;
    if mask.width == 0 || mask.height == 0 {
        return;
    }

    let shadow_dx = shadow.offset.x * scale;
    let shadow_dy = shadow.offset.y * scale;
    let blur_radius = (shadow.blur_radius * scale).max(0.0);
    let sigma = shadow_blur_sigma(blur_radius);
    let blur_margin = if sigma > 0.0 {
        whole::to_i32(whole::ceil(sigma * 3.0))
    } else {
        0
    };

    let padded_width = mask.width + (blur_margin as usize) * 2;
    let padded_height = mask.height + (blur_margin as usize) * 2;
    let mut padded_mask = vec![0.0f32; padded_width * padded_height];

    // An unblurred shadow is text in the shadow color and gets its mask
    // gamma; Skia never corrects a mask a blur filters.
    let correction = (sigma <= 0.0).then(|| TextLuminance::of_color(shadow.color).correction());
    for y in 0..mask.height {
        let src_offset = y * mask.width;
        let dst_offset = (y + blur_margin as usize) * padded_width + blur_margin as usize;
        let source = &mask.alpha[src_offset..src_offset + mask.width];
        let target = &mut padded_mask[dst_offset..dst_offset + mask.width];
        match correction {
            Some(correction) => target
                .iter_mut()
                .zip(source)
                .for_each(|(target, &coverage)| *target = correction.apply_unit(coverage)),
            None => target
                .iter_mut()
                .zip(source)
                .for_each(|(target, &coverage)| {
                    *target = crate::text_mask_gamma::unit_coverage(coverage);
                }),
        }
    }

    let blurred = if sigma > 0.0 {
        gaussian_blur_alpha(&padded_mask, padded_width, padded_height, sigma)
    } else {
        padded_mask
    };

    let shadow_rgba = color_to_rgba(shadow.color);
    let shadow_origin_x = placed.x - blur_margin;
    let shadow_origin_y = placed.y - blur_margin;

    for y in 0..padded_height {
        for x in 0..padded_width {
            let alpha = blurred[y * padded_width + x] * shadow_rgba[3];
            if alpha <= 0.0 {
                continue;
            }

            let target_x = shadow_origin_x as f32 + x as f32 + shadow_dx;
            let target_y = shadow_origin_y as f32 + y as f32 + shadow_dy;
            if static_motion {
                blend_shadow_pixel(
                    canvas,
                    width,
                    height,
                    whole::to_i32(whole::round(target_x)),
                    whole::to_i32(whole::round(target_y)),
                    shadow_rgba,
                    alpha.clamp(0.0, 1.0),
                );
            } else {
                blend_shadow_pixel_subpixel(
                    canvas,
                    width,
                    height,
                    target_x,
                    target_y,
                    shadow_rgba,
                    alpha.clamp(0.0, 1.0),
                );
            }
        }
    }
}
