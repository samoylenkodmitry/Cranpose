mod robot_exit;
mod robot_shot;

use std::{path::PathBuf, process::ExitCode, sync::atomic::AtomicBool, time::Duration};

use cranpose::{
    liquid::prelude::*,
    rememberMutableStateOf,
    widgets::{Box as CBox, BoxSpec},
    AppLauncher, Color, Modifier, RobotScreenshot, Size,
};

const WINDOW_WIDTH: u32 = 880;
const WINDOW_HEIGHT: u32 = 260;
const WIDE_TAB: f32 = 200.0;
const TAB_COUNT: usize = 4;
const BAR_HEIGHT: f32 = 64.0;
const BAR_TOP: f32 = 90.0;
const BAR_WIDTH: f32 = WIDE_TAB * TAB_COUNT as f32;
const BAR_LEFT: f32 = (WINDOW_WIDTH as f32 - BAR_WIDTH) * 0.5;
const STRIPE: f32 = 8.0;
const DARK_STRIPE: Color = Color(0.05, 0.05, 0.08, 1.0);
const LIGHT_STRIPE: Color = Color(0.92, 0.94, 0.99, 1.0);
const CHANGE_FLOOR: i32 = 6;
const CHANGED_PIXELS_PER_COLUMN: usize = 3;
const ESCAPE_BUDGET_PX: f32 = 2.0;
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
            robot_exit::arm_timeout(180);
            std::thread::sleep(Duration::from_millis(700));
            robot_shot::settle(&robot, SETTLE_MS);

            let interior = robot.screenshot().expect("interior selection shot");
            robot_shot::save(&interior, &shot_dir, "0-interior-cell.png");
            let scale = interior.width as f32 / interior.logical_width;
            let left_edge = BAR_LEFT * scale;
            let right_edge = (BAR_LEFT + BAR_WIDTH) * scale;
            println!(
                "the bar was given columns {left_edge:.1}..={right_edge:.1} \
                 (scale {scale:.2}); nothing it draws may land outside them"
            );

            for (name, cell) in [("leading", 0usize), ("trailing", TAB_COUNT - 1)] {
                click_cell(&robot, cell);
                robot_shot::settle(&robot, SETTLE_MS);
                let shot = robot.screenshot().expect("end selection shot");
                robot_shot::save(&shot, &shot_dir, &format!("1-{name}-end-cell.png"));

                let (first, last) = changed_span(&interior, &shot).unwrap_or_else(|| {
                    robot_exit::fail_and_await_shutdown(&robot, &FAILED,
                        &format!("selecting the {name} end cell changed nothing on screen"))
                });
                println!("the {name} end bubble occupies columns {first}..={last}");

                let past_left = left_edge - first as f32;
                let past_right = last as f32 - right_edge;
                if past_left > ESCAPE_BUDGET_PX || past_right > ESCAPE_BUDGET_PX {
                    let escape = past_left.max(past_right) / scale;
                    robot_exit::fail_and_await_shutdown(&robot, &FAILED,
                        &format!(
                            "the resting bubble on the {name} end cell is drawing outside the \
                             bar: columns {first}..={last} against the {left_edge:.1}..={right_edge:.1} \
                             it was given, about {escape:.1}dp of glass past the pill's rounded \
                             end. Its overhang past its own cell has outgrown the pill's margin."
                        ));
                }

                click_cell(&robot, 1);
                robot_shot::settle(&robot, SETTLE_MS);
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

    robot_exit::exit_code(&FAILED)
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
