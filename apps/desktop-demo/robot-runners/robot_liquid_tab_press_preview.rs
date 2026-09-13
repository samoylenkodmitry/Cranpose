mod robot_exit;
mod robot_shot;

#[path = "../src/test_screens/liquid_tab_reference.rs"]
mod liquid_tab_reference;

use std::{path::PathBuf, time::Duration};

use cranpose::{AppLauncher, RobotScreenshot};

fn main() -> anyhow::Result<()> {
    let directory = PathBuf::from(
        std::env::var("CRANPOSE_ROBOT_OUTPUT_DIR")
            .unwrap_or_else(|_| "target/liquid-tab-press-preview".to_string()),
    );
    std::fs::create_dir_all(&directory)?;
    AppLauncher::new()
        .with_title("Liquid tab press preview")
        .with_size(402, 874)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(90);
            robot_shot::settle(&robot, 700);
            let rest = robot.screenshot().expect("rest screenshot");
            robot_shot::save_checked(&directory.join("rest.png"), &rest).expect("save rest");
            assert!(
                accent_ink(&rest, 0) > 30,
                "the selected first tab must contain accent ink"
            );
            assert!(
                accent_ink(&rest, 3) < 5,
                "the last tab must start unselected"
            );
            let x = 20.0 + 90.5 * 3.5;
            let y = 821.0;
            robot.touch_down(x, y).expect("hold Account");
            std::thread::sleep(Duration::from_millis(450));
            robot.pump_frames(10).expect("render press");
            let held = robot.screenshot().expect("held screenshot");
            robot_shot::save_checked(&directory.join("held.png"), &held).expect("save held");
            let target_ink = accent_ink(&held, 3);
            let previous_ink = accent_ink(&held, 0);
            println!("held accent pixels: target={target_ink} previous={previous_ink}");
            if target_ink <= 30 || previous_ink >= 5 {
                robot_exit::fail(
                    &robot,
                    "pressing an unselected tab must preview its lens before release",
                );
            }
            robot.touch_up(x, y).expect("release Account");
            robot_shot::settle(&robot, 600);
            let released = robot.screenshot().expect("release screenshot");
            robot_shot::save_checked(&directory.join("released.png"), &released)
                .expect("save release");
            assert!(
                accent_ink(&released, 3) > 30,
                "the released destination remains selected"
            );
            robot.exit().expect("exit");
        })
        .try_run(|| liquid_tab_reference::LiquidTabReference(false, false))?;
    Ok(())
}

fn accent_ink(shot: &RobotScreenshot, index: usize) -> usize {
    let scale = shot.width as f32 / shot.logical_width;
    let center = 20.0 + 90.5 * (index as f32 + 0.5);
    let mut count = 0;
    for y in (793.0 * scale) as u32..(847.0 * scale) as u32 {
        for x in ((center - 22.0) * scale) as u32..((center + 22.0) * scale) as u32 {
            let offset = ((y * shot.width + x) * 4) as usize;
            let rgb = &shot.pixels[offset..offset + 3];
            if rgb[2] > 160 && rgb[0] < 90 && rgb[1] > 60 && rgb[1] < 190 {
                count += 1;
            }
        }
    }
    count
}
