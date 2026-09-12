#![allow(dead_code)]

use std::time::Duration;

use cranpose::Robot;
use desktop_app::app::{DemoTab, TEST_ACTIVE_TAB_STATE, TEST_LIQUID_SCROLL_STATE};

const SETTLE_ROUNDS: usize = 6;
const SETTLE_FRAMES: u32 = 12;
const SETTLE_MS: u64 = 250;
const HOLD_ROUNDS: usize = 14;
const HOLD_FRAMES: u32 = 4;
const HOLD_MS: u64 = 350;

type Bounds = (f32, f32, f32, f32);

/// The liquid pages' robot hooks: select a tab by name, and drive the Liquid
/// UI page's own scroll state so a runner places content exactly instead of
/// flinging a drag at it and landing wherever the settle policy snaps.
pub(crate) fn app_hook(name: String, argument: String) -> Result<Option<String>, String> {
    match name.as_str() {
        "set-tab" => {
            let tab = match argument.as_str() {
                "liquid" => DemoTab::Liquid,
                "receipts" => DemoTab::GlassFeed,
                other => return Err(format!("unknown tab {other}")),
            };
            TEST_ACTIVE_TAB_STATE
                .with(|cell| cell.borrow().as_ref().copied())
                .ok_or_else(|| "active tab state was not installed".to_string())?
                .set(tab);
            Ok(None)
        }
        "scroll-liquid-to" => {
            let offset: f32 = argument
                .parse()
                .map_err(|err| format!("invalid liquid scroll offset '{argument}': {err}"))?;
            let scroll = TEST_LIQUID_SCROLL_STATE
                .with(|cell| *cell.borrow())
                .ok_or_else(|| "liquid scroll state was not installed".to_string())?;
            scroll.scroll_to(offset);
            Ok(Some(scroll.value_non_reactive().to_string()))
        }
        other => Err(format!("unsupported robot app hook {other}({argument})")),
    }
}

/// Opens the Liquid UI tab and waits for it to come to rest.
pub(crate) fn open(robot: &Robot) {
    robot
        .invoke_app_hook("set-tab", "liquid")
        .expect("select the liquid tab");
    settle(robot);
}

/// Opens the receipts feed and waits for it to come to rest.
pub(crate) fn open_receipts(robot: &Robot) {
    robot
        .invoke_app_hook("set-tab", "receipts")
        .expect("select the receipts tab");
    settle(robot);
}

/// Scrolls the page to an absolute offset and waits for it to come to rest.
pub(crate) fn scroll_to(robot: &Robot, offset: f32) {
    robot
        .invoke_app_hook("scroll-liquid-to", &offset.to_string())
        .expect("scroll the liquid page");
    settle(robot);
}

/// Presses at a point and holds until the press feedback has settled; the
/// caller releases with `touch_up`.
pub(crate) fn hold(robot: &Robot, x: f32, y: f32) {
    robot.touch_down(x, y).expect("touch down");
    for _ in 0..HOLD_ROUNDS {
        robot.pump_frames(HOLD_FRAMES).expect("pump frames");
    }
    std::thread::sleep(Duration::from_millis(HOLD_MS));
    robot.pump_frames(SETTLE_FRAMES).expect("pump frames");
}

/// The centre of a semantics box.
pub(crate) fn centre(bounds: Bounds) -> (f32, f32) {
    (bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
}

/// Pumps until animations and scrolling have stopped.
pub(crate) fn settle(robot: &Robot) {
    for _ in 0..SETTLE_ROUNDS {
        robot.pump_frames(SETTLE_FRAMES).expect("pump frames");
    }
    std::thread::sleep(Duration::from_millis(SETTLE_MS));
    robot.pump_frames(SETTLE_FRAMES).expect("pump frames");
}
