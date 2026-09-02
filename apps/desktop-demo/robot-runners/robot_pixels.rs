#![allow(dead_code)]

use cranpose::RobotScreenshot;

/// The RGBA bytes under a logical-pixel position, using the screenshot's
/// own logical-to-physical scale.
pub fn pixel_at_logical(screenshot: &RobotScreenshot, x: f32, y: f32) -> [u8; 4] {
    let scale = if screenshot.logical_width.is_finite() && screenshot.logical_width > 0.0 {
        screenshot.width as f32 / screenshot.logical_width
    } else {
        1.0
    };
    let px = ((x * scale) as u32).min(screenshot.width.saturating_sub(1));
    let py = ((y * scale) as u32).min(screenshot.height.saturating_sub(1));
    let index = (py as usize * screenshot.width as usize + px as usize) * 4;
    let bytes = &screenshot.pixels[index..index + 4];
    [bytes[0], bytes[1], bytes[2], bytes[3]]
}

/// Whether every colour channel of `actual` is within `tolerance` of
/// `expected`; alpha is ignored.
pub fn color_close(actual: [u8; 4], expected: [u8; 4], tolerance: i32) -> bool {
    actual
        .iter()
        .zip(expected.iter())
        .take(3)
        .all(|(a, e)| (*a as i32 - *e as i32).abs() <= tolerance)
}
