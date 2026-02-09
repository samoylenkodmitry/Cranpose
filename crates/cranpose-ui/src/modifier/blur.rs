use super::{inspector_metadata, Modifier};
use crate::modifier_nodes::GraphicsLayerElement;
use cranpose_ui_graphics::{GraphicsLayer, RenderEffect, TileMode};

impl Modifier {
    /// Apply a Gaussian blur effect to this composable's rendered content.
    ///
    /// The blur is applied by rendering the subtree to an offscreen texture
    /// and then applying a separable Gaussian blur post-process.
    ///
    /// Example: `Modifier::empty().blur(10.0)`
    pub fn blur(self, radius: f32) -> Self {
        self.blur_xy(radius, radius, TileMode::default())
    }

    /// Apply a Gaussian blur effect with separate horizontal and vertical radii.
    ///
    /// Example: `Modifier::empty().blur_xy(10.0, 5.0, TileMode::Clamp)`
    pub fn blur_xy(self, radius_x: f32, radius_y: f32, edge_treatment: TileMode) -> Self {
        let layer = GraphicsLayer {
            render_effect: Some(RenderEffect::blur_xy(radius_x, radius_y, edge_treatment)),
            ..Default::default()
        };
        let rx = radius_x;
        let ry = radius_y;
        let modifier = Self::with_element(GraphicsLayerElement::new(layer))
            .with_inspector_metadata(inspector_metadata("blur", move |info| {
                info.add_property("radiusX", rx.to_string());
                info.add_property("radiusY", ry.to_string());
            }));
        self.then(modifier)
    }
}
