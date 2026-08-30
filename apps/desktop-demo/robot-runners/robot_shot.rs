#![allow(dead_code)]

use std::{path::Path, time::Duration};

use cranpose::{Robot, RobotScreenshot};

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
