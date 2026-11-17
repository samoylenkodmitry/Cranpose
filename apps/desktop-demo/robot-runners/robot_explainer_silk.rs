// The Explainer tab's silk: a drag over the card's words marks them in
// highlighter, the wheel walks the section from the small curtain to the
// grown one, a picture at each step, a drag over the curtain's headline
// lays its words in gold leaf, and the grown curtain's button, pressed
// where it shows on the moving folds, sends a gust that blows the cloth
// farther than the breeze does.

use std::time::Duration;

use crate::{
    explainer_robot::{drag, pixels, save, text_bounds, wait, Walk},
    output_paths, robot_exit, robot_launch,
};

use cranpose::Robot;

const GUST: &str = "Send a gust";
const CARD_WORDS: &str = "Ready-made";
const HEADLINE: &str = "In Cranpose you";
// How many more pixels show highlighter once words on the card are marked.
const MARKED_PIXELS: usize = 300;
const WIDTH: f32 = 1200.0;
const HEIGHT: f32 = 800.0;
// The steps that walk the section to its end.
const STEPS: u32 = 8;
// View heights of the silk section.
const SILK_PAGES: f32 = 2.6;
// How long the cloth moves before its button is found again.
const WATCH: Duration = Duration::from_millis(350);

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Explainer Silk ===");
    robot_exit::arm_timeout(90);
    let shots = output_paths::diagnostic_path("cranpose-robot-explainer");
    if let Err(err) = std::fs::create_dir_all(&shots) {
        robot_exit::fail_without_shutdown(&format!("{}: {err}", shots.display()));
    }
    robot_launch::launch("Robot Explainer Silk", WIDTH as u32, HEIGHT as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(move |robot| {
            robot_launch::switch_tab(&robot, "explainer");
            wait(&robot, Duration::from_millis(300));
            let walk = Walk::new(&robot, "silk", SILK_PAGES);
            // The wheel scrolls the article under the pointer.
            if let Err(err) = robot.mouse_move(WIDTH * 0.5, HEIGHT * 0.6) {
                robot_exit::fail(&robot, &format!("move the mouse: {err}"));
            }
            walk.walk_to(&robot, 0.0);
            save(&robot, &shots, "walk_00.png");
            mark_card(&robot, &shots);
            for step in 1..=STEPS {
                walk.walk_to(&robot, step as f32 / STEPS as f32);
                save(&robot, &shots, &format!("walk_{step:02}.png"));
            }
            // The cloth catches up with the last step first.
            wait(&robot, Duration::from_millis(1200));
            if robot
                .find_button_bounds_exact(GUST)
                .ok()
                .flatten()
                .is_none()
            {
                robot_exit::fail(&robot, "the grown curtain shows no gust button");
            }
            gild_headline(&robot, &shots);
            let resting = button_center(&robot);
            wait(&robot, WATCH);
            let breeze = distance(resting, button_center(&robot));
            let before = button_center(&robot);
            if let Err(err) = robot.click(before.0, before.1) {
                robot_exit::fail(&robot, &format!("click the gust button: {err}"));
            }
            wait(&robot, WATCH);
            let gust = distance(before, button_center(&robot));
            save(&robot, &shots, "gust.png");
            println!("  the button moved {breeze:.1} px in the breeze, {gust:.1} px in the gust");
            if gust < breeze * 3.0 + 12.0 {
                robot_exit::fail(
                    &robot,
                    "a click on the gust button, where it shows on the folds, sent no gust",
                );
            }
            println!("PASS: silk shots in {}", shots.display());
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}

// Drags across the first words on the card and checks that they show
// marked in highlighter.
fn mark_card(robot: &Robot, shots: &std::path::Path) {
    let words = text_bounds(robot, CARD_WORDS);
    let before = pixels(robot, WIDTH, words, is_highlighter);
    let (x, y, width, _) = words;
    drag(robot, (x + 2.0, y + 12.0), x + width * 0.5);
    wait(robot, Duration::from_millis(200));
    save(robot, shots, "card.png");
    let after = pixels(robot, WIDTH, words, is_highlighter);
    println!("  highlighter pixels on the card: {before} before the drag, {after} after");
    if after < before + MARKED_PIXELS {
        robot_exit::fail(robot, "a drag over the card's words marked none of them");
    }
}

// Drags across the first line of the curtain's headline and checks that
// leaf comes up round it.
fn gild_headline(robot: &Robot, shots: &std::path::Path) {
    let (x, y, width, height) = text_bounds(robot, HEADLINE);
    let half = height * 0.5;
    let lines = [
        (x - 40.0, y - 20.0, width + 80.0, half + 20.0),
        (x - 40.0, y + half, width + 80.0, half + 20.0),
    ];
    let before = lines.map(|line| pixels(robot, WIDTH, line, is_leaf));
    drag(robot, (x + 4.0, y + height * 0.25), x + width * 0.9);
    wait(robot, Duration::from_millis(300));
    save(robot, shots, "headline.png");
    let after = lines.map(|line| pixels(robot, WIDTH, line, is_leaf));
    let gained = [0, 1].map(|line| after[line].saturating_sub(before[line]));
    println!("  leaf pixels gained round the headline's lines: {gained:?}");
    if gained[0] < gained[1] + MARKED_PIXELS {
        robot_exit::fail(robot, "a drag over the headline's first line laid no leaf round it");
    }
}

fn is_highlighter([red, green, blue]: [u8; 3]) -> bool {
    red > 170 && green < 140 && blue < 140
}

fn is_leaf([red, green, blue]: [u8; 3]) -> bool {
    red > 190 && green > 140 && blue < 120
}

fn button_center(robot: &Robot) -> (f32, f32) {
    match robot.find_button_bounds_exact(GUST) {
        Ok(Some((x, y, width, height))) => (x + width * 0.5, y + height * 0.5),
        Ok(None) => robot_exit::fail(robot, &format!("no button {GUST:?}")),
        Err(err) => robot_exit::fail(robot, &format!("button query: {err}")),
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}
