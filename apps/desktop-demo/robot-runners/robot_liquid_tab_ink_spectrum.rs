use crate::{liquid_tab_reference, robot_exit, robot_shot};

use cranpose::{AppLauncher, RobotScreenshot};

pub(crate) fn main() -> anyhow::Result<()> {
    let json = std::env::var("REFERENCE_CONTENT").unwrap_or_else(|_| {
        r#"{"titles":["Inbox","WWW","II","Settings"],"icons":[1,3,2,0],"accent":[0.2,0.65,0.1]}"#.to_string()
    });
    let content: serde_json::Value = serde_json::from_str(&json)?;
    anyhow::ensure!(
        content["accent"] == serde_json::json!([0.2, 0.65, 0.1]),
        "the spectrum regression requires the green foreground fixture"
    );
    liquid_tab_reference::configure_reference_content(&json)?;
    let output = std::path::PathBuf::from(
        std::env::var("CRANPOSE_ROBOT_OUTPUT_DIR")
            .unwrap_or_else(|_| "target/liquid-tab-ink-spectrum".to_string()),
    );
    std::fs::create_dir_all(&output)?;
    AppLauncher::new()
        .with_title("Liquid tab foreground spectrum")
        .with_size(440, 956)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(90);
            robot_shot::settle(&robot, 700);
            robot.touch_down(362.75, 904.0).expect("hold last destination");
            robot_shot::settle(&robot, 900);
            let mut fringes = 0;
            for (index, x) in [340.0, 315.0, 290.0, 265.0, 240.0, 215.0, 240.0, 265.0, 290.0, 315.0, 290.0, 265.0, 240.0].into_iter().enumerate() {
                robot.touch_move(x, 904.0).expect("move bubble across bookmark");
                robot_shot::settle(&robot, 16);
                let shot = robot.screenshot().expect("capture foreground dispersion");
                robot_shot::save_checked(&output.join(format!("frame-{index:02}.png")), &shot).expect("save pixels");
                fringes = fringes.max(spectral_pixels(&shot));
            }
            println!("Blue-over-red foreground fringe pixels: {fringes}");
            if fringes < 3 {
                robot_exit::fail(&robot, "the neutral backdrop must transmit the bookmark's separated color channels at the rim");
            }
            robot.touch_up(240.0, 904.0).expect("release bookmark");
            robot.exit().expect("exit");
        })
        .try_run(|| liquid_tab_reference::LiquidTabReference(false, false))?;
    Ok(())
}

fn spectral_pixels(shot: &RobotScreenshot) -> usize {
    let scale = shot.width as f32 / shot.logical_width;
    let mut count = 0;
    for y in (865.0 * scale) as u32..(884.0 * scale) as u32 {
        for x in (250.0 * scale) as u32..(286.0 * scale) as u32 {
            let pixel = &shot.pixels[((y * shot.width + x) * 4) as usize..];
            count += usize::from(i32::from(pixel[2]) - i32::from(pixel[0]) > 45);
        }
    }
    count
}
