use super::{inspector_metadata, GraphicsLayer, Modifier};

impl Modifier {
    pub fn graphics_layer(layer: GraphicsLayer) -> Self {
        Self::with_state(move |state| {
            state.graphics_layer = Some(layer);
        })
        .with_inspector_metadata(inspector_metadata("graphicsLayer", move |info| {
            info.add_property("graphicsLayer", format!("{layer:?}"));
        }))
    }
}
