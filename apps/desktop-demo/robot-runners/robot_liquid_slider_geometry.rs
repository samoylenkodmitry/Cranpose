use crate::{robot_exit, robot_shot};
use cranpose::{
    liquid::prelude::*, rememberMutableStateOf, widgets::{Box, BoxSpec},
    AppLauncher, Color, Modifier, RobotScreenshot,
};
use std::path::PathBuf;

pub(crate) fn main() -> anyhow::Result<()> {
    let output = PathBuf::from(std::env::var("CRANPOSE_ROBOT_OUTPUT_DIR")
        .unwrap_or_else(|_| "target/liquid-slider-geometry".to_owned()));
    std::fs::create_dir_all(&output)?;
    AppLauncher::new()
        .with_title("Liquid slider geometry")
        .with_size(402, 120)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(90);
            robot_shot::settle(&robot, 700);
            for (name, pointer, expected_left) in [
                ("middle", None, 183.0),
                ("minimum", Some(51.0), 51.0),
                ("maximum", Some(351.0), 314.0),
            ] {
                if let Some(x) = pointer {
                    robot.click(x, 60.0).expect("set slider value");
                    robot_shot::settle(&robot, 900);
                }
                let shot = robot.screenshot().expect("capture slider");
                robot_shot::save_checked(&output.join(format!("{name}.png")), &shot)
                    .expect("save slider");
                let [left, top, width, height] = white_bounds(&shot);
                println!("{name}: thumb at {left},{top}, size {width}x{height} pt");
                if (width - 37.0).abs() > 0.5 || (height - 24.0).abs() > 0.5 {
                    robot_exit::fail(&robot, "slider thumb must match the native 37 by 24 point capsule");
                }
                if (left - expected_left).abs() > 0.5 || (top - 48.0).abs() > 0.5 {
                    robot_exit::fail(&robot, "slider thumb must preserve native endpoint travel");
                }
            }
            verify_held_slider(&robot, &output);
            robot.exit().expect("exit");
        })
        .try_run(|| {
            LiquidTheme(LiquidThemeSpec { scheme: SchemeMode::Dark, ..Default::default() }, || {
                let value = rememberMutableStateOf(|| 0.5);
                Box(Modifier::empty().fill_max_size().background(Color::BLACK), BoxSpec::default(), move || {
                    LiquidSlider(Modifier::empty().offset(51.0, 44.0).width(300.0), value.get(), move |next| value.set(next));
                });
            });
        })?;
    Ok(())
}

fn verify_held_slider(robot: &cranpose::Robot, output: &std::path::Path) {
    robot.touch_down(201.0, 60.0).expect("hold slider");
    robot_shot::settle(robot, 900);
    let held = robot.screenshot().expect("capture held slider");
    robot_shot::save_checked(&output.join("held.png"), &held).expect("save held slider");
    let scale = held.width as f32 / held.logical_width;
    let center_x = (201.0 * scale) as usize;
    let mut top = held.height;
    let mut bottom = 0;
    let mut lower_rim = 0;
    for y in (30.0 * scale) as u32..(90.0 * scale) as u32 {
        let pixel = &held.pixels[(y as usize * held.width as usize + center_x) * 4..];
        if pixel[0] > 8 {
            top = top.min(y);
            bottom = bottom.max(y + 1);
        }
        if y as f32 / scale > 70.0 {
            lower_rim = lower_rim.max(pixel[0]);
        }
    }
    let held_height = (bottom - top) as f32 / scale;
    println!("held lens: height {held_height} pt, lower rim {lower_rim}/255");
    if (held_height - 37.0).abs() > 1.0 / scale || lower_rim > 100 {
        robot_exit::fail(robot, "held slider must match the native 37 point lens and restrained rim");
    }
    robot.touch_up(201.0, 60.0).expect("release slider");
}

fn white_bounds(shot: &RobotScreenshot) -> [f32; 4] {
    let scale = shot.width as f32 / shot.logical_width;
    let mut left = shot.width;
    let mut top = shot.height;
    let mut right = 0;
    let mut bottom = 0;
    for (index, pixel) in shot.pixels.chunks_exact(4).enumerate() {
        if pixel[..3].iter().all(|channel| *channel > 240) {
            let x = index as u32 % shot.width;
            let y = index as u32 / shot.width;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }
    assert!(left < right && top < bottom, "expected a visible white thumb");
    let mut coverage = 0.0;
    let mut moment = 0.0;
    let row_start = (shot.height / 2 * shot.width * 4) as usize;
    let row_end = row_start + shot.width as usize * 4;
    for (x, pixel) in shot.pixels[row_start..row_end].chunks_exact(4).enumerate() {
        let background = if pixel[2] > pixel[0] { 0.0 } else { 25.0 };
        let weight = ((f32::from(pixel[0]) - background) / (255.0 - background)).max(0.0);
        coverage += weight;
        moment += (x as f32 + 0.5) * weight;
    }
    assert!(coverage > 0.0, "expected visible thumb coverage");
    [
        (moment / coverage - coverage * 0.5) / scale,
        top as f32 / scale,
        coverage / scale,
        (bottom - top) as f32 / scale,
    ]
}
