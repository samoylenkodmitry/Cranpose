use std::f32::consts::TAU;

use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};
use cranpose_core::{State, derivedStateOf};
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, Point, SemanticsConfiguration, SemanticsWidgetRole, Size,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Brush, Color, DrawScope, Stroke, StrokeCap};

use crate::theme::liquid_colors;

// The iOS 27 spinner: eight rounded spokes that step once an eighth of a
// second, clockwise. The leading spoke and the three behind it fall from
// full opacity toward the faintest share in five equal steps, so the
// leading one shows at 0.854; the rest hold the faintest share.
const SPOKES: u8 = 8;
const TURN_MILLIS: u64 = 1000;
const FADE_STEPS: f32 = 5.0;
const FAINTEST: f32 = 0.27;

/// The two sizes of the iOS spinner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LiquidActivityIndicatorSize {
    /// 20 points across: inline with text and in buttons.
    #[default]
    Medium,
    /// 35 points across in a 37 point frame: a screen or a panel that is
    /// loading.
    Large,
}

// A spinner's frame and spokes, measured on the simulator at 3x. The large
// spinner's frame is 37 points, with its 35 point wheel at the top leading
// corner, as UIKit lays it out.
#[derive(Clone, Copy)]
pub(super) struct Wheel {
    pub(super) side: f32,
    center: f32,
    inner: f32,
    outer: f32,
    width: f32,
}

impl Wheel {
    // The wheel at `factor` times this one's size.
    pub(super) fn scaled(self, factor: f32) -> Self {
        Self {
            side: self.side * factor,
            center: self.center * factor,
            inner: self.inner * factor,
            outer: self.outer * factor,
            width: self.width * factor,
        }
    }

    // Draws the first `shown` spokes in `color`, the one at `head` leading.
    pub(super) fn draw(self, scope: &mut dyn DrawScope, color: Color, head: f32, shown: u8) {
        let center = Point::new(self.center, self.center);
        let stroke = Stroke::new(self.width).with_cap(StrokeCap::Round);
        let reach = (self.outer - self.inner - self.width).max(0.0);
        let spokes = f32::from(SPOKES);
        for spoke in (0..shown.min(SPOKES)).map(f32::from) {
            let behind = (head - spoke).rem_euclid(spokes);
            let alpha = (1.0 - (1.0 - FAINTEST) * (behind + 1.0) / FADE_STEPS).max(FAINTEST);
            // Spoke 0 points straight up; the rest follow clockwise.
            let angle = spoke / spokes * TAU - TAU / 4.0;
            let (sin, cos) = angle.sin_cos();
            let start = self.inner + self.width * 0.5;
            scope.draw_line(
                Brush::solid(color.with_alpha(color.a() * alpha)),
                Point::new(center.x + cos * start, center.y + sin * start),
                Point::new(
                    center.x + cos * (start + reach),
                    center.y + sin * (start + reach),
                ),
                stroke,
            );
        }
    }
}

impl LiquidActivityIndicatorSize {
    pub(super) fn wheel(self) -> Wheel {
        match self {
            Self::Medium => Wheel {
                side: 20.0,
                center: 10.0,
                inner: 10.0 / 3.0,
                outer: 10.0,
                width: 2.5,
            },
            Self::Large => Wheel {
                side: 37.0,
                center: 17.5,
                inner: 29.0 / 6.0,
                outer: 17.5,
                width: 5.0,
            },
        }
    }
}

// The spoke that leads a turning wheel: it steps once an eighth of a
// second. Drawing reads it, so a wheel repaints eight times a turn and not
// on every frame of the transition.
#[composable]
pub(super) fn rememberSpinnerHead() -> State<f32> {
    let turn = rememberInfiniteTransition("liquid_activity_indicator").animateFloat(
        0.0,
        f32::from(SPOKES),
        infiniteRepeatable(
            AnimationSpec::linear(TURN_MILLIS),
            RepeatMode::Restart,
            StartOffset::default(),
        ),
        "liquid_activity_turn",
    );
    derivedStateOf(move || turn.value().floor())
}

/// A spinner for work whose length is unknown. `color` paints the spokes;
/// `None` takes the theme's secondary label gray, as iOS does. Readers hear
/// it as a busy progress indicator.
///
/// UIKit's `UIActivityIndicatorView` and SwiftUI's `ProgressView()`.
#[composable]
pub fn LiquidActivityIndicator(
    modifier: Modifier,
    size: LiquidActivityIndicatorSize,
    color: Option<Color>,
) {
    let color = color.unwrap_or_else(|| liquid_colors().secondary_label);
    let wheel = size.wheel();
    let head = rememberSpinnerHead();
    Box(
        modifier
            .size(Size::new(wheel.side, wheel.side))
            .semantics(|config: &mut SemanticsConfiguration| {
                config.role = Some(SemanticsWidgetRole::ProgressBar);
            })
            .draw_behind(move |scope| wheel.draw(scope, color, head.get(), SPOKES)),
        BoxSpec::default(),
        || {},
    );
}
