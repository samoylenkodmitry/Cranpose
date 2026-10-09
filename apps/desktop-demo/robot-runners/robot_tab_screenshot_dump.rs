use crate::{output_paths, robot_launch};

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use cranpose::AppLauncher;
use desktop_app::app::{DemoTab, DEMO_TABS};
use image::RgbaImage;

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 800;
const WINDOW_TITLE: &str = "Robot Tab Screenshot Dump";
const SHOT_DIR_ENV: &str = "ROBOT_SHOT_DIR";
const SETTLE_MS_ENV: &str = "ROBOT_SHOT_SETTLE_MS";

const NONDETERMINISTIC_TABS: [DemoTab; 7] = [
    DemoTab::HackerNews,
    DemoTab::WebFetch,
    DemoTab::Xkcd,
    DemoTab::Animations,
    DemoTab::InteractiveAnim,
    DemoTab::ShaderRect,
    DemoTab::Wear,
];

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Tab Screenshot Dump ===");

    let shot_dir = shot_dir();
    std::fs::create_dir_all(&shot_dir)
        .unwrap_or_else(|err| panic!("failed to create {}: {err}", shot_dir.display()));

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(headless())
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(700));

            for tab in DEMO_TABS {
                dump_tab(&robot, tab, &shot_dir);
            }

            println!(
                "PASS: dumped {} tab screenshots to {}",
                DEMO_TABS.len(),
                shot_dir.display()
            );
            robot.exit().expect("exit");
        })
        .run(robot_launch::counter_demo);
}

fn dump_tab(robot: &cranpose::Robot, tab: DemoTab, shot_dir: &Path) {
    let slug = tab.slug();
    robot_launch::switch_tab(robot, slug);

    robot
        .pump_frames(3)
        .unwrap_or_else(|err| panic!("failed to settle tab '{slug}': {err}"));

    if NONDETERMINISTIC_TABS.contains(&tab) {
        std::thread::sleep(Duration::from_millis(settle_ms()));
        robot
            .pump_frames(3)
            .unwrap_or_else(|err| panic!("failed to sample tab '{slug}': {err}"));
    }

    let screenshot = robot
        .screenshot()
        .unwrap_or_else(|err| panic!("screenshot failed for tab '{slug}': {err}"));
    let image = RgbaImage::from_raw(screenshot.width, screenshot.height, screenshot.pixels)
        .unwrap_or_else(|| panic!("screenshot buffer for tab '{slug}' had an unexpected size"));
    let path = shot_dir.join(format!("{slug}.png"));
    image
        .save(&path)
        .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));
    println!("captured {slug} -> {}", path.display());
}

fn shot_dir() -> PathBuf {
    std::env::var_os(SHOT_DIR_ENV).map_or_else(
        || output_paths::diagnostic_path("cranpose-robot-shots"),
        PathBuf::from,
    )
}

fn headless() -> bool {
    env_bool("CRANPOSE_HEADLESS", true)
}

fn settle_ms() -> u64 {
    std::env::var(SETTLE_MS_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(250)
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key).ok().map_or(default, |value| {
        matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES")
    })
}
