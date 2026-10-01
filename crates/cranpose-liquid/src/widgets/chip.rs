use cranpose_macros::composable;
use cranpose_services::HapticFeedback;
use cranpose_ui::Modifier;

use super::button::{GlassButtonLabel, GlassButtonSize, GlassButtonSpec, GlassButtonWithFeedback};

/// A selectable compact glass button. Selection fills its glass body with
/// the accent color and changes its label to the contrasting foreground.
#[composable]
pub fn LiquidChip(
    modifier: Modifier,
    selected: bool,
    on_click: impl Fn() + 'static,
    label: impl Into<String>,
) {
    let label = label.into();
    let spec = if selected {
        GlassButtonSpec::prominent()
    } else {
        GlassButtonSpec::glass()
    }
    .with_size(GlassButtonSize::Small);
    GlassButtonWithFeedback(
        modifier.stable_semantics(move |config| config.selected = Some(selected)),
        spec.clone(),
        HapticFeedback::Selection,
        on_click,
        move || GlassButtonLabel(label.clone(), spec.clone()),
    );
}

/// A small glass action button with no selected state: Save, Cancel, Retry.
/// `prominent` fills the surface with the accent color.
#[composable]
pub fn LiquidActionChip(
    modifier: Modifier,
    prominent: bool,
    on_click: impl Fn() + 'static,
    label: impl Into<String>,
) {
    let spec = if prominent {
        super::button::GlassButtonSpec::prominent()
    } else {
        super::button::GlassButtonSpec::glass()
    }
    .with_size(super::button::GlassButtonSize::Small);
    let label = label.into();
    super::button::GlassButton(modifier, spec.clone(), on_click, move || {
        super::button::GlassButtonLabel(label.clone(), spec.clone());
    });
}
