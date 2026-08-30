#![allow(dead_code)]

use std::{path::Path, time::Duration};

use cranpose::{Robot, RobotScreenshot};

pub fn save(shot: &RobotScreenshot, directory: &Path, name: &str) {
    if let Some(image) = image::RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone()) {
        let _ = image.save(directory.join(name));
    }
}

pub fn settle(robot: &Robot, millis: u64) {
    let _ = robot.wait_for_idle();
    std::thread::sleep(Duration::from_millis(millis));
    let _ = robot.wait_for_idle();
}
