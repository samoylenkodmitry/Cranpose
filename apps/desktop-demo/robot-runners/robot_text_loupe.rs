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

use cranpose::widgets::{BasicTextFieldOptions, BasicTextFieldWithOptions, Box as CBox, BoxSpec};
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

const TEXT: &str =
    "Silence. Melody. Then beats. Subtle electronic beats goaantra trance pp ulsy catching melody";

static FAILED: AtomicBool = AtomicBool::new(false);

fn text_style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.color = Some(TEXT_COLOR);
    style.span_style.font_size = TextUnit::Sp(16.0);
    // The reference line pitch is 24pt for 16pt text; the default Noto pitch
    // (~21dp) leaked the next line into the loupe's interior where the
    // reference shows the inter-line gap.
    style.paragraph_style.line_height = TextUnit::Sp(24.0);
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

            let melody_center =
                FIELD_X + 0.5 * (width_of("Silence. ") + width_of("Silence. Melody"));
            let end_x = FIELD_X + width_of("Silence. Melody");
            let drag_to_x = FIELD_X + width_of("Silence. Melody. Then");

            // Focus with a touch tap, then a second tap on the same word to
            // select it (touch is what arms the finger handles).
            robot
                .drag(melody_center, line1_mid, melody_center, line1_mid)
                .expect("tap 1");
            std::thread::sleep(Duration::from_millis(120));
            robot
                .drag(melody_center, line1_mid, melody_center, line1_mid)
                .expect("tap 2");
            settle(&robot, 700);
            let idle = robot.screenshot_with_scale(3.0).expect("idle");
            save(&idle, &shot_dir, "01-idle-selection");

            // ---- Grow-in keyframes: one grab, exact clock steps. The
            // headless loop free-runs and high-resolution captures stall
            // unpredictably, so wall-clock sleeps cannot keyframe the ~120 ms
            // birth + inflate honestly; capture_keyframes advances the
            // animation clock deterministically instead. Offsets mirror the
            // reference sheet frames: birth ~120 ms after the grab, grow
            // frames at birth +0/+35/+90/+180/+380 ms, plus a mid-delay
            // probe (+60) that must show NO bubble yet.
            robot.touch_down(end_x, line1_mid).expect("grab end handle");
            let grow_steps = [
                (0.0, false),  // process the grab; the birth gate starts here
                (60.0, true),  // mid birth-delay: menu dissolving, no bubble
                (60.0, true),  // +120: the birth pose
                (35.0, true),  // +155
                (55.0, true),  // +210
                (90.0, true),  // +300: near the width peak
                (200.0, true), // +500: settled
            ];
            let grow_names = [
                "01b-menu-dissolving",
                "02-grow-a",
                "02b-grow-b",
                "03-grow-c",
                "04-grow-peak",
                "05-grow-settled",
            ];
            let shots = robot
                .capture_keyframes(3.0, &grow_steps)
                .expect("grow keyframes");
            // Presence band: strictly ABOVE the edit menu's capsule+halo and
            // the line's own glyphs/handles — only a bubble reaches up here
            // (the grown top sits ~116dp over the line mid, the birth pose
            // ~100dp).
            let loupe_region_at =
                |x: f32| (x - 70.0, line1_mid - 125.0, x + 70.0, line1_mid - 75.0);
            for (i, (shot, name)) in shots.iter().zip(grow_names).enumerate() {
                save(shot, &shot_dir, name);
                let present = region_has_structure(shot, loupe_region_at(end_x));
                if i == 0 && present {
                    fail(&robot, "a bubble appeared during the birth delay");
                }
                if i > 0 && !present {
                    fail(&robot, &format!("{name}: bubble missing at its keyframe"));
                }
            }
            // The exact-clock steps ran the animation clock ~620 ms ahead of
            // wall time; let wall catch up so the real-time drag below
            // animates normally (updates clamp to the advanced clock until
            // then).
            std::thread::sleep(Duration::from_millis(700));

            // ---- Slow drag right: the bubble follows with its lag ----
            let steps = 30;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                let x = end_x + (drag_to_x - end_x) * t;
                robot.touch_move(x, line1_mid).expect("drag move");
                std::thread::sleep(Duration::from_millis(16));
                if i == steps / 3 {
                    save(
                        &robot.screenshot_with_scale(3.0).expect("f1"),
                        &shot_dir,
                        "06-follow-a",
                    );
                }
                if i == 2 * steps / 3 {
                    save(
                        &robot.screenshot_with_scale(3.0).expect("f2"),
                        &shot_dir,
                        "07-follow-b",
                    );
                }
            }
            std::thread::sleep(Duration::from_millis(320));
            save(
                &robot.screenshot_with_scale(3.0).expect("steady"),
                &shot_dir,
                "08-steady",
            );

            // ---- Release: the bubble deflates back into the line, then the
            // menu rematerializes — ONE release, exact clock steps at the
            // reference offsets (+8/+25/+42/+55/+90 for the fade, +300/+360
            // for the menu's 250 ms-delay + 140 ms materialize window).
            robot.touch_up(drag_to_x, line1_mid).expect("release");
            let dissolve_steps = [
                (0.0, false), // process the release; the fade starts here
                (8.0, true),
                (17.0, true),  // +25
                (17.0, true),  // +42
                (13.0, true),  // +55
                (35.0, true),  // +90
                (210.0, true), // +300
                (60.0, true),  // +360
            ];
            let dissolve_names = [
                "09-dissolve-a",
                "10-dissolve-b",
                "10b-dissolve-c",
                "11-after-release",
                "11d-gone",
                "11b-menu-materializing",
                "11c-menu-materializing-2",
            ];
            let shots = robot
                .capture_keyframes(3.0, &dissolve_steps)
                .expect("dissolve keyframes");
            for (shot, name) in shots.iter().zip(dissolve_names) {
                save(shot, &shot_dir, name);
            }
            // The fade must actually be a fade: clearly present at +8 and
            // +25, gone by +90 (the 55 ms tween unmounts at ~54 ms).
            let fade_region = loupe_region_at(drag_to_x);
            if !region_has_structure(&shots[0], fade_region) {
                fail(&robot, "dissolve +8: bubble already gone");
            }
            if !region_has_structure(&shots[1], fade_region) {
                fail(&robot, "dissolve +25: bubble already gone");
            }
            if region_has_structure(&shots[4], fade_region) {
                fail(&robot, "dissolve +90: bubble residue never unmounted");
            }
            std::thread::sleep(Duration::from_millis(500));
            let after = robot.screenshot_with_scale(3.0).expect("after");
            save(&after, &shot_dir, "12-menu-returned");

            // ---- Dot-grab drag: NO loupe ----
            let end2_x = FIELD_X + width_of("Silence. Melody. Then");
            let dot_y = line1_bottom + 6.0; // end dot center
            robot.touch_down(end2_x, dot_y).expect("grab end dot");
            std::thread::sleep(Duration::from_millis(240));
            let dot_drag = robot.screenshot_with_scale(3.0).expect("dot drag");
            save(&dot_drag, &shot_dir, "13-dot-grab-no-loupe");
            // While a dot-drag is in flight the band above the line must be
            // EMPTY: the menu is hidden and no bubble may appear — plain
            // backdrop only (structure there means a loupe wrongly grew).
            // Band strictly ABOVE the ghost toolbar text (which lives at
            // y≈122..138): only a wrongly-grown bubble reaches up here.
            let dot_loupe_region = (
                end2_x - 70.0,
                line1_mid - 120.0,
                end2_x + 70.0,
                line1_mid - 72.0,
            );
            if region_has_structure(&dot_drag, dot_loupe_region) {
                fail(&robot, "a loupe appeared for a dot grab below the line");
            }
            robot.touch_move(end2_x + 40.0, dot_y).expect("dot move");
            std::thread::sleep(Duration::from_millis(200));
            save(
                &robot.screenshot_with_scale(3.0).expect("dot2"),
                &shot_dir,
                "14-dot-drag-moved",
            );
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
            // Page content BEHIND the edit menu's anchor band (like the
            // reference's toolbar row): the menu's glass body must show it
            // ghosting through — a flat empty backdrop would make the
            // material's transparency unjudgeable.
            let ghost_style = {
                let mut style = TextStyle::default();
                style.span_style.color = Some(Color(0.62, 0.58, 0.56, 1.0));
                style.span_style.font_size = TextUnit::Sp(15.0);
                style
            };
            cranpose::widgets::Text(
                "Styles  •  cinematic  •  anime  •  catchy  •  beats  •  trance  •  lo-fi  •  vocal"
                    .to_string(),
                Modifier::empty().absolute_offset(34.0, 122.0),
                ghost_style,
            );
            let state =
                cranpose_core::remember(|| TextFieldState::new(TEXT)).with(TextFieldState::clone);
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
