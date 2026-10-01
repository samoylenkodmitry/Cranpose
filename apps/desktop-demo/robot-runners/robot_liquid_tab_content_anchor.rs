use crate::{liquid_tab_reference, robot_exit, robot_shot};

use std::path::PathBuf;

use cranpose::{AppLauncher, RobotScreenshot};

pub(crate) fn main() -> anyhow::Result<()> {
    let output = PathBuf::from(
        std::env::var("CRANPOSE_ROBOT_OUTPUT_DIR")
            .unwrap_or_else(|_| "target/liquid-tab-content-anchor".to_string()),
    );
    std::fs::create_dir_all(&output)?;
    AppLauncher::new()
        .with_title("Liquid tab content anchor")
        .with_size(440, 956)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(90);
            robot_shot::settle(&robot, 700);
            robot.touch_down(172.417, 904.0).expect("hold Browse");
            let mut anchors = Vec::with_capacity(43);
            for (index, position) in [158.0, 187.0, 158.0].into_iter().enumerate() {
                robot.touch_move(position, 904.0).expect("move held lens");
                robot_shot::settle(&robot, 900);
                anchors.push(capture_anchor(&robot, &output, index));
            }
            for step in 0..40 {
                let progress = if step < 20 { step + 1 } else { 39 - step } as f32 / 20.0;
                let position = 158.0 + 29.0 * progress;
                robot.touch_move(position, 904.0).expect("sweep held lens");
                robot.pump_frames(1).expect("draw moving lens");
                anchors.push(capture_anchor(&robot, &output, step + 3));
            }
            robot.touch_up(158.0, 904.0).expect("release");
            for (axis, message) in [
                "the lens translated the icon relative to neighboring tab anchors",
                "the moving lens shifted the caption relative to its icon",
            ].into_iter().enumerate() {
                let (min, max) = anchors.iter().map(|anchor| anchor[axis]).fold(
                    (f32::INFINITY, f32::NEG_INFINITY),
                    |(min, max), value| (min.min(value), max.max(value)),
                );
                let drift = max - min;
                println!("Browse anchor axis {axis}: {min}..{max}; drift {drift} pt");
                if drift > 0.5 {
                    robot_exit::fail(&robot, message);
                }
            }
            robot.exit().expect("exit");
        })
        .try_run(|| liquid_tab_reference::LiquidTabReference(false, false))?;
    Ok(())
}

fn capture_anchor(robot: &cranpose::Robot, output: &std::path::Path, index: usize) -> [f32; 2] {
    let shot = robot.screenshot().expect("capture anchored content");
    robot_shot::save_checked(&output.join(format!("anchor-{index}.png")), &shot)
        .expect("save pixels");
    let browse = ink_center(&shot, 172.417, true, false);
    let caption = ink_center(&shot, 172.417, true, true);
    let neighbors =
        (ink_center(&shot, 77.25, false, false).0 + ink_center(&shot, 267.583, false, false).0) * 0.5;
    [browse.0 - neighbors, caption.1 - browse.1]
}

fn ink_center(shot: &RobotScreenshot, center: f32, accent: bool, caption: bool) -> (f32, f32) {
    let scale = shot.width as f32 / shot.logical_width;
    let mut sum = (0.0, 0.0);
    let mut weight = 0.0;
    let (top, bottom, radius) = if caption { (912.0, 932.0, 40.0) } else { (884.0, 910.0, 24.0) };
    for y in (top * scale) as u32..(bottom * scale) as u32 {
        for x in ((center - radius) * scale) as u32..((center + radius) * scale) as u32 {
            let pixel = &shot.pixels[((y * shot.width + x) * 4) as usize..];
            let coverage = if accent {
                pixel[2].saturating_sub(pixel[0])
            } else {
                255 - pixel[0].max(pixel[1]).max(pixel[2])
            };
            let coverage = f32::from(coverage.saturating_sub(77)) / 178.0;
            sum.0 += (x as f32 + 0.5) / scale * coverage;
            sum.1 += (y as f32 + 0.5) / scale * coverage;
            weight += coverage;
        }
    }
    assert!(
        weight > 30.0,
        "expected visible content at {center}, found {weight} covered pixels"
    );
    (sum.0 / weight, sum.1 / weight)
}
