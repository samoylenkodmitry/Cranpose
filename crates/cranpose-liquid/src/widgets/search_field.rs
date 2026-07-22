//! A pill search field on glass.

use crate::material::Glass;
use crate::material::LiquidModifierExt;
use crate::theme::{liquid_colors, liquid_typography};
use cranpose_foundation::text::TextFieldState;
use cranpose_macros::composable;
use cranpose_ui::text::{SpanStyle, TextStyle};
use cranpose_ui::widgets::{BasicTextField, Box, BoxSpec, Row, RowSpec, Text};
use cranpose_ui::Modifier;
use cranpose_ui_layout::{Alignment, VerticalAlignment};

/// Configuration for [`LiquidSearchField`].
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidSearchFieldSpec {
    pub placeholder: String,
    /// Render on glass (floating) instead of the flat fill (inline in lists).
    pub on_glass: bool,
}

impl Default for LiquidSearchFieldSpec {
    fn default() -> Self {
        Self {
            placeholder: "Search".to_string(),
            on_glass: true,
        }
    }
}

/// A capsule search field: magnifier icon and editable text.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidSearchField(modifier: Modifier, state: TextFieldState, spec: LiquidSearchFieldSpec) {
    let colors = liquid_colors();
    let typography = liquid_typography();

    let base = if spec.on_glass {
        Modifier::empty().glass_effect(Glass::regular())
    } else {
        let fill = colors.fill;
        Modifier::empty()
            .rounded_corners(999.0)
            .draw_behind(move |scope| {
                scope.draw_round_rect(
                    cranpose_ui_graphics::Brush::solid(fill),
                    cranpose_ui_graphics::CornerRadii::uniform(999.0),
                );
            })
    };

    let placeholder = spec.placeholder.clone();
    Box(
        base.then(modifier).padding_symmetric(14.0, 9.0),
        BoxSpec::default().content_alignment(Alignment::new(
            cranpose_ui_layout::HorizontalAlignment::Start,
            cranpose_ui_layout::VerticalAlignment::CenterVertically,
        )),
        move || {
            let placeholder = placeholder.clone();
            let typography = typography.clone();
            let state = state.clone();
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    crate::icons::Icon(crate::icons::SEARCH, 18.0, colors.secondary_label);
                    Box(Modifier::empty().width(8.0), BoxSpec::default(), || {});

                    let is_empty = state.text().is_empty();
                    let field_style = TextStyle {
                        span_style: SpanStyle {
                            color: Some(colors.label),
                            ..typography.body.span_style.clone()
                        },
                        ..typography.body.clone()
                    };
                    let placeholder = placeholder.clone();
                    let placeholder_typography = typography.clone();
                    let state = state.clone();
                    // The field must fill the pill so it has a non-zero width
                    // (and stays hit-testable) even when empty — otherwise an
                    // empty search field measures to 0px and rejects every tap,
                    // so focus + the soft keyboard never fire.
                    Box(Modifier::empty().weight(1.0), BoxSpec::default(), move || {
                        if is_empty {
                            let placeholder_style = TextStyle {
                                span_style: SpanStyle {
                                    color: Some(colors.tertiary_label),
                                    ..placeholder_typography.body.span_style.clone()
                                },
                                ..placeholder_typography.body.clone()
                            };
                            Text(placeholder.clone(), Modifier::empty(), placeholder_style);
                        }
                        BasicTextField(
                            state.clone(),
                            Modifier::empty().fill_max_width(),
                            field_style.clone(),
                        );
                    });
                },
            );
        },
    );
}
