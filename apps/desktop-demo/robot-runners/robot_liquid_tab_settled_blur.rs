use crate::{liquid_tab_reference, robot_exit, robot_shot};

use cranpose::{AppLauncher, RobotScreenshot};

pub(crate) fn main() -> anyhow::Result<()> {
    let output = std::path::PathBuf::from(
        std::env::var("CRANPOSE_ROBOT_OUTPUT_DIR")
            .unwrap_or_else(|_| "target/liquid-tab-settled-blur".to_string()),
    );
    std::fs::create_dir_all(&output)?;
    AppLauncher::new()
        .with_title("Liquid tab settled backdrop")
        .with_size(440, 956)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(90);
            robot_shot::settle(&robot, 700);
            robot.touch_down(77.25, 904.0).expect("hold selected tab");
            robot_shot::settle(&robot, 800);
            robot.touch_up(77.25, 904.0).expect("release selected tab");
            robot_shot::settle(&robot, 1200);
            let shot = robot.screenshot().expect("capture settled selection");
            robot_shot::save_checked(&output.join("released.png"), &shot).expect("save pixels");
            let roughness = backdrop_roughness(&shot);
            println!("Settled backdrop second/first difference ratio: {roughness}");
            if roughness >= 1.25 {
                robot_exit::fail(
                    &robot,
                    "settled selection must gain backdrop blur after release",
                );
            }
            robot.exit().expect("exit");
        })
        .try_run(|| liquid_tab_reference::LiquidTabReference(true, false))?;
    Ok(())
}

fn backdrop_roughness(shot: &RobotScreenshot) -> f32 {
    let sample = robot_shot::logical_sampler(shot);
    let mut first = 0.0;
    let mut second = 0.0;
    for y in 894..910 {
        for x in 96..104 {
            let (ar, ag, ab) = sample(x as f32, y as f32);
            let (br, bg, bb) = sample(x as f32 + 1.0, y as f32);
            let (cr, cg, cb) = sample(x as f32 + 2.0, y as f32);
            for (a, b, c) in [(ar, br, cr), (ag, bg, cg), (ab, bb, cb)] {
                first += (b as f32 - a as f32).abs();
                second += (c as f32 - 2.0 * b as f32 + a as f32).abs();
            }
        }
    }
    assert!(first > 100.0, "the checkerboard must remain visible");
    second / first
}
