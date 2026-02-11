use super::{inspector_metadata, GraphicsLayer, Modifier};
use crate::modifier_nodes::{GraphicsLayerElement, LazyGraphicsLayerElement};
use cranpose_ui_graphics::{Color, ColorFilter, RenderEffect, RuntimeShader};
use std::rc::Rc;

impl Modifier {
    /// Apply a graphics layer with transformations and alpha.
    ///
    /// Example: `Modifier::empty().graphics_layer(GraphicsLayer { alpha: 0.5, ..Default::default() })`
    pub fn graphics_layer(self, layer: GraphicsLayer) -> Self {
        let inspector_values = layer.clone();
        let modifier = Self::with_element(GraphicsLayerElement::new(layer))
            .with_inspector_metadata(inspector_metadata("graphicsLayer", move |info| {
                info.add_property("alpha", inspector_values.alpha.to_string());
                info.add_property("scale", inspector_values.scale.to_string());
                info.add_property("translationX", inspector_values.translation_x.to_string());
                info.add_property("translationY", inspector_values.translation_y.to_string());
                if let Some(filter) = inspector_values.color_filter {
                    info.add_property("colorFilter", format!("{filter:?}"));
                }
            }));
        self.then(modifier)
    }

    /// Apply a lazily evaluated graphics layer.
    ///
    /// The closure is evaluated during scene building, not composition, which lets
    /// layer properties update without forcing recomposition.
    pub fn graphics_layer_lazy(self, layer: impl Fn() -> GraphicsLayer + 'static) -> Self {
        let modifier = Self::with_element(LazyGraphicsLayerElement::new(Rc::new(layer)))
            .with_inspector_metadata(inspector_metadata("graphicsLayer", |info| {
                info.add_property("lazy", "true");
            }));
        self.then(modifier)
    }

    /// Apply a backdrop effect to content behind this composable's bounds.
    pub fn backdrop_effect(self, effect: RenderEffect) -> Self {
        let layer = GraphicsLayer {
            backdrop_effect: Some(effect),
            ..Default::default()
        };
        let modifier = Self::with_element(GraphicsLayerElement::new(layer))
            .with_inspector_metadata(inspector_metadata("backdropEffect", |info| {
                info.add_property("enabled", "true");
            }));
        self.then(modifier)
    }

    /// Convenience alias for applying a backdrop shader effect.
    pub fn shader_background(self, shader: RuntimeShader) -> Self {
        self.backdrop_effect(RenderEffect::runtime_shader(shader))
    }

    /// Apply a color filter to this composable's graphics layer output.
    ///
    /// This mirrors Compose's `graphicsLayer(colorFilter = ...)` capability.
    pub fn color_filter(self, filter: ColorFilter) -> Self {
        let layer = GraphicsLayer {
            color_filter: Some(filter),
            ..Default::default()
        };
        let modifier = Self::with_element(GraphicsLayerElement::new(layer))
            .with_inspector_metadata(inspector_metadata("colorFilter", |info| {
                info.add_property("enabled", "true");
            }));
        self.then(modifier)
    }

    /// Convenience color filter that tints layer output.
    pub fn tint(self, tint: Color) -> Self {
        self.color_filter(ColorFilter::tint(tint))
    }
}
