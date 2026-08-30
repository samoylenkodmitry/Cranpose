use std::time::{Duration, Instant};

use cranpose::{AppLauncher, FramePacingMode};
use cranpose_testing::find_text_in_semantics;

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 700;
const MEASURE_FOR: Duration = Duration::from_millis(1500);
const WARMUP_FOR: Duration = Duration::from_millis(1500);
const HARD60_MIN_FPS: f32 = 40.0;
const HARD60_MAX_FPS: f32 = 80.0;
const FREE_RUN_PANEL_MULTIPLE: f32 = 2.0;
const TAB: &str = "Animations";

struct Measured {
    fps: f32,
    work_fps: f32,
}

fn measure(robot: &cranpose::Robot, window: Duration) -> Measured {
    let started = Instant::now();
    robot.reset_fps_stats().expect("reset fps stats");
    std::thread::sleep(window);
    let elapsed = started.elapsed();
    let stats = robot.fps_stats().expect("read fps stats");
    Measured {
        fps: stats.frame_count as f32 / elapsed.as_secs_f32(),
        work_fps: stats.work_fps,
    }
}

fn press(robot: &cranpose::Robot, mode: FramePacingMode) {
    let (x, y) = robot
        .pacing_control_center(mode)
        .unwrap_or_else(|err| panic!("query the {} control: {err}", mode.label()))
        .unwrap_or_else(|| panic!("the {} control must be in the dev overlay", mode.label()));
    robot
        .click(x, y)
        .unwrap_or_else(|err| panic!("press {}: {err}", mode.label()));
    std::thread::sleep(Duration::from_millis(400));
}

fn fail(message: String) -> ! {
    println!("FAIL: {message}");
    std::process::exit(1);
}

fn main() {
    let _ = env_logger::try_init();

    AppLauncher::new()
        .with_title("Frame pacing")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_frame_pacing_mode(FramePacingMode::Vsync)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_fps_counter(true)
        .with_frame_pacing_controls(true)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() == Ok("1"))
        .with_test_driver(move |robot| {
            let _ = robot.wait_for_idle();

            let (tab_x, tab_y, tab_w, tab_h) =
                find_text_in_semantics(&robot, TAB).expect("the tab under test must be on screen");
            robot
                .click(tab_x + tab_w * 0.5, tab_y + tab_h * 0.5)
                .expect("click the tab under test");
            let _ = robot.wait_for_idle();
            std::thread::sleep(Duration::from_millis(300));

            let stage = |label: &str, window: Duration| {
                let measured = measure(&robot, window);
                println!(
                    "frame_pacing stage={label} observed_fps={:.1} work_fps={:.1}",
                    measured.fps, measured.work_fps
                );
                measured
            };

            stage("warmup", WARMUP_FOR);
            let vsync = stage("vsync", MEASURE_FOR);

            press(&robot, FramePacingMode::Hard60);
            let hard60 = stage("hard60", MEASURE_FOR);

            press(&robot, FramePacingMode::NoVsync);
            let free = stage("novsync", MEASURE_FOR);

            press(&robot, FramePacingMode::Vsync);
            let recapped = stage("vsync-again", MEASURE_FOR);

            robot.exit().ok();

            if !(HARD60_MIN_FPS..=HARD60_MAX_FPS).contains(&hard60.fps) {
                fail(format!(
                    "pressing 60fps must cap the loop near 60fps, measured {:.1}fps. A reading far \
                     above the cap means the press never reached the loop; far below means the app \
                     cannot keep up with its own cap (work_fps={:.1}).",
                    hard60.fps, hard60.work_fps,
                ));
            }

            let panel_cadence = vsync.fps.max(hard60.fps);
            if free.fps <= panel_cadence * FREE_RUN_PANEL_MULTIPLE {
                fail(format!(
                    "pressing NoVSync must free the loop from the display: measured {:.1}fps \
                     against a panel cadence of {:.1}fps (work_fps={:.1}). A result at or near the \
                     refresh rate means the loop is still following the panel -- either waiting on \
                     it for the next redraw, or waiting on a swapchain that was never taken off \
                     vsync. A result well above the refresh rate but still far under work_fps is \
                     the cost of presenting in this environment rather than a loop that will not \
                     free-run: a software X server presents in tens of milliseconds.",
                    free.fps, panel_cadence, free.work_fps,
                ));
            }

            if recapped.fps >= free.fps * 0.5 {
                fail(format!(
                    "pressing VSync must put the cap back: measured {:.1}fps after {:.1}fps \
                     free-running (vsync baseline was {:.1}fps).",
                    recapped.fps, free.fps, vsync.fps,
                ));
            }

            println!(
                "PASS: vsync={:.1}fps 60fps-cap={:.1}fps novsync={:.1}fps vsync-again={:.1}fps",
                vsync.fps, hard60.fps, free.fps, recapped.fps,
            );
        })
        .try_run(desktop_app::app::DesktopApp)
        .expect("launch the demo");
}
