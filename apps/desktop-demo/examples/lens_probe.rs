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
const LENS_W: f32 = 58.0;
const LENS_H: f32 = 109.0 / 3.0;

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
                // Toggle gray-HOLD backdrop, exactly like the live widget at
                // 132ms: white card, light-gray track capsule, NO thumb (it
                // melts into the glass). The only content transitions are the
                // track<->card edges — the ring must be built from them.
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
                                Brush::solid(Color::from_rgb_u8(229, 229, 234)),
                                cranpose::CornerRadii::uniform(14.0),
                            );
                        }),
                    BoxSpec::default(),
                    || {},
                );
                // The lens node: the hold dome centered on the track's thumb
                // side, node padded like the widget's headroom.
                let lens = Modifier::empty()
                    .required_size(Size::new(NODE_W, NODE_H))
                    .offset(120.0, 140.0 + (28.0 - NODE_H) * 0.5)
                    .graphics_layer(move || GraphicsLayer {
                        translation_x: 24.5 + (37.0 - NODE_W) * 0.5 + 12.0,
                        ..Default::default()
                    })
                    .glass_effect_with(
                        Glass::lens()
                            .shape(LiquidShape::Capsule)
                            .tint(Color::WHITE.with_alpha(0.02))
                            .blur_radius(1.2)
                            .saturation(1.0)
                            .refraction_depth(0.34)
                            .refraction_curve(0.45)
                            .optical_zoom(1.95)
                            .transmission_refraction(1.0)
                            .meniscus_absorption(0.55)
                            .dispersion(1.0)
                            .highlight(0.04)
                            .no_clip(),
                        move || GlassDynamics {
                            morph: Some(GlassMorph {
                                node_size: (NODE_W, NODE_H),
                                primary: (NODE_W * 0.5, NODE_H * 0.5, LENS_W, LENS_H, -1.0),
                                shapes: Vec::new(),
                                glue: 0.0,
                                wobble_amplitude: 0.0,
                                wobble_phase: 0.0,
                                bulge_amplitude: 0.0,
                                bulge_direction: 0.0,
                                ellipse_blend: 0.0,
                                deformation: None,
                                zoom_anchor: (0.0, 0.0),
                            }),
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
        .run(ProbeApp);
}
