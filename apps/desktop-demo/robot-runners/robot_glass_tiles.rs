mod liquid_page;
mod robot_exit;
mod robot_shot;

use std::{path::PathBuf, process::ExitCode, time::Duration};

use cranpose::{AppLauncher, Color, Robot, RobotScreenshot};
use cranpose_testing::{find_in_semantics, find_text_exact};
use desktop_app::app::{self, glass_tile_description, GLASS_TILES};

const WINDOW_WIDTH: u32 = 1100;
const WINDOW_HEIGHT: u32 = 800;
const SHOT_SCALE: f32 = 2.0;
/// The tile the runner hovers: top right, the reference's lit one.
const SUBJECT: usize = 1;
/// Where a tile's interior is read, in dp from its top-left corner: inside
/// the glass, clear of the label and of the rim's bloom.
const SAMPLE_INSET: (f32, f32) = (40.0, 164.0);
/// Half the side of the patch averaged around the sample point, in dp.
const SAMPLE_REACH: f32 = 3.0;
/// How much more of its own colour a hovered tile's interior must carry than
/// at rest, in 8-bit channel units along the colour's chroma.
const FLOOD: f32 = 25.0;
/// How far from rest the interior may sit once the pointer has left the tile:
/// the stage lights keep drifting behind it.
const REST_TOLERANCE: f32 = 15.0;

type Bounds = (f32, f32, f32, f32);

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/glass-tiles".into()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");
    AppLauncher::new()
        .with_title("Glass Tiles")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_robot_app_hook(liquid_page::app_hook)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(180);
            std::thread::sleep(Duration::from_millis(700));
            liquid_page::open_tab(&robot, "tiles");

            let tiles: Vec<Bounds> = GLASS_TILES
                .iter()
                .map(|spec| tile_bounds(&robot, &glass_tile_description(spec)))
                .collect();
            let subject = tiles[SUBJECT];
            let colour = GLASS_TILES[SUBJECT].color;
            let first = tiles[0];
            let last = tiles[tiles.len() - 1];
            let slab_centre = (
                (first.0 + last.0 + last.2) * 0.5,
                (first.1 + last.1 + last.3) * 0.5,
            );
            println!("[tiles] tiles {tiles:?} slab centre {slab_centre:?}");

            let rest = robot
                .screenshot_with_scale(SHOT_SCALE)
                .expect("resting screenshot");
            robot_shot::save(&rest, &shot_dir, "rest.png");
            let resting = interior(&rest, subject, colour);

            let (cx, cy) = liquid_page::centre(subject);
            robot.mouse_move(cx, cy).expect("hover the tile");
            liquid_page::settle(&robot);
            let hovered_shot = robot
                .screenshot_with_scale(SHOT_SCALE)
                .expect("hovered screenshot");
            robot_shot::save(&hovered_shot, &shot_dir, "hovered.png");
            let hovered = interior(&hovered_shot, subject, colour);

            robot
                .mouse_move(slab_centre.0, slab_centre.1)
                .expect("leave the tile");
            liquid_page::settle(&robot);
            let left_shot = robot
                .screenshot_with_scale(SHOT_SCALE)
                .expect("left screenshot");
            robot_shot::save(&left_shot, &shot_dir, "left.png");
            let left = interior(&left_shot, subject, colour);

            println!("[tiles] interior rest {resting:.1} hovered {hovered:.1} left {left:.1}");
            if hovered - resting < FLOOD {
                robot_exit::fail(
                    &robot,
                    &format!(
                        "hovering a tile must flood it with its colour: its interior carried \
                         {resting:.1} of it at rest and {hovered:.1} under the pointer"
                    ),
                );
            }
            if (left - resting).abs() > REST_TOLERANCE {
                robot_exit::fail(
                    &robot,
                    &format!(
                        "a tile must settle back once the pointer leaves it: its interior carried \
                         {resting:.1} of its colour at rest and {left:.1} afterwards"
                    ),
                );
            }
            println!(
                "✓ PASS: every tile is on the page, and hovering one floods it and lets it go"
            );
            robot.exit().expect("exit");
        })
        .try_run(app::combined_app)
        .expect("launch glass tiles runner");
    ExitCode::SUCCESS
}

fn tile_bounds(robot: &Robot, description: &str) -> Bounds {
    find_in_semantics(robot, |element| find_text_exact(element, description))
        .unwrap_or_else(|| robot_exit::fail(robot, &format!("{description} must be on the page")))
}

/// How much of `colour` the patch around a tile's sample point carries: the
/// mean projection of each pixel's chroma onto the colour's chroma, so a white
/// beam crossing behind the glass changes it little and a flood of the tile's
/// own colour changes it a lot.
fn interior(shot: &RobotScreenshot, tile: Bounds, colour: Color) -> f32 {
    let sample = robot_shot::logical_sampler(shot);
    let (x, y) = (tile.0 + SAMPLE_INSET.0, tile.1 + SAMPLE_INSET.1);
    let axis = chroma([colour.r() * 255.0, colour.g() * 255.0, colour.b() * 255.0]);
    let length = axis.iter().map(|c| c * c).sum::<f32>().sqrt().max(1.0);
    let mut total = 0.0;
    let mut count = 0.0;
    let mut dy = -SAMPLE_REACH;
    while dy <= SAMPLE_REACH {
        let mut dx = -SAMPLE_REACH;
        while dx <= SAMPLE_REACH {
            let (r, g, b) = sample(x + dx, y + dy);
            let pixel = chroma([f32::from(r), f32::from(g), f32::from(b)]);
            total += pixel.iter().zip(axis).map(|(p, a)| p * a).sum::<f32>() / length;
            count += 1.0;
            dx += 1.0;
        }
        dy += 1.0;
    }
    total / count
}

fn chroma(channels: [f32; 3]) -> [f32; 3] {
    let mean = (channels[0] + channels[1] + channels[2]) / 3.0;
    channels.map(|channel| channel - mean)
}
