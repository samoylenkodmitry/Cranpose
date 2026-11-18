use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    text::TextAlign,
    widgets::{Box, BoxSpec, Column, ColumnSpec, Text},
};

use super::progress::{draw_capsule_bar, progress_semantics};
use crate::theme::{liquid_colors, liquid_typography};

// The iOS 27 gauge, measured on the simulator: a 16 point capsule under its
// centered label, the label's line ending 32/3 points above the bar.
const BAR_HEIGHT: f32 = 16.0;
const LABEL_GAP: f32 = 32.0 / 3.0;

/// A labeled capsule that fills with the accent color as `value` runs from 0
/// to 1, over the tertiary fill, with `label` centered above it. Readers hear
/// the label with the value as a percent.
///
/// SwiftUI's `Gauge(value:label:)` in its default style.
#[composable]
pub fn LiquidGauge(modifier: Modifier, value: f32, label: &str) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let value = value.clamp(0.0, 1.0);
    let (track, fill) = (colors.tertiary_fill, colors.accent);
    let mut label_style = typography.body;
    label_style.span_style.color = Some(colors.label);
    label_style.paragraph_style.text_align = TextAlign::Center;
    let label = label.to_owned();
    let description = label.clone();
    Column(
        modifier.semantics(move |config| {
            progress_semantics(value)(config);
            config.content_description = Some(description.clone());
        }),
        ColumnSpec::default(),
        move || {
            Text(
                label.clone(),
                Modifier::empty()
                    .fill_max_width()
                    .padding_each(0.0, 0.0, 0.0, LABEL_GAP)
                    .hide_from_accessibility(),
                label_style.clone(),
            );
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(BAR_HEIGHT)
                    .draw_behind(move |scope| draw_capsule_bar(scope, value, track, fill)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}
