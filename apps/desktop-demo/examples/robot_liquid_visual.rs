//! Liquid UI visual walkthrough: opens the Liquid tab, captures the resting
//! state, scrolls to the materials lab, exercises the tab-bar blob and the
//! morphing menu, and writes numbered PNGs into `ROBOT_SHOT_DIR`.

use cranpose::AppLauncher;
use desktop_app::app::{self, TEST_ACTIVE_TAB_STATE};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::time::Duration;

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 800;

fn main() {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/liquid-shots".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Liquid Visual")
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
            settle(&robot, 1000);
            shot(&robot, &shot_dir, "01-top");

            // Toggle lens: press-and-hold the Wi-Fi thumb — the whole thumb
            // lifts into the transparent magnifying capsule (reference
            // "toggle in action"); then drag it mid-track. The controls card
            // must be scrolled into comfortable view first.
            scroll(&robot, 450.0, 500.0, -520.0);
            settle(&robot, 800);
            if let Some((_, y, _, h)) = cranpose_testing::find_text_in_semantics(&robot, "Wi-Fi") {
                // The switch hugs the card's trailing edge.
                let toggle_y = y + h * 0.5;
                let thumb_x = 838.0;
                robot.mouse_move(thumb_x, toggle_y).expect("hover thumb");
                robot.mouse_down().expect("press thumb");
                std::thread::sleep(Duration::from_millis(380));
                shot(&robot, &shot_dir, "01a-toggle-press-lens");
                for step in 1..=6 {
                    robot
                        .mouse_move(thumb_x - step as f32 * 4.0, toggle_y)
                        .expect("drag thumb");
                    std::thread::sleep(Duration::from_millis(30));
                }
                // Hold until the lens fully materializes.
                std::thread::sleep(Duration::from_millis(420));
                shot(&robot, &shot_dir, "01b-toggle-drag-lens");
                robot.mouse_up().expect("release thumb");
                // The lens lingers through the settle flight.
                std::thread::sleep(Duration::from_millis(220));
                shot(&robot, &shot_dir, "01b2-toggle-settle-lens");
                settle(&robot, 900);
                shot(&robot, &shot_dir, "01b3-toggle-settled");
            }

            // Tab-bar drag lens: press Discover and slide toward Home — the
            // selection lens bubble rides the finger.
            {
                let discover = cranpose_testing::find_button_exact_in_semantics(&robot, "Discover")
                    .expect("discover tab in semantics");
                let start_x = discover.0 + discover.2 * 0.5;
                let bar_y = discover.1 + discover.3 * 0.5;
                robot.mouse_move(start_x, bar_y).expect("hover tab");
                robot.mouse_down().expect("press tab");
                std::thread::sleep(Duration::from_millis(80));
                for step in 1..=8 {
                    robot
                        .mouse_move(start_x + step as f32 * 22.0, bar_y)
                        .expect("drag tab lens");
                    std::thread::sleep(Duration::from_millis(30));
                }
                std::thread::sleep(Duration::from_millis(420));
                shot(&robot, &shot_dir, "01c-tabbar-drag-lens");
                // Keep dragging to the bar's end: the lens necks with the
                // search circle through the liquid field.
                for step in 1..=12 {
                    robot
                        .mouse_move(start_x + 176.0 + step as f32 * 22.0, bar_y)
                        .expect("drag toward search");
                    std::thread::sleep(Duration::from_millis(30));
                }
                std::thread::sleep(Duration::from_millis(380));
                shot(&robot, &shot_dir, "01c2-lens-glues-search");
                robot.mouse_up().expect("release tab");
                settle(&robot, 700);
                // Return to the first tab for the rest of the walkthrough.
                robot.click(start_x, bar_y).expect("back to discover");
                settle(&robot, 700);
            }
            scroll(&robot, 450.0, 500.0, 1200.0);
            settle(&robot, 700);
            shot(&robot, &shot_dir, "01d-after-interactions");

            // Scroll to the materials lab (glass tile over the rainbow strip),
            // past the WWDC sessions list.
            scroll(&robot, 450.0, 500.0, -1400.0);
            settle(&robot, 900);
            shot(&robot, &shot_dir, "02-list-and-lab");

            scroll(&robot, 450.0, 500.0, -850.0);
            settle(&robot, 900);
            shot(&robot, &shot_dir, "03-materials-lab");

            // Tab-bar blob: hop from Discover to Settings mid-flight capture.
            // The tab cell is resolved from semantics — the pill stretches
            // tabs when width allows, so fixed coordinates would drift.
            let settings = cranpose_testing::find_button_exact_in_semantics(&robot, "Settings")
                .expect("settings tab in semantics");
            robot
                .click(settings.0 + settings.2 * 0.5, settings.1 + settings.3 * 0.5)
                .expect("tap settings tab");
            std::thread::sleep(Duration::from_millis(120));
            shot(&robot, &shot_dir, "04-blob-mid-flight");
            settle(&robot, 900);
            shot(&robot, &shot_dir, "05-blob-settled");

            // Morphing menu from the nav trailing button: the droplet inflates
            // out of the "…" circle (mid-flight frames), settles with a size
            // overshoot, then deflates back into the button on dismiss. Park
            // the colorful gradient stripe under the menu region first so the
            // glass transparency reads against real content.
            scroll(&robot, 450.0, 400.0, 4200.0);
            settle(&robot, 900);
            scroll(&robot, 450.0, 400.0, -260.0);
            settle(&robot, 700);
            robot.click(858.0, 122.0).expect("open menu");
            std::thread::sleep(Duration::from_millis(50));
            shot(&robot, &shot_dir, "06a-menu-morph-early");
            std::thread::sleep(Duration::from_millis(70));
            shot(&robot, &shot_dir, "06b-menu-morph-mid");
            std::thread::sleep(Duration::from_millis(90));
            shot(&robot, &shot_dir, "06c-menu-morph-late");
            settle(&robot, 700);
            shot(&robot, &shot_dir, "07-menu-open");
            // Dismiss: tap outside — the droplet sucks back into the anchor.
            robot.click(200.0, 400.0).expect("dismiss menu");
            std::thread::sleep(Duration::from_millis(60));
            shot(&robot, &shot_dir, "08a-menu-close-mid");
            settle(&robot, 600);
            shot(&robot, &shot_dir, "08b-menu-closed");

            println!(
                "PASS: liquid visual walkthrough written to {}",
                shot_dir.display()
            );
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn set_tab_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-tab" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    if argument != "liquid" {
        return Err(format!("unknown demo tab '{argument}'"));
    }
    let tab = app::DemoTab::Liquid;
    let state = TEST_ACTIVE_TAB_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| "active tab state was not installed before selecting a tab".to_string())?;
    state.set(tab);
    Ok(None)
}

fn settle(robot: &cranpose::Robot, ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
    let _ = robot.wait_for_idle();
}

fn scroll(robot: &cranpose::Robot, x: f32, y: f32, delta_y: f32) {
    robot.mouse_move(x, y).expect("move cursor");
    robot.mouse_scroll(0.0, delta_y).expect("scroll");
}

fn shot(robot: &cranpose::Robot, dir: &Path, name: &str) {
    let screenshot = robot.screenshot().expect("screenshot");
    let image = RgbaImage::from_raw(screenshot.width, screenshot.height, screenshot.pixels)
        .expect("screenshot buffer size");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save screenshot");
    println!("captured {name} -> {}", path.display());
}
