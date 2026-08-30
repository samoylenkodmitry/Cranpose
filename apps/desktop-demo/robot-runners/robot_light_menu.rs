use std::{path::PathBuf, process::ExitCode, time::Duration};

use cranpose::{
    widgets::{BasicTextFieldOptions, BasicTextFieldWithOptions, Box as CBox, BoxSpec},
    AppLauncher, Color, Modifier, Size,
};
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::{
    local_on_light_surface,
    text::{AnnotatedString, TextStyle, TextUnit},
};
use image::RgbaImage;

const WINDOW_WIDTH: u32 = 460;
const WINDOW_HEIGHT: u32 = 300;
const FIELD_X: f32 = 20.0;
const FIELD_Y: f32 = 150.0;
const TEXT: &str = "Silence. Melody. Then beats. Subtle electronic melody";

fn text_style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.color = Some(Color(0.1, 0.1, 0.12, 1.0));
    style.span_style.font_size = TextUnit::Sp(16.0);
    style.paragraph_style.line_height = TextUnit::Sp(24.0);
    style
}

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/light-menu".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Adaptive Menu Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(600));
            let _ = robot.wait_for_idle();

            let style = text_style();
            let width_of = |s: &str| {
                robot
                    .measure_text(&AnnotatedString::from(s), &style)
                    .expect("measure")
                    .width
            };
            let line_h = robot
                .measure_text(&AnnotatedString::from("Ag"), &style)
                .expect("measure")
                .height
                .max(1.0);
            let word_x = FIELD_X + 0.5 * (width_of("Silence. ") + width_of("Silence. Melody"));
            let mid = FIELD_Y + 0.5 * line_h;

            robot.drag(word_x, mid, word_x, mid).expect("tap 1");
            std::thread::sleep(Duration::from_millis(120));
            robot.drag(word_x, mid, word_x, mid).expect("tap 2");
            std::thread::sleep(Duration::from_millis(700));

            let shot = robot.screenshot_with_scale(3.0).expect("menu");
            let img = RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
                .expect("screenshot buffer");
            let path = shot_dir.join("light-menu.png");
            img.save(&path).expect("save");
            println!("PASS: saved {}", path.display());
            let _ = robot.exit();
        })
        .try_run(content)
        .expect("launch adaptive menu runner");
    ExitCode::SUCCESS
}

fn content() {
    CBox(
        Modifier::empty()
            .size(Size {
                width: WINDOW_WIDTH as f32,
                height: WINDOW_HEIGHT as f32,
            })
            .background(Color(1.0, 1.0, 1.0, 1.0)),
        BoxSpec::default(),
        || {
            let ghost = {
                let mut s = TextStyle::default();
                s.span_style.color = Some(Color(0.55, 0.55, 0.6, 1.0));
                s.span_style.font_size = TextUnit::Sp(15.0);
                s
            };
            cranpose::widgets::Text(
                "cinematic  •  anime  •  catchy  •  beats  •  trance".to_string(),
                Modifier::empty().absolute_offset(34.0, 108.0),
                ghost,
            );
            cranpose_core::CompositionLocalProvider(
                vec![local_on_light_surface().provides(true)],
                || {
                    let state = cranpose_core::remember(|| TextFieldState::new(TEXT))
                        .with(TextFieldState::clone);
                    BasicTextFieldWithOptions(
                        state,
                        Modifier::empty()
                            .absolute_offset(FIELD_X, FIELD_Y)
                            .size(Size {
                                width: 420.0,
                                height: 60.0,
                            }),
                        BasicTextFieldOptions {
                            text_style: text_style(),
                            cursor_color: Color(0.965, 0.208, 0.557, 1.0),
                            ..BasicTextFieldOptions::default()
                        },
                    );
                },
            );
        },
    );
}
