use super::{inspector_metadata, GraphicsLayer, Modifier};
use crate::modifier_nodes::GraphicsLayerElement;

impl Modifier {
    /// Apply a graphics layer with transformations and alpha.
    ///
    /// Example: `Modifier::empty().graphics_layer(GraphicsLayer { alpha: 0.5, ..Default::default() })`
    pub fn graphics_layer(self, layer: GraphicsLayer) -> Self {
        let inspector_values = layer;
        let modifier = Self::with_element(GraphicsLayerElement::new(layer))
            .with_inspector_metadata(inspector_metadata("graphicsLayer", move |info| {
                info.add_property("alpha", inspector_values.alpha.to_string());
                info.add_property("scale", inspector_values.scale.to_string());
                info.add_property("translationX", inspector_values.translation_x.to_string());
                info.add_property("translationY", inspector_values.translation_y.to_string());
            }));
        self.then(modifier)
    }
}
