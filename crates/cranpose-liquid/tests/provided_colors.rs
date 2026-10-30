use cranpose_liquid::prelude::*;
use cranpose_ui::{local_on_light_surface, run_test_composition};
use cranpose_ui_graphics::Color;

#[test]
fn custom_colors_reach_controls_and_restore_the_parent_theme() {
    run_test_composition(|| {
        LiquidTheme(LiquidThemeSpec::default(), || {
            let original = liquid_colors();
            let typography = liquid_typography();
            let tint = liquid_glass_tint_amount();
            let mut custom = LiquidColors::dark(Color::from_rgb_u8(176, 230, 210));
            custom.background = Color::from_rgb_u8(10, 40, 30);
            custom.on_accent = Color::BLACK;
            ProvideLiquidColors(custom, || {
                assert_eq!(liquid_colors(), custom);
                assert!(!local_on_light_surface().current());
                assert_eq!(liquid_typography(), typography);
                assert_eq!(liquid_glass_tint_amount(), tint);
                assert_eq!(liquid_segmented_style().label, custom.label);
                GlassButtonLabel("Custom theme".into(), GlassButtonSpec::prominent());
            });
            assert_eq!(liquid_colors(), original);
            assert!(local_on_light_surface().current());
        });
    });
}
