// The Controls UI tab after the Room tab: a click on the Checkmark control
// flips its state, and the window draws the flip. The stage is a layer with
// nothing to draw of its own, painted by a runtime shader, so a window frame
// that draws a part of the window must count the shader's rect in that part.
// A robot screenshot draws the whole scene offscreen and never shows such a
// miss, so this test reads the damage of the frames the window presents
// after each click, which needs a visible window: a headless run presents
// nothing.
//
// The switches vary as the owner's runs did: some click the control three
// times before they leave for the Room tab, and some stay on the Room tab
// while its globe turns, long enough for the window to stop working out what
// its frames change.

use std::time::Duration;

use cranpose::Robot;
use cranpose_render_wgpu::{FrameDamage, RenderStatsSnapshot};

use crate::robot_controls_grid::card;
use crate::{robot_exit, robot_launch};

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 800;
const CARD: &str = "Checkmark";
const SWITCHES_ENV: &str = "CRANPOSE_ROBOT_SWITCHES";
const DEFAULT_SWITCHES: u32 = 12;
// The frames the Room tab draws on a short stay.
const ROOM_FRAMES: u32 = 3;
// A long stay on the Room tab, on every third switch.
const LONG_STAY: Duration = Duration::from_millis(1500);
// The clicks on the stage before the way to the Room tab, on every second
// switch.
const CLICKS_BEFORE: u32 = 3;
// The frames the controls draw before the click.
const CONTROLS_FRAMES: u32 = 1;
// The frames after the click that may draw the flip.
const FRAMES_AFTER_CLICK: u32 = 12;
// The watchdog: a fixed start plus this long for each switch.
const START_SECONDS: u64 = 60;
const SECONDS_PER_SWITCH: u64 = 4;

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Controls After Room ===");
    let switches = switches();
    robot_exit::arm_timeout(START_SECONDS + SECONDS_PER_SWITCH * u64::from(switches));

    robot_launch::launch("Robot Controls After Room", WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(move |robot| {
            robot_launch::switch_tab(&robot, "controls");
            settle(&robot);
            let (_, stage) = card(&robot, CARD);
            let centre = (stage.0 + stage.2 * 0.5, stage.1 + stage.3 * 0.5);
            let window_centre = window_pixel(&robot, centre);
            println!(
                "  {CARD} stage centre {centre:?} logical, {window_centre:?} in window pixels"
            );

            let (mut judged, mut missed, mut tracked) = (0, 0, false);
            for switch in 1..=switches {
                if switch % 2 == 0 {
                    for _ in 0..CLICKS_BEFORE {
                        click(&robot, centre);
                        present(&robot, 1);
                    }
                }
                robot_launch::switch_tab(&robot, "room");
                if switch % 3 == 0 {
                    std::thread::sleep(LONG_STAY);
                }
                present(&robot, ROOM_FRAMES);
                robot_launch::switch_tab(&robot, "controls");
                present(&robot, CONTROLS_FRAMES);
                let (before, _) = card(&robot, CARD);
                let frames = click_and_read_frames(&robot, centre, window_centre);
                settle(&robot);
                let (after, _) = card(&robot, CARD);
                if after == before {
                    robot_exit::fail(
                        &robot,
                        &format!("switch {switch}: the click left {CARD} reading '{after}'"),
                    );
                }
                tracked |= frames.tracked;
                let Some(drawn) = frames.part_holds_stage else {
                    continue;
                };
                judged += 1;
                if !drawn {
                    missed += 1;
                    println!(
                        "switch {switch}: the first part of the window drawn after the click \
                         missed the {CARD} stage centre {window_centre:?}; the reading went \
                         from '{before}' to '{after}' and the damage of the frames was:\n  {}",
                        frames.log.join("\n  ")
                    );
                }
            }

            if missed > 0 {
                robot_exit::fail(
                    &robot,
                    &format!("of {judged} clicks drawn in a part of the window, {missed} missed the {CARD} stage"),
                );
            }
            if judged == 0 {
                if tracked {
                    robot_exit::fail(
                        &robot,
                        "the window drew parts of itself, but never the first frame that changed after a click",
                    );
                }
                println!("  the window drew every frame whole, so no click drew a part of it");
            }
            println!("PASS: {judged} clicks after the Room tab drew the {CARD} stage");
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}

fn switches() -> u32 {
    let Ok(value) = std::env::var(SWITCHES_ENV) else {
        return DEFAULT_SWITCHES;
    };
    match value.parse() {
        Ok(switches) if switches > 0 => switches,
        _ => robot_exit::fail_without_shutdown(&format!(
            "{SWITCHES_ENV} is '{value}', not a count above zero"
        )),
    }
}

fn click(robot: &Robot, (x, y): (f32, f32)) {
    if let Err(err) = robot.click(x, y) {
        robot_exit::fail(robot, &format!("click: {err}"));
    }
}

fn settle(robot: &Robot) {
    if let Err(err) = robot.wait_for_idle() {
        robot_exit::fail(robot, &format!("wait for idle: {err}"));
    }
}

fn present(robot: &Robot, frames: u32) {
    for _ in 0..frames {
        if let Err(err) = robot.wait_for_present_frame() {
            robot_exit::fail(robot, &format!("wait for a presented frame: {err}"));
        }
    }
}

// The window pixel under a logical point: the damage rects count window
// pixels, and the surface and the logical viewport give the scale of each
// axis.
fn window_pixel(robot: &Robot, (x, y): (f32, f32)) -> (u32, u32) {
    let presentation = robot
        .presentation_info()
        .unwrap_or_else(|err| robot_exit::fail(robot, &format!("presentation info: {err}")));
    let viewport = robot
        .screenshot()
        .unwrap_or_else(|err| robot_exit::fail(robot, &format!("screenshot: {err}")));
    let (width, height) = presentation.surface_size;
    let scale_x = width as f32 / viewport.logical_width;
    let scale_y = height as f32 / viewport.logical_height;
    ((x * scale_x) as u32, (y * scale_y) as u32)
}

fn frame_stats(robot: &Robot) -> RenderStatsSnapshot {
    match robot.get_render_stats() {
        Ok(Some(stats)) => stats,
        Ok(None) => robot_exit::fail(robot, "the renderer reported no frame stats"),
        Err(err) => robot_exit::fail(robot, &format!("render stats: {err}")),
    }
}

// What the window drew in the frames after a click.
struct ClickFrames {
    // Whether the first frame that drew a part of the window drew the pixel
    // the click changes: the flip and the stage's animation start in the
    // frame that changes the reading. `None` when every frame that changed
    // something drew the whole window.
    part_holds_stage: Option<bool>,
    // Whether a frame reported the part of the window it drew.
    tracked: bool,
    log: Vec<String>,
}

// Clicks `at` and reads the damage of the frames after the click until one
// draws a part of the window, and says whether that part holds `stage`, a
// window pixel. A read that finds the statistics of the frame read before
// found no new frame.
fn click_and_read_frames(robot: &Robot, at: (f32, f32), stage: (u32, u32)) -> ClickFrames {
    let mut frames = ClickFrames {
        part_holds_stage: None,
        tracked: false,
        log: Vec::new(),
    };
    let mut last = frame_stats(robot);
    click(robot, at);
    for _ in 0..FRAMES_AFTER_CLICK {
        present(robot, 1);
        let stats = frame_stats(robot);
        if stats == last {
            continue;
        }
        last = stats;
        let damage = stats.damage;
        frames.log.push(describe(damage));
        if let FrameDamage::Part(rects) = damage {
            frames.tracked = true;
            if !rects.rects().is_empty() {
                frames.part_holds_stage = Some(rects.contains(stage.0, stage.1));
                break;
            }
        }
    }
    frames
}

fn describe(damage: FrameDamage) -> String {
    match damage {
        FrameDamage::Part(rects) => format!("Part {:?}", rects.rects()),
        other => format!("{other:?}"),
    }
}
