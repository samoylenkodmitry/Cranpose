use std::{cell::RefCell, rc::Rc};

use cranpose_animation::animateColorAsState;
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, rememberMutableInteractionSource,
    text::{SpanStyle, TextStyle},
    widgets::{Box, BoxSpec, Column, ColumnSpec, Text},
};
use cranpose_ui_graphics::{Brush, CornerRadii};

use crate::{
    material::{Glass, GlassDynamics, LiquidModifierExt, LiquidShape},
    motion::LiquidMotion,
    theme::{liquid_colors, liquid_typography},
};

const CARD_RADIUS: f32 = 20.0;

/// Theme-aware surface container using the current Liquid color roles.
#[composable]
#[allow(non_snake_case)]
pub fn Surface(modifier: Modifier, content: impl FnMut() + 'static) {
    let color = liquid_colors().surface;
    Box(
        Modifier::empty()
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(color), CornerRadii::uniform(12.0));
            })
            .then(modifier),
        BoxSpec::default(),
        content,
    );
}

/// Theme-aware elevated card container.
#[composable]
#[allow(non_snake_case)]
pub fn Card(modifier: Modifier, content: impl FnMut() + 'static) {
    LiquidCard(modifier, content);
}

/// An elevated glass pane with the grouped-inset card look: a resting pane
/// that transmits its backdrop through the translucent surface wash.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidCard(modifier: Modifier, content: impl FnMut() + 'static) {
    let colors = liquid_colors();
    let surface_glass = colors.surface_glass;
    let base = Modifier::empty().glass_effect_with(
        Glass::regular().shape(LiquidShape::RoundedRect(CARD_RADIUS)),
        move || GlassDynamics {
            activity: Some(0.0),
            resting_tint: Some(surface_glass),
            ..Default::default()
        },
    );
    Box(base.then(modifier), BoxSpec::default(), content);
}

/// A titled group of rows on one card (iOS grouped list section).
#[composable]
#[allow(non_snake_case)]
pub fn LiquidListSection(
    modifier: Modifier,
    header: impl Into<String>,
    content: impl FnMut() + 'static,
) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let header = header.into();
    let content = Rc::new(RefCell::new(content));
    Column(modifier, ColumnSpec::default(), move || {
        if !header.is_empty() {
            let style = TextStyle {
                span_style: SpanStyle {
                    color: Some(colors.secondary_label),
                    ..typography.footnote.span_style.clone()
                },
                ..typography.footnote.clone()
            };
            Text(
                header.to_uppercase(),
                Modifier::empty().padding_each(20.0, 0.0, 20.0, 6.0),
                style,
            );
        }
        let content = Rc::clone(&content);
        LiquidCard(Modifier::empty().fill_max_width(), move || {
            let content = Rc::clone(&content);
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default(),
                move || {
                    (content.borrow_mut())();
                },
            );
        });
    });
}

/// Configuration for [`LiquidListRow`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiquidListRowSpec {
    /// Draw a hairline separator under the row.
    pub separator: bool,
}

impl LiquidListRowSpec {
    pub fn with_separator(mut self, separator: bool) -> Self {
        self.separator = separator;
        self
    }
}

/// One tappable row inside a [`LiquidCard`] / [`LiquidListSection`]: a press
/// wash and an optional hairline separator; `content` lays out the row.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidListRow(
    modifier: Modifier,
    spec: LiquidListRowSpec,
    on_click: impl Fn() + 'static,
    content: impl FnMut() + 'static,
) {
    let colors = liquid_colors();
    let interaction = rememberMutableInteractionSource();
    let pressed = interaction.collectIsPressedAsState();
    let wash = animateColorAsState(
        if pressed.get() {
            colors.surface_pressed
        } else {
            colors.surface_pressed.with_alpha(0.0)
        },
        LiquidMotion::snappy(),
        "row-wash",
    );

    let separator = spec.separator;
    let separator_color = colors.separator;
    let on_click = Rc::new(RefCell::new(on_click));
    let base = Modifier::empty()
        .fill_max_width()
        .press_interaction_source(interaction)
        .clickable(move |_point| {
            (on_click.borrow_mut())();
        })
        .draw_behind(move |scope| {
            let size = scope.size();
            scope.draw_rect(Brush::solid(wash.get()));
            if separator {
                scope.draw_rect_at(
                    cranpose_ui_graphics::Rect {
                        x: 16.0,
                        y: size.height - 0.5,
                        width: (size.width - 16.0).max(0.0),
                        height: 0.5,
                    },
                    Brush::solid(separator_color),
                );
            }
        })
        .padding_symmetric(16.0, 12.0);

    Box(base.then(modifier), BoxSpec::default(), content);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_draws_no_separator_until_one_is_asked_for() {
        assert!(
            !LiquidListRowSpec::default().separator,
            "the last row of a section must not draw a trailing hairline"
        );
        assert!(LiquidListRowSpec::default().with_separator(true).separator);
        assert!(
            !LiquidListRowSpec::default()
                .with_separator(true)
                .with_separator(false)
                .separator,
            "the latest answer is the one that holds"
        );
    }
}
