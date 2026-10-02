use std::{path::PathBuf, process::ExitCode};

use cranpose::{
    liquid::prelude::*,
    rememberMutableStateOf,
    widgets::{Box, BoxSpec, Text},
    AppLauncher, Color, Modifier, RobotScreenshot, RobotTimelineAction, RobotTimelineStep, Size,
};

use crate::{robot_exit, robot_shot};

const SCALE: f32 = 2.0;
const CENTRE_X: f32 = 200.0;

pub(crate) fn main() -> ExitCode {
    let output = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/interaction-shadow".into()),
    );
    std::fs::create_dir_all(&output).expect("create output");
    AppLauncher::new()
        .with_title("Interaction shadow continuity")
        .with_size(400, 320)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(180);
            robot_shot::settle(&robot, 700);
            let mut failure = None;
            for (name, y, background) in [
                ("slider", 80.0, [192, 192, 192]),
                ("button", 240.0, [74, 0, 224]),
            ] {
                let steps = press_and_release(y);
                let frames = robot
                    .capture_interaction_keyframes(SCALE, &steps)
                    .expect("capture press and release");
                for (index, frame) in frames.iter().enumerate() {
                    robot_shot::save(frame, &output, &format!("{name}-{index:03}.png"));
                    let (cut, depth) = shadow_profile(frame, y, background, name == "slider");
                    if let Some(cut) = cut {
                        failure.get_or_insert_with(|| format!("{name} frame {index}: {cut}"));
                    }
                    if index == 0 {
                        println!("{name}: resting shadow depth {depth}/255");
                        if depth < 6 {
                            failure.get_or_insert_with(|| {
                                format!("{name}: missing shadow ({depth}/255)")
                            });
                        }
                    }
                }
                robot_shot::settle(&robot, 700);
            }
            if let Some(failure) = failure {
                robot_exit::fail(&robot, &failure);
            }
            println!("PASS: slider and floating button shadows fade throughout press and release");
            robot.exit().expect("exit");
        })
        .try_run(|| {
            LiquidTheme(
                LiquidThemeSpec {
                    scheme: SchemeMode::Light,
                    ..Default::default()
                },
                || {
                    let value = rememberMutableStateOf(|| 0.5);
                    Box(
                        Modifier::empty()
                            .fill_max_size()
                            .background(Color::from_rgb_u8(192, 192, 192)),
                        BoxSpec::default(),
                        move || {
                            LiquidSlider(
                                Modifier::empty().offset(50.0, 64.0).width(300.0),
                                value.get(),
                                move |next| value.set(next),
                            );
                            Box(
                                Modifier::empty()
                                    .offset(0.0, 160.0)
                                    .size(Size::new(400.0, 160.0))
                                    .background(Color::from_rgb_u8(74, 0, 224)),
                                BoxSpec::default(),
                                || {
                                    GlassButton(
                                        Modifier::empty()
                                            .offset(178.0, 58.0)
                                            .size(Size::new(44.0, 44.0)),
                                        GlassButtonSpec::glass()
                                            .with_glass(Glass::regular().blur_radius(0.0)),
                                        || {},
                                        || {
                                            Text("★", Modifier::empty(), Default::default());
                                        },
                                    );
                                },
                            );
                        },
                    );
                },
            );
        })
        .expect("launch shadow regression");
    ExitCode::SUCCESS
}

fn press_and_release(y: f32) -> Vec<RobotTimelineStep> {
    let mut steps = vec![
        RobotTimelineStep {
            advance_ms: 0.0,
            actions: vec![RobotTimelineAction::MoveTo { x: CENTRE_X, y }],
            capture: true,
        },
        RobotTimelineStep {
            advance_ms: 0.0,
            actions: vec![RobotTimelineAction::MouseDown],
            capture: true,
        },
    ];
    for release in [false, true] {
        if release {
            steps.push(RobotTimelineStep {
                advance_ms: 0.0,
                actions: vec![RobotTimelineAction::MouseUp],
                capture: true,
            });
        }
        steps.extend((0..48).map(|_| RobotTimelineStep {
            advance_ms: 1000.0 / 120.0,
            actions: Vec::new(),
            capture: true,
        }));
    }
    steps
}

fn shadow_profile(
    frame: &RobotScreenshot,
    centre_y: f32,
    background: [u8; 3],
    has_track: bool,
) -> (Option<String>, u8) {
    let sample = robot_shot::logical_sampler(frame);
    let (cx, cy, axial, radius) = glass_outline(&sample, centre_y, background);
    let mut cut = None;
    let mut depth = 0;
    for row in -90..=90 {
        let y = centre_y + row as f32 * 0.5 + 0.25;
        if has_track && (y - centre_y).abs() < 4.0 {
            continue;
        }
        for direction in [-1.0, 1.0] {
            let mut previous = background;
            for step in 0..120 {
                let x = CENTRE_X + direction * (60.0 - step as f32 * 0.5) + 0.25;
                let distance = ((x - cx).abs() - axial).max(0.0).hypot(y - cy) - radius;
                if distance < 1.5 {
                    break;
                }
                let (r, g, b) = sample(x, y);
                let pixel = [r, g, b];
                depth = depth.max(
                    background
                        .iter()
                        .zip(pixel)
                        .map(|(&a, b)| a.saturating_sub(b))
                        .max()
                        .unwrap_or(0),
                );
                let drop = previous
                    .iter()
                    .zip(pixel)
                    .map(|(&a, b)| a.saturating_sub(b))
                    .max()
                    .unwrap_or(0);
                if drop > 4 && cut.is_none() {
                    cut = Some(format!(
                        "shadow jumps {drop}/255 over half a point at ({x}, {y})"
                    ));
                }
                previous = pixel;
            }
        }
    }
    (cut, depth)
}

fn glass_outline(
    sample: &impl Fn(f32, f32) -> (u8, u8, u8),
    centre_y: f32,
    background: [u8; 3],
) -> (f32, f32, f32, f32) {
    let (mut left, mut top) = (f32::INFINITY, f32::INFINITY);
    let (mut right, mut bottom) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for row in -90..=90 {
        for column in -120..=120 {
            let x = CENTRE_X + column as f32 * 0.5 + 0.25;
            let y = centre_y + row as f32 * 0.5 + 0.25;
            let (r, g, b) = sample(x, y);
            if [r, g, b]
                .iter()
                .zip(background)
                .all(|(&v, bg)| i16::from(v) > i16::from(bg) + 8)
            {
                left = left.min(x - 0.25);
                right = right.max(x + 0.25);
                top = top.min(y - 0.25);
                bottom = bottom.max(y + 0.25);
            }
        }
    }
    assert!(
        left < right && top < bottom,
        "the control must have a visible glass contour"
    );
    let radius = (bottom - top) * 0.5;
    (
        (left + right) * 0.5,
        (top + bottom) * 0.5,
        ((right - left) * 0.5 - radius).max(0.0),
        radius,
    )
}
