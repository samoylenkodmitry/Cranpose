//! The curved indicator a round watch puts at 3 o'clock.
//!
//! The geometry is [`crate::round_scroll_indicator`], which is golden-tested
//! against where the shipping Compose build puts pixels on 454x454 and 384x384
//! displays. This module is the widget around it: it reads a list's layout,
//! turns it into three segments, and draws them.
//!
//! Two things about this indicator are not guessable and are worth restating
//! where the drawing is. It is **three segments with a gap at each end of the
//! thumb**, not a thumb painted over a continuous rail — drawing a full-length
//! track underneath gives a visibly different picture. And a segment shorter
//! than its own stroke is drawn as a **circle that shrinks and fades on the
//! same fraction**, so a segment leaving the screen dwindles to a dot instead
//! of stopping at one round cap's width.

#![allow(non_snake_case)]

use cranpose_core::NodeId;
use cranpose_ui_graphics::{DrawScope, Stroke, StrokeCap};

use crate::{
    composable,
    modifier::{Brush, Color, Modifier, Point},
    round_scroll_indicator::{
        IndicatorGeometry, IndicatorPart, IndicatorSegment, indicator_arc, indicator_segments,
        scaling_list_geometry,
    },
    widgets::{
        Canvas,
        wear::{scaling_list::WearScalingListState, theme::WearColors},
    },
};

/// How a [`ScrollIndicator`] is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollIndicatorSpec {
    pub colors: WearColors,
    /// How visible the whole indicator is.
    ///
    /// Wear fades this to zero two seconds after the last scroll, so a
    /// screenshot of a settled screen shows no indicator at all. That is a
    /// policy, not geometry, and an app with an always-on indicator sets this
    /// to `1.0` and never animates it — which is why the widget takes an alpha
    /// rather than owning a timer.
    pub alpha: f32,
}

impl Default for ScrollIndicatorSpec {
    fn default() -> Self {
        Self {
            colors: WearColors::default(),
            alpha: 1.0,
        }
    }
}

impl ScrollIndicatorSpec {
    pub fn colors(mut self, colors: WearColors) -> Self {
        self.colors = colors;
        self
    }

    pub fn alpha(mut self, alpha: f32) -> Self {
        self.alpha = alpha;
        self
    }
}

/// The thumb for a scaling list.
///
/// The thumb is measured in **fractional item indices**, which is what
/// `ScalingLazyColumnStateAdapter` does and what
/// [`scaling_list_geometry`] ports: its length is the share of the *items* on
/// screen and its position is how many items are left, so a list whose rows
/// differ in height puts it somewhere a pixel model cannot. The two answers
/// agree only on a list of uniform rows that is as tall as its content, and a
/// Wear list has a header. Measured against the shipping Compose build on a
/// settled Settings screen, by integrating the indicator's ink across the
/// stroke band row by row: the thumb's centroid was 5.97 device pixels below
/// Compose's at 192dp and 7.73 at 227dp under the flat model, and is 0.61 and
/// 0.83 under this one.
///
/// Whether a list is scrollable at all is a separate question, and Wear leaves
/// it to `ScreenScaffold` rather than to the adapter — so it is asked here,
/// against the list's own travel, and not inside the geometry.
pub fn indicator_for_scaling_list(state: &WearScalingListState) -> Option<IndicatorGeometry> {
    let info = state.layout_info();
    let travel = info.travel();
    if travel <= 0.0 || info.viewport <= 0.0 || info.content <= info.viewport {
        return None;
    }
    state.with_indicator_list(scaling_list_geometry)
}

/// Draws the indicator into a scope whose bounds are the whole display.
///
/// Split out from the composable so it can be tested against a bare
/// `DrawScopeDefault` and reused by an app that still draws its own screens.
pub fn draw_scroll_indicator(
    scope: &mut dyn DrawScope,
    geometry: IndicatorGeometry,
    spec: ScrollIndicatorSpec,
) {
    let size = scope.size();
    let radius = size.width.min(size.height) * 0.5;
    if radius <= 0.0 || !radius.is_finite() || spec.alpha <= 0.0 {
        return;
    }
    let centre = Point {
        x: size.width * 0.5,
        y: size.height * 0.5,
    };
    let arc = indicator_arc(radius);
    if arc.centreline() <= 0.0 {
        return;
    }
    let stroke = Stroke::new(arc.width()).with_cap(StrokeCap::Round);
    for (part, segment) in indicator_segments(arc, geometry, spec.alpha) {
        let color = match part {
            IndicatorPart::Track => spec.colors.indicator_track,
            IndicatorPart::Thumb => spec.colors.indicator_thumb,
        };
        match segment {
            IndicatorSegment::Arc {
                start,
                sweep,
                alpha,
            } => {
                if sweep <= 0.0 || alpha <= 0.0 {
                    continue;
                }
                scope.draw_arc(
                    Brush::Solid(with_alpha(color, alpha)),
                    centre,
                    arc.centreline(),
                    start,
                    sweep,
                    stroke,
                );
            }
            IndicatorSegment::Dot {
                angle,
                radius: dot,
                alpha,
            } => {
                if dot <= 0.0 || alpha <= 0.0 {
                    continue;
                }
                scope.draw_circle(
                    Brush::Solid(with_alpha(color, alpha)),
                    Point {
                        x: centre.x + arc.centreline() * angle.cos(),
                        y: centre.y + arc.centreline() * angle.sin(),
                    },
                    dot,
                );
            }
        }
    }
}

fn with_alpha(color: Color, alpha: f32) -> Color {
    Color::rgba(color.0, color.1, color.2, color.3 * alpha)
}

/// The curved scroll indicator, sized to the display it sits on.
///
/// It draws nothing when the list fits on screen, which is what Wear does.
#[composable]
pub fn ScrollIndicator(
    modifier: Modifier,
    state: WearScalingListState,
    spec: ScrollIndicatorSpec,
) -> NodeId {
    let draw_state = state;
    Canvas(
        modifier.fill_max_size(),
        move |scope: &mut dyn DrawScope| {
            if let Some(geometry) = indicator_for_scaling_list(&draw_state) {
                draw_scroll_indicator(scope, geometry, spec);
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::{DrawPrimitive, DrawScopeDefault, Size};

    use super::*;

    fn scene(geometry: IndicatorGeometry, alpha: f32) -> Vec<DrawPrimitive> {
        let mut scope = DrawScopeDefault::new(Size::new(227.0, 227.0));
        draw_scroll_indicator(
            &mut scope,
            geometry,
            ScrollIndicatorSpec::default().alpha(alpha),
        );
        scope.into_primitives()
    }

    #[test]
    fn the_indicator_is_three_segments_not_a_thumb_over_a_rail() {
        let primitives = scene(
            IndicatorGeometry {
                thumb: 0.4,
                offset: 0.3,
            },
            1.0,
        );
        assert_eq!(
            primitives.len(),
            3,
            "track, thumb, track — and no full-length rail underneath"
        );
    }

    #[test]
    fn a_segment_shorter_than_its_stroke_becomes_a_dot() {
        // The thumb pinned to the very top leaves nothing above it.
        let primitives = scene(
            IndicatorGeometry {
                thumb: 0.7,
                offset: 0.0,
            },
            1.0,
        );
        // The leading track has zero sweep and draws nothing at all; what is
        // left is the thumb and the trailing track.
        assert_eq!(primitives.len(), 2);
    }

    #[test]
    fn a_faded_indicator_draws_nothing_at_all() {
        let primitives = scene(
            IndicatorGeometry {
                thumb: 0.4,
                offset: 0.3,
            },
            0.0,
        );
        assert!(primitives.is_empty());
    }

    #[test]
    fn a_display_with_no_room_does_not_panic() {
        let mut scope = DrawScopeDefault::new(Size::new(1.0, 1.0));
        draw_scroll_indicator(
            &mut scope,
            IndicatorGeometry {
                thumb: 0.4,
                offset: 0.3,
            },
            ScrollIndicatorSpec::default(),
        );
        assert!(scope.into_primitives().is_empty());
    }
}
