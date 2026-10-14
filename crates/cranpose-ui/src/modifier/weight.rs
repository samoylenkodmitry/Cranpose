use super::{Modifier, inspector_metadata};
use crate::modifier_nodes::WeightElement;

impl Modifier {
    /// Gives this child a share of the space that the unweighted children leave on the main axis of
    /// a `Row` or `Column`, in proportion to `weight`. The child fills its share. Compose's
    /// `Modifier.weight`.
    pub fn weight(self, weight: f32) -> Self {
        self.weight_with_fill(weight, true)
    }

    /// Like [`Self::weight`]. With `fill` set to false the child may be smaller than its share.
    pub fn weight_with_fill(self, weight: f32, fill: bool) -> Self {
        let modifier = Self::with_element(WeightElement::new(weight, fill))
            .with_inspector_metadata(inspector_metadata("weight", move |info| {
                info.add_property("weight", weight.to_string());
                info.add_property("fill", fill.to_string());
            }));
        self.then(modifier)
    }

    /// Same as [`Self::weight_with_fill`], for a child of a `Column`.
    #[expect(non_snake_case)]
    pub fn columnWeight(self, weight: f32, fill: bool) -> Self {
        self.weight_with_fill(weight, fill)
    }

    /// Same as [`Self::weight_with_fill`], for a child of a `Row`.
    #[expect(non_snake_case)]
    pub fn rowWeight(self, weight: f32, fill: bool) -> Self {
        self.weight_with_fill(weight, fill)
    }
}
