#![allow(dead_code)]

use cranpose::RobotScreenshot;

pub fn is_blue_handle(r: u8, g: u8, b: u8) -> bool {
    b > 170 && b.saturating_sub(r) > 55 && b.saturating_sub(g) > 25
}

#[allow(clippy::too_many_arguments)]
pub fn lower_band_center(
    shot: &RobotScreenshot,
    sx: f32,
    sy: f32,
    left: usize,
    right: usize,
    top: usize,
    bottom: usize,
    matches: impl Fn(u8, u8, u8) -> bool,
) -> Option<(f32, f32)> {
    let is_match = |px: usize, py: usize| {
        let i = (py * shot.width as usize + px) * 4;
        let (r, g, b) = (shot.pixels[i], shot.pixels[i + 1], shot.pixels[i + 2]);
        matches(r, g, b)
    };
    let max_y = (top..bottom)
        .rev()
        .find(|&py| (left..right).any(|px| is_match(px, py)))?;
    let band_top = max_y.saturating_sub((4.0 * sy).ceil() as usize);
    let mut sum_x = 0usize;
    let mut sum_y = 0usize;
    let mut count = 0usize;
    for py in band_top..=max_y {
        for px in left..right {
            if is_match(px, py) {
                sum_x += px;
                sum_y += py;
                count += 1;
            }
        }
    }
    (count > 0).then(|| {
        (
            sum_x as f32 / count as f32 / sx,
            sum_y as f32 / count as f32 / sy,
        )
    })
}

#[allow(clippy::too_many_arguments)]
pub fn count_upper_lower_bands(
    shot: &RobotScreenshot,
    left: usize,
    right: usize,
    top: usize,
    bottom: usize,
    upper_end: usize,
    lower_start: usize,
    matches: impl Fn(u8, u8, u8) -> bool,
) -> (usize, usize) {
    let mut upper = 0usize;
    let mut lower = 0usize;
    for py in top..bottom {
        for px in left..right {
            let i = (py * shot.width as usize + px) * 4;
            let (r, g, b) = (shot.pixels[i], shot.pixels[i + 1], shot.pixels[i + 2]);
            if matches(r, g, b) {
                if py < upper_end {
                    upper += 1;
                } else if py >= lower_start {
                    lower += 1;
                }
            }
        }
    }
    (upper, lower)
}
