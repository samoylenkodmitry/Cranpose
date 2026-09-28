use cranpose_foundation::text::TextFieldState;
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    text::{SpanStyle, TextStyle},
    widgets::{BasicTextFieldDecorated, BasicTextFieldOptions, Box, BoxSpec, Row, RowSpec, Text},
};
use cranpose_ui_graphics::Color;
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
    /// Material used by the floating variant.
    pub glass: Glass,
    /// Foreground override for the icon, input, and placeholder. `None` uses
    /// the theme's primary/secondary search colors.
    pub foreground: Option<Color>,
}

impl Default for LiquidSearchFieldSpec {
    fn default() -> Self {
        Self {
            placeholder: "Search".to_string(),
            on_glass: true,
            glass: Glass::regular(),
            foreground: None,
        }
    }
}

/// A capsule search field: magnifier icon and editable text.
#[composable]
pub fn LiquidSearchField(modifier: Modifier, state: TextFieldState, spec: LiquidSearchFieldSpec) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let foreground = spec.foreground.unwrap_or(colors.label);
    let secondary_foreground = spec.foreground.unwrap_or(colors.secondary_label);

    let base = if spec.on_glass {
        let glass = spec
            .foreground
            .map(|color| {
                spec.glass
                    .clone()
                    .adaptive_frost(color, spec.glass.adaptive_frost)
            })
            .unwrap_or_else(|| spec.glass.clone());
        Modifier::empty().glass_effect(glass)
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

    let body = typography.body;
    let field_style = TextStyle {
        span_style: SpanStyle {
            color: Some(foreground),
            ..body.span_style.clone()
        },
        ..body.clone()
    };
    let placeholder_style = TextStyle {
        span_style: SpanStyle {
            color: Some(secondary_foreground),
            ..body.span_style.clone()
        },
        ..body
    };
    let placeholder = spec.placeholder;
    BasicTextFieldDecorated(
        state,
        modifier
            .then(base)
            .padding_symmetric(14.0, 0.0)
            .role(cranpose_ui::SemanticsWidgetRole::SearchField)
            .content_description(placeholder.clone()),
        BasicTextFieldOptions {
            text_style: field_style,
            ..BasicTextFieldOptions::default()
        },
        move |inner| {
            let placeholder = placeholder.clone();
            let placeholder_style = placeholder_style.clone();
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    crate::icons::Icon(crate::icons::SEARCH, None, 18.0, secondary_foreground);
                    Box(Modifier::empty().width(8.0), BoxSpec::default(), || {});
                    let placeholder = placeholder.clone();
                    let placeholder_style = placeholder_style.clone();
                    let inner = inner.clone();
                    Box(
                        Modifier::empty().weight(1.0).padding_symmetric(0.0, 9.0),
                        BoxSpec {
                            content_alignment: Alignment::new(
                                cranpose_ui_layout::HorizontalAlignment::Start,
                                cranpose_ui_layout::VerticalAlignment::CenterVertically,
                            ),
                            propagate_min_constraints: true,
                        },
                        move || {
                            if state.text().is_empty() {
                                Text(
                                    placeholder.clone(),
                                    Modifier::empty().hide_from_accessibility(),
                                    placeholder_style.clone(),
                                );
                            }
                            inner.inner_text_field();
                        },
                    );
                },
            );
        },
    );
}

#[cfg(test)]
#[path = "tests/search_field_tests.rs"]
mod tests;

/// The themed search field with the conventional Compose name.
#[composable]
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
pub fn SearchField(modifier: Modifier, state: TextFieldState, placeholder: impl Into<String>) {
    SearchBar(modifier, state, placeholder);
}
