//! Text-selection loupe capture runner: replays the reference recording's
//! choreography (grab the end handle ON the line → loupe grows in → slow drag
//! → release → loupe deflates; then a dot-grab drag that must show NO loupe)
//! against a scene matching `example/target/text-selection/` (dark warm
//! backdrop, white two-line text, pink accent), and saves screenshots at the
//! measured keyframes for the strict visual judges.
//!
//! Run with:
//! `cargo run --package desktop-app --example robot_text_loupe --features robot-app`
//!
//! Screenshots land in `ROBOT_SHOT_DIR` (default `target/text-loupe`).

use cranpose::widgets::{BasicTextFieldWithOptions, BasicTextFieldOptions, Box as CBox, BoxSpec};
use cranpose::{AppLauncher, Color, Modifier, Size};
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::text::{AnnotatedString, TextStyle, TextUnit};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const WINDOW_WIDTH: u32 = 460;
const WINDOW_HEIGHT: u32 = 340;

/// The reference scene colors: warm near-black backdrop, white text, the
/// recording's pink accent.
const BACKDROP: Color = Color(0.149, 0.129, 0.125, 1.0);
const TEXT_COLOR: Color = Color(0.94, 0.92, 0.90, 1.0);
const ACCENT: Color = Color(0.965, 0.208, 0.557, 1.0);

const FIELD_X: f32 = 20.0;
const FIELD_Y: f32 = 170.0;
const FIELD_WIDTH: f32 = 420.0;

const TEXT: &str = "Silence. Melody. Then beats. Subtle electronic beats goaantra trance pp ulsy catching melody";

static FAILED: AtomicBool = AtomicBool::new(false);

fn text_style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.color = Some(TEXT_COLOR);
    style.span_style.font_size = TextUnit::Sp(16.0);
    style
}

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/text-loupe".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Text Loupe Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(600));
            let _ = robot.wait_for_idle();

            // Measured text geometry (the same measurer the field uses).
            let style = text_style();
            let width_of = |s: &str| -> f32 {
                robot
                    .measure_text(&AnnotatedString::from(s), &style)
                    .expect("measure")
                    .width
            };
            let line_height = robot
                .measure_text(&AnnotatedString::from("Ag"), &style)
                .expect("measure")
                .height
                .max(1.0);
            let line1_top = FIELD_Y;
            let line1_bottom = line1_top + line_height;
            let line1_mid = line1_top + 0.5 * line_height;

            let melody_center = FIELD_X + 0.5 * (width_of("Silence. ") + width_of("Silence. Melody"));
            let end_x = FIELD_X + width_of("Silence. Melody");
            let drag_to_x = FIELD_X + width_of("Silence. Melody. Then");

            // Focus with a touch tap, then a second tap on the same word to
            // select it (touch is what arms the finger handles).
            robot.drag(melody_center, line1_mid, melody_center, line1_mid).expect("tap 1");
            std::thread::sleep(Duration::from_millis(120));
            robot.drag(melody_center, line1_mid, melody_center, line1_mid).expect("tap 2");
            settle(&robot, 700);
            let idle = robot.screenshot().expect("idle");
            save(&idle, &shot_dir, "01-idle-selection");

            // ---- Grab the end handle ON the line: the loupe grows in ----
            robot.touch_down(end_x, line1_mid).expect("grab end handle");
            std::thread::sleep(Duration::from_millis(20));
            save(
                &robot.screenshot().expect("menu fade"),
                &shot_dir,
                "01b-menu-dissolving",
            );
            let grow_shots = [
                (30u64, "02-grow-a"),
                (70, "03-grow-b"),
                (80, "04-grow-peak"),
                (180, "05-grow-settled"),
            ];
            for (wait_ms, name) in grow_shots {
                std::thread::sleep(Duration::from_millis(wait_ms));
                let shot = robot.screenshot().expect(name);
                save(&shot, &shot_dir, name);
            }
            // The bubble must exist: the region above the line must differ
            // from the idle frame there.
            let grown = robot.screenshot().expect("grown");
            let loupe_region = (
                end_x - 70.0,
                line1_mid - 120.0,
                end_x + 70.0,
                line1_mid - 30.0,
            );
            if !regions_differ(&idle, &grown, loupe_region, 8) {
                fail(&robot, "no loupe appeared above the grabbed line");
            }

            // ---- Slow drag right: the bubble follows with its lag ----
            let steps = 30;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                let x = end_x + (drag_to_x - end_x) * t;
                robot.touch_move(x, line1_mid).expect("drag move");
                std::thread::sleep(Duration::from_millis(16));
                if i == steps / 3 {
                    save(&robot.screenshot().expect("f1"), &shot_dir, "06-follow-a");
                }
                if i == 2 * steps / 3 {
                    save(&robot.screenshot().expect("f2"), &shot_dir, "07-follow-b");
                }
            }
            std::thread::sleep(Duration::from_millis(320));
            save(&robot.screenshot().expect("steady"), &shot_dir, "08-steady");

            // ---- Release: the bubble deflates back into the line ----
            robot.touch_up(drag_to_x, line1_mid).expect("release");
            let dissolve_shots = [(25u64, "09-dissolve-a"), (45, "10-dissolve-b"), (140, "11-after-release")];
            for (wait_ms, name) in dissolve_shots {
                std::thread::sleep(Duration::from_millis(wait_ms));
                let shot = robot.screenshot().expect(name);
                save(&shot, &shot_dir, name);
            }
            std::thread::sleep(Duration::from_millis(120));
            save(
                &robot.screenshot().expect("menu mat"),
                &shot_dir,
                "11b-menu-materializing",
            );
            settle(&robot, 500);
            let after = robot.screenshot().expect("after");
            save(&after, &shot_dir, "12-menu-returned");

            // ---- Dot-grab drag: NO loupe ----
            let end2_x = FIELD_X + width_of("Silence. Melody. Then");
            let dot_y = line1_bottom + 6.0; // end dot center
            robot.touch_down(end2_x, dot_y).expect("grab end dot");
            std::thread::sleep(Duration::from_millis(240));
            let dot_drag = robot.screenshot().expect("dot drag");
            save(&dot_drag, &shot_dir, "13-dot-grab-no-loupe");
            // While a dot-drag is in flight the band above the line must be
            // EMPTY: the menu is hidden and no bubble may appear — plain
            // backdrop only (structure there means a loupe wrongly grew).
            let dot_loupe_region = (
                end2_x - 70.0,
                line1_mid - 120.0,
                end2_x + 70.0,
                line1_mid - 34.0,
            );
            if region_has_structure(&dot_drag, dot_loupe_region) {
                fail(&robot, "a loupe appeared for a dot grab below the line");
            }
            robot.touch_move(end2_x + 40.0, dot_y).expect("dot move");
            std::thread::sleep(Duration::from_millis(200));
            save(&robot.screenshot().expect("dot2"), &shot_dir, "14-dot-drag-moved");
            robot.touch_up(end2_x + 40.0, dot_y).expect("dot release");
            settle(&robot, 400);

            println!("PASS: text loupe contract");
            let _ = robot.exit();
        })
        .try_run(content)
        .expect("launch text loupe runner");

    if FAILED.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn content() {
    CBox(
        Modifier::empty()
            .size(Size {
                width: WINDOW_WIDTH as f32,
                height: WINDOW_HEIGHT as f32,
            })
            .background(BACKDROP),
        BoxSpec::default(),
        || {
            let state = cranpose_core::remember(|| TextFieldState::new(TEXT))
                .with(TextFieldState::clone);
            BasicTextFieldWithOptions(
                state,
                Modifier::empty()
                    .absolute_offset(FIELD_X, FIELD_Y)
                    .size(Size {
                        width: FIELD_WIDTH,
                        height: 120.0,
                    }),
                BasicTextFieldOptions {
                    text_style: text_style(),
                    cursor_color: ACCENT,
                    ..BasicTextFieldOptions::default()
                },
            );
        },
    );
}

fn settle(robot: &cranpose::Robot, ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
    let _ = robot.wait_for_idle();
}

fn save(shot: &cranpose::RobotScreenshot, dir: &Path, name: &str) {
    let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
        .expect("screenshot buffer");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save screenshot");
    println!("saved {}", path.display());
}

/// Whether more than `tolerance_px` pixels differ noticeably (channel delta
/// > 12) between the two screenshots inside the logical-region rect.
fn regions_differ(
    a: &cranpose::RobotScreenshot,
    b: &cranpose::RobotScreenshot,
    region: (f32, f32, f32, f32),
    tolerance_px: usize,
) -> bool {
    let scale_a = a.width as f32 / WINDOW_WIDTH as f32;
    let scale_b = b.width as f32 / WINDOW_WIDTH as f32;
    let mut differing = 0usize;
    let (l, t, r, btm) = region;
    let mut y = t;
    while y < btm {
        let mut x = l;
        while x < r {
            let pa = sample(a, x * scale_a, y * scale_a);
            let pb = sample(b, x * scale_b, y * scale_b);
            if let (Some(pa), Some(pb)) = (pa, pb) {
                let d = pa
                    .iter()
                    .zip(pb.iter())
                    .map(|(u, v)| (*u as i16 - *v as i16).unsigned_abs() as u16)
                    .max()
                    .unwrap_or(0);
                if d > 12 {
                    differing += 1;
                }
            }
            x += 1.0;
        }
        y += 1.0;
    }
    differing > tolerance_px
}

/// Whether the region contains any visual structure (luminance span > 26):
/// an empty patch of the flat backdrop stays under it; a bubble's magnified
/// text/rim or a menu immediately exceeds it.
fn region_has_structure(shot: &cranpose::RobotScreenshot, region: (f32, f32, f32, f32)) -> bool {
    let scale = shot.width as f32 / WINDOW_WIDTH as f32;
    let (l, t, r, b) = region;
    let (mut lo, mut hi) = (u16::MAX, 0u16);
    let mut y = t;
    while y < b {
        let mut x = l;
        while x < r {
            if let Some(p) = sample(shot, x * scale, y * scale) {
                let lum = p[0] as u16 + p[1] as u16 + p[2] as u16;
                lo = lo.min(lum);
                hi = hi.max(lum);
            }
            x += 1.0;
        }
        y += 1.0;
    }
    hi.saturating_sub(lo) > 26 * 3
}

fn sample(shot: &cranpose::RobotScreenshot, x: f32, y: f32) -> Option<[u8; 3]> {
    let (xi, yi) = (x as u32, y as u32);
    if xi >= shot.width || yi >= shot.height {
        return None;
    }
    let idx = ((yi * shot.width + xi) * 4) as usize;
    let px = &shot.pixels[idx..idx + 3];
    Some([px[0], px[1], px[2]])
}

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    FAILED.store(true, Ordering::Relaxed);
    // Give the harness a beat to flush output, then hard-exit if the clean
    // shutdown hangs (never race the GPU teardown with process::exit).
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(15));
        std::process::exit(1);
    });
    let _ = robot.exit();
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
