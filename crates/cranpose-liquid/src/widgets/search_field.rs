//! A pill search field on glass.

use cranpose_foundation::text::TextFieldState;
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    text::{SpanStyle, TextStyle},
    widgets::{BasicTextFieldDecorated, BasicTextFieldOptions, Box, BoxSpec, Row, RowSpec, Text},
};
use cranpose_ui_layout::{Alignment, VerticalAlignment};

use crate::{
    material::{Glass, LiquidModifierExt},
    theme::{liquid_colors, liquid_typography},
};

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
            let body = typography.body.clone();
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    crate::icons::Icon(crate::icons::SEARCH, 18.0, colors.secondary_label);
                    Box(Modifier::empty().width(8.0), BoxSpec::default(), || {});

                    let field_style = TextStyle {
                        span_style: SpanStyle {
                            color: Some(colors.label),
                            ..body.span_style.clone()
                        },
                        ..body.clone()
                    };
                    let placeholder_style = TextStyle {
                        span_style: SpanStyle {
                            color: Some(colors.secondary_label),
                            ..body.span_style.clone()
                        },
                        ..body.clone()
                    };
                    let placeholder = placeholder.clone();
                    // The field must fill the pill so it has a non-zero width
                    // (and stays hit-testable) even when empty — otherwise an
                    // empty search field measures to 0px and rejects every tap,
                    // so focus + the soft keyboard never fire.
                    Box(
                        Modifier::empty().weight(1.0),
                        BoxSpec::default(),
                        move || {
                            let placeholder = placeholder.clone();
                            let placeholder_style = placeholder_style.clone();
                            BasicTextFieldDecorated(
                                state,
                                Modifier::empty().fill_max_width(),
                                BasicTextFieldOptions {
                                    text_style: field_style.clone(),
                                    ..BasicTextFieldOptions::default()
                                },
                                move |inner| {
                                    let empty = state.text().is_empty();
                                    let placeholder = placeholder.clone();
                                    let placeholder_style = placeholder_style.clone();
                                    Box(
                                        Modifier::empty().fill_max_width(),
                                        BoxSpec::default().content_alignment(Alignment::new(
                                            cranpose_ui_layout::HorizontalAlignment::Start,
                                            cranpose_ui_layout::VerticalAlignment::CenterVertically,
                                        )),
                                        move || {
                                            if empty {
                                                Text(
                                                    placeholder.clone(),
                                                    Modifier::empty(),
                                                    placeholder_style.clone(),
                                                );
                                            }
                                            inner.inner_text_field();
                                        },
                                    )
                                },
                            );
                        },
                    );
                },
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_spec_defaults_to_a_search_label_and_glass() {
        let spec = LiquidSearchFieldSpec::default();
        assert_eq!(spec.placeholder, "Search");
        assert!(spec.on_glass);
    }
}

/// The themed search field with the conventional Compose name.
#[composable]
#[allow(non_snake_case)]
pub fn SearchBar(modifier: Modifier, state: TextFieldState, placeholder: impl Into<String>) {
    LiquidSearchField(
        modifier,
        state,
        LiquidSearchFieldSpec {
            placeholder: placeholder.into(),
            ..Default::default()
        },
    );
}

/// Field-oriented name for [`SearchBar`].
#[composable]
#[allow(non_snake_case)]
pub fn SearchField(modifier: Modifier, state: TextFieldState, placeholder: impl Into<String>) {
    SearchBar(modifier, state, placeholder);
}
