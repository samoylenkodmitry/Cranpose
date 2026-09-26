use crate::{robot_exit, robot_shot};

use std::{path::PathBuf, process::ExitCode, sync::atomic::AtomicBool, time::Duration};

use cranpose::{
    widgets::{
        BasicTextField, Box as CBox, BoxSpec, Column, ColumnSpec, DisableSelection,
        SelectionContainer, Text,
    },
    AppLauncher, Color, Modifier, Robot, RobotScreenshot,
};
use cranpose_foundation::text::TextFieldState;
use cranpose_testing::{find_in_semantics, find_text};
use cranpose_ui::text::{TextStyle, TextUnit};

const FIRST: &str = "Selectable first line";
const SECOND: &str = "alpha beta gamma";
const DISABLED: &str = "Left out of the selection";
const FIELD_HINT: &str = "paste target";

static FAILED: AtomicBool = AtomicBool::new(false);

fn style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.color = Some(Color(0.08, 0.08, 0.1, 1.0));
    style.span_style.font_size = TextUnit::Sp(22.0);
    style
}

/// Where `text` is drawn, as its semantics report it.
fn bounds_of(robot: &Robot, text: &str) -> (f32, f32, f32, f32) {
    find_in_semantics(robot, |element| find_text(element, text))
        .unwrap_or_else(|| robot_exit::fail(robot, &format!("no text \"{text}\" on screen")))
}

/// The padding every text in the fixture sits in.
const PADDING: f32 = 8.0;

/// Where `text`'s glyphs are, inside its padding.
fn content_of(robot: &Robot, text: &str) -> (f32, f32, f32, f32) {
    let (x, y, width, height) = bounds_of(robot, text);
    (
        x + PADDING,
        y + PADDING,
        width - 2.0 * PADDING,
        height - 2.0 * PADDING,
    )
}

fn row(bounds: (f32, f32, f32, f32)) -> f32 {
    bounds.1 + bounds.3 * 0.5
}

/// Pixels of the selection's highlight blue over the white page inside
/// `bounds`.
fn highlighted(shot: &RobotScreenshot, bounds: (f32, f32, f32, f32)) -> usize {
    let scale = shot.width as f32 / shot.logical_width;
    let (x, y, width, height) = bounds;
    let mut count = 0;
    for py in (y * scale) as u32..((y + height) * scale) as u32 {
        for px in (x * scale) as u32..((x + width) * scale) as u32 {
            let at = ((py * shot.width + px) * 4) as usize;
            let (r, g, b) = (shot.pixels[at], shot.pixels[at + 1], shot.pixels[at + 2]);
            count += usize::from(b > 240 && r < 200 && (190..235).contains(&g));
        }
    }
    count
}

fn check(robot: &Robot, passed: bool, message: &str) {
    if passed {
        println!("  PASS: {message}");
    } else {
        robot_exit::fail_and_await_shutdown(robot, &FAILED, message);
    }
}

fn command_key(robot: &Robot, key: &str) {
    let macos = cfg!(target_os = "macos");
    robot
        .send_key_with_modifiers(key, false, !macos, false, macos)
        .expect("send a command shortcut");
    let _ = robot.wait_for_idle();
}

/// Pastes the clipboard into the field at `field` and says whether the
/// field then holds `expected`.
fn pastes(robot: &Robot, field: (f32, f32, f32, f32), expected: &str) -> bool {
    robot
        .click(field.0 + field.2 * 0.5, field.1 + field.3 * 0.5)
        .expect("focus the field");
    let _ = robot.wait_for_idle();
    command_key(robot, "a");
    command_key(robot, "v");
    std::thread::sleep(Duration::from_millis(200));
    find_in_semantics(robot, |element| find_text(element, expected)).is_some()
}

pub(crate) fn main() -> ExitCode {
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR")
            .unwrap_or_else(|_| "target/selection-container".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Selection container")
        .with_size(640, 420)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(90);
            std::thread::sleep(Duration::from_millis(500));
            robot_shot::settle(&robot, 300);

            let field = bounds_of(&robot, FIELD_HINT);

            println!("--- a mouse drag selects across two texts ---");
            let first = content_of(&robot, FIRST);
            let second = content_of(&robot, SECOND);
            robot.mouse_move(first.0 + 1.0, row(first)).expect("move to the first text");
            robot.mouse_down().expect("press");
            for step in 1..=8 {
                let t = step as f32 / 8.0;
                let x = first.0 + 1.0 + (second.0 + second.2 - 1.0 - first.0 - 1.0) * t;
                let y = row(first) + (row(second) - row(first)) * t;
                robot.mouse_move(x, y).expect("drag");
            }
            robot.mouse_up().expect("release");
            robot_shot::settle(&robot, 100);
            let shot = robot.screenshot().expect("selection shot");
            robot_shot::save(&shot, &shot_dir, "1-drag.png");
            let (in_first, in_second) = (highlighted(&shot, first), highlighted(&shot, second));
            println!("  highlight pixels: first={in_first} second={in_second}");
            check(
                &robot,
                in_first > 200 && in_second > 200,
                "both texts are highlighted",
            );
            check(
                &robot,
                highlighted(&shot, content_of(&robot, DISABLED)) == 0,
                "the text in DisableSelection stays unhighlighted",
            );
            command_key(&robot, "c");
            check(
                &robot,
                pastes(&robot, field, &format!("{FIRST}\n{SECOND}")),
                "the copy holds both texts, one line each",
            );

            println!("--- a double click selects a word ---");
            let second = content_of(&robot, SECOND);
            let (x, y) = (second.0 + second.2 * 0.5, row(second));
            robot.click(x, y).expect("first click");
            robot.click(x, y).expect("second click");
            robot_shot::settle(&robot, 100);
            let shot = robot.screenshot().expect("word shot");
            robot_shot::save(&shot, &shot_dir, "2-word.png");
            command_key(&robot, "c");
            check(&robot, pastes(&robot, field, "beta"), "the double click selected the word");

            println!("--- a finger resting on a word selects it and offers Copy ---");
            let first = content_of(&robot, FIRST);
            let (x, y) = (first.0 + first.2 * 0.95, row(first));
            robot.touch_down(x, y).expect("touch");
            robot_shot::settle(&robot, 800);
            robot.touch_up(x, y).expect("lift");
            robot_shot::settle(&robot, 300);
            let shot = robot.screenshot().expect("long press shot");
            robot_shot::save(&shot, &shot_dir, "3-long-press.png");
            let copy = bounds_of(&robot, "Copy");
            robot
                .click(copy.0 + copy.2 * 0.5, copy.1 + copy.3 * 0.5)
                .expect("tap Copy");
            let _ = robot.wait_for_idle();
            check(&robot, pastes(&robot, field, "line"), "the menu copied the word under the finger");

            println!("PASS: text in a SelectionContainer selects and copies");
            robot.exit().expect("exit");
        })
        .try_run(|| {
            CBox(
                Modifier::empty().fill_max_size().background(Color::WHITE),
                BoxSpec::default(),
                || {
                    Column(
                        Modifier::empty().padding(24.0),
                        ColumnSpec::default(),
                        || {
                            SelectionContainer(Modifier::empty(), || {
                                Column(Modifier::empty(), ColumnSpec::default(), || {
                                    Text(FIRST, Modifier::empty().padding(PADDING), style());
                                    Text(SECOND, Modifier::empty().padding(PADDING), style());
                                    DisableSelection(|| {
                                        Text(DISABLED, Modifier::empty().padding(PADDING), style());
                                    });
                                });
                            });
                            let field = cranpose_core::remember(|| TextFieldState::new(FIELD_HINT))
                                .with(TextFieldState::clone);
                            BasicTextField(
                                field,
                                Modifier::empty().padding(PADDING).fill_max_width(),
                                style(),
                            );
                        },
                    );
                },
            );
        })
        .map_or(ExitCode::FAILURE, |()| robot_exit::exit_code(&FAILED))
}
