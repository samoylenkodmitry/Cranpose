//! End-to-end vertical selection-handle grab-offset contract.

use cranpose::widgets::{BasicTextFieldOptions, BasicTextFieldWithOptions, Box as CBox, BoxSpec};
use cranpose::{AppLauncher, Color, Modifier, RobotScreenshot, SemanticElement, Size};
use cranpose_core::remember;
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::text::{AnnotatedString, TextStyle, TextUnit};
use cranpose_ui::text_selection::GRAB_DIRECT_FOLLOW_DISTANCE;
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const TEXT: &str = "alpha bravo alpha bravo alpha bravo alpha bravo";
const FIELD_X: f32 = 42.0;
const FIELD_Y: f32 = 84.0;
const FIELD_WIDTH: f32 = 110.0;
const ACCENT: Color = Color(0.965, 0.208, 0.557, 1.0);
type SelectedEditable = ((f32, f32, f32, f32), (usize, usize));

fn text_style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.color = Some(Color(0.95, 0.94, 0.93, 1.0));
    style.span_style.font_size = TextUnit::Sp(16.0);
    style.paragraph_style.line_height = TextUnit::Sp(24.0);
    style
}

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR")
            .unwrap_or_else(|_| "target/selection-vertical-grab".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create screenshot directory");

    AppLauncher::new()
        .with_title("Selection Vertical Grab Contract")
        .with_size(400, 260)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(500));
            let style = text_style();
            let width = |text: &str| {
                robot
                    .measure_text(&AnnotatedString::from(text), &style)
                    .expect("measure text")
                    .width
            };
            let bravo_center = FIELD_X + 0.5 * (width("alpha ") + width("alpha bravo"));
            let line_mid = FIELD_Y
                + robot
                    .measure_text(&AnnotatedString::from("Ag"), &style)
                    .expect("measure line")
                    .height
                    * 0.5;

            robot
                .drag(bravo_center, line_mid, bravo_center, line_mid)
                .expect("focus tap");
            std::thread::sleep(Duration::from_millis(120));
            robot
                .drag(bravo_center, line_mid, bravo_center, line_mid)
                .expect("word-select tap");
            std::thread::sleep(Duration::from_millis(650));

            let (field_bounds, initial) =
                selected_editable(&robot.get_semantics().expect("initial selection semantics"))
                    .expect("double tap must create a range selection");
            let initial = normalized(initial);
            assert_eq!(initial.0, 6, "fixture selection start");
            assert!((7..=11).contains(&initial.1), "fixture selection end");
            let fixed_start = initial.0;
            let initial_shot = robot.screenshot().expect("initial handle frame");
            save(&initial_shot, &shot_dir, "00-initial");
            let end_dot = lower_handle_center(&initial_shot, field_bounds)
                .expect("selection end dot must be measurable");

            robot
                .stylus_down(end_dot.0, end_dot.1)
                .expect("stylus must grab the rendered end dot");
            std::thread::sleep(Duration::from_millis(20));

            let direct_y = end_dot.1 + GRAB_DIRECT_FOLLOW_DISTANCE;
            let drift_y = end_dot.1 + 40.0;
            let full_y = end_dot.1 + 64.0;
            let strict_y = full_y + 24.0;
            let phases = [
                ("01-direct", direct_y, 8usize, 0.0f32, 8.0f32),
                ("02-drift", drift_y, 20usize, 24.0f32, 16.0f32),
                ("03-full-view", full_y, 32usize, 48.0f32, 16.0f32),
                ("04-strict-follow", strict_y, 44usize, 72.0f32, 16.0f32),
            ];
            for (name, finger_y, expected_end, line_advance, finger_clearance) in phases {
                robot
                    .stylus_move_and_wait_for_frame(end_dot.0, finger_y)
                    .expect("vertical stylus handle move");
                let semantics = robot.get_semantics().expect("phase semantics");
                let (bounds, selection) =
                    selected_editable(&semantics).expect("phase range selection");
                let selection = normalized(selection);
                assert_eq!(selection.0, fixed_start, "fixed selection edge at {name}");
                assert_eq!(
                    selection.1, expected_end,
                    "{name}: wrong endpoint for the predicted visual line"
                );
                let shot = robot.screenshot().expect("phase frame");
                save(&shot, &shot_dir, name);
                let dot = lower_handle_center(&shot, bounds)
                    .expect("dragged end dot must remain visible");
                let expected_dot_y = end_dot.1 + line_advance;
                assert!(
                    (dot.1 - expected_dot_y).abs() <= 1.5,
                    "{name}: handle y {:.1}, expected visual-line y {expected_dot_y:.1}",
                    dot.1
                );
                assert!(
                    (finger_y - dot.1 - finger_clearance).abs() <= 1.5,
                    "{name}: pointer/handle relation drifted: finger={finger_y:.1}, handle={:.1}",
                    dot.1
                );
                println!(
                    "{name}: finger_y={finger_y:.1} endpoint={} handle_y={:.1}",
                    selection.1, dot.1
                );
            }
            robot
                .stylus_up(end_dot.0, strict_y)
                .expect("release stylus handle");
            println!("PASS: soft-wrapped stylus selection grab phases");
            let _ = robot.exit();
        })
        .try_run(content)
        .expect("launch vertical selection runner");
    ExitCode::SUCCESS
}

fn content() {
    CBox(
        Modifier::empty()
            .size(Size {
                width: 400.0,
                height: 260.0,
            })
            .background(Color(0.149, 0.129, 0.125, 1.0)),
        BoxSpec::default(),
        || {
            let state = remember(|| TextFieldState::new(TEXT)).with(TextFieldState::clone);
            BasicTextFieldWithOptions(
                state,
                Modifier::empty()
                    .absolute_offset(FIELD_X, FIELD_Y)
                    .size(Size {
                        width: FIELD_WIDTH,
                        height: 124.0,
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

fn selected_editable(elements: &[SemanticElement]) -> Option<SelectedEditable> {
    for element in elements {
        if element.editable_text {
            if let Some(selection) = element.text_selection {
                if selection.0 != selection.1 {
                    let bounds = element.bounds;
                    return Some(((bounds.x, bounds.y, bounds.width, bounds.height), selection));
                }
            }
        }
        if let Some(found) = selected_editable(&element.children) {
            return Some(found);
        }
    }
    None
}

fn normalized(selection: (usize, usize)) -> (usize, usize) {
    (selection.0.min(selection.1), selection.0.max(selection.1))
}

fn lower_handle_center(shot: &RobotScreenshot, rect: (f32, f32, f32, f32)) -> Option<(f32, f32)> {
    let (sx, sy) = (
        shot.width as f32 / shot.logical_width.max(1.0),
        shot.height as f32 / shot.logical_height.max(1.0),
    );
    let (x, y, width, height) = rect;
    let left = (x.max(0.0) * sx) as usize;
    let right = (((x + width) * sx) as usize).min(shot.width as usize);
    let top = (y.max(0.0) * sy) as usize;
    let bottom = (((y + height + 10.0) * sy) as usize).min(shot.height as usize);
    let is_accent = |px: usize, py: usize| {
        let i = (py * shot.width as usize + px) * 4;
        let (r, g, b) = (shot.pixels[i], shot.pixels[i + 1], shot.pixels[i + 2]);
        r > 180 && r.saturating_sub(g) > 70 && b.saturating_sub(g) > 25
    };
    let max_y = (top..bottom)
        .rev()
        .find(|&py| (left..right).any(|px| is_accent(px, py)))?;
    let band_top = max_y.saturating_sub((4.0 * sy).ceil() as usize);
    let mut sum_x = 0usize;
    let mut sum_y = 0usize;
    let mut count = 0usize;
    for py in band_top..=max_y {
        for px in left..right {
            if is_accent(px, py) {
                sum_x += px;
                sum_y += py;
                count += 1;
            }
        }
    }
    (count > 0).then(|| {
        (
            sum_x as f32 / count as f32 / sx,
            sum_y as f32 / count as f32 / sy,
        )
    })
}

fn save(shot: &RobotScreenshot, dir: &Path, name: &str) {
    let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
        .expect("screenshot buffer");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save screenshot");
}
