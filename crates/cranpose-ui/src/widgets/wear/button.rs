//! A Wear filled button: a 52dp capsule with one or two label lines.
//!
//! This is a sibling of [`crate::widgets::Button`], not an extension of it.
//! That one is `Layout` plus `clickable` — no shape, no background, no minimum
//! size, no colours and no second label slot. A Wear button is all five of
//! those, and folding them into the Material button would put a watch's
//! geometry on every platform.
//!
//! Two behaviours worth naming, because both are easy to implement the obvious
//! way and be wrong:
//!
//! - the caller's `fillMaxWidth()` sits **outside** the button's own
//!   `width(IntrinsicSize.Max)`, so it pins `min == max` and the intrinsic
//!   width is coerced away. A port that honours the intrinsic faithfully but
//!   gets the coercion order wrong shrinks every row to its text width;
//! - there is **no press animation**. No shape morph, no scale. `IconButton`
//!   and `TextButton` do morph, and a port that generalises from them adds
//!   motion the platform does not have.

#![allow(non_snake_case)]

use crate::composable;
use crate::modifier::{Brush, Color, CornerRadii, Modifier, SemanticsConfiguration};
use crate::widgets::wear::density::WearDensity;
use crate::widgets::wear::theme::{WearColors, WearTextStyle};
use crate::widgets::{Layout, Text};
use crate::SemanticsWidgetRole;
use cranpose_core::NodeId;
use cranpose_ui_graphics::{DrawScope, Size};
use cranpose_ui_layout::{Constraints, Measurable, MeasurePolicy, MeasureResult, Placement};

/// `ButtonDefaults` plus `FilledButtonTokens`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WearButtonSpec {
    pub colors: WearColors,
    pub label_style: WearTextStyle,
    pub secondary_style: WearTextStyle,
    /// `FilledButtonTokens.ContainerHeight`.
    pub min_height: f32,
    /// `ShapeTokens.CornerLarge`.
    pub corner_radius: f32,
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    /// `Spacer(Modifier.size(1.dp))` between the two labels.
    pub label_spacing: f32,
    /// The alpha a secondary label draws its colour at.
    pub secondary_alpha: f32,
}

impl Default for WearButtonSpec {
    fn default() -> Self {
        Self {
            colors: WearColors::default(),
            label_style: WearTextStyle::LABEL_MEDIUM,
            secondary_style: WearTextStyle::LABEL_SMALL,
            min_height: 52.0,
            corner_radius: 26.0,
            padding_horizontal: 14.0,
            padding_vertical: 6.0,
            label_spacing: 1.0,
            secondary_alpha: 0.8,
        }
    }
}

impl WearButtonSpec {
    pub fn colors(mut self, colors: WearColors) -> Self {
        self.colors = colors;
        self
    }
}

/// A filled Wear button with a label and an optional secondary label.
#[composable]
pub fn WearButton<F>(
    modifier: Modifier,
    spec: WearButtonSpec,
    label: String,
    secondary_label: Option<String>,
    on_click: F,
) -> NodeId
where
    F: FnMut() + 'static,
{
    let density = WearDensity::current();
    let container = spec.colors.primary;
    let radius = density.dp(spec.corner_radius);
    let description = match &secondary_label {
        Some(secondary) => format!("{label}, {secondary}"),
        None => label.clone(),
    };
    let chrome = modifier
        .draw_behind(move |scope: &mut dyn DrawScope| {
            scope.draw_round_rect(Brush::Solid(container), CornerRadii::uniform(radius));
        })
        .clickable(move |_point| on_click())
        .semantics(move |config: &mut SemanticsConfiguration| {
            // `Role.Button` on the `combinedClickable`.
            config.role = Some(SemanticsWidgetRole::Button);
            config.is_clickable = true;
            config.content_description = Some(description.clone());
        });

    let label_style = spec.label_style.resolve(spec.colors.on_primary);
    let secondary_style = spec
        .secondary_style
        .resolve(fade(spec.colors.on_primary, spec.secondary_alpha));
    Layout(
        chrome,
        WearButtonMeasurePolicy {
            spec,
            density: density.density(),
        },
        move || {
            Text(label.clone(), Modifier::empty(), label_style.clone());
            if let Some(secondary) = secondary_label.clone() {
                Text(secondary, Modifier::empty(), secondary_style.clone());
            }
        },
    )
}

fn fade(color: Color, alpha: f32) -> Color {
    Color::rgba(color.0, color.1, color.2, color.3 * alpha)
}

#[derive(Clone, Debug, PartialEq)]
struct WearButtonMeasurePolicy {
    spec: WearButtonSpec,
    density: f32,
}

impl MeasurePolicy for WearButtonMeasurePolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let size = self.measure_into(measurables, constraints, &mut placements);
        MeasureResult::new(size, placements)
    }

    fn measure_into(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> Size {
        placements.clear();
        let density = WearDensity::new(self.density, 1.0);
        let horizontal = density.dp(self.spec.padding_horizontal) * 2.0;
        let vertical = density.dp(self.spec.padding_vertical) * 2.0;
        let width = if constraints.max_width.is_finite() {
            constraints.max_width
        } else {
            constraints.min_width
        };
        let available = (width - horizontal).max(0.0);
        let child_constraints = Constraints {
            min_width: 0.0,
            max_width: available,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };

        let mut placeables = Vec::with_capacity(measurables.len());
        for measurable in measurables {
            placeables.push(measurable.measure(child_constraints));
        }

        let spacing = density.dp(self.spec.label_spacing);
        let column: f32 = placeables
            .iter()
            .map(|placeable| density.ceil(placeable.height()))
            .sum::<f32>()
            + spacing * placeables.len().saturating_sub(1) as f32;

        let height = density
            .dp(self.spec.min_height)
            .max(density.ceil(column) + vertical)
            .clamp(constraints.min_height, constraints.max_height);

        // The label column is centred in the content area, not in the whole
        // capsule: the vertical padding is taken off first and the slack is
        // then split on a whole pixel.
        let content = height - vertical;
        let mut y = density.dp(self.spec.padding_vertical) + density.centre(content, column);
        let x = density.dp(self.spec.padding_horizontal);
        for placeable in &placeables {
            // The label column has no weight, so it wraps its content and sits
            // at the start; the text inside is start-aligned too.
            placements.push(Placement::new(placeable.node_id(), x, y, 0));
            y += density.ceil(placeable.height()) + spacing;
        }
        Size::new(width, height)
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        let density = WearDensity::new(self.density, 1.0);
        measurables
            .iter()
            .map(|m| m.min_intrinsic_width(height))
            .fold(0.0, f32::max)
            + density.dp(self.spec.padding_horizontal) * 2.0
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        let density = WearDensity::new(self.density, 1.0);
        measurables
            .iter()
            .map(|m| m.max_intrinsic_width(height))
            .fold(0.0, f32::max)
            + density.dp(self.spec.padding_horizontal) * 2.0
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        let density = WearDensity::new(self.density, 1.0);
        let spacing =
            density.dp(self.spec.label_spacing) * measurables.len().saturating_sub(1) as f32;
        let column = measurables
            .iter()
            .map(|m| m.min_intrinsic_height(width))
            .sum::<f32>()
            + spacing;
        density
            .dp(self.spec.min_height)
            .max(density.ceil(column) + density.dp(self.spec.padding_vertical) * 2.0)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        self.min_intrinsic_height(measurables, width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_ones_the_tokens_declare() {
        let spec = WearButtonSpec::default();
        assert_eq!(spec.min_height, 52.0, "FilledButtonTokens.ContainerHeight");
        assert_eq!(spec.corner_radius, 26.0, "ShapeTokens.CornerLarge");
        assert_eq!(spec.padding_horizontal, 14.0);
        assert_eq!(spec.padding_vertical, 6.0);
        assert_eq!(spec.label_spacing, 1.0);
        assert_eq!(spec.secondary_alpha, 0.8);
        assert_eq!(spec.label_style, WearTextStyle::LABEL_MEDIUM);
        assert_eq!(spec.secondary_style, WearTextStyle::LABEL_SMALL);
    }

    #[test]
    fn a_secondary_label_draws_its_colour_at_four_fifths() {
        let faded = fade(Color::rgba(1.0, 1.0, 1.0, 1.0), 0.8);
        assert_eq!(faded.3, 0.8);
    }
}
