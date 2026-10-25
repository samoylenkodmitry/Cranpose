use cranpose_ui_layout::{Constraints, MeasurePolicy, MeasureResult, MeasureScope, Placement};

use crate::layout::core::Measurable;

/// What a layout with nothing in it holds for: it takes its min
/// constraints, so any constraints with the same mins and a max that allows
/// them size it the same.
pub(super) fn empty_layout_hold(
    constraints: Constraints,
    size: crate::modifier::Size,
) -> Option<cranpose_ui_layout::ConstraintsHold> {
    (size.width == constraints.min_width && size.height == constraints.min_height)
        .then(|| cranpose_ui_layout::ConstraintsHold::at_min(size.width, size.height))
}

/// EmptyMeasurePolicy that delegates all measurement to modifier nodes.
///
/// This is used when a Layout has no child layout logic - all measurement
/// is handled by modifier nodes (e.g., TextModifierNode for Text widgets).
/// Matches Jetpack Compose's EmptyMeasurePolicy pattern used in BasicText.
#[derive(Clone, Debug, PartialEq)]
pub struct EmptyMeasurePolicy;

impl EmptyMeasurePolicy {
    /// Creates the policy.
    pub fn new() -> Self {
        Self
    }
}

impl Default for EmptyMeasurePolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl MeasurePolicy for EmptyMeasurePolicy {
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let measurement = self.measure_into(scope, &[], constraints, &mut placements);
        MeasureResult::new(measurement, placements)
    }

    fn measure_into(
        &self,
        _scope: &dyn MeasureScope,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> cranpose_ui_layout::Measurement {
        placements.clear();
        let (width, height) = constraints.constrain(0.0, 0.0);

        crate::modifier::Size { width, height }.into()
    }

    fn measure_hold(
        &self,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        size: crate::modifier::Size,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        empty_layout_hold(constraints, size)
    }

    fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        0.0
    }

    fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        0.0
    }
}
