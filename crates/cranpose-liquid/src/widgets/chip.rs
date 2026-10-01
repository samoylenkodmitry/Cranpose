use cranpose_animation::{Animatable, AnimationSpec, AnimationType, Easing};
use cranpose_core::{RuntimeHandle, remember, with_current_composer};
use cranpose_macros::composable;
use cranpose_services::HapticFeedback;
use cranpose_ui::{Box, BoxSpec, Modifier};
use cranpose_ui_graphics::GraphicsLayer;
use cranpose_ui_layout::Alignment;

use super::button::{GlassButtonLabel, GlassButtonSize, GlassButtonSpec, GlassButtonWithFeedback};

struct ChipAppearance {
    selected: bool,
    prominent_on_top: bool,
    label: Animatable<f32>,
    tint: Animatable<f32>,
    blue_vibrancy: Animatable<f32>,
}

fn appearance_animation(delay: u64) -> AnimationType {
    AnimationType::Tween(
        AnimationSpec::tween(470, Easing::CubicBezier(0.25, 0.1, 0.25, 1.0)).with_delay(delay),
    )
}

impl ChipAppearance {
    fn new(selected: bool, runtime: RuntimeHandle) -> Self {
        Self {
            selected,
            prominent_on_top: selected,
            label: Animatable::new(f32::from(selected), runtime.clone()),
            tint: Animatable::new(f32::from(selected), runtime.clone()),
            blue_vibrancy: Animatable::new(f32::from(!selected), runtime),
        }
    }

    fn retarget(&mut self, selected: bool) {
        if self.selected == selected {
            return;
        }
        if !self.label.is_running() {
            self.prominent_on_top = selected;
            if !selected {
                self.blue_vibrancy.snapTo(0.0);
            }
        }
        if selected {
            let current = self
                .blue_vibrancy
                .state()
                .try_value()
                .expect("owned chip vibrancy");
            self.blue_vibrancy.snapTo(current);
        } else {
            self.blue_vibrancy.animateTo(1.0, appearance_animation(24));
        }
        self.label
            .animateTo(f32::from(selected), appearance_animation(16));
        self.tint
            .animateTo(f32::from(selected), appearance_animation(24));
        self.selected = selected;
    }
}

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
    let runtime = with_current_composer(|composer| composer.runtime_handle());
    let appearance = remember(move || ChipAppearance::new(selected, runtime));
    let (label_progress, tint_progress, blue_vibrancy, prominent_on_top) =
        appearance.update(|appearance| {
            appearance.retarget(selected);
            (
                appearance.label.state(),
                appearance.tint.state(),
                appearance.blue_vibrancy.state(),
                appearance.prominent_on_top,
            )
        });
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
            for prominent in [!prominent_on_top, prominent_on_top] {
                let label = label.clone();
                let layer = Modifier::empty()
                    .stable_semantics(|config| config.hidden = true)
                    .graphics_layer(move || GraphicsLayer {
                        alpha: if prominent {
                            label_progress.get()
                        } else {
                            (1.0 - label_progress.get()) * blue_vibrancy.get()
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
