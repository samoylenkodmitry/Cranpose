use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    text::TextAlign,
    widgets::{Column, ColumnSpec, Text},
};
use cranpose_ui_layout::HorizontalAlignment;

use crate::theme::{liquid_colors, liquid_typography};

// The iOS 27 content-unavailable view, measured on the simulator at 3x: the
// title in the title 3 size at the body weight and the label color, the
// description centered below it in the same font and the secondary label
// color, then the actions 89/3 points under the description.
const DESCRIPTION_GAP: f32 = 3.0;
const ACTIONS_GAP: f32 = 89.0 / 3.0;

/// A message in place of content that is not there: `title`, then
/// `description` centered under it, then `actions`, such as a button that
/// makes the content.
///
/// SwiftUI's `ContentUnavailableView`.
#[composable]
pub fn LiquidContentUnavailableView(
    modifier: Modifier,
    title: &str,
    description: &str,
    actions: impl FnMut() + 'static,
) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let mut title_style = typography.title3;
    // The body weight, which bold text raises.
    title_style.span_style.font_weight = typography.body.span_style.font_weight;
    let mut description_style = title_style.clone();
    title_style.span_style.color = Some(colors.label);
    description_style.span_style.color = Some(colors.secondary_label);
    description_style.paragraph_style.text_align = TextAlign::Center;
    let (title, description) = (title.to_owned(), description.to_owned());
    let mut actions = actions;
    Column(
        modifier,
        ColumnSpec::default().horizontal_alignment(HorizontalAlignment::CenterHorizontally),
        move || {
            Text(title.clone(), Modifier::empty(), title_style.clone());
            Text(
                description.clone(),
                Modifier::empty().padding_each(0.0, DESCRIPTION_GAP, 0.0, ACTIONS_GAP),
                description_style.clone(),
            );
            actions();
        },
    );
}
