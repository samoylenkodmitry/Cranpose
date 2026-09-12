mod liquid_page;
mod robot_exit;
mod robot_shot;

use std::{path::PathBuf, process::ExitCode, time::Duration};

use cranpose::{AppLauncher, RobotScreenshot, SemanticElement};
use cranpose_testing::{find_in_semantics, find_text_exact};
use desktop_app::app;

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 700;
const SHOT_SCALE: f32 = 2.0;
/// The card whose star this runner holds: its neighbours' stars sit under the
/// feed's chrome.
const CARD: &str = "Receipt #0004 — 12 items";
const STAR: &str = "★";
/// How far left of the star's box the walk toward it starts, in dp: plain
/// card there, well past the shadow's reach.
const FAR: f32 = 32.0;
/// Rows the walk samples, in dp from the star's vertical centre.
const ROWS: [f32; 3] = [-10.0, 0.0, 10.0];
/// The most a shadow that fades may darken from one dp to the next.
const STEP_TOLERANCE: f32 = 3.0;
/// A brightening this large is the glass rim: the walk has reached the control.
const RIM_STEP: f32 = 20.0;

type Bounds = (f32, f32, f32, f32);

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/feed-touched-shadow".into()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");
    AppLauncher::new()
        .with_title("Feed Touched Shadow")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_robot_app_hook(liquid_page::app_hook)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(180);
            std::thread::sleep(Duration::from_millis(700));
            liquid_page::open_tab(&robot, "receipts");

            let card = find_in_semantics(&robot, |element| find_text_exact(element, CARD))
                .unwrap_or_else(|| {
                    robot_exit::fail(&robot, &format!("{CARD} must be on the page"))
                });
            let star = find_in_semantics(&robot, |element| {
                star_beside(element, card.1 + card.3 * 0.5)
            })
            .unwrap_or_else(|| robot_exit::fail(&robot, &format!("{CARD} must carry a {STAR}")));
            println!("[touched] {CARD} {card:?} star {star:?}");

            let rest = robot
                .screenshot_with_scale(SHOT_SCALE)
                .expect("resting screenshot");
            robot_shot::save(&rest, &shot_dir, "rest.png");
            let resting = walks(&rest, star);
            let depth = resting
                .iter()
                .map(|walk| walk[0] - walk.iter().copied().fold(f32::INFINITY, f32::min))
                .fold(0.0f32, f32::max);
            if depth < STEP_TOLERANCE * 2.0 {
                robot_exit::fail(
                    &robot,
                    &format!(
                        "the star must cast a shadow on the card beside it for this to test \
                         anything: the card darkens by only {depth:.1} toward the star"
                    ),
                );
            }
            if let Some(cut) = first_cut(&resting) {
                robot_exit::fail(
                    &robot,
                    &format!("the resting page already steps beside the star: {cut}"),
                );
            }

            let (cx, cy) = liquid_page::centre(star);
            liquid_page::hold(&robot, cx, cy);
            let held = robot
                .screenshot_with_scale(SHOT_SCALE)
                .expect("held screenshot");
            robot_shot::save(&held, &shot_dir, "held.png");
            let pressed = walks(&held, star);
            robot.touch_up(cx, cy).expect("touch up");
            liquid_page::settle(&robot);
            let released = robot
                .screenshot_with_scale(SHOT_SCALE)
                .expect("released screenshot");
            robot_shot::save(&released, &shot_dir, "released.png");

            println!(
                "[touched] centre row rest {:?}\n[touched] centre row held {:?}",
                resting[1], pressed[1]
            );
            if let Some(cut) = first_cut(&pressed) {
                robot_exit::fail(
                    &robot,
                    &format!(
                        "holding the star cut its shadow into a rectangle: {cut}. A held \
                         control's shadow must fade like the resting one."
                    ),
                );
            }
            println!("✓ PASS: holding the star keeps its shadow fading into the card");
            robot.exit().expect("exit");
        })
        .try_run(app::combined_app)
        .expect("launch feed touched shadow runner");
    ExitCode::SUCCESS
}

/// The clickable element holding a star whose box spans `y`.
fn star_beside(element: &SemanticElement, y: f32) -> Option<Bounds> {
    let bounds = element.bounds;
    if element.clickable
        && (bounds.y..bounds.y + bounds.height).contains(&y)
        && find_text_exact(element, STAR).is_some()
    {
        return Some((bounds.x, bounds.y, bounds.width, bounds.height));
    }
    element
        .children
        .iter()
        .find_map(|child| star_beside(child, y))
}

/// The page's luma along each of [`ROWS`], walking from `FAR` dp left of the
/// star's box toward its edge one dp at a time.
fn walks(shot: &RobotScreenshot, star: Bounds) -> Vec<Vec<f32>> {
    let sample = robot_shot::logical_sampler(shot);
    let luma = |x: f32, y: f32| {
        let (r, g, b) = sample(x, y);
        0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b)
    };
    let centre = star.1 + star.3 * 0.5;
    ROWS.iter()
        .map(|row| {
            let y = centre + row;
            (0..FAR as usize)
                .map(|step| luma(star.0 - FAR + step as f32, y))
                .collect()
        })
        .collect()
}

/// Where a walk toward the star darkens by more than a fading shadow can in
/// one dp before it reaches the glass rim: the edge of a cut shadow.
fn first_cut(walks: &[Vec<f32>]) -> Option<String> {
    walks.iter().zip(ROWS).find_map(|(walk, row)| {
        walk.windows(2)
            .enumerate()
            .find_map(|(step, pair)| {
                let inward = pair[1] - pair[0];
                if inward > RIM_STEP {
                    return Some(None);
                }
                (-inward > STEP_TOLERANCE).then(|| {
                    Some(format!(
                        "{:.1} dp below the centre row, the card goes from {:.1} to {:.1} between \
                     {:.0} and {:.0} dp left of the star's box",
                        row,
                        pair[0],
                        pair[1],
                        FAR - step as f32,
                        FAR - step as f32 - 1.0
                    ))
                })
            })
            .flatten()
    })
}
