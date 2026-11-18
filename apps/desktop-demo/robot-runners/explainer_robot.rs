// What the Explainer tab's robot runners share: waiting for frames,
// pictures, walking the article to a section or to an exact offset, finding
// words and buttons, clicks, drags, counting pixels in a part of the
// window, and copying a selection out of the app.

use std::{path::Path, time::Duration};

use cranpose::{PointerButton, Robot, RobotScreenshot};
use cranpose_ui::Modifiers;

use crate::{pointer_drag, robot_exit, robot_launch, robot_shot};

pub(crate) const WIDTH: f32 = 1200.0;
pub(crate) const HEIGHT: f32 = 800.0;
const CLIPBOARD: &str = "clipboard";

// A part of the window, in logical pixels: x, y, width and height.
pub(crate) type Area = (f32, f32, f32, f32);

// The wheel step the runners walk the article with.
const STEP: f32 = 180.0;
// The most wheel steps a runner takes to bring a section into view.
const MOST_STEPS: u32 = 30;

// The app hook the runners launch with: the tab switch and a read of the
// clipboard.
pub(crate) fn app_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name == CLIPBOARD {
        return Ok(cranpose_ui::clipboard_session::clipboard_read_text());
    }
    robot_launch::set_tab_hook(name, argument)
}

// Lets `time` pass and the app draw a few frames.
pub(crate) fn wait(robot: &Robot, time: Duration) {
    std::thread::sleep(time);
    if let Err(err) = robot.pump_frames(3) {
        robot_exit::fail(robot, &format!("pump frames: {err}"));
    }
}

// Lets every animation run out: a scene that moves only while it is
// touched settles.
pub(crate) fn settle(robot: &Robot) {
    if let Err(err) = robot.wait_for_idle() {
        robot_exit::fail(robot, &format!("wait for idle: {err}"));
    }
}

// Runs `count` frame updates, one at a time. A robot without a window
// draws no frame for them.
pub(crate) fn render_frames(robot: &Robot, count: u32) {
    for _ in 0..count {
        if let Err(err) = robot.pump_frames(1) {
            robot_exit::fail(robot, &format!("pump frames: {err}"));
        }
    }
}

pub(crate) fn shot(robot: &Robot) -> RobotScreenshot {
    match robot.screenshot() {
        Ok(shot) => shot,
        Err(err) => robot_exit::fail(robot, &format!("screenshot: {err}")),
    }
}

pub(crate) fn save(robot: &Robot, directory: &Path, name: &str) {
    robot_shot::save(&shot(robot), directory, name);
}

pub(crate) fn text_bounds(robot: &Robot, prefix: &str) -> Area {
    match robot.find_text_by_prefix(prefix) {
        Ok(Some((x, y, width, height, _))) => (x, y, width, height),
        Ok(None) => robot_exit::fail(robot, &format!("no text {prefix:?}")),
        Err(err) => robot_exit::fail(robot, &format!("text query: {err}")),
    }
}

pub(crate) fn button_bounds(robot: &Robot, label: &str) -> Area {
    match robot.find_button_bounds_exact(label) {
        Ok(Some(bounds)) => bounds,
        Ok(None) => robot_exit::fail(robot, &format!("no button {label:?}")),
        Err(err) => robot_exit::fail(robot, &format!("button query: {err}")),
    }
}

pub(crate) fn move_to(robot: &Robot, x: f32, y: f32) {
    if let Err(err) = robot.mouse_move(x, y) {
        robot_exit::fail(robot, &format!("move the mouse: {err}"));
    }
}

pub(crate) fn click(robot: &Robot, x: f32, y: f32) {
    if let Err(err) = robot.click(x, y) {
        robot_exit::fail(robot, &format!("click at ({x:.0}, {y:.0}): {err}"));
    }
}

// Presses at `from`, drags along its row to `to_x` and lets go: a selection
// of the words between, or a slider's thumb moved.
pub(crate) fn drag(robot: &Robot, (x, y): (f32, f32), to_x: f32) {
    pointer_drag::drag(robot, PointerButton::Primary, (x, y), (to_x, y));
}

// How many pixels of a fresh screenshot in `area` pass `test`; `width` is
// the window's logical width.
pub(crate) fn pixels(robot: &Robot, width: f32, area: Area, test: fn([u8; 3]) -> bool) -> usize {
    let shot = shot(robot);
    in_area(&shot, width, area)
        .filter(|&at| test([shot.pixels[at], shot.pixels[at + 1], shot.pixels[at + 2]]))
        .count()
}

// The sum of the brightness of the pixels of a fresh screenshot in `area`;
// `width` is the window's logical width.
pub(crate) fn brightness(robot: &Robot, width: f32, area: Area) -> f32 {
    let shot = shot(robot);
    in_area(&shot, width, area)
        .map(|at| {
            f32::from(shot.pixels[at])
                + f32::from(shot.pixels[at + 1])
                + f32::from(shot.pixels[at + 2])
        })
        .sum()
}

// How many pixels in `area` differ by more than a little between `a` and
// `b`.
pub(crate) fn changed_pixels(
    a: &RobotScreenshot,
    b: &RobotScreenshot,
    width: f32,
    area: Area,
) -> usize {
    in_area(a, width, area)
        .filter(|&at| {
            (0..3).any(|channel| a.pixels[at + channel].abs_diff(b.pixels[at + channel]) > 12)
        })
        .count()
}

// The byte offsets of the pixels of `shot` in `area`, given in logical
// pixels of a window `width` wide.
fn in_area(
    shot: &RobotScreenshot,
    width: f32,
    (x, y, w, h): Area,
) -> impl Iterator<Item = usize> + '_ {
    let scale = shot.width as f32 / width;
    let columns = ((x * scale).max(0.0) as u32)..(((x + w) * scale) as u32).min(shot.width);
    let rows = ((y * scale).max(0.0) as u32)..(((y + h) * scale) as u32).min(shot.height);
    rows.flat_map(move |row| {
        columns
            .clone()
            .map(move |column| ((row * shot.width + column) * 4) as usize)
    })
}

// Turns the wheel over the window of `size` until `text` stands in its
// upper part, saving a picture after each step.
pub(crate) fn scroll_to(robot: &Robot, shots: &Path, text: &str, (width, height): (f32, f32)) {
    move_to(robot, width * 0.5, height * 0.6);
    for step in 1..=MOST_STEPS {
        let found = robot.find_text_by_prefix(text).ok().flatten();
        if found
            .as_ref()
            .is_some_and(|(_, y, _, _, _)| (0.0..height * 0.45).contains(y))
        {
            return;
        }
        // A text found far below comes up in one scroll; a stage that pins
        // it holds it there, and the wheel walks on in steps.
        let ahead = found.map_or(STEP, |(_, y, _, _, _)| (y - height * 0.3).max(STEP));
        if let Err(err) = robot.mouse_scroll(0.0, -ahead) {
            robot_exit::fail(robot, &format!("scroll: {err}"));
        }
        wait(robot, Duration::from_millis(250));
        save(robot, shots, &format!("walk_{step:02}.png"));
    }
    robot_exit::fail(
        robot,
        &format!("the wheel never brought {text:?} into view"),
    );
}

// Clicks the middle of the first text that starts with `prefix`.
pub(crate) fn click_text(robot: &Robot, prefix: &str) {
    let (x, y, width, height) = text_bounds(robot, prefix);
    if let Err(err) = robot.click(x + width * 0.5, y + height * 0.5) {
        robot_exit::fail(robot, &format!("click {prefix:?}: {err}"));
    }
}

// The walk through a section of the article: the section's stage pins to
// the top of the article's view at `top` down the scroll and stays there
// for `travel` more. The section carries a test tag; a text cannot tell how
// far the page has scrolled, since its stage holds it in view.
pub(crate) struct Walk {
    tag: &'static str,
    // The height of the window.
    window: f32,
    // The height of the article's view, which the stage fills.
    page: f32,
    top: f32,
    travel: f32,
}

impl Walk {
    // Read with the article at its top, as a launch shows it. The section is
    // `pages` view heights tall.
    pub(crate) fn new(robot: &Robot, tag: &'static str, pages: f32) -> Self {
        Self::in_window(robot, tag, pages, HEIGHT)
    }

    // The same in a window `window` pixels tall.
    pub(crate) fn in_window(robot: &Robot, tag: &'static str, pages: f32, window: f32) -> Self {
        let (y, height) = section_bounds(robot, tag);
        let page = height / pages;
        Self {
            tag,
            window,
            page,
            top: y - (window - page),
            travel: page * (pages - 1.0),
        }
    }

    fn scrolled(&self, robot: &Robot) -> f32 {
        let (y, _) = section_bounds(robot, self.tag);
        self.top - (y - (self.window - self.page))
    }

    // Scrolls the article to `progress` along the walk: 0 where the stage
    // pins, 1 where it lets go.
    pub(crate) fn walk_to(&self, robot: &Robot, progress: f32) {
        let target = self.top + self.travel * progress;
        for _ in 0..4 {
            let off = target - self.scrolled(robot);
            if off.abs() < 0.5 {
                wait(robot, Duration::from_millis(300));
                return;
            }
            if let Err(err) = robot.mouse_scroll_and_wait_for_frame(0.0, -off) {
                robot_exit::fail(robot, &format!("scroll: {err}"));
            }
        }
        let scrolled = self.scrolled(robot);
        robot_exit::fail(
            robot,
            &format!("the page does not scroll to {target}: it stays at {scrolled}"),
        );
    }
}

// The top and the height of the section tagged `tag`.
fn section_bounds(robot: &Robot, tag: &str) -> (f32, f32) {
    let semantics = match robot.get_semantics() {
        Ok(semantics) => semantics,
        Err(err) => robot_exit::fail(robot, &format!("semantics: {err}")),
    };
    match Robot::find_by_tag(&semantics, tag) {
        Some(section) => (section.bounds.y, section.bounds.height),
        None => robot_exit::fail(robot, &format!("no section tagged {tag:?}")),
    }
}

// Fails unless `landmark` shows where it showed at `at`: the stage holds it
// in view while the scroll walks its section.
pub(crate) fn check_held(robot: &Robot, landmark: &str, at: f32) {
    let now = text_bounds(robot, landmark).1;
    if (now - at).abs() > 2.0 {
        robot_exit::fail(
            robot,
            &format!("{landmark:?} left its place in view: {at} -> {now}"),
        );
    }
}

// Copies the selection and returns what the clipboard then holds.
pub(crate) fn copy_selection(robot: &Robot) -> String {
    if let Err(err) = robot.send_key_with_modifiers("c", Modifiers::COMMAND) {
        robot_exit::fail(robot, &format!("copy: {err}"));
    }
    wait(robot, Duration::from_millis(200));
    match robot.invoke_app_hook(CLIPBOARD, "") {
        Ok(Some(text)) => text,
        Ok(None) => robot_exit::fail(robot, "the copy left the clipboard empty"),
        Err(err) => robot_exit::fail(robot, &format!("clipboard: {err}")),
    }
}
