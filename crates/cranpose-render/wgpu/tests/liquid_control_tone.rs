use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_liquid::prelude::*;
use cranpose_macros::composable;
use cranpose_render_common::Renderer;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::Color;

use crate::support;

#[composable]
fn NeutralControls(dark: bool, card: bool) {
    LiquidTheme(
        LiquidThemeSpec {
            scheme: if dark {
                SchemeMode::Dark
            } else {
                SchemeMode::Light
            },
            ..Default::default()
        },
        move || {
            for (index, gray) in [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().enumerate() {
                Box(
                    Modifier::empty()
                        .offset(index as f32 * 360.0, 0.0)
                        .width(360.0)
                        .height(240.0)
                        .background(Color::rgba(gray, gray, gray, 1.0)),
                    BoxSpec::default(),
                    move || {
                        if card {
                            LiquidCard(
                                Modifier::empty()
                                    .offset(30.0, 40.0)
                                    .width(300.0)
                                    .height(160.0),
                                || {},
                            );
                        } else {
                            GlassButton(
                                Modifier::empty().offset(126.0, 95.0).width(108.0),
                                GlassButtonSpec::glass().with_size(GlassButtonSize::Large),
                                || {},
                                || {},
                            );
                        }
                    },
                );
            }
        },
    );
}

#[test]
fn control_gray_responses_follow_the_native_simulator_samples() {
    for (dark, card, expected) in [
        (false, true, [136u8, 171, 203, 232, 254]),
        (true, true, [32, 81, 116, 129, 122]),
        (false, false, [130, 161, 192, 221, 249]),
        (true, false, [31, 88, 132, 163, 182]),
    ] {
        let (_lock, mut renderer) = support::headless_renderer_parts().expect("material renderer");
        let context = cranpose_ui::AppContext::new();
        renderer.attach_app_context_services(&context);
        let mut shell = AppShell::new(
            renderer,
            location_key(file!(), line!(), column!()),
            move || NeutralControls(dark, card),
        );
        shell.set_viewport(1800.0, 240.0);
        shell.set_buffer_size(1800, 240);
        shell.update();
        shell.update();
        let frame = shell
            .renderer()
            .capture_frame(1800, 240)
            .expect("control ramp");
        for (index, expected) in expected.into_iter().enumerate() {
            let pixel = (120 * 1800 + index * 360 + 180) * 4;
            let actual = &frame.pixels[pixel..pixel + 3];
            assert!(
                actual.iter().all(|value| value.abs_diff(expected) <= 8),
                "native neutral response: dark={dark} card={card} sample={index}, expected={expected}, actual={actual:?}"
            );
        }
        assert_eq!(shell.renderer().device_error_count_for_tests(), 0);
    }
}

#[test]
fn chip_transition_endpoints_preserve_the_clear_and_prominent_materials() {
    for (dark, selected) in [(false, false), (false, true), (true, false), (true, true)] {
        let (_lock, mut renderer) = support::headless_renderer_parts().expect("chip renderer");
        let context = cranpose_ui::AppContext::new();
        renderer.attach_app_context_services(&context);
        let mut shell = AppShell::new(
            renderer,
            location_key(file!(), line!(), column!()),
            move || {
                LiquidTheme(
                    LiquidThemeSpec {
                        scheme: if dark {
                            SchemeMode::Dark
                        } else {
                            SchemeMode::Light
                        },
                        ..Default::default()
                    },
                    move || {
                        for (index, color) in [
                            Color::BLACK,
                            Color::WHITE,
                            Color::rgba(0.25, 0.5, 0.75, 1.0),
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            Box(
                                Modifier::empty()
                                    .offset(index as f32 * 240.0, 0.0)
                                    .size_points(240.0, 80.0)
                                    .background(color),
                                BoxSpec::default(),
                                move || {
                                    let spec = if selected {
                                        GlassButtonSpec::prominent()
                                    } else {
                                        GlassButtonSpec::glass()
                                    }
                                    .with_size(GlassButtonSize::Small);
                                    GlassButton(
                                        Modifier::empty()
                                            .offset(10.0, 20.0)
                                            .size_points(100.0, 28.0),
                                        spec,
                                        || {},
                                        || {},
                                    );
                                    LiquidChip(
                                        Modifier::empty()
                                            .offset(130.0, 20.0)
                                            .size_points(100.0, 28.0),
                                        selected,
                                        || {},
                                        "",
                                    );
                                },
                            );
                        }
                    },
                );
            },
        );
        shell.set_viewport(720.0, 80.0);
        shell.set_buffer_size(720, 80);
        shell.update();
        shell.update();
        let frame = shell
            .renderer()
            .capture_frame(720, 80)
            .expect("chip material endpoints");
        for index in 0..3 {
            let center = (34 * 720 + index * 240 + 60) * 4;
            let chip = center + 120 * 4;
            assert_eq!(
                &frame.pixels[center..center + 4],
                &frame.pixels[chip..chip + 4],
                "chip material endpoint differs: dark={dark}, selected={selected}, background={index}"
            );
        }
        assert_eq!(shell.renderer().device_error_count_for_tests(), 0);
    }
}
