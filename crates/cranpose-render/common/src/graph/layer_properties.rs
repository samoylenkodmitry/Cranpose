// A layer node's graphics layer with implicit defaults.

use std::ops::{Deref, DerefMut};

use cranpose_ui::GraphicsLayer;

use super::graphics_layers_read_alike;

/// Scene properties with implicit defaults and independently mutable clones.
#[derive(Clone, Debug, Default)]
pub struct LayerProperties(Option<Box<GraphicsLayer>>);

impl LayerProperties {
    /// The properties that were set, or `None` when they read as defaults.
    pub fn get(&self) -> Option<&GraphicsLayer> {
        self.0.as_deref()
    }
}

impl Deref for LayerProperties {
    type Target = GraphicsLayer;

    fn deref(&self) -> &Self::Target {
        self.0.as_deref().unwrap_or(&GraphicsLayer::DEFAULT)
    }
}

impl DerefMut for LayerProperties {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.get_or_insert_with(Box::default)
    }
}

impl From<GraphicsLayer> for LayerProperties {
    fn from(value: GraphicsLayer) -> Self {
        Self(Some(Box::new(value)))
    }
}

impl From<Option<GraphicsLayer>> for LayerProperties {
    fn from(value: Option<GraphicsLayer>) -> Self {
        Self(value.map(Box::new))
    }
}

impl PartialEq for LayerProperties {
    fn eq(&self, other: &Self) -> bool {
        graphics_layers_read_alike(self.get(), other.get())
    }
}
