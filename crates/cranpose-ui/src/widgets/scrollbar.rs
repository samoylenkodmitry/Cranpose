//! An explicit scrollbar: a track, a thumb that reports the scroll position,
//! and a thumb the user can drag.
//!
//! A scroll indicator that only reports is half a scrollbar. On a desktop, and
//! on any platform driven by a mouse, the bar is also a control: grabbing the
//! thumb and pulling it is how a long document is crossed, and a bar that
//! cannot be grabbed sends the user back to the wheel for every long jump.
//!
//! The thumb geometry is [`crate::scrollbar`], shared with the curved indicator
//! a round watch draws, so both answer "how long is the thumb and where does it
//! sit" the same way. The drag is [`Modifier::draggable`], so pulling a thumb
//! obeys the same touch slop and axis locking as scrolling the content itself.

use cranpose_core::NodeId;
use cranpose_ui_graphics::{Brush, Color, CornerRadii, DrawScope, Point, Rect, Size};
use cranpose_ui_layout::Axis;

use crate::{
    composable,
    draggable::rememberDraggableState,
    modifier::Modifier,
    scroll::ScrollState,
    scrollbar::{ThumbBounds, content_delta_for_thumb_drag},
    widgets::{
        BoxWithConstraints, Canvas,
        scopes::{BoxWithConstraintsScope, BoxWithConstraintsScopeImpl},
    },
};

/// How wide a bar is across its short axis.
pub const DEFAULT_SCROLLBAR_THICKNESS: f32 = 8.0;
/// How short the thumb may get, in logical pixels.
///
/// A thumb proportional to a very long document shrinks to a sliver nobody can
/// hit; a floor in pixels is what keeps it grabbable, and a floor as a fraction
/// of the track — which is what a watch's indicator uses — would make the bar
/// lie about how much content there is on short lists.
pub const DEFAULT_MIN_THUMB_EXTENT: f32 = 24.0;

/// The colours a [`Scrollbar`] paints with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarColors {
    /// The rail behind the thumb. Fully transparent hides it.
    pub track: Color,
    /// The thumb at rest.
    pub thumb: Color,
    /// The thumb while it is being dragged.
    pub dragged_thumb: Color,
}

impl ScrollbarColors {
    /// The thumb colour for the current interaction.
    pub fn thumb_for(self, dragging: bool) -> Color {
        if dragging {
            self.dragged_thumb
        } else {
            self.thumb
        }
    }
}

impl Default for ScrollbarColors {
    fn default() -> Self {
        Self {
            track: Color(0.0, 0.0, 0.0, 0.06),
            thumb: Color(0.0, 0.0, 0.0, 0.32),
            dragged_thumb: Color(0.0, 0.0, 0.0, 0.56),
        }
    }
}

/// How a [`Scrollbar`] is drawn and how short its thumb may get.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarSpec {
    /// Width across the short axis.
    pub thickness: f32,
    /// The shortest the thumb may get, in logical pixels.
    pub min_thumb_extent: f32,
    /// Corner radius of the track and the thumb. Defaults to a full pill.
    pub corner_radius: Option<f32>,
    pub colors: ScrollbarColors,
    /// Whether the bar disappears entirely when the content fits.
    ///
    /// A bar left visible over content that cannot scroll invites a drag that
    /// does nothing.
    pub hide_when_content_fits: bool,
}

impl ScrollbarSpec {
    pub fn thickness(mut self, thickness: f32) -> Self {
        self.thickness = thickness.max(0.0);
        self
    }

    pub fn min_thumb_extent(mut self, extent: f32) -> Self {
        self.min_thumb_extent = extent.max(0.0);
        self
    }

    pub fn corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = Some(radius.max(0.0));
        self
    }

    pub fn colors(mut self, colors: ScrollbarColors) -> Self {
        self.colors = colors;
        self
    }

    pub fn hide_when_content_fits(mut self, hide: bool) -> Self {
        self.hide_when_content_fits = hide;
        self
    }

    /// The corner radius to paint with on a bar of this thickness: a full pill
    /// unless the caller asked for something squarer.
    pub fn resolved_corner_radius(&self, thickness: f32) -> f32 {
        self.corner_radius.unwrap_or(thickness * 0.5).max(0.0)
    }

    /// How short the thumb may get on a track of `track`, as a fraction.
    pub fn thumb_bounds(&self, track: f32) -> ThumbBounds {
        ThumbBounds::at_least(self.min_thumb_extent, track)
    }
}

impl Default for ScrollbarSpec {
    fn default() -> Self {
        Self {
            thickness: DEFAULT_SCROLLBAR_THICKNESS,
            min_thumb_extent: DEFAULT_MIN_THUMB_EXTENT,
            corner_radius: None,
            colors: ScrollbarColors::default(),
            hide_when_content_fits: true,
        }
    }
}

/// A vertical scrollbar for `state`, drawn down the space the modifier gives it.
///
/// Place it beside or over the scrolling content — a `Box` with the bar aligned
/// to the end edge is the ordinary arrangement.
#[composable]
pub fn VerticalScrollbar(modifier: Modifier, state: ScrollState) -> NodeId {
    Scrollbar(modifier, state, Axis::Vertical, ScrollbarSpec::default())
}

/// A horizontal scrollbar for `state`.
#[composable]
pub fn HorizontalScrollbar(modifier: Modifier, state: ScrollState) -> NodeId {
    Scrollbar(modifier, state, Axis::Horizontal, ScrollbarSpec::default())
}

/// A scrollbar along `axis`, drawn and bounded by `spec`.
#[composable]
pub fn Scrollbar(
    modifier: Modifier,
    state: ScrollState,
    axis: Axis,
    spec: ScrollbarSpec,
) -> NodeId {
    BoxWithConstraints(modifier, move |constraints: BoxWithConstraintsScopeImpl| {
        let constraints = constraints.constraints();
        let track = if axis.is_vertical() {
            constraints.max_height
        } else {
            constraints.max_width
        };
        let track = if track.is_finite() {
            track.max(0.0)
        } else {
            0.0
        };
        let bounds = spec.thumb_bounds(track);

        let dragged = rememberDraggableState(move |delta| {
            let metrics = state.metrics();
            let Some(geometry) = metrics.thumb(bounds) else {
                return;
            };
            let scroll = content_delta_for_thumb_drag(delta, track, geometry, metrics.max_offset);
            if scroll != 0.0 {
                state.dispatch_raw_delta(scroll);
            }
        });

        let drawn = dragged.clone();
        Canvas(
            Modifier::empty().fill_max_size().draggable(axis, dragged),
            move |scope: &mut dyn DrawScope| {
                draw_scrollbar(scope, state, axis, spec, drawn.is_dragging());
            },
        );
    })
}

/// Draws a scrollbar into a scope whose bounds are the whole bar.
///
/// Split out from the composable so the picture can be asserted against a bare
/// draw scope, and so an application still drawing its own chrome can use it.
pub fn draw_scrollbar(
    scope: &mut dyn DrawScope,
    state: ScrollState,
    axis: Axis,
    spec: ScrollbarSpec,
    dragging: bool,
) {
    let size = scope.size();
    let track = if axis.is_vertical() {
        size.height
    } else {
        size.width
    };
    let thickness = if axis.is_vertical() {
        size.width
    } else {
        size.height
    };
    if track <= 0.0 || thickness <= 0.0 {
        return;
    }

    let metrics = state.metrics();
    let geometry = metrics.thumb(spec.thumb_bounds(track));
    if geometry.is_none() && spec.hide_when_content_fits {
        return;
    }

    let radii = CornerRadii::uniform(spec.resolved_corner_radius(thickness));
    if spec.colors.track.3 > 0.0 {
        scope.draw_round_rect_at(
            Rect::from_size(size),
            Brush::Solid(spec.colors.track),
            radii,
        );
    }

    let Some(geometry) = geometry else {
        return;
    };
    let thumb_extent = (geometry.length * track).max(0.0);
    let thumb_offset = (geometry.offset * track).max(0.0);
    if thumb_extent <= 0.0 {
        return;
    }
    let rect = if axis.is_vertical() {
        Rect::from_origin_size(
            Point::new(0.0, thumb_offset),
            Size::new(thickness, thumb_extent),
        )
    } else {
        Rect::from_origin_size(
            Point::new(thumb_offset, 0.0),
            Size::new(thumb_extent, thickness),
        )
    };
    scope.draw_round_rect_at(rect, Brush::Solid(spec.colors.thumb_for(dragging)), radii);
}

#[cfg(test)]
#[path = "tests/scrollbar_tests.rs"]
mod tests;
