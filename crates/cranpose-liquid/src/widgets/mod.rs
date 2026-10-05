//! The Liquid component set.

mod button;
mod card;
mod chip;
mod content_scope;
mod control_lens;
mod control_material;
mod control_motion;
mod floating_button;
mod glass_surface;
#[cfg(feature = "localization")]
mod language_picker;
mod lens_motion;
mod menu;
mod nav_bar;
mod search_field;
mod segmented;
mod selection;
mod slider;
mod slider_gesture;
mod slider_motion;
mod tab_bar;
mod tab_lighting;
mod toggle;
mod vibrancy;

pub use button::{
    GlassButton, GlassButtonLabel, GlassButtonSize, GlassButtonSpec, GlassButtonStyle,
    GlassIconButton, GlassIconButtonGroup, GlassIconButtonGroupItem, GlassIconButtonGroupScope,
    GlassIconButtonGroupSpec,
};
pub use card::{Card, LiquidCard, LiquidListRow, LiquidListRowSpec, LiquidListSection, Surface};
pub use chip::{LiquidActionChip, LiquidChip};
pub use glass_surface::GlassSurface;
#[cfg(feature = "localization")]
pub use language_picker::LiquidLanguagePicker;
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

#[cfg(test)]
#[path = "tests/widgets_warm_up_tests.rs"]
mod warm_up_tests;
