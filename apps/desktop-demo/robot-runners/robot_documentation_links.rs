// The guide's links in the Documentation tab. A link to another site opens
// that site's address. A link to a heading of the guide opens the guide at
// that heading. That link sits on a wrapped line of its paragraph.

use std::{cell::RefCell, rc::Rc};

use cranpose::{AppLauncher, Robot};
use cranpose_services::{set_platform_uri_handler, UriHandler, UriHandlerError};
use cranpose_testing::{find_button_exact_in_semantics, find_text_in_semantics};
use desktop_app::app;

use crate::{robot_exit, robot_launch};

const WIDTH: u32 = 1100;
const HEIGHT: u32 = 820;
const OPENED: &str = "opened_links";
const HEADING: &str =
    "https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/guide.md#build-apple-apps-without-a-mac";

thread_local! {
    static OPENED_LINKS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

// Notes the addresses the app opens in place of a browser.
struct Recorder;

impl UriHandler for Recorder {
    fn open_uri(&self, uri: &str) -> Result<(), UriHandlerError> {
        OPENED_LINKS.with(|opened| opened.borrow_mut().push(uri.to_owned()));
        Ok(())
    }
}

fn hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name == OPENED {
        set_platform_uri_handler(Rc::new(Recorder));
        return Ok(Some(OPENED_LINKS.with(|opened| opened.borrow().join("\n"))));
    }
    robot_launch::set_tab_hook(name, argument)
}

fn opened(robot: &Robot) -> Vec<String> {
    match robot.invoke_app_hook(OPENED, "") {
        Ok(links) => links
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect(),
        Err(err) => robot_exit::fail(robot, &format!("opened links: {err}")),
    }
}

fn click_button(robot: &Robot, label: &str) {
    let Some((x, y, width, height)) = find_button_exact_in_semantics(robot, label) else {
        robot_exit::fail(robot, &format!("no button {label:?}"));
    };
    robot.click(x + width * 0.5, y + height * 0.5).ok();
    robot.wait_for_idle().ok();
}

// Scrolls the reader until a paragraph holding `text` shows, and returns its
// bounds.
fn scroll_to(robot: &Robot, text: &str) -> (f32, f32, f32, f32) {
    robot.move_to(WIDTH as f32 * 0.9, HEIGHT as f32 * 0.6).ok();
    for _ in 0..200 {
        if let Some(bounds @ (_, y, _, height)) = find_text_in_semantics(robot, text) {
            if y >= 100.0 && y + height <= HEIGHT as f32 - 100.0 {
                return bounds;
            }
        }
        robot.mouse_scroll_and_wait_for_frame(0.0, -120.0).ok();
    }
    robot_exit::fail(robot, &format!("the reader never showed {text:?}"));
}

// Clicks across the paragraph at `bounds`, line by line, until the app opens
// a link, and returns what it opened. The sweep finds a link wherever the
// paragraph wraps it.
fn click_first_link(robot: &Robot, bounds: (f32, f32, f32, f32)) -> Option<String> {
    let (x, y, width, height) = bounds;
    let before = opened(robot).len();
    let mut down = 8.0;
    while down < height {
        let mut along = 4.0;
        while along < width {
            robot.click(x + along, y + down).ok();
            robot.wait_for_idle().ok();
            if let Some(link) = opened(robot).get(before) {
                return Some(link.clone());
            }
            along += 8.0;
        }
        down += 12.0;
    }
    None
}

pub(crate) fn main() {
    println!("=== Robot Documentation Links ===");
    robot_exit::arm_timeout(120);
    AppLauncher::new()
        .with_title("Cranpose guide")
        .with_size(WIDTH, HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_robot_app_hook(hook)
        .with_test_driver(move |robot| {
            robot.wait_for_idle().ok();
            opened(&robot);
            click_button(&robot, "Get started");
            let paragraph = scroll_to(&robot, "Install Rust with rustup");
            match click_first_link(&robot, paragraph) {
                Some(link) if link == "https://rustup.rs/" => println!("  opened {link}"),
                Some(link) => robot_exit::fail(&robot, &format!("rustup opened {link}")),
                None => robot_exit::fail(&robot, "a click on rustup opened nothing"),
            }
            let paragraph = scroll_to(&robot, "An iOS build needs macOS");
            match click_first_link(&robot, paragraph) {
                Some(link) if link == HEADING => println!("  build on GitHub opened {link}"),
                other => robot_exit::fail(&robot, &format!("build on GitHub opened {other:?}")),
            }
            println!("PASS: documentation links");
            robot.exit().ok();
        })
        .run(app::DesktopApp);
}
