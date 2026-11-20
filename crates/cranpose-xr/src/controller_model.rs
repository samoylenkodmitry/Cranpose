//! The model that draws a controller in the headset.

#[cfg(feature = "gltf")]
use crate::SpatialGltfModelSource;

/// The model that draws one hand's controller in the headset. Jetpack XR has no controller
/// models, so the names are Cranpose's own.
///
/// A model stands at the controller's grip pose, in the 3D layer that carries the app's
/// [`SpatialVolume`](crate::SpatialVolume)s, so a panel and a model hide each other by their
/// distance from the head where the runtime tests depth (see
/// [`SpatialVolume`](crate::SpatialVolume)). [`ControllerModels::set_model`](crate::ControllerModels::set_model)
/// chooses it for a hand.
///
/// A model that fails to load shows the [`Plain`](Self::Plain) one, so a controller in use never
/// goes without a picture.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum ControllerModel {
    /// The runtime's own model of the controller, where the runtime has one and the app declared
    /// `Use::controller_models()` in its build and turned on the `gltf` feature of this crate: a Meta
    /// Quest hands out glTF models through `XR_FB_render_model`. Where the runtime has none, or the
    /// app did not ask for them, the hand shows the [`Plain`](Self::Plain) model. This is the
    /// default.
    #[default]
    Runtime,
    /// A small controller that Cranpose builds in code: a grip, a head, a stick, a trigger and a
    /// tip that shows where the controller points. It has no textures and no asset file.
    Plain,
    /// A glTF 2.0 model of the app's own. Its origin lies at the controller's grip pose, with the
    /// glTF axes of the runtime's models: x to the right, y up and z toward the user, in meters.
    /// The model is not scaled or moved to fit a box, so a model with its origin in the middle of
    /// the hand sits in the hand. Needs the `gltf` feature.
    #[cfg(feature = "gltf")]
    Gltf(SpatialGltfModelSource),
}
