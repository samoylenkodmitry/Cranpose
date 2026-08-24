//! The liquid tab bar's painted footprint must not grow when the selection
//! settles on an end cell.
//!
//! The bar draws a pill exactly as wide as its cell strip plus one margin a
//! side, and rests its glass bubble centered on the selected cell. The bubble
//! is wider than a cell, so on the first or last cell it overhangs the strip —
//! and it is legal only while that overhang stays inside the margin. Nothing
//! enforced that: the rest width was a fixed 1.10 multiple of the cell, so a
//! bar built with cells wider than 80dp rested its end bubbles *outside* its
//! own pill, glass floating over the backdrop past the pill's rounded end.
//! The one unit test asserting the rule hardcoded the default 78dp cell, which
//! is the single width where it happened to hold.
//!
//! The contract is stated without reaching for any framework internal: the
//! bar is handed a box, and nothing it draws may land outside that box. The
//! pill fills the box, so a bubble that stays inside the box is a bubble
//! inside the pill.
//!
//! Finding the bubble by comparing against the backdrop does not work — a
//! lens is nearly transparent at its meniscus, so its outermost few pixels
//! barely move anything and the escape reads as one or two pixels instead of
//! six. What isolates it exactly is comparing two selections of the *same*
//! scene: the stripes, the pill and its shadow are identical in both, so
//! every column that changes between them is the bubble and nothing else.
//!
//! The backdrop is striped because glass over a flat color is close to
//! invisible — there is nothing for it to refract.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_liquid_tab_bar_pill_containment --features desktop,robot-app
//! ```

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use cranpose::{
    liquid::prelude::*,
    rememberMutableStateOf,
    widgets::{Box as CBox, BoxSpec},
    AppLauncher, Color, Modifier, RobotScreenshot, Size,
};

const WINDOW_WIDTH: u32 = 880;
const WINDOW_HEIGHT: u32 = 260;
/// Well past the 80dp at which a 1.10 rest factor overhangs a 4dp margin. A
/// caller may ask for exactly this: `LiquidTabBarSpec::new` bounds nothing
/// from above, and a tablet-width bar is an ordinary thing to want.
///
/// The width is chosen so the defect is unmistakable rather than marginal.
/// The bar insets its own strip by one margin a side, so cells come out at
/// (200·4 − 8)/4 = 198dp, and a 1.10 rest factor overhangs those by 9.9dp
/// against a 4dp margin — 5.9dp of glass outside the pill, far clear of the
/// antialiasing budget below. At 120dp cells the same defect is only 1.9dp
/// and would hide inside that budget.
const WIDE_TAB: f32 = 200.0;
const TAB_COUNT: usize = 4;
const BAR_HEIGHT: f32 = 64.0;
const BAR_TOP: f32 = 90.0;
const BAR_WIDTH: f32 = WIDE_TAB * TAB_COUNT as f32;
const BAR_LEFT: f32 = (WINDOW_WIDTH as f32 - BAR_WIDTH) * 0.5;
/// Stripe pitch: fine enough that a bubble covers several, wide enough to
/// survive the screenshot's own filtering.
const STRIPE: f32 = 8.0;
const DARK_STRIPE: Color = Color(0.05, 0.05, 0.08, 1.0);
const LIGHT_STRIPE: Color = Color(0.92, 0.94, 0.99, 1.0);
/// A channel moving by more than this between two renders of the same scene
/// is the bubble, not the renderer's dithering. Both shots are settled and
/// differ only in where the bubble rests, so there is no other source of
/// change to talk down.
const CHANGE_FLOOR: i32 = 6;
/// A column counts as changed only when this many of its pixels moved, which
/// rejects a stray antialiased sample without rejecting a real lens edge.
const CHANGED_PIXELS_PER_COLUMN: usize = 3;
/// Antialiasing may put the bubble's own edge one pixel past the box it is
/// allowed to fill. Six pixels of glass outside it is the defect.
const ESCAPE_BUDGET_PX: f32 = 2.0;
/// A settled glide. The lens spring converges well inside this.
const SETTLE_MS: u64 = 900;

static FAILED: AtomicBool = AtomicBool::new(false);

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR")
            .unwrap_or_else(|_| "target/liquid-tab-bar-pill-containment".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Liquid Tab Bar Pill Containment")
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

            // The bar opens on cell 1, an interior cell: the bubble cannot
            // overhang the strip from there, and everything except the bubble
            // is identical between this and any other selection.
            let interior = robot.screenshot().expect("interior selection shot");
            save(&interior, &shot_dir, "0-interior-cell.png");
            let scale = interior.width as f32 / interior.logical_width;
            let left_edge = BAR_LEFT * scale;
            let right_edge = (BAR_LEFT + BAR_WIDTH) * scale;
            println!(
                "the bar was given columns {left_edge:.1}..={right_edge:.1} \
                 (scale {scale:.2}); nothing it draws may land outside them"
            );

            for (name, cell) in [("leading", 0usize), ("trailing", TAB_COUNT - 1)] {
                click_cell(&robot, cell);
                settle(&robot, SETTLE_MS);
                let shot = robot.screenshot().expect("end selection shot");
                save(&shot, &shot_dir, &format!("1-{name}-end-cell.png"));

                let (first, last) = changed_span(&interior, &shot).unwrap_or_else(|| {
                    fail(
                        &robot,
                        &format!("selecting the {name} end cell changed nothing on screen"),
                    )
                });
                println!("the {name} end bubble occupies columns {first}..={last}");

                let past_left = left_edge - first as f32;
                let past_right = last as f32 - right_edge;
                if past_left > ESCAPE_BUDGET_PX || past_right > ESCAPE_BUDGET_PX {
                    let escape = past_left.max(past_right) / scale;
                    fail(
                        &robot,
                        &format!(
                            "the resting bubble on the {name} end cell is drawing outside the \
                             bar: columns {first}..={last} against the {left_edge:.1}..={right_edge:.1} \
                             it was given, about {escape:.1}dp of glass past the pill's rounded \
                             end. Its overhang past its own cell has outgrown the pill's margin."
                        ),
                    );
                }

                // Back to the interior cell, so each end is measured from the
                // same settled starting point.
                click_cell(&robot, 1);
                settle(&robot, SETTLE_MS);
            }

            println!(
                "PASS: the tab bar keeps its resting bubble inside its pill at {WIDE_TAB}dp cells"
            );
            robot.exit().expect("exit");
        })
        .try_run(move || {
            LiquidTheme(LiquidThemeSpec::default(), || {
                CBox(
                    Modifier::empty().size(Size {
                        width: WINDOW_WIDTH as f32,
                        height: WINDOW_HEIGHT as f32,
                    }),
                    BoxSpec::default(),
                    move || {
                        // Vertical stripes: this test can only see the bubble
                        // where the bubble moves something.
                        let stripes = (WINDOW_WIDTH as f32 / STRIPE).ceil() as usize;
                        for index in 0..stripes {
                            let color = if index % 2 == 0 {
                                DARK_STRIPE
                            } else {
                                LIGHT_STRIPE
                            };
                            CBox(
                                Modifier::empty()
                                    .absolute_offset(index as f32 * STRIPE, 0.0)
                                    .size(Size {
                                        width: STRIPE,
                                        height: WINDOW_HEIGHT as f32,
                                    })
                                    .background(color),
                                BoxSpec::default(),
                                || {},
                            );
                        }

                        let selected = rememberMutableStateOf(|| 1usize);
                        LiquidTabBar(
                            Modifier::empty()
                                .absolute_offset(BAR_LEFT, BAR_TOP)
                                .size(Size {
                                    width: BAR_WIDTH,
                                    height: BAR_HEIGHT,
                                }),
                            LiquidTabBarSpec::new(WIDE_TAB),
                            selected.get(),
                            move |index| selected.set(index),
                            |scope| {
                                for (icon, label) in TABS {
                                    scope.tab(icon, label);
                                }
                            },
                        );
                    },
                );
            });
        })
        .expect("launch tab bar pill containment runner");

    if FAILED.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

const TABS: [(&str, &str); TAB_COUNT] = [
    (cranpose::liquid::icons::STAR, "Discover"),
    (cranpose::liquid::icons::LIST_OUTLINE, "Library"),
    (cranpose::liquid::icons::SCHEDULE, "Recent"),
    (cranpose::liquid::icons::SEARCH, "Search"),
];

fn click_cell(robot: &cranpose::Robot, cell: usize) {
    let x = BAR_LEFT + WIDE_TAB * (cell as f32 + 0.5);
    let y = BAR_TOP + BAR_HEIGHT * 0.5;
    robot.click(x, y).expect("click a tab cell");
}

/// The first and last screen column that differs between two settled shots of
/// the same scene — which, when the only thing that moved is the selection, is
/// exactly the span the bubble occupies in one of them.
fn changed_span(before: &RobotScreenshot, after: &RobotScreenshot) -> Option<(u32, u32)> {
    let width = before.width.min(after.width);
    let height = before.height.min(after.height);
    let mut first = None;
    let mut last = None;
    for x in 0..width {
        let changed = (0..height)
            .filter(|y| moved(pixel(before, x, *y), pixel(after, x, *y)))
            .count();
        if changed >= CHANGED_PIXELS_PER_COLUMN {
            first.get_or_insert(x);
            last = Some(x);
        }
    }
    Some((first?, last?))
}

fn moved(before: (u8, u8, u8), after: (u8, u8, u8)) -> bool {
    (before.0 as i32 - after.0 as i32)
        .abs()
        .max((before.1 as i32 - after.1 as i32).abs())
        .max((before.2 as i32 - after.2 as i32).abs())
        > CHANGE_FLOOR
}

fn pixel(shot: &RobotScreenshot, x: u32, y: u32) -> (u8, u8, u8) {
    let index = ((y * shot.width + x) * 4) as usize;
    (
        shot.pixels[index],
        shot.pixels[index + 1],
        shot.pixels[index + 2],
    )
}

fn save(shot: &RobotScreenshot, directory: &Path, name: &str) {
    if let Some(image) = image::RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone()) {
        let _ = image.save(directory.join(name));
    }
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
