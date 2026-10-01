use cranpose_animation::animateFloatAsState;
use cranpose_macros::composable;
use cranpose_services::{HapticFeedback, default_haptics};
use cranpose_ui::{
    Modifier, rememberMutableInteractionSource,
    text::{FontWeight, SpanStyle, TextStyle},
    widgets::{Box, BoxSpec, Text},
};
use cranpose_ui_layout::Alignment;

use crate::{
    material::{Glass, GlassDynamics, LiquidModifierExt},
    motion::{LiquidMotion, liquid_press_scale},
    theme::{liquid_colors, liquid_typography},
};

/// A selectable filter pill (the Library "All / Receipts" chips). One
/// persistent glass pane: selection raises its optical activity, and an
/// unselected chip rests as a fill-washed pane that still transmits its
/// backdrop.
#[composable]
pub fn LiquidChip(
    modifier: Modifier,
    selected: bool,
    on_click: impl Fn() + 'static,
    label: impl Into<String>,
) {
    let label = label.into();
    let colors = liquid_colors();
    let typography = liquid_typography();
    let interaction = rememberMutableInteractionSource();
    let (pressed_modifier, _pressed, content_alpha) =
        liquid_press_scale(Modifier::empty(), interaction, 1.18);

    let label_color = colors.accent;

    let activity = animateFloatAsState(
        if selected { 1.0 } else { 0.0 },
        LiquidMotion::smooth(),
        "chip-activity",
    );
    let fill = colors.fill;
    let base = Modifier::empty().glass_effect_with(
        Glass::regular().adaptive_frost(colors.accent, 0.65),
        move || GlassDynamics {
            activity: Some(activity.get()),
            resting_tint: Some(fill),
            ..Default::default()
        },
    );

    let base = base
        .press_interaction_source(interaction)
        .stable_semantics(move |config| config.selected = Some(selected))
        .clickable(move |_point| {
            default_haptics().perform(HapticFeedback::Selection);
            on_click();
        })
        .padding_symmetric(13.0, 6.0);

    let chip = modifier.then(base);
    Box(pressed_modifier, BoxSpec::default(), move || {
        let label = label.clone();
        let typography = typography.clone();
        Box(
            chip.clone(),
            BoxSpec::default().content_alignment(Alignment::CENTER),
            move || {
                let label = label.clone();
                let style = TextStyle {
                    span_style: SpanStyle {
                        color: Some(label_color),
                        font_weight: Some(if selected {
                            FontWeight::SEMI_BOLD
                        } else {
                            FontWeight::NORMAL
                        }),
                        ..typography.footnote.span_style.clone()
                    },
                    ..typography.footnote.clone()
                };
                let content_layer =
                    Modifier::empty().graphics_layer(move || cranpose_ui_graphics::GraphicsLayer {
                        alpha: content_alpha.get().clamp(0.0, 1.0),
                        ..Default::default()
                    });
                Text(label, content_layer, style);
            },
        );
    });
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
