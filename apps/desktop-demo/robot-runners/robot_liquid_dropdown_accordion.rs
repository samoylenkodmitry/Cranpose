//! A dropdown row that keeps its menu open must keep its menu open.
//!
//! `LiquidMenuItem::keeps_open()` marks an accordion header: tapping it runs
//! its action and leaves the menu standing so the caller can swap the rows
//! under it and let the surface morph to the new size. `LiquidMenu` honours
//! that. `LiquidDropdownMenu` did not — it dismissed from its own `on_item`
//! closure, unconditionally, on top of the dismissal `LiquidMenu` already
//! performs for a row that is not `keeps_open`. Every accordion inside a
//! dropdown therefore closed on the tap that was supposed to unfold it, and
//! the second dismissal was invisible to anyone reading either function alone.
//!
//! This drives it the way a person does: open the dropdown, tap the accordion
//! header, and require that the menu is still on screen with the rows the
//! header just unfolded.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_liquid_dropdown_accordion --features desktop,robot-app
//! ```

use cranpose::liquid::prelude::*;
use cranpose::text::TextStyle;
use cranpose::widgets::{Box as CBox, BoxSpec, Text};
use cranpose::{rememberMutableStateOf, AppLauncher, Color, Modifier, Size};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const WINDOW_WIDTH: u32 = 520;
const WINDOW_HEIGHT: u32 = 520;
const TRIGGER_LABEL: &str = "Options";
const ACCORDION_HEADER: &str = "Sort by";
/// A row the accordion header unfolds. Absent until the header is tapped, and
/// absent again if the menu wrongly dismissed on that tap.
const UNFOLDED_ROW: &str = "Newest to oldest";
/// A plain row: tapping it *should* dismiss, which is the other half of the
/// contract and the reason the fix is not "never dismiss".
const PLAIN_ROW: &str = "Mark all read";
/// The menu's open/close animation settles well inside this.
const SETTLE_MS: u64 = 800;

static FAILED: AtomicBool = AtomicBool::new(false);

fn main() -> ExitCode {
    let _ = env_logger::try_init();

    AppLauncher::new()
        .with_title("Liquid Dropdown Accordion")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            const TEST_TIMEOUT_SECS: u64 = 180;
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(TEST_TIMEOUT_SECS));
                println!("\n✗ Test timed out after {TEST_TIMEOUT_SECS} seconds");
                std::process::exit(1);
            });
            std::thread::sleep(Duration::from_millis(700));
            settle(&robot, SETTLE_MS);

            if visible(&robot, ACCORDION_HEADER) {
                fail(
                    &robot,
                    "the dropdown was already open before anything was tapped",
                );
            }

            tap(&robot, TRIGGER_LABEL, "open the dropdown");
            settle(&robot, SETTLE_MS);
            if !visible(&robot, ACCORDION_HEADER) {
                fail(&robot, "tapping the trigger did not open the dropdown");
            }
            if visible(&robot, UNFOLDED_ROW) {
                fail(
                    &robot,
                    "the accordion was unfolded before its header was tapped",
                );
            }

            // The tap under test. A `keeps_open` row runs its action and
            // leaves the menu standing.
            tap(&robot, ACCORDION_HEADER, "tap the accordion header");
            settle(&robot, SETTLE_MS);

            if !visible(&robot, ACCORDION_HEADER) {
                fail(
                    &robot,
                    "tapping a `keeps_open` row dismissed the dropdown. The row asked the \
                     menu to stay open and the menu closed anyway, so an accordion inside a \
                     dropdown can never unfold.",
                );
            }
            if !visible(&robot, UNFOLDED_ROW) {
                fail(
                    &robot,
                    &format!(
                        "the dropdown stayed open but never unfolded: `{UNFOLDED_ROW}` is not \
                         on screen after tapping `{ACCORDION_HEADER}`."
                    ),
                );
            }
            println!("a `keeps_open` row kept its menu open and unfolded its section");

            // The other half: an ordinary row still dismisses, exactly once.
            tap(&robot, PLAIN_ROW, "tap an ordinary row");
            settle(&robot, SETTLE_MS);
            if visible(&robot, ACCORDION_HEADER) {
                fail(
                    &robot,
                    "tapping an ordinary row left the dropdown open. A row that does not ask \
                     to stay open has to dismiss.",
                );
            }
            println!("an ordinary row dismissed the dropdown");

            // And it reopens: a dismissal that ran twice can leave a menu
            // unable to open again.
            tap(&robot, TRIGGER_LABEL, "reopen the dropdown");
            settle(&robot, SETTLE_MS);
            if !visible(&robot, ACCORDION_HEADER) {
                fail(
                    &robot,
                    "the dropdown would not reopen after being dismissed",
                );
            }

            println!("PASS: a dropdown row dismisses its menu exactly when it asks to");
            robot.exit().expect("exit");
        })
        .try_run(move || {
            LiquidTheme(LiquidThemeSpec::default(), || {
                CBox(
                    Modifier::empty()
                        .size(Size {
                            width: WINDOW_WIDTH as f32,
                            height: WINDOW_HEIGHT as f32,
                        })
                        .background(Color(0.14, 0.15, 0.19, 1.0)),
                    BoxSpec::default(),
                    move || {
                        let expanded = rememberMutableStateOf(|| false);
                        let unfolded = rememberMutableStateOf(|| false);
                        LiquidDropdownMenu(
                            Modifier::empty().absolute_offset(40.0, 60.0),
                            expanded.get(),
                            LiquidDropdownMenuSpec::default().menu(LiquidMenuSpec::new(260.0)),
                            move || expanded.set(false),
                            move || {
                                GlassButton(
                                    Modifier::empty(),
                                    GlassButtonSpec::default(),
                                    move || expanded.set(true),
                                    || {
                                        Text(
                                            TRIGGER_LABEL,
                                            Modifier::empty(),
                                            TextStyle::default(),
                                        );
                                    },
                                );
                            },
                            move |scope| {
                                scope.item(
                                    LiquidMenuItem::new(ACCORDION_HEADER).keeps_open(),
                                    move || unfolded.set(!unfolded.get()),
                                );
                                if unfolded.get() {
                                    scope.item(LiquidMenuItem::new(UNFOLDED_ROW), || {});
                                }
                                scope.item(LiquidMenuItem::new(PLAIN_ROW).section_start(), || {});
                            },
                        );
                    },
                );
            });
        })
        .expect("launch dropdown accordion runner");

    if FAILED.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
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
        .unwrap_or_else(|| fail(robot, &format!("cannot {what}: `{text}` is not on screen")));
    robot
        .click(bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
        .unwrap_or_else(|error| fail(robot, &format!("cannot {what}: {error}")));
}

fn settle(robot: &cranpose::Robot, millis: u64) {
    let _ = robot.wait_for_idle();
    std::thread::sleep(Duration::from_millis(millis));
    let _ = robot.wait_for_idle();
}

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("\n✗ {message}");
    FAILED.store(true, Ordering::Relaxed);
    let _ = robot.exit();
    std::process::exit(1);
}
