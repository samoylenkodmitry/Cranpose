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
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
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
    let wrap_boundary = Arc::new(Mutex::new(None::<usize>));
    let wrap_boundary_for_driver = Arc::clone(&wrap_boundary);

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
            let first_wrap_boundary = wrap_boundary_for_driver
                .lock()
                .expect("wrap boundary lock")
                .expect("composed scene must publish its first wrap boundary");
            let wrap_boundary_x = FIELD_X + width_of(&TEXT[..first_wrap_boundary]);

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
            // The taps ride wall-clock timing: VERIFY the selection armed
            // (the start handle's dot floats above the line) and retry the
            // pair on a race — everything downstream grabs that handle.
            let start_x = FIELD_X + width_of("Silence. ");
            for attempt in 0..3 {
                let probe = robot.screenshot_with_scale(1.0).expect("selection probe");
                if region_has_structure(
                    &probe,
                    (
                        start_x - 12.0,
                        line1_top - 16.0,
                        start_x + 12.0,
                        line1_top - 2.0,
                    ),
                ) {
                    break;
                }
                if attempt == 2 {
                    fail(&robot, "double-tap selection never armed");
                }
                robot
                    .drag(melody_center, line1_mid, melody_center, line1_mid)
                    .expect("re-tap 1");
                std::thread::sleep(Duration::from_millis(120));
                robot
                    .drag(melody_center, line1_mid, melody_center, line1_mid)
                    .expect("re-tap 2");
                settle(&robot, 700);
            }
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
            // Animations stamp their start on their FIRST frame after
            // animateTo — 1 ms stamp-steps right after each event/flip keep
            // that lag at 1 ms instead of a whole keyframe step.
            let grow_steps = [
                (0.0, false),  // process the grab
                (1.0, false),  // birth gate stamps here
                (59.0, true),  // +60: mid birth-delay, no bubble
                (61.0, true),  // +121: gate completes; grow stamps next
                (1.0, true),   // +122: the birth pose (grow t=0)
                (34.0, true),  // +156: grow t=35
                (55.0, true),  // +211: grow t=90
                (90.0, true),  // +301: grow t=180 (the overshoot)
                (200.0, true), // +501: settled
            ];
            let grow_names = [
                "01b-menu-dissolving",
                "01c-birth-boundary",
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

            // Direct-manipulation contract: pointer coordinates are never an
            // animation target. Step the held pointer by a large amount and
            // inspect the very next rendered frame; a chase spring leaves the
            // bubble at its source for one or more frames and fails here.
            let held = robot.screenshot_with_scale(3.0).expect("held loupe");
            let held_center = loupe_top_rim_center_x(&held)
                .unwrap_or_else(|| fail(&robot, "held loupe top rim was not measurable"));
            let jump_x = (end_x + 90.0).min(FIELD_X + FIELD_WIDTH - 60.0);
            robot
                .touch_move(jump_x, line1_mid)
                .expect("abrupt direct-manipulation step");
            let jumped = robot
                .screenshot_with_scale(3.0)
                .expect("immediate jump frame");
            save(&jumped, &shot_dir, "05b-direct-follow");
            let jumped_center = loupe_top_rim_center_x(&jumped)
                .unwrap_or_else(|| fail(&robot, "jumped loupe top rim was not measurable"));
            println!(
                "loupe direct follow held={held_center:.2} pointer={jump_x:.2} frame={jumped_center:.2}"
            );
            if (jumped_center - jump_x).abs() > 8.0
                || jumped_center - held_center < jump_x - end_x - 12.0
            {
                fail(
                    &robot,
                    &format!(
                        "loupe animated toward the pointer instead of following the input frame: held={held_center:.2}, pointer={jump_x:.2}, frame={jumped_center:.2}"
                    ),
                );
            }
            robot
                .touch_move(end_x, line1_mid)
                .expect("restore follow choreography");
            settle(&robot, 320);

            // ---- Slow drag right: the bubble follows every input frame ----
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
            // reference offsets (+8/+25/+42/+55 for the fade, +300/+360
            // for the menu's 250 ms-delay + 140 ms materialize window).
            robot.touch_up(drag_to_x, line1_mid).expect("release");
            let dissolve_steps = [
                (0.0, false),  // process the release
                (1.0, false),  // the fade stamps here
                (7.0, true),   // +8
                (17.0, true),  // +25
                (17.0, true),  // +42
                (13.0, true),  // +55
                (35.0, true),  // +90
                (20.0, true),  // +110
                (20.0, true),  // +130: gone
                (130.0, true), // +260: materialize just started (dim)
                (40.0, true),  // +300
                (60.0, true),  // +360
            ];
            let dissolve_names = [
                "09-dissolve-a",
                "10-dissolve-b",
                "10b-dissolve-c",
                "11-after-release",
                "11d-dissolve-late",
                "11e-terminal",
                "11f-gone",
                "11a-menu-materializing-0",
                "11b-menu-materializing",
                "11c-menu-materializing-2",
            ];
            let shots = robot
                .capture_keyframes(3.0, &dissolve_steps)
                .expect("dissolve keyframes");
            for (shot, name) in shots.iter().zip(dissolve_names) {
                save(shot, &shot_dir, name);
            }
            // The fade must actually be a fade: still at +8, reduced at +25,
            // translucent at +42, and gone by +55.
            let fade_region = loupe_region_at(drag_to_x);
            if !region_has_structure(&shots[0], fade_region) {
                fail(&robot, "dissolve +8: bubble already gone");
            }
            if !region_has_structure(&shots[1], fade_region) {
                fail(&robot, "dissolve +25: bubble already gone");
            }
            let terminal_region = (
                drag_to_x - 25.0,
                line1_mid - 37.0,
                drag_to_x + 25.0,
                line1_mid - 14.0,
            );
            if region_has_structure(&shots[3], terminal_region) {
                fail(&robot, "dissolve +55: bubble residue never unmounted");
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
            robot
                .touch_move(wrap_boundary_x, dot_y)
                .expect("dot move to soft-wrap boundary");
            std::thread::sleep(Duration::from_millis(200));
            let wrapped = robot.screenshot_with_scale(3.0).expect("wrapped handle");
            save(&wrapped, &shot_dir, "14-dot-drag-moved");
            let upper_end_pixels = count_accent_pixels(
                &wrapped,
                (
                    wrap_boundary_x - 12.0,
                    line1_bottom - 4.0,
                    wrap_boundary_x + 12.0,
                    line1_bottom + 18.0,
                ),
            );
            println!(
                "wrapped boundary byte={first_wrap_boundary} x={wrap_boundary_x:.2} upper-end pixels={upper_end_pixels}"
            );
            if upper_end_pixels < 20 {
                fail(
                    &robot,
                    "end handle at a shared soft-wrap boundary jumped off the upper visual line",
                );
            }
            robot
                .touch_up(wrap_boundary_x, dot_y)
                .expect("dot release at soft-wrap boundary");
            settle(&robot, 400);

            println!("PASS: text loupe contract");
            let _ = robot.exit();
        })
        .try_run(move || content(Arc::clone(&wrap_boundary)))
        .expect("launch text loupe runner");

    if FAILED.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn content(wrap_boundary: Arc<Mutex<Option<usize>>>) {
    let annotated = AnnotatedString::from(TEXT);
    let ranges = cranpose_ui::text::wrapped_line_ranges(
        None,
        &annotated,
        &text_style(),
        cranpose_ui::text::TextLayoutOptions::default(),
        Some(FIELD_WIDTH),
    );
    assert!(
        ranges.len() >= 2 && ranges[0].end < TEXT.len(),
        "loupe fixture must soft-wrap: {ranges:?}"
    );
    *wrap_boundary.lock().expect("wrap boundary lock") = Some(ranges[0].end);
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

/// Center of the loupe's bright top rim in logical coordinates. The sampled
/// band is above both the text and edit menu, so every bright pixel belongs to
/// the bubble silhouette.
fn loupe_top_rim_center_x(shot: &cranpose::RobotScreenshot) -> Option<f32> {
    let scale = shot.width as f32 / WINDOW_WIDTH as f32;
    let y0 = (45.0 * scale) as u32;
    let y1 = (105.0 * scale) as u32;
    let mut min_x = u32::MAX;
    let mut max_x = 0u32;
    let mut count = 0usize;
    for y in y0..y1.min(shot.height) {
        for x in 0..shot.width {
            let index = ((y * shot.width + x) * 4) as usize;
            let r = shot.pixels[index] as u16;
            let g = shot.pixels[index + 1] as u16;
            let b = shot.pixels[index + 2] as u16;
            if r + g + b > 285 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                count += 1;
            }
        }
    }
    (count >= 20).then(|| (min_x + max_x) as f32 * 0.5 / scale)
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

fn count_accent_pixels(shot: &cranpose::RobotScreenshot, region: (f32, f32, f32, f32)) -> usize {
    let scale = shot.width as f32 / WINDOW_WIDTH as f32;
    let (left, top, right, bottom) = region;
    let (left, top, right, bottom) = (
        (left * scale).max(0.0) as u32,
        (top * scale).max(0.0) as u32,
        ((right * scale) as u32).min(shot.width),
        ((bottom * scale) as u32).min(shot.height),
    );
    let mut count = 0usize;
    for y in top..bottom {
        for x in left..right {
            let index = ((y * shot.width + x) * 4) as usize;
            let r = shot.pixels[index];
            let g = shot.pixels[index + 1];
            let b = shot.pixels[index + 2];
            if r > 190 && r.saturating_sub(g) > 70 && b.saturating_sub(g) > 25 {
                count += 1;
            }
        }
    }
    count
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
