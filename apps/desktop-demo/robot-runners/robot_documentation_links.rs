// The guide's links in the Documentation tab: a link to another site opens
// that site's address, and a link to a heading of the guide takes the reader
// to that heading.

use std::{cell::RefCell, rc::Rc};

use cranpose::{AppLauncher, Robot};
use cranpose_services::{set_platform_uri_handler, UriHandler, UriHandlerError};
use cranpose_testing::{
    find_button_exact_in_semantics, find_text_in_semantics, find_text_in_semantics_exact,
};
use desktop_app::app;

use crate::{robot_exit, robot_launch};

const WIDTH: u32 = 1100;
const HEIGHT: u32 = 820;
const OPENED: &str = "opened_links";

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

// Clicks along the first line of the paragraph at `bounds` until the app
// opens a link, and returns what it opened.
fn click_first_link(robot: &Robot, bounds: (f32, f32, f32, f32)) -> Option<String> {
    let (x, y, width, _) = bounds;
    let before = opened(robot).len();
    let mut along = 4.0;
    while along < width {
        robot.click(x + along, y + 8.0).ok();
        robot.wait_for_idle().ok();
        if let Some(link) = opened(robot).get(before) {
            return Some(link.clone());
        }
        along += 8.0;
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
            // The second line of the paragraph starts with the link.
            let (x, y, _, _) = scroll_to(&robot, "An iOS build needs macOS");
            let before = opened(&robot).len();
            robot.click(x + 100.0, y + 35.0).ok();
            robot.pump_frames(60).ok();
            robot.wait_for_idle().ok();
            if opened(&robot).len() != before {
                robot_exit::fail(&robot, "a link to a heading of the guide left the app");
            }
            match find_text_in_semantics_exact(&robot, "Build Apple apps without a Mac") {
                Some((_, top, _, _)) if (0.0..HEIGHT as f32 * 0.4).contains(&top) => {
                    println!("  build on GitHub moved the reader to its heading at {top}");
                    if let Ok(shot) = robot.screenshot() {
                        crate::robot_shot::save_checked(
                            &crate::output_paths::diagnostic_path("doclinks-heading.png"),
                            &shot,
                        )
                        .ok();
                    }
                }
                other => robot_exit::fail(
                    &robot,
                    &format!("build on GitHub left the heading at {other:?}"),
                ),
            }
            println!("PASS: documentation links");
            robot.exit().ok();
        })
        .run(app::DesktopApp);
}
