use std::{cell::RefCell, rc::Rc};

use cranpose_animation::{animateColorAsState, animateFloatAsState};
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
#[allow(non_snake_case)]
pub fn LiquidChip(
    modifier: Modifier,
    selected: bool,
    on_click: impl Fn() + 'static,
    label: impl Into<String>,
) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let interaction = rememberMutableInteractionSource();
    let (pressed_modifier, _pressed, content_alpha) =
        liquid_press_scale(Modifier::empty(), interaction, 1.18);

    let label_color = animateColorAsState(
        if selected {
            colors.accent
        } else {
            colors.secondary_label
        },
        LiquidMotion::smooth(),
        "chip-label",
    );

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

    let on_click = Rc::new(RefCell::new(on_click));
    let base = base
        .press_interaction_source(interaction)
        .clickable(move |_point| {
            default_haptics().perform(HapticFeedback::Selection);
            (on_click.borrow_mut())();
        })
        .padding_symmetric(14.0, 7.0);

    let label = label.into();
    let chip = base.then(modifier);
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
                        color: Some(label_color.get()),
                        font_weight: Some(if selected {
                            FontWeight::SEMI_BOLD
                        } else {
                            FontWeight::MEDIUM
                        }),
                        ..typography.subheadline.span_style.clone()
                    },
                    ..typography.subheadline.clone()
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
