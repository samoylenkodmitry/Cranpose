mod liquid_page;
mod robot_exit;
mod robot_shot;
mod robot_tab_fixture;

use std::{path::PathBuf, process::ExitCode, time::Duration};

use cranpose::{
    liquid::prelude::*,
    widgets::{Box as CBox, BoxSpec},
    AppLauncher, Color, Modifier, Robot, RobotScreenshot, Size,
};
use cranpose_testing::{find_in_semantics, find_text_exact};

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 260;
const SHOT_SCALE: f32 = 2.0;
const BACKGROUND: u8 = 192;
const TAB_WIDTH: f32 = 78.0;
const BAR_WIDTH: f32 = TAB_WIDTH * 4.0 + 8.0;
const BAR_HEIGHT: f32 = 62.0;
const BAR_LEFT: f32 = (WINDOW_WIDTH as f32 - BAR_WIDTH) * 0.5;
const BAR_TOP: f32 = 90.0;
const FIRST: &str = "Discover";
const LAST: &str = "Account";
const BAND_TOP: f32 = 8.0;
const BAND_BOTTOM: f32 = 22.0;
const TOLERANCE: f32 = 3.0;
const TABS: [(&str, &str); 4] = [
    (cranpose::liquid::icons::STAR, FIRST),
    (cranpose::liquid::icons::LIST_OUTLINE, "Library"),
    (cranpose::liquid::icons::SCHEDULE, "Recent"),
    (cranpose::liquid::icons::ACCOUNT_CIRCLE, LAST),
];

type Bounds = (f32, f32, f32, f32);

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/touched-shadow".into()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");
    AppLauncher::new()
        .with_title("Liquid Touched Shadow")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(180);
            std::thread::sleep(Duration::from_millis(700));
            liquid_page::settle(&robot);

            let first = text_bounds(&robot, FIRST);
            let last = text_bounds(&robot, LAST);
            let bottom = BAR_TOP + BAR_HEIGHT;
            println!("[touched] {FIRST} {first:?} {LAST} {last:?} band {bottom:.0} dp");

            let rest = robot
                .screenshot_with_scale(SHOT_SCALE)
                .expect("resting screenshot");
            robot_shot::save(&rest, &shot_dir, "rest.png");
            let resting = band(&rest, first.0, last.0 + last.2, bottom);
            let darkest = resting.iter().copied().fold(f32::INFINITY, f32::min);
            if f32::from(BACKGROUND) - darkest < TOLERANCE * 2.0 {
                robot_exit::fail(
                    &robot,
                    &format!(
                        "the bar must cast a shadow on the band below it for this to test \
                         anything: darkest sample {darkest:.1}, background {BACKGROUND}"
                    ),
                );
            }

            let (cx, cy) = liquid_page::centre(first);
            liquid_page::hold(&robot, cx, cy);
            let held = robot
                .screenshot_with_scale(SHOT_SCALE)
                .expect("held screenshot");
            robot_shot::save(&held, &shot_dir, "held.png");
            let pressed = band(&held, first.0, last.0 + last.2, bottom);
            robot.touch_up(cx, cy).expect("touch up");
            if resting
                .iter()
                .chain(&pressed)
                .any(|value| *value > f32::from(BACKGROUND) + TOLERANCE)
            {
                robot_exit::fail(
                    &robot,
                    "glass illumination entered the exterior shadow band",
                );
            }

            let mut worst = (0.0f32, 0usize);
            for (index, (before, after)) in resting.iter().zip(pressed.iter()).enumerate() {
                let lifted = after - before;
                if lifted > worst.0 {
                    worst = (lifted, index);
                }
            }
            println!(
                "[touched] band rest {:?} held {:?}",
                &resting[..resting.len().min(8)],
                &pressed[..pressed.len().min(8)]
            );
            if worst.0 > TOLERANCE {
                robot_exit::fail(
                    &robot,
                    &format!(
                        "holding a destination erased the bar's shadow beside it: the page \
                         under the bar went from {:.1} to {:.1}, {:.1} lighter, at sample {} of \
                         {}. A press must not repaint what lies outside the control.",
                        resting[worst.1],
                        pressed[worst.1],
                        worst.0,
                        worst.1,
                        resting.len()
                    ),
                );
            }
            println!("✓ PASS: holding a destination leaves the shadow under the bar alone");
            robot.exit().expect("exit");
        })
        .try_run(|| {
            LiquidTheme(LiquidThemeSpec::default(), || {
                CBox(
                    Modifier::empty()
                        .size(Size {
                            width: WINDOW_WIDTH as f32,
                            height: WINDOW_HEIGHT as f32,
                        })
                        .background(Color::from_rgb_u8(BACKGROUND, BACKGROUND, BACKGROUND)),
                    BoxSpec::default(),
                    || {
                        robot_tab_fixture::bar(
                            Modifier::empty()
                                .absolute_offset(BAR_LEFT, BAR_TOP)
                                .size(Size {
                                    width: BAR_WIDTH,
                                    height: BAR_HEIGHT,
                                }),
                            LiquidTabBarSpec::new(TAB_WIDTH),
                            0,
                            &TABS,
                        );
                    },
                );
            });
        })
        .expect("launch touched shadow runner");
    ExitCode::SUCCESS
}

fn band(shot: &RobotScreenshot, left: f32, right: f32, bar_bottom: f32) -> Vec<f32> {
    let sample = robot_shot::logical_sampler(shot);
    let luma = |x: f32, y: f32| {
        let (r, g, b) = sample(x, y);
        0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b)
    };
    let mut band = Vec::new();
    let mut x = left;
    while x <= right {
        let mut total = 0.0;
        let mut count = 0.0;
        let mut y = bar_bottom + BAND_TOP;
        while y <= bar_bottom + BAND_BOTTOM {
            total += luma(x, y);
            count += 1.0;
            y += 2.0;
        }
        band.push(total / count);
        x += 10.0;
    }
    band
}

fn text_bounds(robot: &Robot, text: &str) -> Bounds {
    find_in_semantics(robot, |element| find_text_exact(element, text))
        .unwrap_or_else(|| robot_exit::fail(robot, &format!("{text} must be on the page")))
}
