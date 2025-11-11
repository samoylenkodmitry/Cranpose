use super::{inspector_metadata, GraphicsLayer, Modifier};
use crate::modifier_nodes::GraphicsLayerElement;

impl Modifier {
    pub fn graphics_layer(layer: GraphicsLayer) -> Self {
        Self::with_element(GraphicsLayerElement::new(layer)).with_inspector_metadata(
            inspector_metadata("graphicsLayer", move |info| {
                info.add_property("graphicsLayer", format!("{layer:?}"));
            }),
        )
    }
}
