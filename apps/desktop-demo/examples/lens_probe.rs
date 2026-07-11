//! Minimal lens-glass probe: one morphing lens node overhanging a two-tone
//! background, captured at several times after mount. Isolates the pressed
//! toggle's "white box" overhang artifact from the demo page.

use cranpose::liquid::material::GlassMorph;
use cranpose::liquid::prelude::*;
use cranpose::widgets::{Box as CBox, BoxSpec};
use cranpose::{AppLauncher, Brush, Color, GraphicsLayer, Modifier, Size};
use std::path::PathBuf;
use std::time::Duration;

const NODE_W: f32 = 78.0;
const NODE_H: f32 = 59.0;

#[cranpose::composable]
#[allow(non_snake_case)]
fn ProbeApp() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        CBox(
            Modifier::empty().fill_max_size().draw_behind(|scope| {
                scope.draw_rect(Brush::solid(Color::from_rgb_u8(240, 240, 244)));
            }),
            BoxSpec::default(),
            || {
                // Two-tone backdrop: green capsule (the "track") on white card.
                CBox(
                    Modifier::empty()
                        .offset(80.0, 100.0)
                        .size(Size::new(200.0, 120.0))
                        .draw_behind(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color::from_rgb_u8(255, 255, 255)),
                                cranpose::CornerRadii::uniform(16.0),
                            );
                        }),
                    BoxSpec::default(),
                    || {},
                );
                CBox(
                    Modifier::empty()
                        .offset(120.0, 140.0)
                        .size(Size::new(63.0, 28.0))
                        .draw_behind(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color::from_rgb_u8(52, 199, 89)),
                                cranpose::CornerRadii::uniform(14.0),
                            );
                        }),
                    BoxSpec::default(),
                    || {},
                );
                // The lens node: overhangs the green capsule's right end,
                // exactly like the pressed toggle lens.
                let lens = Modifier::empty()
                    .required_size(Size::new(NODE_W, NODE_H))
                    .offset(120.0, 140.0 + (28.0 - NODE_H) * 0.5)
                    .graphics_layer(move || GraphicsLayer {
                        translation_x: 24.5 + (37.0 - NODE_W) * 0.5 + 12.0,
                        ..Default::default()
                    })
                    .glass_effect_with(
                        Glass::lens().shape(LiquidShape::Capsule).no_clip(),
                        move || GlassDynamics {
                            morph: Some(GlassMorph {
                                node_size: (NODE_W, NODE_H),
                                primary: (NODE_W * 0.5, NODE_H * 0.5, 58.0, 39.0, -1.0),
                                shapes: Vec::new(),
                                glue: 0.0,
                                wobble_amplitude: 0.0,
                                wobble_phase: 0.0,
                                bulge_amplitude: 0.0,
                                bulge_direction: 0.0,
                            }),
                            magnify_boost: 0.25,
                            ..Default::default()
                        },
                    );
                CBox(lens, BoxSpec::default(), || {});
            },
        );
    });
}

fn main() {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/lens-probe".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Lens Probe")
        .with_size(400, 320)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            for (label, wait_ms) in [("t060", 60u64), ("t250", 190), ("t800", 550)] {
                std::thread::sleep(Duration::from_millis(wait_ms));
                let shot = robot.screenshot().expect("shot");
                let image = image::RgbaImage::from_raw(shot.width, shot.height, shot.pixels)
                    .expect("decode");
                image
                    .save(shot_dir.join(format!("{label}.png")))
                    .expect("save");
            }
            robot.exit().expect("exit");
        })
        .run(|| ProbeApp());
}
