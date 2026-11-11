// The Music tab in the real app: a move of the song's position draws the
// notes of that moment over the keyboard. The run presses no key and never
// plays, so it opens no output device and makes no sound.

use crate::{output_paths, robot_launch};

use std::{path::PathBuf, time::Duration};

use cranpose::{AppLauncher, Robot};
use image::RgbaImage;

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 800;
const SHOT_DIR_ENV: &str = "ROBOT_SHOT_DIR";
const START_CLOCK: &str = "0:00 / ";
// The share of the song the run moves to: the middle section, where both
// hands play running notes.
const SEEK_SHARE: f32 = 0.4;
const PAGE_PADDING: f32 = 16.0;
const KEYBOARD_HEIGHT: f32 = 96.0;
// The strip of the roll right above the keyboard: the notes that sound now
// and within the next second.
const STRIP_HEIGHT: f32 = 120.0;
// The roll's own color, and how far a pixel may be from it and still be roll.
const ROLL_COLOR: [u8; 3] = [0x0B, 0x10, 0x20];
const COLOR_TOLERANCE: i32 = 12;
const FEWEST_NOTE_PIXELS: usize = 400;

fn shot_dir() -> PathBuf {
    std::env::var_os(SHOT_DIR_ENV).map_or_else(
        || output_paths::diagnostic_path("cranpose-robot-music"),
        PathBuf::from,
    )
}

fn capture(robot: &Robot, name: &str) -> RgbaImage {
    let shot = robot.screenshot().expect("screenshot");
    let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels)
        .expect("a screenshot holds its width times its height in pixels");
    let path = shot_dir().join(format!("{name}.png"));
    image
        .save(&path)
        .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));
    println!("captured {}", path.display());
    image
}

// Pixels of the strip above the keyboard that a note covers.
fn note_pixels(image: &RgbaImage) -> usize {
    let scale = image.height() as f32 / WINDOW_HEIGHT as f32;
    let scale_x = image.width() as f32 / WINDOW_WIDTH as f32;
    let bottom = WINDOW_HEIGHT as f32 - PAGE_PADDING - KEYBOARD_HEIGHT;
    let rows = ((bottom - STRIP_HEIGHT) * scale) as u32..(bottom * scale) as u32;
    let columns =
        (PAGE_PADDING * scale_x) as u32..((WINDOW_WIDTH as f32 - PAGE_PADDING) * scale_x) as u32;
    image
        .enumerate_pixels()
        .filter(|(x, y, _)| columns.contains(x) && rows.contains(y))
        .filter(|(_, _, pixel)| {
            ROLL_COLOR
                .iter()
                .zip(pixel.0)
                .any(|(roll, have)| (i32::from(*roll) - i32::from(have)).abs() > COLOR_TOLERANCE)
        })
        .count()
}

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Music Tab ===");
    std::fs::create_dir_all(shot_dir()).expect("the shot directory");

    AppLauncher::new()
        .with_title("Robot Music Tab")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(700));
            robot_launch::switch_tab(&robot, "music");
            robot.pump_frames(3).expect("the tab settles");

            let (clock_x, clock_y, clock_width, clock_height, _) = robot
                .find_text_by_prefix(START_CLOCK)
                .expect("semantics")
                .expect("the tab shows its clock at the start of the song");

            // The position fader fills the row from the clock to the page's edge.
            let fader_left = clock_x + clock_width + PAGE_PADDING;
            let fader_right = WINDOW_WIDTH as f32 - PAGE_PADDING;
            let seek_x = fader_left + (fader_right - fader_left) * SEEK_SHARE;
            robot
                .click(seek_x, clock_y + clock_height * 0.5)
                .expect("a click on the position fader");
            robot.pump_frames(3).expect("the tab settles");

            assert!(
                robot
                    .find_text_by_prefix(START_CLOCK)
                    .expect("semantics")
                    .is_none(),
                "the clock left the start of the song"
            );
            let moved = capture(&robot, "music-moved");
            let pixels = note_pixels(&moved);
            println!("note pixels after the move: {pixels}");
            assert!(
                pixels >= FEWEST_NOTE_PIXELS,
                "the roll draws the notes of the moment: {pixels} pixels in a note's color"
            );

            println!("PASS: the Music tab draws the notes at the position of the song");
            robot.exit().expect("exit");
        })
        .run(robot_launch::counter_demo);
}
