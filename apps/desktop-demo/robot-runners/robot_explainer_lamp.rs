use std::{path::Path, time::Duration};

use crate::{
    explainer_robot::{
        app_hook, brightness, check_held, copy_selection, drag, move_to, pixels, save,
        text_bounds, wait, Area, Walk, HEIGHT, WIDTH,
    },
    output_paths, robot_exit, robot_launch,
};

use cranpose::Robot;

const HEADLINE: &str = "Words can light";
const PARAGRAPH: &str = "Each letter is a hole in the wall with a lamp behind it. Cranpose works \
                         the light out from the letters, so when they change, it changes. Select \
                         some words to color the room.";
// View heights of the lamp section.
const LAMP_PAGES: f32 = 1.9;
const STEPS: u32 = 3;
const LIT_FLOOR: usize = 3000;
const LIT_HOLES: usize = 1500;
const GLASS_PIXELS: usize = 300;

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Explainer Lamp ===");
    robot_exit::arm_timeout(120);
    let shots = output_paths::diagnostic_path("cranpose-robot-explainer-lamp");
    if let Err(err) = std::fs::create_dir_all(&shots) {
        robot_exit::fail_without_shutdown(&format!("{}: {err}", shots.display()));
    }
    robot_launch::launch("Robot Explainer Lamp", WIDTH as u32, HEIGHT as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_robot_app_hook(app_hook)
        .with_test_driver(move |robot| {
            robot_launch::switch_tab(&robot, "explainer");
            wait(&robot, Duration::from_millis(300));
            move_to(&robot, WIDTH - 4.0, HEIGHT * 0.5);
            let walk = Walk::new(&robot, "lamp", LAMP_PAGES);
            walk.walk_to(&robot, 0.0);
            save(&robot, &shots, "lamp_00.png");
            let held = text_bounds(&robot, HEADLINE).1;
            for step in 1..=STEPS {
                walk.walk_to(&robot, step as f32 / STEPS as f32);
                save(&robot, &shots, &format!("lamp_{step:02}.png"));
            }
            check_held(&robot, HEADLINE, held);
            let floor = (0.0, HEIGHT * 0.7, WIDTH, HEIGHT * 0.3);
            let lit_floor = pixels(&robot, WIDTH, floor, is_lit);
            let lit_holes = pixels(&robot, WIDTH, text_bounds(&robot, HEADLINE), is_lit);
            println!("  lit pixels: {lit_floor} on the floor, {lit_holes} in the headline");
            if lit_floor < LIT_FLOOR {
                robot_exit::fail(&robot, "the lamp threw no light through the words onto the floor");
            }
            if lit_holes < LIT_HOLES {
                robot_exit::fail(&robot, "the headline's holes do not glow");
            }
            glaze(&robot, &shots, text_bounds(&robot, "Each letter"), floor);
            println!("PASS: lamp shots in {}", shots.display());
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}

// Selects words of the paragraph: their holes turn to coloured glass, and
// the light they let into the room changes with them.
fn glaze(robot: &Robot, shots: &Path, paragraph: Area, floor: Area) {
    let (x, y, width, height) = paragraph;
    let wall = (x - 60.0, y - 40.0, width + 120.0, height + 120.0);
    let floor_before = brightness(robot, WIDTH, floor);
    let wall_before = brightness(robot, WIDTH, wall);
    let glass_before = pixels(robot, WIDTH, wall, is_glass);
    let tinted_before = pixels(robot, WIDTH, floor, is_glass);
    drag(robot, (x + width * 0.2, y + height * 0.84), x + width * 0.75);
    wait(robot, Duration::from_millis(900));
    save(robot, shots, "glass.png");
    let floor_after = brightness(robot, WIDTH, floor);
    let wall_after = brightness(robot, WIDTH, wall);
    let glass_after = pixels(robot, WIDTH, wall, is_glass);
    let tinted_after = pixels(robot, WIDTH, floor, is_glass);
    println!(
        "  glass pixels {glass_before} -> {glass_after}; glass light on the floor \
         {tinted_before} -> {tinted_after}; wall light {wall_before:.0} -> {wall_after:.0}; \
         floor light {floor_before:.0} -> {floor_after:.0}"
    );
    if glass_after < glass_before + GLASS_PIXELS {
        robot_exit::fail(robot, "a selection showed no coloured glass");
    }
    if tinted_after < tinted_before + GLASS_PIXELS {
        robot_exit::fail(robot, "the glass coloured no light on the floor");
    }
    if (floor_after - floor_before).abs() < floor_before * 0.01 {
        robot_exit::fail(robot, "a selection left the light on the floor as it was");
    }
    if (wall_after - wall_before).abs() < wall_before * 0.01 {
        robot_exit::fail(robot, "a selection left the light on the wall as it was");
    }
    let copied = copy_selection(robot);
    println!("  copied {copied:?}");
    if copied.trim().len() < 4 || !PARAGRAPH.contains(copied.trim()) {
        robot_exit::fail(robot, "the copied words are not the paragraph's");
    }
}

fn is_lit([red, green, blue]: [u8; 3]) -> bool {
    red > 150 && green > 90 && u16::from(red) > u16::from(blue) + 40
}

fn is_glass([red, green, blue]: [u8; 3]) -> bool {
    let (red, green, blue) = (u16::from(red), u16::from(green), u16::from(blue));
    blue > 120 && blue > red + 30 && blue > green + 30
}
