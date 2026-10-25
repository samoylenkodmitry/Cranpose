use cranpose_ui_layout::{
    Constraints, MeasurePolicy, MeasureResult, MeasureScope, ParentData, Placement,
};
use smallvec::SmallVec;

use super::empty_measure_policy::empty_layout_hold;
use crate::layout::core::{Alignment, Measurable};

/// MeasurePolicy for Box layout - overlays children according to alignment.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxMeasurePolicy {
    /// Where children sit inside the box when they are smaller than it.
    pub content_alignment: Alignment,
    /// Whether the box's minimum constraints pass on to its children. When `false`, children may
    /// measure smaller than the box's minimum size.
    pub propagate_min_constraints: bool,
    /// Direction used to resolve start and end alignment.
    pub layout_direction: crate::LayoutDirection,
}

impl BoxMeasurePolicy {
    /// Creates a policy that places children by `content_alignment` in a left-to-right layout
    /// direction.
    pub fn new(content_alignment: Alignment, propagate_min_constraints: bool) -> Self {
        Self {
            content_alignment,
            propagate_min_constraints,
            layout_direction: crate::LayoutDirection::Ltr,
        }
    }
}

impl MeasurePolicy for BoxMeasurePolicy {
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

    /// A box holds while every child holds under the constraints it hands
    /// on and the largest child, which sizes the box, still fits.
    fn measure_hold(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        size: crate::modifier::Size,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        if measurables.is_empty() {
            return empty_layout_hold(constraints, size);
        }
        let mut width = cranpose_ui_layout::BoundRange::ANY;
        let mut height = cranpose_ui_layout::BoundRange::ANY;
        let mut held = cranpose_ui_layout::ConstraintsHold::sized(size.width, size.height);
        let mut largest = crate::modifier::Size::default();
        for measurable in measurables {
            let (child, hold) = measurable.measured_hold()?;
            if self.propagate_min_constraints {
                held = held.intersect(hold)?;
            } else {
                width = width.intersect(hold.width.max)?;
                height = height.intersect(hold.height.max)?;
            }
            largest.width = largest.width.max(child.width);
            largest.height = largest.height.max(child.height);
        }
        // Bounds that cut or stretched the box size it from themselves.
        if size != largest {
            return None;
        }
        held.width.max = held.width.max.intersect(width)?;
        held.height.max = held.height.max.intersect(height)?;
        Some(held)
    }

    /// A box reads only where each child aligns.
    fn reads_parent_data_change(&self, before: &ParentData, after: &ParentData) -> bool {
        before.box_alignment != after.box_alignment
    }

    /// A box whose constraints fix its size places a child at its top start
    /// at the origin, whatever the child's size.
    fn ignores_child_sizes(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> bool {
        constraints.min_width == constraints.max_width
            && constraints.min_height == constraints.max_height
            && !self.layout_direction.is_rtl()
            && measurables.iter().all(|measurable| {
                measurable
                    .parent_data()
                    .box_alignment
                    .unwrap_or(self.content_alignment)
                    == Alignment::TOP_START
            })
    }

    fn measure_into(
        &self,
        scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> cranpose_ui_layout::Measurement {
        placements.clear();
        let child_constraints = if self.propagate_min_constraints {
            constraints
        } else {
            Constraints {
                min_width: 0.0,
                max_width: constraints.max_width,
                min_height: 0.0,
                max_height: constraints.max_height,
            }
        };

        let mut max_width = 0.0_f32;
        let mut max_height = 0.0_f32;
        let mut placeables: SmallVec<[(cranpose_ui_layout::Placeable, Alignment); 8]> =
            SmallVec::new();

        for measurable in measurables {
            let placeable = measurable.measure(child_constraints);
            max_width = max_width.max(placeable.width());
            max_height = max_height.max(placeable.height());
            let alignment = measurable
                .parent_data()
                .box_alignment
                .unwrap_or(self.content_alignment);
            placeables.push((placeable, alignment));
        }

        let width = max_width.clamp(constraints.min_width, constraints.max_width);
        let height = max_height.clamp(constraints.min_height, constraints.max_height);

        placements.reserve(placeables.len());
        // In place: a by-value loop moves the list's inline storage, hundreds
        // of bytes, before its first step.
        for (placeable, alignment) in &placeables {
            let child_width = placeable.width();
            let child_height = placeable.height();

            let x = alignment
                .horizontal
                .align(width, child_width, scope.density());
            let x = self.layout_direction.place_x(x, width, child_width);
            let y = alignment
                .vertical
                .align(height, child_height, scope.density());

            placeable.place(x, y);
            placements.push(Placement::new(placeable.node_id(), x, y, 0));
        }

        crate::modifier::Size { width, height }.into()
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.min_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.max_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.min_intrinsic_height(width))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.max_intrinsic_height(width))
            .fold(0.0, f32::max)
    }
}
