use cranpose_foundation::text::TextFieldState;
use cranpose_macros::composable;
use cranpose_ui::{
    Alignment, Modifier,
    widgets::{BasicTextFieldDecorated, BasicTextFieldOptions, Box, BoxSpec, Text},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, Rect, Stroke};
use cranpose_ui_layout::{HorizontalAlignment, VerticalAlignment};

use crate::theme::{liquid_colors, liquid_typography};

// The iOS 27 rounded-border text field, measured on the simulator at 3x: 34
// points tall, the system background under a hairline of the label color at
// 20% with 5 point corners, and the text 22/3 points from the leading edge.
// iOS curves the corners continuously over 23/3 points of each edge; these
// are circular arcs.
const HEIGHT: f32 = 34.0;
const CORNER: f32 = 5.0;
const HAIRLINE: f32 = 1.0 / 3.0;
const BORDER_ALPHA: f32 = 0.2;
const LEADING: f32 = 22.0 / 3.0;

/// A single-line text field on the system background inside a rounded
/// hairline border. `state` holds the text; `placeholder` shows in the
/// tertiary label color while it is empty, and names the field for readers.
///
/// UIKit's `UITextField` with `.roundedRect` and SwiftUI's
/// `TextField(_:text:)` with `.roundedBorder`.
#[composable]
pub fn LiquidTextField(modifier: Modifier, state: TextFieldState, placeholder: &str) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let border = colors.label.with_alpha(BORDER_ALPHA);
    let fill = colors.system_background;
    let mut field_style = typography.body.clone();
    field_style.span_style.color = Some(colors.label);
    let mut placeholder_style = typography.body;
    placeholder_style.span_style.color = Some(colors.tertiary_label);
    let placeholder = placeholder.to_owned();
    BasicTextFieldDecorated(
        state,
        modifier
            .height(HEIGHT)
            .draw_behind(move |scope| draw_field(scope, fill, border))
            .padding_symmetric(LEADING, 0.0)
            .content_description(placeholder.clone()),
        BasicTextFieldOptions {
            text_style: field_style,
            line_limits: cranpose_foundation::text::TextFieldLineLimits::SingleLine,
            ..BasicTextFieldOptions::default()
        },
        move |inner| {
            let placeholder = placeholder.clone();
            let placeholder_style = placeholder_style.clone();
            Box(
                Modifier::empty().fill_max_size(),
                BoxSpec {
                    content_alignment: Alignment::new(
                        HorizontalAlignment::Start,
                        VerticalAlignment::CenterVertically,
                    ),
                    propagate_min_constraints: false,
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
}

fn draw_field(scope: &mut dyn cranpose_ui_graphics::DrawScope, fill: Color, border: Color) {
    let size = scope.size();
    let half = HAIRLINE * 0.5;
    scope.draw_round_rect(Brush::solid(fill), CornerRadii::uniform(CORNER));
    scope.draw_round_rect_at_stroked(
        Rect {
            x: half,
            y: half,
            width: size.width - HAIRLINE,
            height: size.height - HAIRLINE,
        },
        Brush::solid(border),
        CornerRadii::uniform(CORNER - half),
        Stroke::new(HAIRLINE),
    );
}
