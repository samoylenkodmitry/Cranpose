//! Diagnostic probe for the "round liquid-glass surfaces have cramped
//! edges" defect: a circular `Glass::lens()` surface sits over a plain
//! backdrop with one small, uniquely colored marker dead center. The
//! wcKSRD source mapping pulls rim content toward the shape's optical
//! axis, so an unbounded pull lets a rim pixel sample all the way back to
//! the marker — the marker "bleeds" out to the rim, which is the same
//! defect the checkerboard/quadrant probes showed as a pinwheel swirl,
//! made into a single sharp color check instead of a visual judgment call.
//!
//! Not a robot test — a throwaway visual aid and pixel-probe used while
//! diagnosing the defect (`robot_liquid_round_glass_edge_refraction.rs` is
//! the permanent regression test). Run with:
//! ```bash
//! cargo run --package desktop-app --example round_glass_probe --features desktop,robot-app
//! ```

use std::path::PathBuf;

use cranpose::{
    liquid::prelude::*,
    widgets::{Box as CBox, BoxSpec},
    AppLauncher, Brush, Color, Modifier, Size,
};

const CANVAS: f32 = 340.0;
const CENTER: f32 = CANVAS * 0.5;
const CIRCLE_DIAMETER: f32 = 260.0;
const CIRCLE_RADIUS: f32 = CIRCLE_DIAMETER * 0.5;
const MARKER_RADIUS: f32 = 10.0;

const BACKDROP: Color = Color(0.5, 0.5, 0.5, 1.0);
const MARKER: Color = Color(1.0, 0.0, 1.0, 1.0);

#[cranpose::composable]
#[allow(non_snake_case)]
fn ProbeApp() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        CBox(
            Modifier::empty().fill_max_size().draw_behind(|scope| {
                scope.draw_rect(Brush::solid(BACKDROP));
                scope.draw_circle(
                    Brush::solid(MARKER),
                    cranpose::Point {
                        x: CENTER,
                        y: CENTER,
                    },
                    MARKER_RADIUS,
                );
            }),
            BoxSpec::default(),
            || {
                CBox(
                    Modifier::empty()
                        .offset(CENTER - CIRCLE_RADIUS, CENTER - CIRCLE_RADIUS)
                        .size(Size::new(CIRCLE_DIAMETER, CIRCLE_DIAMETER))
                        .glass_effect(
                            Glass::lens()
                                .shape(LiquidShape::Circle)
                                .blur_radius(0.0)
                                .dispersion(0.0)
                                .highlight(0.0)
                                .tint(Color::TRANSPARENT),
                        ),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });
}

fn color_distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> f32 {
    let dr = a.0 as f32 - b.0 as f32;
    let dg = a.1 as f32 - b.1 as f32;
    let db = a.2 as f32 - b.2 as f32;
    (dr * dr + dg * dg + db * db).sqrt()
}

fn main() {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/round-glass-probe".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Round Glass Probe")
        .with_size(CANVAS as u32, CANVAS as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            std::thread::sleep(std::time::Duration::from_millis(700));
            let shot = robot.screenshot().expect("shot");
            let scale = shot.width as f32 / shot.logical_width;
            let sample = |lx: f32, ly: f32| -> (u8, u8, u8) {
                let px = (lx * scale).round().clamp(0.0, shot.width as f32 - 1.0) as u32;
                let py = (ly * scale).round().clamp(0.0, shot.height as f32 - 1.0) as u32;
                let idx = ((py * shot.width + px) * 4) as usize;
                (shot.pixels[idx], shot.pixels[idx + 1], shot.pixels[idx + 2])
            };
            let marker_rgb = (
                (MARKER.r() * 255.0) as u8,
                (MARKER.g() * 255.0) as u8,
                (MARKER.b() * 255.0) as u8,
            );
            println!("angle_deg,logical_x,logical_y,r,g,b,dist_to_marker");
            let mut worst = f32::MAX;
            for step in 0..36 {
                let angle = (step as f32) * std::f32::consts::TAU / 36.0;
                let lx = CENTER + (CIRCLE_RADIUS - 1.0) * angle.cos();
                let ly = CENTER + (CIRCLE_RADIUS - 1.0) * angle.sin();
                let rgb = sample(lx, ly);
                let dist = color_distance(rgb, marker_rgb);
                worst = worst.min(dist);
                println!(
                    "{:.1},{:.1},{:.1},{},{},{},{:.1}",
                    angle.to_degrees(),
                    lx,
                    ly,
                    rgb.0,
                    rgb.1,
                    rgb.2,
                    dist
                );
            }
            println!("closest_rim_sample_to_marker_color = {worst:.1}");
            let image =
                image::RgbaImage::from_raw(shot.width, shot.height, shot.pixels).expect("decode");
            image
                .save(shot_dir.join("round-glass-probe.png"))
                .expect("save");
            robot.exit().expect("exit");
        })
        .run(ProbeApp);
}
