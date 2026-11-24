//! The viewport a lazy list's measure pass works in, from its constraints.

use cranpose_foundation::lazy::LazyListMeasureResult;
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

pub(super) struct PlacedLazyFrame {
    pub(super) frame: LazyFrame,
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) main_axis_size: f32,
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

    // Grids keep the incoming cross-axis bound because their cells split that span;
    // lists wrap their widest or tallest measured item plus cross-axis padding.
    pub(super) fn resolve_placement(
        &self,
        constraints: Constraints,
        result: &LazyListMeasureResult,
        config: &LazyListLayoutConfig,
    ) -> PlacedLazyFrame {
        let (cross_start, cross_end) = config.cross_padding;
        let (min_main, max_main, min_cross, max_cross) = if self.axis.is_vertical() {
            (
                constraints.min_height,
                constraints.max_height,
                constraints.min_width,
                constraints.max_width,
            )
        } else {
            (
                constraints.min_width,
                constraints.max_width,
                constraints.min_height,
                constraints.max_height,
            )
        };
        let cross_axis_size = if config.grid.is_some() {
            self.cross_axis_size
        } else if min_cross >= max_cross && max_cross.is_finite() {
            max_cross
        } else {
            let max_item_cross = result
                .visible_items
                .iter()
                .map(|item| item.cross_axis_size)
                .fold(0.0_f32, f32::max);
            (max_item_cross + cross_start + cross_end)
                .max(min_cross)
                .min(max_cross)
        };
        let main_axis_size = if max_main.is_finite() {
            result.total_content_size.clamp(min_main, max_main)
        } else {
            result
                .total_content_size
                .min(result.viewport_size)
                .max(min_main)
        };
        let (width, height) = if self.axis.is_vertical() {
            (cross_axis_size, main_axis_size)
        } else {
            (main_axis_size, cross_axis_size)
        };
        PlacedLazyFrame {
            frame: Self {
                axis: self.axis,
                raw_viewport_size: self.raw_viewport_size,
                cross_axis_size,
                item_cross_axis_size: (cross_axis_size - cross_start - cross_end).max(0.0),
            },
            width,
            height,
            main_axis_size,
        }
    }
}
