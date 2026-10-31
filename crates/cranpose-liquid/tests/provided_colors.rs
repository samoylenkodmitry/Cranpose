use cranpose_core::CompositionLocalProvider;
use cranpose_liquid::prelude::*;
use cranpose_services::{
    AccessibilityOptions, SystemTheme, local_accessibility_options, local_system_theme,
};
use cranpose_ui::{local_on_light_surface, run_test_composition};
use cranpose_ui_graphics::Color;

fn custom() -> LiquidColors {
    let mut colors = LiquidColors::dark(Color::from_rgb_u8(176, 230, 210));
    colors.background = Color::from_rgb_u8(10, 40, 30);
    colors.on_accent = Color::BLACK;
    colors.glass_tint = Color::from_rgba_u8(20, 80, 60, 80);
    colors
}

fn with_options(options: AccessibilityOptions, content: impl FnOnce()) {
    CompositionLocalProvider([local_accessibility_options().provides(options)], content);
}

fn with_palette(palette: LiquidPalette, content: impl FnOnce()) {
    LiquidTheme(
        LiquidThemeSpec {
            palette,
            ..Default::default()
        },
        content,
    );
}

#[test]
fn custom_colors_reach_controls_and_restore_the_parent_theme() {
    for scheme in [SchemeMode::Light, SchemeMode::Dark] {
        run_test_composition(move || {
            let spec = LiquidThemeSpec::system_with_accent(scheme, Color::BLUE);
            LiquidTheme(spec, || {
                let original = liquid_colors();
                let colors = custom();
                with_palette(LiquidPalette::Custom(colors), || {
                    assert_eq!(liquid_colors(), colors);
                    assert!(!local_on_light_surface().current());
                    assert_eq!(liquid_segmented_style().label, colors.label);
                    GlassButtonLabel("Custom theme".into(), GlassButtonSpec::prominent());
                });
                assert_eq!(liquid_colors(), original);
                assert_eq!(local_on_light_surface().current(), !original.is_dark);
            });
        });
    }
}

#[test]
fn custom_palette_honors_accessibility_contrast() {
    run_test_composition(|| {
        with_options(
            AccessibilityOptions {
                increase_contrast: true,
                ..Default::default()
            },
            || {
                let colors = custom();
                with_palette(LiquidPalette::Custom(colors), || {
                    let shown = liquid_colors();
                    assert!(shown.secondary_label.a() > colors.secondary_label.a());
                    assert!(shown.tertiary_label.a() > colors.tertiary_label.a());
                    assert_eq!(shown.background, colors.background);
                    assert_eq!(shown.accent, colors.accent);
                });
            },
        );
    });
}

#[test]
fn custom_inversion_preserves_opacity_and_sets_text_selection_appearance() {
    run_test_composition(|| {
        with_options(
            AccessibilityOptions {
                invert_colors: true,
                ..Default::default()
            },
            || {
                let colors = custom();
                with_palette(LiquidPalette::Custom(colors), || {
                    let shown = liquid_colors();
                    for (before, after) in [
                        (colors.background, shown.background),
                        (colors.accent, shown.accent),
                        (colors.glass_tint, shown.glass_tint),
                    ] {
                        assert_eq!(
                            after,
                            Color(1. - before.0, 1. - before.1, 1. - before.2, before.3)
                        );
                    }
                    assert_eq!(shown.label, Color::BLACK);
                    assert_eq!(shown.on_accent, Color::WHITE);
                    assert!(!shown.is_dark);
                    assert!(local_on_light_surface().current());
                });
            },
        );
    });
}

#[test]
fn accessibility_options_apply_together_to_both_palette_sources() {
    for palette in [
        LiquidPalette::System {
            scheme: SchemeMode::Dark,
            accent: Color::BLUE,
        },
        LiquidPalette::Custom(custom()),
    ] {
        run_test_composition(move || {
            with_options(
                AccessibilityOptions {
                    invert_colors: true,
                    increase_contrast: true,
                    bold_text: true,
                    ..Default::default()
                },
                || {
                    let amount = GlassTintAmount::from_percent(25.).unwrap();
                    LiquidTheme(
                        LiquidThemeSpec {
                            palette,
                            glass_tint_amount: amount,
                            ..Default::default()
                        },
                        || {
                            let colors = liquid_colors();
                            assert!(!colors.is_dark);
                            assert_eq!(colors.label, Color::BLACK);
                            assert!(colors.secondary_label.a() >= 0.9);
                            assert_eq!(
                                liquid_typography().body.span_style.font_weight,
                                Some(cranpose_ui::text::FontWeight::SEMI_BOLD)
                            );
                            assert_eq!(liquid_glass_tint_amount(), amount);
                            assert!(local_on_light_surface().current());
                            GlassButtonLabel(
                                "Accessible theme".into(),
                                GlassButtonSpec::prominent(),
                            );
                        },
                    );
                },
            );
        });
    }
}

#[test]
fn system_auto_uses_system_appearance_and_inverts_once() {
    for system in [SystemTheme::Light, SystemTheme::Dark] {
        run_test_composition(move || {
            CompositionLocalProvider([local_system_theme().provides(system)], || {
                with_options(
                    AccessibilityOptions {
                        invert_colors: true,
                        ..Default::default()
                    },
                    || {
                        LiquidTheme(LiquidThemeSpec::system(SchemeMode::Auto), || {
                            assert_eq!(liquid_colors().is_dark, system == SystemTheme::Light);
                            assert_eq!(liquid_colors().accent, Color::from_rgb_u8(0, 122, 255));
                            assert_eq!(
                                local_on_light_surface().current(),
                                system == SystemTheme::Dark
                            );
                        });
                    },
                );
            });
        });
    }
}

#[test]
fn increased_contrast_keeps_custom_opaque_labels_opaque() {
    run_test_composition(|| {
        with_options(
            AccessibilityOptions {
                increase_contrast: true,
                ..Default::default()
            },
            || {
                let colors = LiquidColors {
                    secondary_label: Color::WHITE,
                    tertiary_label: Color::WHITE,
                    ..custom()
                };
                with_palette(LiquidPalette::Custom(colors), || {
                    assert_eq!(liquid_colors().secondary_label, Color::WHITE);
                    assert_eq!(liquid_colors().tertiary_label, Color::WHITE);
                });
            },
        );
    });
}
