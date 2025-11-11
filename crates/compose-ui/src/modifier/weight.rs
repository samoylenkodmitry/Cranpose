use super::Modifier;
use crate::modifier_nodes::WeightElement;

impl Modifier {
    pub fn weight(weight: f32) -> Self {
        Self::weight_with_fill(weight, true)
    }

    pub fn weight_with_fill(weight: f32, fill: bool) -> Self {
        Self::with_element(WeightElement::new(weight, fill))
    }

    pub fn columnWeight(self, weight: f32, fill: bool) -> Self {
        self.then(Self::weight_with_fill(weight, fill))
    }

    pub fn rowWeight(self, weight: f32, fill: bool) -> Self {
        self.then(Self::weight_with_fill(weight, fill))
    }
}
