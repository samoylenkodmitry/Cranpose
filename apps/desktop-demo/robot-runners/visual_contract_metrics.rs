#![allow(dead_code)]

use cranpose::RobotScreenshot;
use image::RgbaImage;

#[derive(Clone, Copy, Debug)]
pub(crate) struct FeatureStats {
    pub min_x: u32,
    pub min_y: u32,
    pub max_x: u32,
    pub max_y: u32,
    pub count: usize,
    sum_x: f64,
    sum_y: f64,
}

impl FeatureStats {
    pub(crate) fn centroid_x(self) -> f64 {
        self.sum_x / self.count.max(1) as f64
    }

    pub(crate) fn centroid_y(self) -> f64 {
        self.sum_y / self.count.max(1) as f64
    }

    pub(crate) fn bounds_center_y(self) -> f64 {
        (self.min_y + self.max_y) as f64 / 2.0
    }
}

pub(crate) fn edge_energy_rgba(image: &RgbaImage) -> f32 {
    let (width, height) = image.dimensions();
    let mut total = 0u64;
    let mut samples = 0u64;
    for y in 0..height {
        for x in 0..width {
            let current = luma(image.get_pixel(x, y).0);
            if x + 1 < width {
                total += current.abs_diff(luma(image.get_pixel(x + 1, y).0)) as u64;
                samples += 1;
            }
            if y + 1 < height {
                total += current.abs_diff(luma(image.get_pixel(x, y + 1).0)) as u64;
                samples += 1;
            }
        }
    }
    if samples == 0 {
        0.0
    } else {
        total as f32 / samples as f32
    }
}

pub(crate) fn edge_energy_screenshot(screenshot: &RobotScreenshot) -> f32 {
    let width = screenshot.width as usize;
    let height = screenshot.height as usize;
    let mut total = 0u64;
    let mut samples = 0u64;
    for y in 0..height {
        for x in 0..width {
            let index = (y * width + x) * 4;
            let current = luma([
                screenshot.pixels[index],
                screenshot.pixels[index + 1],
                screenshot.pixels[index + 2],
                screenshot.pixels[index + 3],
            ]);
            if x + 1 < width {
                let right = (y * width + x + 1) * 4;
                total += current.abs_diff(luma([
                    screenshot.pixels[right],
                    screenshot.pixels[right + 1],
                    screenshot.pixels[right + 2],
                    screenshot.pixels[right + 3],
                ])) as u64;
                samples += 1;
            }
            if y + 1 < height {
                let down = ((y + 1) * width + x) * 4;
                total += current.abs_diff(luma([
                    screenshot.pixels[down],
                    screenshot.pixels[down + 1],
                    screenshot.pixels[down + 2],
                    screenshot.pixels[down + 3],
                ])) as u64;
                samples += 1;
            }
        }
    }
    if samples == 0 {
        0.0
    } else {
        total as f32 / samples as f32
    }
}

pub(crate) fn feature_stats_screenshot(
    screenshot: &RobotScreenshot,
    region: (f32, f32, f32, f32),
    predicate: impl Fn([u8; 4]) -> bool,
) -> Option<FeatureStats> {
    let scale_x = screenshot.width as f32 / screenshot.logical_width.max(1.0);
    let scale_y = screenshot.height as f32 / screenshot.logical_height.max(1.0);
    let left = (region.0 * scale_x).floor().max(0.0) as u32;
    let top = (region.1 * scale_y).floor().max(0.0) as u32;
    let right = ((region.0 + region.2) * scale_x)
        .ceil()
        .min(screenshot.width as f32)
        .max(left as f32) as u32;
    let bottom = ((region.1 + region.3) * scale_y)
        .ceil()
        .min(screenshot.height as f32)
        .max(top as f32) as u32;

    let mut stats = None::<FeatureStats>;
    for y in top..bottom {
        for x in left..right {
            let index = ((y * screenshot.width + x) * 4) as usize;
            let pixel = [
                screenshot.pixels[index],
                screenshot.pixels[index + 1],
                screenshot.pixels[index + 2],
                screenshot.pixels[index + 3],
            ];
            if !predicate(pixel) {
                continue;
            }
            stats = Some(match stats {
                Some(mut current) => {
                    current.min_x = current.min_x.min(x);
                    current.min_y = current.min_y.min(y);
                    current.max_x = current.max_x.max(x);
                    current.max_y = current.max_y.max(y);
                    current.count += 1;
                    current.sum_x += x as f64;
                    current.sum_y += y as f64;
                    current
                }
                None => FeatureStats {
                    min_x: x,
                    min_y: y,
                    max_x: x,
                    max_y: y,
                    count: 1,
                    sum_x: x as f64,
                    sum_y: y as f64,
                },
            });
        }
    }
    stats
}

pub(crate) fn crop_rgba(image: &RgbaImage, region: (u32, u32, u32, u32)) -> RgbaImage {
    let (left, top, width, height) = region;
    let mut crop = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let source_x = (left + x).min(image.width().saturating_sub(1));
            let source_y = (top + y).min(image.height().saturating_sub(1));
            crop.put_pixel(x, y, *image.get_pixel(source_x, source_y));
        }
    }
    crop
}

pub(crate) fn feature_stats_rgba(
    image: &RgbaImage,
    region: (u32, u32, u32, u32),
    predicate: impl Fn([u8; 4]) -> bool,
) -> Option<FeatureStats> {
    let (left, top, width, height) = region;
    let right = left.saturating_add(width).min(image.width());
    let bottom = top.saturating_add(height).min(image.height());
    let mut stats = None::<FeatureStats>;
    for y in top..bottom {
        for x in left..right {
            let pixel = image.get_pixel(x, y).0;
            if !predicate(pixel) {
                continue;
            }
            stats = Some(match stats {
                Some(mut current) => {
                    current.min_x = current.min_x.min(x);
                    current.min_y = current.min_y.min(y);
                    current.max_x = current.max_x.max(x);
                    current.max_y = current.max_y.max(y);
                    current.count += 1;
                    current.sum_x += x as f64;
                    current.sum_y += y as f64;
                    current
                }
                None => FeatureStats {
                    min_x: x,
                    min_y: y,
                    max_x: x,
                    max_y: y,
                    count: 1,
                    sum_x: x as f64,
                    sum_y: y as f64,
                },
            });
        }
    }
    stats
}

fn luma(pixel: [u8; 4]) -> i32 {
    (77 * pixel[0] as i32 + 150 * pixel[1] as i32 + 29 * pixel[2] as i32) / 256
}
