//! The Liquid component set.

mod button;
mod card;
mod chip;
mod content_scope;
mod glass_surface;
mod menu;
mod nav_bar;
mod search_field;
mod segmented;
mod slider;
mod tab_bar;
mod tab_lighting;
mod tab_motion;
mod toggle;
mod vibrancy;

pub use button::{
    GlassButton, GlassButtonLabel, GlassButtonSpec, GlassButtonStyle, GlassIconButton,
    GlassIconButtonGroup, GlassIconButtonGroupItem, GlassIconButtonGroupScope,
    GlassIconButtonGroupSpec,
};
pub use card::{Card, LiquidCard, LiquidListRow, LiquidListRowSpec, LiquidListSection, Surface};
pub use chip::LiquidChip;
pub use glass_surface::GlassSurface;
pub use menu::{
    LiquidDropdownMenu, LiquidDropdownMenuSpec, LiquidMenu, LiquidMenuAbsorbedIconButton,
    LiquidMenuAbsorbedSource, LiquidMenuGesture, LiquidMenuIconButton, LiquidMenuItem,
    LiquidMenuScope, LiquidMenuSpec, liquid_menu_trigger_input, rememberLiquidMenuGesture,
};
pub use nav_bar::{LiquidNavBar, LiquidNavBarSpec, liquid_nav_bar_expanded_height};
pub use search_field::{LiquidSearchField, LiquidSearchFieldSpec, SearchBar, SearchField};
pub use segmented::{LiquidSegmentedControl, LiquidSegmentedControlScope};
pub use slider::LiquidSlider;
pub use tab_bar::{
    LiquidTab, LiquidTabBar, LiquidTabBarScope, LiquidTabBarSearchAccessory, LiquidTabBarSpec,
    LiquidTabBarWithAccessory, LiquidTabIcon, LiquidTabIconStyle, tab_lens_rest_width,
    tab_lens_resting_left,
};
pub use toggle::LiquidToggle;

/// Every runtime shader the liquid widgets build at runtime, at the target
/// it draws to, for a renderer's shader warm-up: the ones the renderer
/// cannot name itself because their sources are assembled in the widgets.
pub fn shader_warm_ups() -> Vec<cranpose_ui_graphics::ShaderWarmUp> {
    tab_lighting::shader_warm_ups()
        .into_iter()
        .chain(vibrancy::shader_warm_ups())
        .collect()
}

#[cfg(test)]
mod warm_up_tests {
    use super::shader_warm_ups;

    #[test]
    fn every_widget_shader_is_listed_once() {
        let warm_ups = shader_warm_ups();
        let identities: Vec<_> = warm_ups
            .iter()
            .map(|warm_up| {
                (
                    warm_up.shader.source_hash(),
                    warm_up.shader.overrides_hash(),
                )
            })
            .collect();
        assert_eq!(identities.len(), 2);
        for (index, identity) in identities.iter().enumerate() {
            assert!(
                !identities[..index].contains(identity),
                "a shader listed twice would compile twice"
            );
        }
    }
}
