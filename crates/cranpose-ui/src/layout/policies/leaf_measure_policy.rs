use cranpose_ui_layout::{Constraints, MeasurePolicy, MeasureResult, MeasureScope, Placement};

use crate::layout::core::Measurables;

/// MeasurePolicy for leaf nodes with fixed intrinsic size (like Spacer).
/// This policy respects the provided constraints but has a preferred intrinsic size.
#[derive(Clone, Debug, PartialEq)]
pub struct LeafMeasurePolicy {
    /// Preferred size in logical pixels. The measured size is this size limited by the incoming
    /// constraints.
    pub intrinsic_size: crate::modifier::Size,
}

impl LeafMeasurePolicy {
    /// Creates a policy whose preferred size is `intrinsic_size`.
    pub fn new(intrinsic_size: crate::modifier::Size) -> Self {
        Self { intrinsic_size }
    }
}

impl MeasurePolicy for LeafMeasurePolicy {
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        _measurables: Measurables<'_>,
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let measurement =
            self.measure_into(scope, Measurables::empty(), constraints, &mut placements);
        MeasureResult::new(measurement, placements)
    }

    fn measure_into(
        &self,
        _scope: &dyn MeasureScope,
        _measurables: Measurables<'_>,
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> cranpose_ui_layout::Measurement {
        placements.clear();
        let (width, height) =
            constraints.constrain(self.intrinsic_size.width, self.intrinsic_size.height);

        crate::modifier::Size { width, height }.into()
    }

    fn min_intrinsic_width(&self, _measurables: Measurables<'_>, _height: f32) -> f32 {
        self.intrinsic_size.width
    }

    fn max_intrinsic_width(&self, _measurables: Measurables<'_>, _height: f32) -> f32 {
        self.intrinsic_size.width
    }

    fn min_intrinsic_height(&self, _measurables: Measurables<'_>, _width: f32) -> f32 {
        self.intrinsic_size.height
    }

    fn max_intrinsic_height(&self, _measurables: Measurables<'_>, _width: f32) -> f32 {
        self.intrinsic_size.height
    }
}
