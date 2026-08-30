use std::{
    process::ExitCode,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use cranpose::AppLauncher;
use desktop_app::app::{self, TEST_ACTIVE_TAB_STATE};

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 800;
const MATERIALIZED_LUMA_FLOOR: f32 = 45.0;
const MAX_LATER_BRIGHTENING: f32 = 18.0;

static FAILED: AtomicBool = AtomicBool::new(false);

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    AppLauncher::new()
        .with_title("Liquid Backdrop Feedback Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_robot_app_hook(set_tab_hook)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();
            robot
                .invoke_app_hook("set-tab", "liquid")
                .expect("select liquid tab");
            settle(&robot, 900);

            let Some(pill) = scroll_to_button(&robot, "Sort filter pill", 260.0) else {
                fail(&robot, "sort filter stage not found");
            };
            let (px, py) = center(pill);

            robot.click(px, py).expect("open sort menu");
            let offsets: Vec<(f32, bool)> = vec![
                (25.0, true),
                (17.0, true),
                (17.0, true),
                (25.0, true),
                (100.0, true),
                (300.0, true),
                (500.0, true),
            ];
            let shots = robot
                .capture_keyframes(1.0, &offsets)
                .expect("capture keyframes");
            settle(&robot, 400);

            let times: Vec<f32> = offsets
                .iter()
                .scan(0.0f32, |t, (delay, _)| {
                    *t += delay;
                    Some(*t)
                })
                .collect();
            let lumas: Vec<f32> = shots
                .iter()
                .map(|shot| luma(sample_rgb(shot, px, py)))
                .collect();
            println!("backdrop-feedback pill=({px:.1},{py:.1}) times={times:?} lumas={lumas:?}");

            let Some(first_materialized) = lumas.iter().position(|&l| l >= MATERIALIZED_LUMA_FLOOR)
            else {
                fail(
                    &robot,
                    "droplet face never materialized over the pill (probe point missed)",
                );
            };
            let base = lumas[first_materialized];
            for (index, &later) in lumas.iter().enumerate().skip(first_materialized + 1) {
                if later - base > MAX_LATER_BRIGHTENING {
                    fail(
                        &robot,
                        &format!(
                            "static glass face brightened after its first composite \
                             (self-feedback): {base:.1} at {}ms -> {later:.1} at {}ms",
                            times[first_materialized], times[index]
                        ),
                    );
                }
            }

            println!("PASS: liquid backdrop feedback contract");
            let _ = robot.exit();
        })
        .try_run(app::combined_app)
        .expect("launch backdrop feedback runner");
    if FAILED.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn set_tab_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-tab" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    if argument != "liquid" {
        return Err(format!("unknown demo tab '{argument}'"));
    }
    let state = TEST_ACTIVE_TAB_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| "active tab state was not installed".to_string())?;
    state.set(app::DemoTab::Liquid);
    Ok(None)
}

fn center(bounds: (f32, f32, f32, f32)) -> (f32, f32) {
    (bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
}

fn scroll_to_button(
    robot: &cranpose::Robot,
    label: &str,
    target_y: f32,
) -> Option<(f32, f32, f32, f32)> {
    for _ in 0..40 {
        if let Some(bounds) = robot.find_button_bounds_exact(label).ok().flatten() {
            let cy = bounds.1 + bounds.3 * 0.5;
            if (cy - target_y).abs() < 60.0 {
                return Some(bounds);
            }
            robot
                .mouse_move(WINDOW_WIDTH as f32 * 0.5, 400.0)
                .expect("hover for scroll");
            robot
                .mouse_scroll(0.0, if cy > target_y { -60.0 } else { 60.0 })
                .expect("scroll");
        } else {
            robot
                .mouse_move(WINDOW_WIDTH as f32 * 0.5, 400.0)
                .expect("hover for scroll");
            robot.mouse_scroll(0.0, -160.0).expect("seek scroll");
        }
        settle(robot, 120);
    }
    robot.find_button_bounds_exact(label).ok().flatten()
}

fn settle(robot: &cranpose::Robot, ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
    let _ = robot.wait_for_idle();
}

fn scale(shot: &cranpose::RobotScreenshot) -> (f32, f32) {
    (
        shot.width as f32 / shot.logical_width.max(1.0),
        shot.height as f32 / shot.logical_height.max(1.0),
    )
}

fn sample_rgb(shot: &cranpose::RobotScreenshot, x: f32, y: f32) -> [u8; 3] {
    let (sx, sy) = scale(shot);
    let x = (x * sx) as usize;
    let y = (y * sy) as usize;
    let index = (y * shot.width as usize + x) * 4;
    [
        shot.pixels[index],
        shot.pixels[index + 1],
        shot.pixels[index + 2],
    ]
}

fn luma(rgb: [u8; 3]) -> f32 {
    0.2126 * rgb[0] as f32 + 0.7152 * rgb[1] as f32 + 0.0722 * rgb[2] as f32
}

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    FAILED.store(true, Ordering::Relaxed);
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(15));
        std::process::exit(1);
    });
    let _ = robot.exit();
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
