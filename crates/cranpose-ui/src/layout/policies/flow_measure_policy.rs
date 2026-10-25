use cranpose_ui_layout::{Constraints, MeasurePolicy, MeasureResult, MeasureScope, Placement};
use smallvec::SmallVec;

use crate::layout::core::Measurable;

/// Which way a flow layout places its children before it wraps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowAxis {
    /// Left to right (right to left in a right-to-left layout), wrapping
    /// onto the next row: Compose's `FlowRow`.
    Horizontal,
    /// Top to bottom, wrapping onto the next column, which starts at the
    /// end side in a right-to-left layout: Compose's `FlowColumn`.
    Vertical,
}

/// MeasurePolicy for `FlowRow` and `FlowColumn`: children go along the main
/// axis and wrap onto the next line when the available length runs out
/// (Jetpack Compose `FlowRow`, `FlowColumn`).
///
/// - Children are measured with loose constraints (min = 0) capped at the
///   incoming max width and height, then packed along the main axis.
/// - A child that no longer fits on the current line starts a new line; a
///   child longer than the whole line gets a line of its own (and may
///   overflow, like Compose).
/// - `main_axis_spacing` separates children on the same line and
///   `cross_axis_spacing` separates lines; children align to the start of
///   their line.
/// - With an unbounded main axis everything stays on one line.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowMeasurePolicy {
    /// Which way children go before they wrap.
    pub axis: FlowAxis,
    /// Gap between adjacent children on the same line, in dp.
    pub main_axis_spacing: f32,
    /// Gap between consecutive lines, in dp.
    pub cross_axis_spacing: f32,
    /// Which side the horizontal axis starts at.
    pub layout_direction: crate::LayoutDirection,
}

impl FlowMeasurePolicy {
    /// Creates a policy along `axis` with the given gaps in logical pixels.
    /// Negative gaps count as zero. The layout direction is left to right.
    pub fn new(axis: FlowAxis, main_axis_spacing: f32, cross_axis_spacing: f32) -> Self {
        Self {
            axis,
            main_axis_spacing: main_axis_spacing.max(0.0),
            cross_axis_spacing: cross_axis_spacing.max(0.0),
            layout_direction: crate::LayoutDirection::Ltr,
        }
    }

    /// `(main, cross)` of a width and height, and back: the same swap.
    fn along(&self, width: f32, height: f32) -> (f32, f32) {
        match self.axis {
            FlowAxis::Horizontal => (width, height),
            FlowAxis::Vertical => (height, width),
        }
    }

    /// A child's intrinsic length along the main axis when the cross axis
    /// has `cross` room.
    fn main_intrinsic(&self, measurable: &dyn Measurable, cross: f32, min: bool) -> f32 {
        match (self.axis, min) {
            (FlowAxis::Horizontal, true) => measurable.min_intrinsic_width(cross),
            (FlowAxis::Horizontal, false) => measurable.max_intrinsic_width(cross),
            (FlowAxis::Vertical, true) => measurable.min_intrinsic_height(cross),
            (FlowAxis::Vertical, false) => measurable.max_intrinsic_height(cross),
        }
    }

    /// A child's intrinsic length across the main axis when it takes `main`.
    fn cross_intrinsic(&self, measurable: &dyn Measurable, main: f32, min: bool) -> f32 {
        match (self.axis, min) {
            (FlowAxis::Horizontal, true) => measurable.min_intrinsic_height(main),
            (FlowAxis::Horizontal, false) => measurable.max_intrinsic_height(main),
            (FlowAxis::Vertical, true) => measurable.min_intrinsic_width(main),
            (FlowAxis::Vertical, false) => measurable.max_intrinsic_width(main),
        }
    }

    /// The cross-axis length the children take when they wrap at
    /// `available_main`.
    fn wrapped_cross(
        &self,
        measurables: &[Box<dyn Measurable>],
        available_main: f32,
        min: bool,
    ) -> f32 {
        let mut cursor = 0.0_f32;
        let mut line_start = 0.0_f32;
        let mut line_cross = 0.0_f32;

        for measurable in measurables {
            let child_main = self.main_intrinsic(measurable.as_ref(), f32::INFINITY, false);
            let child_cross = self.cross_intrinsic(measurable.as_ref(), child_main, min);

            if cursor > 0.0 && cursor + self.main_axis_spacing + child_main > available_main {
                line_start += line_cross + self.cross_axis_spacing;
                cursor = 0.0;
                line_cross = 0.0;
            }
            cursor += if cursor > 0.0 {
                self.main_axis_spacing + child_main
            } else {
                child_main
            };
            line_cross = line_cross.max(child_cross);
        }

        line_start + line_cross
    }

    /// The main-axis length of every child on one line.
    fn unwrapped_main(&self, measurables: &[Box<dyn Measurable>], cross: f32) -> f32 {
        let total_spacing = self.main_axis_spacing * measurables.len().saturating_sub(1) as f32;
        measurables
            .iter()
            .map(|m| self.main_intrinsic(m.as_ref(), cross, false))
            .sum::<f32>()
            + total_spacing
    }

    /// The longest child's smallest main-axis length: a line each.
    fn narrowest_main(&self, measurables: &[Box<dyn Measurable>], cross: f32) -> f32 {
        measurables
            .iter()
            .map(|m| self.main_intrinsic(m.as_ref(), cross, true))
            .fold(0.0, f32::max)
    }
}

impl MeasurePolicy for FlowMeasurePolicy {
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let measurement = self.measure_into(scope, measurables, constraints, &mut placements);
        MeasureResult::new(measurement, placements)
    }

    fn measure_into(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> cranpose_ui_layout::Measurement {
        placements.clear();
        if measurables.is_empty() {
            let (width, height) = constraints.constrain(0.0, 0.0);
            return crate::modifier::Size { width, height }.into();
        }

        let child_constraints = Constraints {
            min_width: 0.0,
            max_width: constraints.max_width,
            min_height: 0.0,
            max_height: constraints.max_height,
        };

        let placeables: SmallVec<[cranpose_ui_layout::Placeable; 8]> = measurables
            .iter()
            .map(|measurable| measurable.measure(child_constraints))
            .collect();

        let (max_main, _) = self.along(constraints.max_width, constraints.max_height);
        let mut cursor = 0.0_f32;
        let mut line_start = 0.0_f32;
        let mut line_cross = 0.0_f32;
        let mut longest_line = 0.0_f32;

        placements.reserve(placeables.len());
        for placeable in &placeables {
            let (child_main, child_cross) = self.along(placeable.width(), placeable.height());

            if cursor > 0.0 && cursor + self.main_axis_spacing + child_main > max_main {
                longest_line = longest_line.max(cursor);
                line_start += line_cross + self.cross_axis_spacing;
                cursor = 0.0;
                line_cross = 0.0;
            }

            let main = if cursor > 0.0 {
                cursor + self.main_axis_spacing
            } else {
                0.0
            };
            let (x, y) = self.along(main, line_start);
            if !self.layout_direction.is_rtl() {
                placeable.place(x, y);
            }
            placements.push(Placement::new(placeable.node_id(), x, y, 0));

            cursor = main + child_main;
            line_cross = line_cross.max(child_cross);
        }
        longest_line = longest_line.max(cursor);

        let (width, height) = self.along(longest_line, line_start + line_cross);
        let width = width.clamp(constraints.min_width, constraints.max_width);
        let height = height.clamp(constraints.min_height, constraints.max_height);
        if self.layout_direction.is_rtl() {
            for (placement, placeable) in placements.iter_mut().zip(&placeables) {
                placement.x = self
                    .layout_direction
                    .place_x(placement.x, width, placeable.width());
                placeable.place(placement.x, placement.y);
            }
        }
        crate::modifier::Size { width, height }.into()
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        match self.axis {
            FlowAxis::Horizontal => self.narrowest_main(measurables, height),
            FlowAxis::Vertical => self.wrapped_cross(measurables, height, true),
        }
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        match self.axis {
            FlowAxis::Horizontal => self.unwrapped_main(measurables, height),
            FlowAxis::Vertical => self.wrapped_cross(measurables, height, false),
        }
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        match self.axis {
            FlowAxis::Horizontal => self.wrapped_cross(measurables, width, true),
            FlowAxis::Vertical => self.narrowest_main(measurables, width),
        }
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        match self.axis {
            FlowAxis::Horizontal => self.wrapped_cross(measurables, width, false),
            FlowAxis::Vertical => self.unwrapped_main(measurables, width),
        }
    }
}
