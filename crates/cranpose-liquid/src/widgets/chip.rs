use cranpose_animation::{AnimationSpec, AnimationType, Easing, animateFloatAsState};
use cranpose_macros::composable;
use cranpose_services::HapticFeedback;
use cranpose_ui::{Box, BoxSpec, Modifier};
use cranpose_ui_graphics::GraphicsLayer;
use cranpose_ui_layout::Alignment;

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
    let appearance = |delay| {
        AnimationType::Tween(
            AnimationSpec::tween(470, Easing::CubicBezier(0.25, 0.1, 0.25, 1.0)).with_delay(delay),
        )
    };
    let label_progress = animateFloatAsState(f32::from(selected), appearance(16), "chip-label");
    let tint_progress = animateFloatAsState(f32::from(selected), appearance(24), "chip-tint");
    let description = label.clone();
    GlassButtonWithFeedback(
        modifier.stable_semantics(move |config| {
            config.selected = Some(selected);
            config.content_description = Some(description.clone());
        }),
        GlassButtonSpec::prominent().with_size(GlassButtonSize::Small),
        HapticFeedback::Selection,
        Some(tint_progress),
        on_click,
        move || {
            for prominent in [!selected, selected] {
                let label = label.clone();
                let layer = Modifier::empty()
                    .stable_semantics(|config| config.hidden = true)
                    .graphics_layer(move || GraphicsLayer {
                        alpha: if prominent {
                            label_progress.get()
                        } else {
                            (1.0 - label_progress.get())
                                * if selected {
                                    1.0
                                } else {
                                    1.0 - tint_progress.get()
                                }
                        },
                        ..Default::default()
                    });
                Box(
                    layer,
                    BoxSpec::default().content_alignment(Alignment::CENTER),
                    move || {
                        let spec = if prominent {
                            GlassButtonSpec::prominent()
                        } else {
                            GlassButtonSpec::glass()
                        };
                        GlassButtonLabel(label.clone(), spec.with_size(GlassButtonSize::Small));
                    },
                );
            }
        },
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
