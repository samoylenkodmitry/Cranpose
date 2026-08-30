#![allow(dead_code)]

use std::{path::Path, time::Duration};

use cranpose::{Color, Robot, RobotScreenshot};

pub fn save(shot: &RobotScreenshot, directory: &Path, name: &str) {
    if let Some(image) = image::RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone()) {
        let _ = image.save(directory.join(name));
    }
}

pub fn save_checked(path: &Path, shot: &RobotScreenshot) -> Result<(), String> {
    let image: image::RgbaImage =
        image::ImageBuffer::from_raw(shot.width, shot.height, shot.pixels.clone())
            .ok_or_else(|| "invalid screenshot dimensions".to_string())?;
    image
        .save(path)
        .map_err(|err| format!("failed to save {}: {}", path.display(), err))
}

pub fn settle(robot: &Robot, millis: u64) {
    let _ = robot.wait_for_idle();
    std::thread::sleep(Duration::from_millis(millis));
    let _ = robot.wait_for_idle();
}

pub fn color_distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> f32 {
    let dr = a.0 as f32 - b.0 as f32;
    let dg = a.1 as f32 - b.1 as f32;
    let db = a.2 as f32 - b.2 as f32;
    (dr * dr + dg * dg + db * db).sqrt()
}

pub fn to_rgb8(color: Color) -> (u8, u8, u8) {
    (
        (color.r() * 255.0) as u8,
        (color.g() * 255.0) as u8,
        (color.b() * 255.0) as u8,
    )
}

pub fn logical_sampler(shot: &RobotScreenshot) -> impl Fn(f32, f32) -> (u8, u8, u8) + '_ {
    let scale = shot.width as f32 / shot.logical_width;
    move |lx: f32, ly: f32| -> (u8, u8, u8) {
        let px = (lx * scale).round().clamp(0.0, shot.width as f32 - 1.0) as u32;
        let py = (ly * scale).round().clamp(0.0, shot.height as f32 - 1.0) as u32;
        let idx = ((py * shot.width + px) * 4) as usize;
        (shot.pixels[idx], shot.pixels[idx + 1], shot.pixels[idx + 2])
    }
}
