use crate::{robot_exit, robot_liquid_stage, robot_shot};

use std::{path::PathBuf, process::ExitCode, sync::atomic::AtomicBool, time::Duration};

use cranpose::{liquid::prelude::*, rememberMutableStateOf, RobotTimelineAction, RobotTimelineStep, AppLauncher, Modifier};
use robot_liquid_stage::LiquidStripedStage;

const WINDOW_WIDTH: u32 = 720;
const WINDOW_HEIGHT: u32 = 200;
const SEGMENT_COUNT: usize = 3;
const CONTROL_WIDTH: f32 = 600.0;
const CONTROL_LEFT: f32 = (WINDOW_WIDTH as f32 - CONTROL_WIDTH) * 0.5;
const CONTROL_TOP: f32 = 70.0;
const SEGMENT_Y: f32 = CONTROL_TOP + 16.0;
const GLIDE_FRAMES: u32 = 40;

/// Frames the glide must draw for its recompositions to mean anything: a
/// window that rendered nothing says nothing about what a glide costs.
const GLIDE_FRAMES_DRAWN: u64 = 20;

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

fn check_resting_track(resting: &cranpose::RobotScreenshot) {
    let scale = resting.width as f32 / resting.logical_width;
    let track_red = |x: f32| {
        let x = (x * scale) as usize;
        let y = (154.0 * scale) as usize;
        resting.pixels[(y * resting.width as usize + x) * 4]
    };
    let transmitted_contrast = track_red(380.0).saturating_sub(track_red(372.0));
    assert!(transmitted_contrast >= 120,
        "native inactive switch track transmits the striped backdrop; red contrast={transmitted_contrast}");
    let x = (segment_x(0) * scale) as usize;
    let y = ((CONTROL_TOP + 4.0) * scale) as usize;
    let red = resting.pixels[(y * resting.width as usize + x) * 4];
    let track_x = (segment_x(2) * scale) as usize;
    let track = resting.pixels[(y * resting.width as usize + track_x) * 4];
    let expected = if track < 128 { 90 } else { 255 };
    assert!(red.abs_diff(expected) <= 3, "native selection edge is opaque two points inside; got {red}, expected {expected}");

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
            check_resting_track(&resting);

            let mut most = (0, "");
            for (index, (name, x, y)) in [
                ("last-segment", segment_x(SEGMENT_COUNT - 1), SEGMENT_Y),
                ("first-segment", segment_x(0), SEGMENT_Y),
                ("toggle-on", 360.0, 154.0),
                ("toggle-off", 360.0, 154.0),
            ].into_iter().enumerate() {
                robot.reset_fps_stats().expect("reset fps stats");
                let mut timeline = Vec::with_capacity(GLIDE_FRAMES as usize + 1);
                timeline.push(RobotTimelineStep {
                    advance_ms: 0.0,
                    actions: vec![
                        RobotTimelineAction::MoveTo { x, y },
                        RobotTimelineAction::MouseDown,
                        RobotTimelineAction::MouseUp,
                    ],
                    capture: true,
                });
                timeline.extend((0..GLIDE_FRAMES).map(|_| RobotTimelineStep {
                    advance_ms: 1000.0 / 60.0,
                    actions: Vec::new(),
                    capture: true,
                }));
                let frames = robot.capture_interaction_keyframes(1.0, &timeline)
                    .expect("capture exact-clock glide");
                if name == "toggle-on" {
                    let released = &frames[20];
                    let scale = released.width as f32 / released.logical_width;
                    let x = (371.0 * scale) as usize;
                    let y = (154.0 * scale) as usize;
                    let red = released.pixels[(y * released.width as usize + x) * 4];
                    assert!(red >= 230,
                        "native switch restores its white thumb within 333 ms of release; red={red}");
                }
                let shot = frames.last().expect("glide shot");
                robot_shot::save(shot, &shot_dir, &format!("{}-{name}.png", index + 1));
                let stats = robot.fps_stats().expect("glide fps stats");
                let changing_frames = frames.windows(2).filter(|pair| pair[0].pixels != pair[1].pixels).count() as u64;
                println!(
                    "[liquid] {name}: changing_frames={} recompositions={}",
                    changing_frames, stats.recompositions
                );
                if changing_frames < GLIDE_FRAMES_DRAWN {
                    robot_exit::fail_and_await_shutdown(
                        &robot,
                        &FAILED,
                        &format!(
                            "{name} drew {changing_frames} changing frames, fewer than the \
                             {GLIDE_FRAMES_DRAWN} its recompositions are counted over"
                        ),
                    );
                }
                if stats.recompositions > most.0 {
                    most = (stats.recompositions, name);
                }
                robot_shot::settle(&robot, 300);
            }

            if most.0 > RECOMPOSITIONS_ALLOWED_ON_GLIDE {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    &format!(
                        "{} recomposed {} times, past the \
                         {RECOMPOSITIONS_ALLOWED_ON_GLIDE} a glide may: something reads the \
                         gliding lens in composition instead of where it is drawn",
                        most.1, most.0
                    ),
                );
            }
            println!(
                "PASS: a selection animation recomposed at most {} times, within the \
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
                        .width(CONTROL_WIDTH),
                    selected.get(),
                    move |index| selected.set(index),
                    |scope| {
                        for label in SEGMENTS {
                            scope.segment(label);
                        }
                    },
                );
                let checked = rememberMutableStateOf(|| false);
                LiquidToggle(
                    Modifier::empty().absolute_offset(328.5, 140.0),
                    checked.get(),
                    move |value| checked.set(value),
                );
            });
        })
        .map_or(ExitCode::FAILURE, |()| robot_exit::exit_code(&FAILED))
}
