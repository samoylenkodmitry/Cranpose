use crate::{robot_exit, robot_shot};

use std::{process::ExitCode, sync::atomic::AtomicBool, time::Duration};

use cranpose::{
    liquid::prelude::*,
    rememberMutableStateOf,
    widgets::{Box as CBox, BoxSpec},
    AppLauncher, Color, Modifier, Size,
};

const WINDOW_WIDTH: u32 = 520;
const WINDOW_HEIGHT: u32 = 520;
const TRIGGER_LABEL: &str = "Options";
const ACCORDION_HEADER: &str = "Sort by";
const UNFOLDED_ROW: &str = "Newest to oldest";
const PLAIN_ROW: &str = "Mark all read";
const SETTLE_MS: u64 = 800;

static FAILED: AtomicBool = AtomicBool::new(false);

pub(crate) fn main() -> ExitCode {
    let _ = env_logger::try_init();

    AppLauncher::new()
        .with_title("Liquid Dropdown Accordion")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(180);
            std::thread::sleep(Duration::from_millis(700));
            settle(&robot, SETTLE_MS);

            if visible(&robot, ACCORDION_HEADER) {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    "the dropdown was already open before anything was tapped",
                );
            }

            let trigger = robot
                .find_text_bounds(TRIGGER_LABEL)
                .expect("query trigger")
                .expect("trigger bounds");
            tap(&robot, TRIGGER_LABEL, "open the dropdown");
            settle(&robot, SETTLE_MS);
            verify_hidden_trigger(&robot, trigger);
            if !visible(&robot, ACCORDION_HEADER) {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    "tapping the trigger did not open the dropdown",
                );
            }
            if visible(&robot, UNFOLDED_ROW) {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    "the accordion was unfolded before its header was tapped",
                );
            }

            tap(&robot, ACCORDION_HEADER, "tap the accordion header");
            settle(&robot, SETTLE_MS);

            if !visible(&robot, ACCORDION_HEADER) {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    "tapping a `keeps_open` row dismissed the dropdown. The row asked the \
                     menu to stay open and the menu closed anyway, so an accordion inside a \
                     dropdown can never unfold.",
                );
            }
            if !visible(&robot, UNFOLDED_ROW) {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    &format!(
                        "the dropdown stayed open but never unfolded: `{UNFOLDED_ROW}` is not \
                         on screen after tapping `{ACCORDION_HEADER}`."
                    ),
                );
            }
            println!("a `keeps_open` row kept its menu open and unfolded its section");

            tap(&robot, PLAIN_ROW, "tap an ordinary row");
            settle(&robot, SETTLE_MS);
            if visible(&robot, ACCORDION_HEADER) {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    "tapping an ordinary row left the dropdown open. A row that does not ask \
                     to stay open has to dismiss.",
                );
            }
            println!("an ordinary row dismissed the dropdown");

            tap(&robot, TRIGGER_LABEL, "reopen the dropdown");
            settle(&robot, SETTLE_MS);
            if !visible(&robot, ACCORDION_HEADER) {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    "the dropdown would not reopen after being dismissed",
                );
            }

            println!("PASS: a dropdown row dismisses its menu exactly when it asks to");
            robot.exit().expect("exit");
        })
        .try_run(move || {
            LiquidTheme(LiquidThemeSpec {
                scheme: SchemeMode::Dark,
                ..Default::default()
            }, || {
                CBox(
                    Modifier::empty()
                        .size(Size {
                            width: WINDOW_WIDTH as f32,
                            height: WINDOW_HEIGHT as f32,
                        })
                        .background(Color::BLACK),
                    BoxSpec::default(),
                    move || {
                        let expanded = rememberMutableStateOf(|| false);
                        let unfolded = rememberMutableStateOf(|| false);
                        LiquidDropdownMenu(
                            Modifier::empty().absolute_offset(218.0, 350.0),
                            expanded.get(),
                            LiquidDropdownMenuSpec::default().menu(LiquidMenuSpec::new(260.0)),
                            move || expanded.set(false),
                            move || {
                                GlassButton(
                                    Modifier::empty(),
                                    GlassButtonSpec::default(),
                                    move || expanded.set(true),
                                    || {
                                        GlassButtonLabel(TRIGGER_LABEL.into(), GlassButtonSpec::glass());
                                    },
                                );
                            },
                            move |scope| {
                                scope.item(LiquidMenuItem::new(PLAIN_ROW), || {});
                                scope.item(
                                    LiquidMenuItem::new(ACCORDION_HEADER).keeps_open().section_start(),
                                    move || unfolded.set(!unfolded.get()),
                                );
                                if unfolded.get() {
                                    scope.item(LiquidMenuItem::new(UNFOLDED_ROW), || {});
                                }
                            },
                        );
                    },
                );
            });
        })
        .expect("launch dropdown accordion runner");

    robot_exit::exit_code(&FAILED)
}

fn verify_hidden_trigger(robot: &cranpose::Robot, trigger: (f32, f32, f32, f32)) {
    let shot = robot.screenshot().expect("capture open dropdown");
    let scale = shot.width as f32 / shot.logical_width;
    let center = (trigger.0 + trigger.2 * 0.5, trigger.1 + trigger.3 * 0.5);
    let mut blue_excess = 0;
    for y in ((center.1 - 6.0) * scale) as u32..((center.1 + 6.0) * scale) as u32 {
        for x in ((center.0 - 10.0) * scale) as u32..((center.0 + 10.0) * scale) as u32 {
            let pixel = &shot.pixels[((y * shot.width + x) * 4) as usize..];
            blue_excess = blue_excess.max(pixel[2].saturating_sub(pixel[0].max(pixel[1])));
        }
    }
    println!("open menu trigger-region blue excess: {blue_excess}/255");
    if blue_excess > 2 {
        robot_shot::save(
            &shot,
            std::path::Path::new("target"),
            "liquid-menu-trigger-leak.png",
        );
        robot_exit::fail_and_await_shutdown(
            robot,
            &FAILED,
            "the absorbed trigger must not color the settled menu backdrop",
        );
    }
}

fn visible(robot: &cranpose::Robot, text: &str) -> bool {
    robot
        .find_text_bounds(text)
        .expect("query the screen for text")
        .is_some()
}

fn tap(robot: &cranpose::Robot, text: &str, what: &str) {
    let bounds = robot
        .find_text_bounds(text)
        .expect("query the screen for text")
        .unwrap_or_else(|| {
            robot_exit::fail_and_await_shutdown(
                robot,
                &FAILED,
                &format!("cannot {what}: `{text}` is not on screen"),
            )
        });
    robot
        .click(bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
        .unwrap_or_else(|error| {
            robot_exit::fail_and_await_shutdown(robot, &FAILED, &format!("cannot {what}: {error}"))
        });
}

fn settle(robot: &cranpose::Robot, millis: u64) {
    let _ = robot.wait_for_idle();
    std::thread::sleep(Duration::from_millis(millis));
    let _ = robot.wait_for_idle();
}
