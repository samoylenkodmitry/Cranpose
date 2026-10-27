//! The viewport a lazy list's measure pass works in, from its constraints.

use cranpose_ui_layout::{Axis, Constraints};

use super::LazyListLayoutConfig;

/// The constraint-derived sizes a measure pass works with.
pub(super) struct LazyFrame {
    pub(super) axis: Axis,
    pub(super) raw_viewport_size: f32,
    pub(super) cross_axis_size: f32,
    /// The cross-axis size inside the cross-axis content padding.
    pub(super) item_cross_axis_size: f32,
}

impl LazyFrame {
    pub(super) fn new(constraints: Constraints, axis: Axis, config: &LazyListLayoutConfig) -> Self {
        let (raw_viewport_size, cross_axis_size) = if axis.is_vertical() {
            (constraints.max_height, constraints.max_width)
        } else {
            (constraints.max_width, constraints.max_height)
        };
        let (cross_start, cross_end) = config.cross_padding;
        Self {
            axis,
            raw_viewport_size,
            cross_axis_size,
            item_cross_axis_size: (cross_axis_size - cross_start - cross_end).max(0.0),
        }
    }
}
