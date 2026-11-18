use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, SemanticsConfiguration, SemanticsWidgetRole,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, DrawScope, Rect};

use crate::theme::liquid_colors;

// The iOS 27 progress bar is 4 points tall.
const BAR_HEIGHT: f32 = 4.0;

/// A bar that fills with the accent color as `progress` runs from 0 to 1,
/// over the system fill. Readers hear it as a progress bar with its percent.
/// Size its width with `modifier`; its height is the iOS bar's.
///
/// UIKit's `UIProgressView` and SwiftUI's `ProgressView(value:)`.
#[composable]
pub fn LiquidLinearProgressIndicator(modifier: Modifier, progress: f32) {
    let colors = liquid_colors();
    let progress = progress.clamp(0.0, 1.0);
    let (track, fill) = (colors.system_fill, colors.accent);
    Box(
        modifier
            .height(BAR_HEIGHT)
            .draw_behind(move |scope| draw_capsule_bar(scope, progress, track, fill))
            .semantics(progress_semantics(progress)),
        BoxSpec::default(),
        || {},
    );
}

// Paints a capsule bar over the scope: the whole track, then the filled part
// from the leading edge as a capsule of its own.
pub(super) fn draw_capsule_bar(
    scope: &mut dyn DrawScope,
    progress: f32,
    track: Color,
    fill: Color,
) {
    let size = scope.size();
    let radii = CornerRadii::uniform(size.height * 0.5);
    scope.draw_round_rect(Brush::solid(track), radii);
    if progress > 0.0 {
        let filled = Rect {
            x: 0.0,
            y: 0.0,
            width: size.width * progress,
            height: size.height,
        };
        scope.draw_round_rect_at(filled, Brush::solid(fill), radii);
    }
}

pub(super) fn progress_semantics(progress: f32) -> impl Fn(&mut SemanticsConfiguration) {
    move |config| {
        config.role = Some(SemanticsWidgetRole::ProgressBar);
        config.progress = Some(cranpose_ui::ProgressBarRangeInfo::new(
            progress, 0.0, 1.0, 0,
        ));
        config.state_description = Some(format!("{}%", (progress * 100.0).round()));
    }
}
