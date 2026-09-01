//! A physical glass depth must produce the same refractive/chromatic band on
//! short Library/Search chrome as on the taller filter panel without turning
//! that optical depth into a thick painted lower rim. The Receipts feed places
//! both over saturated lazy-list rows, with black gaps that make an oversized
//! structural rim especially visible.

mod robot_exit;
mod robot_shot;

use std::{path::PathBuf, process::ExitCode, sync::atomic::AtomicBool, time::Duration};

use cranpose::{
    liquid::prelude::*,
    text::{SpanStyle, TextStyle, TextUnit},
    widgets::{Box as CBox, BoxSpec, Text},
    Alignment, AppLauncher, Brush, Color, Modifier, Rect, Size,
};
use cranpose_ui::{HorizontalAlignment, VerticalAlignment};
use cranpose_ui_graphics::DrawScope;

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 440;
const SURFACE_X: f32 = 32.0;
const SURFACE_WIDTH: f32 = 836.0;
const SHORT_Y: f32 = 36.0;
const SHORT_HEIGHT: f32 = 56.0;
const TALL_Y: f32 = 152.0;
const TALL_HEIGHT: f32 = 126.0;
const ADAPTIVE_Y: f32 = 330.0;
const ADAPTIVE_HEIGHT: f32 = 56.0;
const UNDERLAY_X: f32 = 330.0;
const RIM_PROBE_X: f32 = SURFACE_X + 300.0;
const RIM_PROBE_WIDTH: f32 = 240.0;
const PHYSICAL_DEPTH_DP: f32 = 21.0;
const PROBE_START_DP: f32 = 0.0;
const PROBE_END_DP: f32 = 70.0;

static FAILED: AtomicBool = AtomicBool::new(false);

fn probe_glass() -> Glass {
    Glass::regular()
        .tint(Color::TRANSPARENT)
        .blur_radius(0.0)
        .saturation(1.0)
        .lift(0.0)
        .contrast(1.0)
        .highlight(0.9)
        .shadow(false)
        .adaptive_frost(Color::WHITE, 0.0)
        .refraction_depth(0.34)
        .refraction_depth_dp(PHYSICAL_DEPTH_DP)
        .refraction_curve(0.55)
        .dispersion(0.30)
        .transmission_refraction(1.0)
}

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR")
            .unwrap_or_else(|_| "target/glass-physical-depth".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create physical-depth shot dir");

    AppLauncher::new()
        .with_title("Glass Physical Depth Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();
            let shot = robot.screenshot().expect("physical-depth screenshot");
            robot_shot::save(&shot, &shot_dir, "glass-physical-depth.png");

            let short_chroma = chromatic_pixels(
                &shot,
                SURFACE_X + PROBE_START_DP,
                SHORT_Y,
                PROBE_END_DP - PROBE_START_DP,
                SHORT_HEIGHT,
            );
            let tall_chroma = chromatic_pixels(
                &shot,
                SURFACE_X + PROBE_START_DP,
                TALL_Y,
                PROBE_END_DP - PROBE_START_DP,
                TALL_HEIGHT,
            );
            let short_foreground = bright_pixels(
                &shot,
                SURFACE_X + 70.0,
                SHORT_Y + 12.0,
                150.0,
                SHORT_HEIGHT - 24.0,
            );
            let adaptive_underlay = bright_pixels(
                &shot,
                UNDERLAY_X,
                ADAPTIVE_Y + 12.0,
                270.0,
                ADAPTIVE_HEIGHT - 24.0,
            );
            let short_lower_rim_depth = lower_rim_depth_dp(
                &shot,
                RIM_PROBE_X,
                SHORT_Y,
                RIM_PROBE_WIDTH,
                SHORT_HEIGHT,
            );
            let tall_lower_rim_depth = lower_rim_depth_dp(
                &shot,
                RIM_PROBE_X,
                TALL_Y,
                RIM_PROBE_WIDTH,
                TALL_HEIGHT,
            );

            println!(
                "physical-depth short_chroma={short_chroma} tall_chroma={tall_chroma} short_foreground={short_foreground} adaptive_underlay={adaptive_underlay} short_lower_rim_depth={short_lower_rim_depth:.2}dp tall_lower_rim_depth={tall_lower_rim_depth:.2}dp"
            );
            const CHROMA_FLOOR: usize = 12;
            if short_chroma < CHROMA_FLOOR || tall_chroma < CHROMA_FLOOR {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    &format!(
                        "21dp glass depth must remain refractive on both 56dp and 126dp surfaces: short={short_chroma}, tall={tall_chroma} chromatic pixels"
                    ),
                );
            }
            if short_foreground < 18 {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    &format!(
                        "short glass foreground lost its light polarity: only {short_foreground} bright label pixels"
                    ),
                );
            }
            if adaptive_underlay < 18 {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    &format!(
                        "adaptive frost inverted the light lazy-row text under glass: only {adaptive_underlay} bright receipt-text pixels"
                    ),
                );
            }
            const MAX_LOWER_RIM_DEPTH_DP: f32 = 3.0;
            if short_lower_rim_depth > MAX_LOWER_RIM_DEPTH_DP
                || tall_lower_rim_depth > MAX_LOWER_RIM_DEPTH_DP
            {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    &format!(
                        "surface glass lower rim must stay thin while refraction remains deep: short={short_lower_rim_depth:.2}dp, tall={tall_lower_rim_depth:.2}dp, maximum={MAX_LOWER_RIM_DEPTH_DP:.2}dp"
                    ),
                );
            }

            println!("PASS: physical glass depth and foreground polarity are geometry-stable");
            robot.exit().expect("exit");
        })
        .try_run(ProbeApp)
        .expect("launch physical-depth runner");

    robot_exit::exit_code(&FAILED)
}

#[cranpose::composable]
#[allow(non_snake_case)]
fn ProbeApp() {
    LiquidTheme(
        LiquidThemeSpec {
            scheme: SchemeMode::Dark,
            ..Default::default()
        },
        || {
            CBox(
                Modifier::empty()
                    .size(Size::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32))
                    .draw_behind(|scope| {
                        scope.draw_rect(Brush::solid(Color::from_rgb_u8(10, 10, 12)));
                        draw_stripes(scope, SHORT_Y, SHORT_HEIGHT);
                        draw_stripes(scope, TALL_Y, TALL_HEIGHT);
                        scope.draw_rect_at(
                            Rect {
                                x: SURFACE_X,
                                y: ADAPTIVE_Y,
                                width: SURFACE_WIDTH,
                                height: ADAPTIVE_HEIGHT,
                            },
                            Brush::solid(Color::from_rgb_u8(17, 153, 142)),
                        );
                    }),
                BoxSpec::default(),
                || {
                    glass_probe(SHORT_Y, SHORT_HEIGHT, "Library & Search");
                    glass_probe(TALL_Y, TALL_HEIGHT, "Newest / By store");
                    Text(
                        "Receipt #0021 — 12 items",
                        Modifier::empty().absolute_offset(UNDERLAY_X, ADAPTIVE_Y + 16.0),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color::WHITE),
                                font_size: TextUnit::Sp(17.0),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    adaptive_glass_probe();
                },
            );
        },
    );
}

fn draw_stripes(scope: &mut dyn DrawScope, y: f32, height: f32) {
    let mut x = SURFACE_X;
    let mut stripe = 0usize;
    while x < SURFACE_X + SURFACE_WIDTH {
        let color = if stripe.is_multiple_of(2) {
            Color::WHITE
        } else {
            Color::BLACK
        };
        scope.draw_rect_at(
            Rect {
                x,
                y,
                width: 4.0,
                height,
            },
            Brush::solid(color),
        );
        x += 4.0;
        stripe += 1;
    }
    scope.draw_rect_at(
        Rect {
            x: SURFACE_X + 60.0,
            y,
            width: 180.0,
            height,
        },
        Brush::solid(Color::BLACK),
    );
    scope.draw_rect_at(
        Rect {
            x: RIM_PROBE_X,
            y,
            width: RIM_PROBE_WIDTH,
            height,
        },
        Brush::solid(Color::BLACK),
    );
}

#[cranpose::composable]
#[allow(non_snake_case)]
fn glass_probe(y: f32, height: f32, label: &'static str) {
    CBox(
        Modifier::empty()
            .absolute_offset(SURFACE_X, y)
            .size(Size::new(SURFACE_WIDTH, height))
            .glass_effect(probe_glass()),
        BoxSpec::default().content_alignment(Alignment::new(
            HorizontalAlignment::Start,
            VerticalAlignment::CenterVertically,
        )),
        move || {
            probe_label(label);
        },
    );
}

#[cranpose::composable]
#[allow(non_snake_case)]
fn adaptive_glass_probe() {
    CBox(
        Modifier::empty()
            .absolute_offset(SURFACE_X, ADAPTIVE_Y)
            .size(Size::new(SURFACE_WIDTH, ADAPTIVE_HEIGHT))
            .glass_effect(probe_glass().adaptive_frost(Color::WHITE, 0.65)),
        BoxSpec::default().content_alignment(Alignment::new(
            HorizontalAlignment::Start,
            VerticalAlignment::CenterVertically,
        )),
        || {
            probe_label("Library");
        },
    );
}

#[cranpose::composable]
#[allow(non_snake_case)]
fn probe_label(label: &'static str) {
    Text(
        label,
        Modifier::empty().padding_each(70.0, 0.0, 0.0, 0.0),
        TextStyle {
            span_style: SpanStyle {
                color: Some(Color::WHITE),
                font_size: TextUnit::Sp(20.0),
                ..Default::default()
            },
            ..Default::default()
        },
    );
}

fn chromatic_pixels(
    shot: &cranpose::RobotScreenshot,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> usize {
    region_pixels(shot, x, y, width, height)
        .filter(|[r, g, b]| {
            let min = (*r).min(*g).min(*b);
            let max = (*r).max(*g).max(*b);
            max - min >= 36
        })
        .count()
}

fn bright_pixels(
    shot: &cranpose::RobotScreenshot,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> usize {
    region_pixels(shot, x, y, width, height)
        .filter(|[r, g, b]| *r >= 220 && *g >= 220 && *b >= 220)
        .count()
}

fn lower_rim_depth_dp(
    shot: &cranpose::RobotScreenshot,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> f32 {
    let sy = shot.height as f32 / shot.logical_height.max(1.0);
    let y0 = ((y + height * 0.5) * sy).floor().max(0.0) as usize;
    let y1 = ((y + height) * sy).ceil().min(shot.height as f32) as usize;
    let body_luma = mean_luma(shot, x, y + height * 0.45, width, 2.0);
    let rim_threshold = body_luma + 18.0;
    let bright_rows = (y0..y1)
        .filter(|py| mean_luma(shot, x, *py as f32 / sy, width, 1.0 / sy) > rim_threshold)
        .count();
    bright_rows as f32 / sy
}

fn mean_luma(shot: &cranpose::RobotScreenshot, x: f32, y: f32, width: f32, height: f32) -> f32 {
    let mut count = 0usize;
    let sum = region_pixels(shot, x, y, width, height)
        .map(|[r, g, b]| {
            count += 1;
            0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
        })
        .sum::<f32>();
    sum / count.max(1) as f32
}

fn region_pixels(
    shot: &cranpose::RobotScreenshot,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> impl Iterator<Item = [u8; 3]> + '_ {
    let sx = shot.width as f32 / shot.logical_width.max(1.0);
    let sy = shot.height as f32 / shot.logical_height.max(1.0);
    let x0 = (x * sx).floor().max(0.0) as usize;
    let y0 = (y * sy).floor().max(0.0) as usize;
    let x1 = ((x + width) * sx).ceil().min(shot.width as f32) as usize;
    let y1 = ((y + height) * sy).ceil().min(shot.height as f32) as usize;
    (y0..y1).flat_map(move |py| {
        (x0..x1).map(move |px| {
            let index = (py * shot.width as usize + px) * 4;
            [
                shot.pixels[index],
                shot.pixels[index + 1],
                shot.pixels[index + 2],
            ]
        })
    })
}
