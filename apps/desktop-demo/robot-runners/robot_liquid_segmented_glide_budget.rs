use crate::{robot_exit, robot_liquid_stage, robot_shot};

use std::{path::PathBuf, process::ExitCode, sync::atomic::AtomicBool, time::Duration};

use cranpose::{liquid::prelude::*, rememberMutableStateOf, AppLauncher, Modifier, Size};
use robot_liquid_stage::LiquidStripedStage;

const WINDOW_WIDTH: u32 = 720;
const WINDOW_HEIGHT: u32 = 200;
const SEGMENT_COUNT: usize = 3;
const CONTROL_WIDTH: f32 = 600.0;
const CONTROL_LEFT: f32 = (WINDOW_WIDTH as f32 - CONTROL_WIDTH) * 0.5;
const CONTROL_TOP: f32 = 70.0;
const SEGMENT_Y: f32 = CONTROL_TOP + 20.0;
const GLIDE_FRAMES: u32 = 40;

/// Frames the glide must draw for its recompositions to mean anything: a
/// window that rendered nothing says nothing about what a glide costs.
const GLIDE_FRAMES_DRAWN: u32 = 20;

/// Recompositions a glide across the control may cost.
///
/// The lens glides by drawing: a frame moves it without recomposing, and the
/// control recomposes for the selection, for the lens lifting and landing,
/// and for each segment the lens crosses. Reading the lens in composition
/// recomposed the control once a frame.
const RECOMPOSITIONS_ALLOWED_ON_GLIDE: u64 = 12;

static FAILED: AtomicBool = AtomicBool::new(false);

const SEGMENTS: [&str; SEGMENT_COUNT] = ["Receiving", "Sending", "Errored"];

fn segment_x(index: usize) -> f32 {
    let width = CONTROL_WIDTH / SEGMENT_COUNT as f32;
    CONTROL_LEFT + width * (index as f32 + 0.5)
}

pub(crate) fn main() -> ExitCode {
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR")
            .unwrap_or_else(|_| "target/liquid-segmented-glide-budget".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Liquid Segmented Glide Budget")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(120);
            std::thread::sleep(Duration::from_millis(700));
            robot_shot::settle(&robot, 300);
            let resting = robot.screenshot().expect("resting shot");
            robot_shot::save(&resting, &shot_dir, "0-resting.png");

            let mut most = (0, 0);
            for (index, segment) in [SEGMENT_COUNT - 1, 0].into_iter().enumerate() {
                robot.reset_fps_stats().expect("reset fps stats");
                robot
                    .click(segment_x(segment), SEGMENT_Y)
                    .expect("click a segment");
                // A headless app renders when it is asked for pixels, so each
                // frame of the glide ends in a screenshot.
                let mut shot = robot.screenshot().expect("glide shot");
                for _ in 0..GLIDE_FRAMES {
                    robot.pump_frames(1).expect("pump a glide frame");
                    shot = robot.screenshot().expect("glide shot");
                }
                robot_shot::save(&shot, &shot_dir, &format!("{}-glide-to-{segment}.png", index + 1));
                let stats = robot.fps_stats().expect("glide fps stats");
                println!(
                    "[segmented] glide to {segment}: frames={} recompositions={}",
                    stats.interval_count, stats.recompositions
                );
                if stats.interval_count < GLIDE_FRAMES_DRAWN {
                    robot_exit::fail_and_await_shutdown(
                        &robot,
                        &FAILED,
                        &format!(
                            "the glide to segment {segment} drew {} frames, fewer than the \
                             {GLIDE_FRAMES_DRAWN} its recompositions are counted over",
                            stats.interval_count
                        ),
                    );
                }
                if stats.recompositions > most.0 {
                    most = (stats.recompositions, segment);
                }
                robot_shot::settle(&robot, 300);
            }

            if most.0 > RECOMPOSITIONS_ALLOWED_ON_GLIDE {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    &format!(
                        "the glide to segment {} recomposed {} times, past the \
                         {RECOMPOSITIONS_ALLOWED_ON_GLIDE} a glide may: something reads the \
                         gliding lens in composition instead of where it is drawn",
                        most.1, most.0
                    ),
                );
            }
            println!(
                "PASS: a segmented glide recomposed at most {} times, within the \
                 {RECOMPOSITIONS_ALLOWED_ON_GLIDE} it may",
                most.0
            );
            robot.exit().expect("exit");
        })
        .try_run(move || {
            LiquidStripedStage(WINDOW_WIDTH, WINDOW_HEIGHT, move || {
                let selected = rememberMutableStateOf(|| 0usize);
                LiquidSegmentedControl(
                    Modifier::empty()
                        .absolute_offset(CONTROL_LEFT, CONTROL_TOP)
                        .size(Size {
                            width: CONTROL_WIDTH,
                            height: 40.0,
                        }),
                    selected.get(),
                    move |index| selected.set(index),
                    |scope| {
                        for label in SEGMENTS {
                            scope.segment(label);
                        }
                    },
                );
            });
        })
        .map_or(ExitCode::FAILURE, |()| robot_exit::exit_code(&FAILED))
}
