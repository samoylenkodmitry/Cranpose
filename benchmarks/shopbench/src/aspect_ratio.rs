// A box as wide as its parent allows and as high as `ratio` makes it, for the
// 3:4 product images: Compose's `Modifier.aspectRatio`, which Cranpose has
// no modifier for.

use cranpose::{
    Constraints, Layout, Measurable, MeasureResult, Modifier, Placement, Size, composable,
};
use cranpose_ui_layout::{MeasurePolicy, MeasureScope};

#[derive(Clone, Copy, PartialEq)]
struct AspectRatioPolicy {
    // Width over height.
    ratio: f32,
}

impl AspectRatioPolicy {
    fn size(&self, constraints: Constraints) -> (f32, f32) {
        let (width, height) = if constraints.max_width.is_finite() {
            (constraints.max_width, constraints.max_width / self.ratio)
        } else {
            (constraints.max_height * self.ratio, constraints.max_height)
        };
        constraints.constrain(width, height)
    }
}

impl MeasurePolicy for AspectRatioPolicy {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let (width, height) = self.size(constraints);
        let placements = measurables
            .iter()
            .map(|child| {
                let placeable = child.measure(Constraints::tight(width, height));
                Placement::new(placeable.node_id(), 0.0, 0.0, 0.0)
            })
            .collect();
        MeasureResult::new(Size::new(width, height), placements)
    }

    fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        height * self.ratio
    }

    fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        height * self.ratio
    }

    fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        width / self.ratio
    }

    fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        width / self.ratio
    }
}

#[composable]
pub(crate) fn AspectRatio(ratio: f32, modifier: Modifier, content: impl FnMut() + 'static) {
    Layout(modifier, AspectRatioPolicy { ratio }, content);
}
