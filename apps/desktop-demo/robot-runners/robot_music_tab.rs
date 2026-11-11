// The Music tab in the real app: a move of the song's position draws the
// notes of that moment over the keyboard and lights the keys under the
// notes that sound. The run presses no key and never
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
// The panel's margin and padding: the position slider ends this far from
// the window's edge.
const PANEL_INSET: f32 = 28.0;
const SLIDER_GAP: f32 = 12.0;
// How far inside the slider the middle of its thumb stands at the start.
const THUMB_REACH: f32 = 14.0;
const KEYBOARD_HEIGHT: f32 = 104.0;
// The strip of the roll right above the keyboard: the notes that sound now
// and within the next second.
const STRIP_HEIGHT: f32 = 120.0;
// A pixel of a note is brighter than the night behind the roll in one of
// its channels by at least this much.
const NOTE_BRIGHTNESS: u8 = 150;
const FEWEST_NOTE_PIXELS: usize = 400;
// A lit key has the color of its note: its channels lie this far apart.
// The channels of a white, a black and a gray pixel lie closer.
const LIGHT_SPREAD: u8 = 80;
const FEWEST_LIT_KEY_PIXELS: usize = 300;

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
    let bottom = WINDOW_HEIGHT as f32 - KEYBOARD_HEIGHT;
    let rows = ((bottom - STRIP_HEIGHT) * scale) as u32..(bottom * scale) as u32;
    let columns = 0..((WINDOW_WIDTH as f32 - PANEL_INSET) * scale_x) as u32;
    image
        .enumerate_pixels()
        .filter(|(x, y, _)| columns.contains(x) && rows.contains(y))
        .filter(|(_, _, pixel)| pixel.0[..3].iter().any(|channel| *channel >= NOTE_BRIGHTNESS))
        .count()
}

// Pixels of the keyboard that have the color of a note.
fn lit_key_pixels(image: &RgbaImage) -> usize {
    let scale = image.height() as f32 / WINDOW_HEIGHT as f32;
    let top = ((WINDOW_HEIGHT as f32 - KEYBOARD_HEIGHT) * scale) as u32;
    image
        .enumerate_pixels()
        .filter(|(_, y, _)| *y >= top)
        .filter(|(_, _, pixel)| {
            let channels = &pixel.0[..3];
            let brightest = channels.iter().max().copied().unwrap_or(0);
            let darkest = channels.iter().min().copied().unwrap_or(0);
            brightest - darkest >= LIGHT_SPREAD
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

            capture(&robot, "music-start");

            // The position slider fills the row from the clock to the panel's
            // edge, and its thumb stands at its start.
            let slider_left = clock_x + clock_width + SLIDER_GAP;
            let slider_right = WINDOW_WIDTH as f32 - PANEL_INSET;
            let middle = clock_y + clock_height * 0.5;
            robot
                .drag(
                    slider_left + THUMB_REACH,
                    middle,
                    slider_left + (slider_right - slider_left) * SEEK_SHARE,
                    middle,
                )
                .expect("a drag of the position slider's thumb");
            // The keys go down and the notes light on springs: they rest
            // a moment after the move.
            robot.wait_for_idle().expect("the lights of the tab come to rest");

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

            let lit = lit_key_pixels(&moved);
            println!("lit key pixels after the move: {lit}");
            assert!(
                lit >= FEWEST_LIT_KEY_PIXELS,
                "a key under a note that sounds is lit: {lit} pixels in a note's color"
            );

            println!("PASS: the Music tab draws the notes at the position of the song");
            robot.exit().expect("exit");
        })
        .run(robot_launch::counter_demo);
}
