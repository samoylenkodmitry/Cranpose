//! A section heading in a Wear list.
//!
//! The one thing that is not obvious from the Kotlin: `ListHeader` **wraps its
//! content width**. It does not span the list, so a header's background — were
//! one ever set; the default is transparent — would be a capsule around the
//! words rather than a band across the screen.
//!
//! The other is the 48dp floor. Its own padding plus one line of `titleMedium`
//! comes to 47dp, which the floor rounds up to 48dp, and the surplus is split
//! by re-centring the padded content in the taller box rather than being added
//! below it. That is a one-pixel difference and it is on every header.
//!
//! The third is the alignment, and it only shows on a header long enough to
//! wrap. `ListHeader` provides `LocalTextConfiguration` with
//! `TextAlign.Center` alongside its `Arrangement.Center` — read out of
//! `ListHeaderKt` in `compose-material3-1.6.2.aar`, where the neighbouring
//! `ListSubHeader` provides `TextAlign.Start` — so **every** line of a wrapped
//! header is centred in the block, not just placed under the start of the
//! first. Centring the block alone is invisible on a one-line header and wrong
//! on a two-line one: on a 384 px Credits screen, `ORBIT BREAKER` wraps after
//! `ORBIT` and Compose puts that line at x 147, while a block-only centring
//! puts it at x 121, 25 px to the left of the `BREAKER` under it.

#![allow(non_snake_case)]

use crate::composable;
use crate::modifier::Modifier;
use crate::text::paragraph::TextAlign;
use crate::widgets::wear::density::WearDensity;
use crate::widgets::wear::theme::{WearColors, WearTextStyle};
use crate::widgets::{Layout, Text};
use cranpose_core::NodeId;
use cranpose_ui_graphics::Size;
use cranpose_ui_layout::{Constraints, Measurable, MeasurePolicy, MeasureResult, Placement};

/// `ListHeaderDefaults` plus `ListHeaderTokens.Height`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListHeaderSpec {
    pub colors: WearColors,
    pub text_style: WearTextStyle,
    /// `ListHeaderTokens.Height`.
    pub min_height: f32,
    pub padding_start: f32,
    pub padding_top: f32,
    pub padding_end: f32,
    pub padding_bottom: f32,
}

impl Default for ListHeaderSpec {
    fn default() -> Self {
        Self {
            colors: WearColors::default(),
            // `TITLE_MEDIUM` is `ListHeaderTokens`; the alignment is the
            // `LocalTextConfiguration` the widget provides around it. It lives
            // on the spec rather than being forced at the call site so a
            // caller who overrides `text_style` overrides the alignment too,
            // the way overriding `LocalTextConfiguration` does in Compose.
            text_style: WearTextStyle::TITLE_MEDIUM.aligned(TextAlign::Center),
            min_height: 48.0,
            padding_start: 14.0,
            padding_top: 16.0,
            padding_end: 14.0,
            padding_bottom: 12.0,
        }
    }
}

impl ListHeaderSpec {
    pub fn colors(mut self, colors: WearColors) -> Self {
        self.colors = colors;
        self
    }
}

/// A section heading.
///
/// The label is taken as text rather than as a content slot because the widget
/// owns the style: `ListHeader` provides `titleMedium` and `onBackground`
/// through composition locals in Compose, and Cranpose has no such local to
/// provide through. Taking the string is the version that cannot be wired up
/// wrong.
#[composable]
pub fn ListHeader<S>(modifier: Modifier, spec: ListHeaderSpec, label: S) -> NodeId
where
    S: crate::widgets::IntoTextSource + Clone + PartialEq + 'static,
{
    let style = spec.text_style.resolve(spec.colors.on_background);
    Layout(
        modifier,
        ListHeaderMeasurePolicy {
            spec,
            density: WearDensity::current().density(),
        },
        move || {
            Text(label.clone(), Modifier::empty(), style.clone());
        },
    )
}

#[derive(Clone, Debug, PartialEq)]
struct ListHeaderMeasurePolicy {
    spec: ListHeaderSpec,
    density: f32,
}

impl ListHeaderMeasurePolicy {
    fn horizontal(&self, density: WearDensity) -> f32 {
        density.dp(self.spec.padding_start) + density.dp(self.spec.padding_end)
    }

    fn vertical(&self, density: WearDensity) -> f32 {
        density.dp(self.spec.padding_top) + density.dp(self.spec.padding_bottom)
    }
}

impl MeasurePolicy for ListHeaderMeasurePolicy {
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
        let horizontal = self.horizontal(density);
        let available = (constraints.max_width - horizontal).max(0.0);
        let child_constraints = Constraints {
            min_width: 0.0,
            max_width: available,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };

        let mut content_width = 0.0f32;
        let mut content_height = 0.0f32;
        let mut placeables = Vec::with_capacity(measurables.len());
        for measurable in measurables {
            let placeable = measurable.measure(child_constraints);
            content_width = content_width.max(placeable.width());
            content_height = content_height.max(placeable.height());
            placeables.push(placeable);
        }

        // `wrapContentSize()`: the header is as wide as its words plus its own
        // padding, not as wide as the list.
        let width = density
            .ceil(content_width + horizontal)
            .clamp(constraints.min_width, constraints.max_width);
        let padded = density.ceil(content_height) + self.vertical(density);
        let height = density
            .dp(self.spec.min_height)
            .max(padded)
            .clamp(constraints.min_height, constraints.max_height);
        // The floor wins over the padded content by a couple of pixels, and
        // `wrapContentSize`'s default `Alignment.Center` re-centres inside the
        // taller box rather than hanging the surplus off the bottom.
        let surplus = density.centre(height, padded);
        let top = density.dp(self.spec.padding_top) + surplus;

        for placeable in placeables {
            let x = density.centre(width, placeable.width());
            placements.push(Placement::new(placeable.node_id(), x, top, 0));
        }
        Size::new(width, height)
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        let density = WearDensity::new(self.density, 1.0);
        measurables
            .iter()
            .map(|m| m.min_intrinsic_width(height))
            .fold(0.0, f32::max)
            + self.horizontal(density)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        let density = WearDensity::new(self.density, 1.0);
        measurables
            .iter()
            .map(|m| m.max_intrinsic_width(height))
            .fold(0.0, f32::max)
            + self.horizontal(density)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        let density = WearDensity::new(self.density, 1.0);
        let content = measurables
            .iter()
            .map(|m| m.min_intrinsic_height(width))
            .fold(0.0, f32::max);
        density
            .dp(self.spec.min_height)
            .max(density.ceil(content) + self.vertical(density))
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
        let spec = ListHeaderSpec::default();
        assert_eq!(spec.min_height, 48.0, "ListHeaderTokens.Height");
        assert_eq!(spec.padding_start, 14.0);
        assert_eq!(spec.padding_end, 14.0);
        assert_eq!(spec.padding_top, 16.0);
        assert_eq!(spec.padding_bottom, 12.0);
        // The type token, plus the alignment the widget provides around it:
        // Compose's ListHeader wraps its content in LocalTextConfiguration
        // with TextAlign.Center, so the style an app actually gets is the
        // centred one. Asserting the bare token here would pass while every
        // real header wrapped its second line to the left.
        assert_eq!(
            spec.text_style,
            WearTextStyle::TITLE_MEDIUM.aligned(TextAlign::Center)
        );
    }
}
