mod robot_exit;
mod robot_shot;

#[path = "../src/test_screens/liquid_tab_reference.rs"]
mod liquid_tab_reference;

use std::path::PathBuf;

use cranpose::{AppLauncher, RobotScreenshot};

fn main() -> anyhow::Result<()> {
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
            let mut anchors = Vec::new();
            for (index, position) in [158.0, 187.0, 158.0].into_iter().enumerate() {
                robot.touch_move(position, 904.0).expect("move held lens");
                robot_shot::settle(&robot, 900);
                let shot = robot.screenshot().expect("capture anchored content");
                robot_shot::save_checked(&output.join(format!("anchor-{index}.png")), &shot)
                    .expect("save pixels");
                let browse = ink_center(&shot, 172.417, true);
                let neighbors =
                    (ink_center(&shot, 77.25, false) + ink_center(&shot, 267.583, false)) * 0.5;
                anchors.push(browse - neighbors);
            }
            robot.touch_up(158.0, 904.0).expect("release");
            let drift = anchors.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                - anchors.iter().copied().fold(f32::INFINITY, f32::min);
            println!("Browse anchor offsets {anchors:?}; drift {drift} pt");
            if drift > 0.5 {
                robot_exit::fail(
                    &robot,
                    "the lens translated the icon relative to neighboring tab anchors",
                );
            }
            robot.exit().expect("exit");
        })
        .try_run(|| liquid_tab_reference::LiquidTabReference(false, false))?;
    Ok(())
}

fn ink_center(shot: &RobotScreenshot, center: f32, accent: bool) -> f32 {
    let scale = shot.width as f32 / shot.logical_width;
    let mut sum = 0.0;
    let mut count = 0;
    for y in (884.0 * scale) as u32..(910.0 * scale) as u32 {
        for x in ((center - 24.0) * scale) as u32..((center + 24.0) * scale) as u32 {
            let pixel = &shot.pixels[((y * shot.width + x) * 4) as usize..];
            let matches = if accent {
                pixel[0] < 10 && (126..=146).contains(&pixel[1]) && pixel[2] > 245
            } else {
                pixel[0] < 40 && pixel[1] < 40 && pixel[2] < 40
            };
            if matches {
                sum += (x as f32 + 0.5) / scale;
                count += 1;
            }
        }
    }
    assert!(
        count > 30,
        "expected visible icon at {center}, found {count} pixels"
    );
    sum / count as f32
}
