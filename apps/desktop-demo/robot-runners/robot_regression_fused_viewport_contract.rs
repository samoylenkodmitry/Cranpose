//! Fused-pass viewport contract: a runtime-shader composite must not leak
//! its sub-viewport into subsequent draws of the same fused render pass.
//!
//! Regression: `draw_prepared_shader_src_over` left the pass viewport at the
//! composite's dest rect, so any text/shape drawn after a shader composite in
//! the same segment chunk (the "SDF Halo Border" header after the fire boxes)
//! was remapped into that sub-rect and vanished at most scroll offsets. Also
//! guards the scrolled-tab-switch path against ghost pixels of the previous
//! tab (a squeezed first frame leaves stale pixels uncovered).

use cranpose::AppLauncher;
use cranpose_testing::find_text_in_semantics;
use desktop_app::app::{self, TEST_ACTIVE_TAB_STATE};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::time::Duration;

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 800;

fn main() {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/culling-probe".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Culling Probe")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_robot_app_hook(set_tab_hook)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();

            // --- Case 1: ShaderRect "SDF Halo Border" visibility across scroll.
            robot
                .invoke_app_hook("set-tab", "shaderrect")
                .expect("select shaderrect tab");
            settle(&robot, 800);
            let texts = [
                "Fire Shader — Animated Flame Border",
                "SDF Halo Border",
                "Box 1",
            ];
            {
                let shot = robot.screenshot().expect("initial screenshot");
                save(&shot, &shot_dir, "initial");
                for label in texts {
                    if let Some((x, y, w, h)) = find_text_in_semantics(&robot, label) {
                        let ink = count_ink(&shot, x, y, w, h);
                        println!("initial '{label}' at ({x:.0},{y:.0}) ink={ink}");
                        let fully_visible = y > 100.0 && y + h < WINDOW_HEIGHT as f32 - 10.0;
                        if fully_visible && ink < 40 {
                            fail(
                                &robot,
                                &format!("'{label}' missing on the initial frame ({ink} ink pixels)"),
                            );
                        }
                    } else {
                        println!("initial '{label}' no semantics");
                    }
                }
            }
            for step in 0..14 {
                let delta = if step < 7 { -220.0 } else { 165.0 };
                scroll(&robot, 450.0, 400.0, delta);
                settle(&robot, 350);
                let shot = robot.screenshot().expect("screenshot");
                let mut line = format!("step={step}");
                for label in texts {
                    if let Some((x, y, w, h)) = find_text_in_semantics(&robot, label) {
                        let visible = y + h > 100.0 && y < WINDOW_HEIGHT as f32 - 10.0;
                        if visible {
                            let ink = count_ink(&shot, x, y, w, h);
                            line.push_str(&format!(" | {label}: y={y:.0} ink={ink}"));
                            if ink < 40 {
                                save(&shot, &shot_dir, &format!("missing-{step:02}"));
                                fail(
                                    &robot,
                                    &format!(
                                        "'{label}' visible in semantics at y={y:.0} but only {ink} ink pixels rendered (step {step})"
                                    ),
                                );
                            }
                        }
                    }
                }
                println!("{line}");
            }

            // --- Case 2: ghost of the scrolled Liquid page behind other tabs.
            // The liquid page hosts backdrop-glass components; switching away
            // while they are on screen must not leave any of them behind.
            // Sweep scroll offsets (glass controls in view / mid page / very
            // bottom) x target tabs (animated shaders page, static markdown).
            for (target, threshold) in [("shaders", 600usize), ("markdown", 220usize)] {
                robot
                    .invoke_app_hook("set-tab", target)
                    .unwrap_or_else(|_| panic!("select {target} tab"));
                settle(&robot, 1000);
                let clean = robot.screenshot().expect("clean screenshot");
                save(&clean, &shot_dir, &format!("{target}-clean"));
                for scroll_delta in [-520.0f32, -1400.0, -4200.0] {
                    robot
                        .invoke_app_hook("set-tab", "liquid")
                        .expect("select liquid tab");
                    settle(&robot, 900);
                    scroll(&robot, 450.0, 400.0, scroll_delta);
                    settle(&robot, 900);
                    robot
                        .invoke_app_hook("set-tab", target)
                        .unwrap_or_else(|_| panic!("select {target} tab"));
                    settle(&robot, 1200);
                    let shot = robot.screenshot().expect("screenshot");
                    let mut diff = 0usize;
                    for y in (100..(WINDOW_HEIGHT as usize - 4)).step_by(2) {
                        for x in (24..(WINDOW_WIDTH as usize - 24)).step_by(2) {
                            let i = (y * shot.width as usize + x) * 4;
                            let d = shot.pixels[i].abs_diff(clean.pixels[i]).max(
                                shot.pixels[i + 1]
                                    .abs_diff(clean.pixels[i + 1])
                                    .max(shot.pixels[i + 2].abs_diff(clean.pixels[i + 2])),
                            );
                            if d > 40 {
                                diff += 1;
                            }
                        }
                    }
                    println!("ghost-diff-pixels target={target} scroll={scroll_delta} diff={diff}");
                    if diff > threshold {
                        save(&shot, &shot_dir, &format!("{target}-ghost-{scroll_delta}"));
                        fail(
                            &robot,
                            &format!(
                                "liquid page (scrolled {scroll_delta}) ghosting through {target} tab: {diff} changed pixels"
                            ),
                        );
                    }
                    // Reset the liquid scroll for the next iteration.
                    robot
                        .invoke_app_hook("set-tab", "liquid")
                        .expect("select liquid tab");
                    settle(&robot, 500);
                    scroll(&robot, 450.0, 400.0, 5000.0);
                    settle(&robot, 500);
                }
            }

            println!("PASS: fused-pass viewport contract");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn count_ink(shot: &cranpose::RobotScreenshot, x: f32, y: f32, w: f32, h: f32) -> usize {
    // Semantic bounds are logical; the capture is physical pixels.
    let sx = shot.width as f32 / shot.logical_width.max(1.0);
    let sy = shot.height as f32 / shot.logical_height.max(1.0);
    let (x, y, w, h) = (x * sx, y * sy, w * sx, h * sy);
    let mut ink = 0usize;
    let x0 = x.max(0.0) as usize;
    let y0 = y.max(0.0) as usize;
    let x1 = ((x + w) as usize).min(shot.width as usize);
    let y1 = ((y + h) as usize).min(shot.height as usize);
    for yy in y0..y1 {
        for xx in x0..x1 {
            let i = (yy * shot.width as usize + xx) * 4;
            let (r, g, b) = (shot.pixels[i], shot.pixels[i + 1], shot.pixels[i + 2]);
            // demo shader pages are dark; headers are light text
            if r > 150 && g > 150 && b > 150 {
                ink += 1;
            }
        }
    }
    ink
}

fn set_tab_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-tab" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    let tab = match argument.as_str() {
        "shaderrect" => app::DemoTab::ShaderRect,
        "shaders" => app::DemoTab::Shaders,
        "liquid" => app::DemoTab::Liquid,
        "markdown" => app::DemoTab::MarkdownViewer,
        _ => return Err(format!("unknown demo tab '{argument}'")),
    };
    let state = TEST_ACTIVE_TAB_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| "active tab state was not installed".to_string())?;
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

fn save(shot: &cranpose::RobotScreenshot, dir: &Path, name: &str) {
    let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
        .expect("screenshot buffer");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save screenshot");
    println!("saved {}", path.display());
}
