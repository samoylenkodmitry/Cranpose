use cranpose_ui::Modifier;
use cranpose_ui_graphics::{LIQUID_GLASS_WGSL, ShaderTarget, shader_warm_ups_after};

use super::{tab_lighting, vibrancy};
use crate::prelude::*;

#[test]
fn composing_a_tab_bar_requests_the_shaders_its_touch_draws() {
    // The bar composes its lighting and cells where it measures them, so
    // the first frame, not composition alone, asks for their pipelines.
    let mut robot = cranpose_testing::create_headless_robot_test(400, 120, || {
        LiquidTheme(LiquidThemeSpec::default(), || {
            LiquidTabBar(
                Modifier::empty(),
                LiquidTabBarSpec::default(),
                0,
                |_| {},
                |tabs| {
                    tabs.tab(icons::STAR, "Discover");
                    tabs.tab(icons::BOOKMARK, "Saved");
                },
            );
        });
    });
    robot.wait_for_idle();

    let requested = shader_warm_ups_after(0);
    for warm_up in tab_lighting::shader_warm_ups()
        .into_iter()
        .chain(vibrancy::shader_warm_ups())
    {
        assert!(
            requested.contains(&warm_up),
            "a composed tab bar must ask for the pipelines its press and ink draw with"
        );
    }
    // A touch strains the lens: one glass layer draws into a texture and
    // the other over the page.
    for target in [ShaderTarget::Layer, ShaderTarget::Page] {
        assert!(
            requested
                .iter()
                .any(|warm_up| warm_up.target == target
                    && warm_up.shader.source() == LIQUID_GLASS_WGSL),
            "a composed tab bar must ask for its touched lens's glass at {target:?}"
        );
    }
}
