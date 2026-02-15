use super::{inspector_metadata, Modifier};
use crate::modifier_nodes::LazyGraphicsLayerElement;
use cranpose_ui_graphics::{BlurredEdgeTreatment, Dp, GraphicsLayer, LayerShape, RenderEffect};
use std::rc::Rc;

impl Modifier {
    /// Apply a Gaussian blur effect to this composable's rendered content.
    ///
    /// `radius` is expressed in Dp and converted to px using the current render
    /// density when modifier slices are evaluated.
    ///
    /// Compose parity: this defaults to bounded rectangular edge treatment.
    pub fn blur(self, radius: Dp) -> Self {
        self.blur_with_edge_treatment(radius, BlurredEdgeTreatment::default())
    }

    /// Apply an isotropic Gaussian blur with explicit edge treatment.
    ///
    /// Compose parity:
    /// - bounded treatment clips to shape and uses clamp sampling
    /// - unbounded treatment disables clip and uses decal sampling
    pub fn blur_with_edge_treatment(
        self,
        radius: Dp,
        edge_treatment: BlurredEdgeTreatment,
    ) -> Self {
        self.blur_xy(radius, radius, edge_treatment)
    }

    /// Apply a Gaussian blur effect with separate horizontal and vertical radii.
    ///
    /// Radii are expressed in Dp and converted to px at evaluation time.
    pub fn blur_xy(self, radius_x: Dp, radius_y: Dp, edge_treatment: BlurredEdgeTreatment) -> Self {
        if radius_x.0 <= 0.0 && radius_y.0 <= 0.0 && !edge_treatment.clip() {
            return self;
        }

        let modifier = Self::with_element(LazyGraphicsLayerElement::new(Rc::new(move || {
            let density = crate::render_state::current_density();
            let radius_x_px = radius_x.to_px(density).max(0.0);
            let radius_y_px = radius_y.to_px(density).max(0.0);
            let render_effect = if radius_x_px > 0.0 && radius_y_px > 0.0 {
                Some(RenderEffect::blur_xy(
                    radius_x_px,
                    radius_y_px,
                    edge_treatment.tile_mode(),
                ))
            } else {
                None
            };
            GraphicsLayer {
                render_effect,
                shape: edge_treatment.shape().unwrap_or(LayerShape::Rectangle),
                clip: edge_treatment.clip(),
                ..Default::default()
            }
        })))
        .with_inspector_metadata(inspector_metadata("blur", move |info| {
            info.add_property("radiusX", radius_x.0.to_string());
            info.add_property("radiusY", radius_y.0.to_string());
            info.add_property("edgeTreatment", format!("{edge_treatment:?}"));
        }));
        self.then(modifier)
    }
}
