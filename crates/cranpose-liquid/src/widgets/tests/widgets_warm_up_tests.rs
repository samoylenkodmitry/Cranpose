use cranpose_ui::Modifier;
use cranpose_ui_graphics::shader_warm_ups_after;

use super::{tab_lighting, vibrancy};
use crate::prelude::*;

#[test]
fn composing_a_tab_bar_requests_its_lighting_and_ink_shaders() {
    let mut rule = cranpose_testing::ComposeTestRule::new();
    rule.set_content(|| {
        LiquidTheme(LiquidThemeSpec::default(), || {
            LiquidTabBar(
                Modifier::empty(),
                LiquidTabBarSpec::default(),
                0,
                |_| {},
                |tabs| {
                    tabs.tab(crate::icons::STAR, "Discover");
                    tabs.tab(crate::icons::BOOKMARK, "Saved");
                },
            );
        });
    })
    .expect("compose tabs");

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
}
