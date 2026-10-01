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
    /// Optional material override for the floating variant.
    pub glass: Option<Glass>,
    /// Foreground override for the icon, input, and placeholder. `None` uses
    /// the theme's primary color and a subdued placeholder.
    pub foreground: Option<Color>,
}

impl Default for LiquidSearchFieldSpec {
    fn default() -> Self {
        Self {
            placeholder: "Search".to_string(),
            on_glass: true,
            glass: None,
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
    let placeholder_color = spec.foreground.unwrap_or_else(|| {
        colors
            .label
            .with_alpha(if colors.is_dark { 0.35 } else { 0.40 })
    });

    let base = if spec.on_glass {
        let mut glass = spec
            .glass
            .unwrap_or_else(|| super::glass_surface::control_surface_material(foreground));
        if let Some(color) = spec.foreground {
            let frost = glass.adaptive_frost;
            glass = glass.adaptive_frost(color, frost);
        }
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
            color: Some(placeholder_color),
            ..body.span_style.clone()
        },
        ..body
    };
    let placeholder = spec.placeholder;
    BasicTextFieldDecorated(
        state,
        modifier
            .then(base)
            .height_in(44.0, f32::INFINITY)
            .padding_symmetric(11.0, 0.0)
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
                    crate::icons::Icon(crate::icons::SEARCH, None, 24.0, foreground);
                    Box(Modifier::empty().width(5.0), BoxSpec::default(), || {});
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
