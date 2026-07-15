//! Filter chip: a pill that lifts onto glass when selected.

use crate::material::{Glass, LiquidModifierExt};
use crate::motion::liquid_press_scale;
use crate::theme::{liquid_colors, liquid_typography};
use cranpose_animation::animateColorAsState;
use cranpose_macros::composable;
use cranpose_services::{default_haptics, HapticFeedback};
use cranpose_ui::rememberMutableInteractionSource;
use cranpose_ui::text::FontWeight;
use cranpose_ui::text::{SpanStyle, TextStyle};
use cranpose_ui::widgets::{Box, BoxSpec, Text};
use cranpose_ui::Modifier;
use cranpose_ui_graphics::{Brush, CornerRadii};
use cranpose_ui_layout::Alignment;
use std::cell::RefCell;
use std::rc::Rc;

use crate::motion::LiquidMotion;

/// A selectable filter pill (the Library "All / Receipts" chips). Selected
/// chips render on glass; unselected ones sit on the fill color.
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
        liquid_press_scale(Modifier::empty(), interaction.clone(), 1.18);

    let label_color = animateColorAsState(
        if selected {
            colors.accent
        } else {
            colors.secondary_label
        },
        LiquidMotion::smooth(),
        "chip-label",
    );

    let mut base = Modifier::empty();
    if selected {
        base = base.glass_effect(Glass::regular().adaptive_frost(colors.accent, 0.65));
    } else {
        let fill = colors.fill;
        base = base.clip_to_bounds().draw_behind(move |scope| {
            scope.draw_round_rect(Brush::solid(fill), CornerRadii::uniform(999.0));
        });
    }

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
