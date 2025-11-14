use super::{inspector_metadata, Modifier};
use crate::modifier_nodes::WeightElement;

impl Modifier {
    pub fn weight(self, weight: f32) -> Self {
        self.weight_with_fill(weight, true)
    }

    pub fn weight_with_fill(self, weight: f32, fill: bool) -> Self {
        let modifier = Self::with_element(WeightElement::new(weight, fill)).with_inspector_metadata(
            inspector_metadata("weight", move |info| {
                info.add_property("weight", weight.to_string());
                info.add_property("fill", fill.to_string());
            }),
        );
        self.then(modifier)
    }

    pub fn columnWeight(self, weight: f32, fill: bool) -> Self {
        self.weight_with_fill(weight, fill)
    }

    pub fn rowWeight(self, weight: f32, fill: bool) -> Self {
        self.weight_with_fill(weight, fill)
    }
}
