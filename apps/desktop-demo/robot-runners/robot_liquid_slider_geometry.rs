use crate::{robot_exit, robot_shot};
use cranpose::{
    liquid::prelude::*, rememberMutableStateOf, widgets::{Box, BoxSpec},
    AppLauncher, Color, Modifier, RobotScreenshot, RobotTimelineAction, RobotTimelineStep,
};
use std::{cell::Cell, path::PathBuf};
use cranpose_core::MutableState;

thread_local! {
    static VALUE: Cell<Option<MutableState<f32>>> = const { Cell::new(None) };
}

pub(crate) fn main() -> anyhow::Result<()> {
    let output = PathBuf::from(std::env::var("CRANPOSE_ROBOT_OUTPUT_DIR")
        .unwrap_or_else(|_| "target/liquid-slider-geometry".to_owned()));
    std::fs::create_dir_all(&output)?;
    AppLauncher::new()
        .with_title("Liquid slider geometry")
        .with_size(402, 120)
        .with_headless(true)
        .with_robot_app_hook(set_value_hook)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(90);
            robot_shot::settle(&robot, 700);
            for (name, value, expected_left) in [
                ("middle", "0.5", 183.0),
                ("minimum", "0", 51.0),
                ("maximum", "1", 314.0),
            ] {
                set_value(&robot, value);
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
            verify_track_touch_is_ignored(&robot);
            verify_native_drag_mapping(&robot);
            verify_native_slider_release(&robot);
            robot.exit().expect("exit");
        })
        .try_run(|| {
            LiquidTheme(LiquidThemeSpec { scheme: SchemeMode::Dark, ..Default::default() }, || {
                let value = rememberMutableStateOf(|| 0.5);
                VALUE.set(Some(value));
                Box(Modifier::empty().fill_max_size().background(Color::BLACK), BoxSpec::default(), move || {
                    LiquidSlider(Modifier::empty().offset(51.0, 44.0).width(300.0), value.get(), move |next| value.set(next));
                });
            });
        })?;
    Ok(())
}

fn verify_held_slider(robot: &cranpose::Robot, output: &std::path::Path) {
    set_value(robot, "0.5");
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

fn verify_native_drag_mapping(robot: &cranpose::Robot) {
    for (direction, route) in [
        (1.0, [(280.0, "73%"), (122.0, "20%"), (280.0, "73%")]),
        (-1.0, [(122.0, "27%"), (280.0, "80%"), (201.0, "53%")]),
    ] {
        set_value(robot, "0.5");
        robot.touch_down(201.0, 60.0).expect("grab slider");
        robot.touch_move(201.0 + 6.0 * direction, 60.0).expect("move inside drag threshold");
        robot_shot::settle(robot, 100);
        assert_slider_value(robot, "50%");
        for (x, expected) in route {
            robot.touch_move(x, 60.0).expect("move held slider");
            robot_shot::settle(robot, 500);
            assert_slider_value(robot, expected);
        }
        robot.touch_up(route[2].0, 60.0).expect("release slider");
    }
}

fn assert_slider_value(robot: &cranpose::Robot, expected: &str) {
    fn find(elements: &[cranpose::SemanticElement]) -> Option<&str> {
        elements.iter().find_map(|element| {
            element.state_description.as_deref().or_else(|| find(&element.children))
        })
    }
    let semantics = robot.get_semantics().expect("read slider value");
    assert_eq!(find(&semantics), Some(expected), "slider must follow native input and release behavior");
}

fn verify_native_slider_release(robot: &cranpose::Robot) {
    set_value(robot, "0.5");
    let mut steps = vec![RobotTimelineStep {
        advance_ms: 0.0,
        actions: vec![RobotTimelineAction::MoveTo { x: 201.0, y: 60.0 }, RobotTimelineAction::MouseDown],
        capture: true,
    }];
    for (advance_ms, x) in [(600.0, 268.0), (10.0, 272.0), (10.0, 276.0), (10.0, 280.0)] {
        steps.push(RobotTimelineStep {
            advance_ms,
            actions: vec![RobotTimelineAction::MoveTo { x, y: 60.0 }],
            capture: true,
        });
    }
    steps.push(RobotTimelineStep { advance_ms: 800.0, actions: vec![RobotTimelineAction::MouseUp], capture: true });
    steps.extend((0..60).map(|_| RobotTimelineStep { advance_ms: 1000.0 / 60.0, actions: Vec::new(), capture: true }));
    let frames = robot.capture_interaction_keyframes(1.0, &steps).expect("capture slider release frames");
    assert!(frames.len() >= 60, "retain the full release");
    assert_slider_value(robot, "86%");
}

fn set_value_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-value" {
        return Err(format!("unsupported hook {name}"));
    }
    let next = argument.parse::<f32>().map_err(|error| error.to_string())?;
    let state = VALUE.get().ok_or("slider state not installed")?;
    state.set(next);
    Ok(None)
}

fn set_value(robot: &cranpose::Robot, value: &str) {
    robot.invoke_app_hook("set-value", value).expect("set controlled slider value");
    robot_shot::settle(robot, 900);
}

fn verify_track_touch_is_ignored(robot: &cranpose::Robot) {
    set_value(robot, "0.5");
    let before = robot.screenshot().expect("capture resting slider");
    robot.touch_down(122.0, 60.0).expect("touch empty track");
    robot_shot::settle(robot, 700);
    assert_slider_value(robot, "50%");
    let held = robot.screenshot().expect("capture untouched slider");
    assert_eq!(held.pixels, before.pixels, "an empty-track touch must not lift the native thumb");
    for x in [201.0, 280.0, 201.0] {
        robot.touch_move(x, 60.0).expect("drag from empty track");
        robot_shot::settle(robot, 100);
        assert_slider_value(robot, "50%");
    }
    robot.touch_up(201.0, 60.0).expect("release empty-track touch");
    robot_shot::settle(robot, 700);
    assert_slider_value(robot, "50%");
}

fn white_bounds(shot: &RobotScreenshot) -> [f32; 4] {
    let scale = shot.width as f32 / shot.logical_width;
    let mut left = shot.width;
    let mut top = shot.height;
    let mut right = 0;
    let mut bottom = 0;
    for (index, pixel) in shot.pixels.as_chunks::<4>().0.iter().enumerate() {
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
    for (x, pixel) in shot.pixels[row_start..row_end].as_chunks::<4>().0.iter().enumerate() {
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
