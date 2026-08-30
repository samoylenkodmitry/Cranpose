use std::{
    process::ExitCode,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use cranpose::{
    widgets::{BasicTextFieldOptions, BasicTextFieldWithOptions, Box as CBox, BoxSpec},
    AppLauncher, Color, Modifier, Size,
};
use cranpose_foundation::text::TextFieldState;
use cranpose_testing::find_text_in_semantics;
use cranpose_ui::text::{AnnotatedString, TextStyle, TextUnit};

const WINDOW_WIDTH: u32 = 460;
const WINDOW_HEIGHT: u32 = 340;
const BACKDROP: Color = Color(0.149, 0.129, 0.125, 1.0);
const TEXT_COLOR: Color = Color(0.94, 0.92, 0.90, 1.0);
const ACCENT: Color = Color(0.965, 0.208, 0.557, 1.0);

const FIELD_X: f32 = 20.0;
const FIELD_Y: f32 = 170.0;
const FIELD_WIDTH: f32 = 420.0;

const TEXT: &str = "Silence. Melody. Then beats.";

static FAILED: AtomicBool = AtomicBool::new(false);

fn text_style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.color = Some(TEXT_COLOR);
    style.span_style.font_size = TextUnit::Sp(16.0);
    style.paragraph_style.line_height = TextUnit::Sp(24.0);
    style
}

fn main() -> ExitCode {
    let _ = env_logger::try_init();

    AppLauncher::new()
        .with_title("Menu Slide Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();

            let style = text_style();
            let width_of = |s: &str| {
                robot
                    .measure_text(&AnnotatedString::from(s), &style)
                    .expect("measure")
                    .width
            };
            let prefix = width_of("Silence. ");
            let word = width_of("Melody");
            let press_x = FIELD_X + prefix + word * 0.5;
            let line_mid = FIELD_Y + 12.0;

            robot.touch_down(press_x, line_mid).expect("press down");
            std::thread::sleep(Duration::from_millis(250));
            if find_text_in_semantics(&robot, "Copy").is_some() {
                fail(&robot, "menu appeared before the long-press threshold");
            }
            std::thread::sleep(Duration::from_millis(600));

            let menu_up_while_down = find_text_in_semantics(&robot, "Copy").is_some();
            if !menu_up_while_down {
                fail(
                    &robot,
                    "menu did not materialize during the hold (finger still down)",
                );
            }
            robot.touch_up(press_x, line_mid).expect("lift");
            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();

            let shot = robot.screenshot().expect("post-lift shot");
            let accent_pixels = count_accentish(&shot, FIELD_X + prefix, FIELD_Y, word, 20.0);
            println!("menu_up_while_down={menu_up_while_down} accent_pixels={accent_pixels}");
            if accent_pixels < 60 {
                fail(
                    &robot,
                    &format!("no selection highlight after the long-press ({accent_pixels} px)"),
                );
            }

            let beats_x = FIELD_X + width_of("Silence. Melody. Then ") + width_of("beats") * 0.5;
            robot
                .mouse_move(beats_x, line_mid)
                .expect("move mouse to second word");
            robot.mouse_down().expect("mouse long-press down");
            std::thread::sleep(Duration::from_millis(750));
            let copy = find_text_in_semantics(&robot, "Copy")
                .unwrap_or_else(|| fail(&robot, "menu absent during the second hold"));
            let (copy_cx, copy_cy) = (copy.0 + copy.2 * 0.5, copy.1 + copy.3 * 0.5);
            robot
                .mouse_move((press_x + copy_cx) * 0.5, (line_mid + copy_cy) * 0.5)
                .expect("slide toward menu");
            std::thread::sleep(Duration::from_millis(60));
            robot.mouse_move(copy_cx, copy_cy).expect("slide onto Copy");
            std::thread::sleep(Duration::from_millis(120));
            robot.mouse_up().expect("release on Copy");
            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();
            if find_text_in_semantics(&robot, "Copy").is_some() {
                fail(
                    &robot,
                    "lifting on Copy did not fire it (menu still open after release)",
                );
            }

            println!("PASS: menu slide contract");
            robot.exit().expect("exit");
        })
        .try_run(move || {
            let state = TextFieldState::new(TEXT);
            CBox(
                Modifier::empty()
                    .size(Size {
                        width: WINDOW_WIDTH as f32,
                        height: WINDOW_HEIGHT as f32,
                    })
                    .background(BACKDROP),
                BoxSpec::default(),
                move || {
                    let state = state;
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
        })
        .expect("launch menu slide runner");

    if FAILED.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    FAILED.store(true, Ordering::Relaxed);
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(15));
        std::process::exit(1);
    });
    let _ = robot.exit();
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

fn count_accentish(
    shot: &cranpose::RobotScreenshot,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> usize {
    let sx = shot.width as f32 / shot.logical_width.max(1.0);
    let sy = shot.height as f32 / shot.logical_height.max(1.0);
    let (x0, y0) = ((x * sx) as usize, (y * sy) as usize);
    let (x1, y1) = (
        (((x + width) * sx) as usize).min(shot.width as usize),
        (((y + height) * sy) as usize).min(shot.height as usize),
    );
    let mut count = 0usize;
    for py in y0..y1 {
        for px in x0..x1 {
            let i = (py * shot.width as usize + px) * 4;
            let (r, g, b) = (shot.pixels[i], shot.pixels[i + 1], shot.pixels[i + 2]);
            if r > 90 && r as i16 - g as i16 > 40 && b > g {
                count += 1;
            }
        }
    }
    count
}
