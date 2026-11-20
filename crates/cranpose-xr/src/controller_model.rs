//! The model that draws a controller in the headset.

#[cfg(feature = "gltf")]
use crate::SpatialGltfModelSource;

/// The model that draws one hand's controller in the headset. Jetpack XR has no controller
/// models, so the names are Cranpose's own.
///
/// A model stands at the controller's grip pose in the 3D layer that carries the app's
/// [`SpatialVolume`](crate::SpatialVolume)s. Where the runtime tests depth, a panel and a model
/// hide each other by their distance from the head. [`ControllerModels::set_model`](crate::ControllerModels::set_model)
/// chooses the model of a hand.
///
/// A model that fails to load shows the [`Plain`](Self::Plain) one. A controller in use has a picture
/// once its model has loaded or failed.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum ControllerModel {
    /// The runtime's own model of the controller. A Meta Quest hands out glTF models through
    /// `XR_FB_render_model`. The app asks for them with `Use::controller_models()` in its build
    /// and with the `gltf` feature of this crate. Where the runtime has no model, or the app asked
    /// for none, the hand shows the [`Plain`](Self::Plain) model. This is the default.
    #[default]
    Runtime,
    /// A small controller that Cranpose builds in code. It has a handle, a head, a thumbstick, a
    /// trigger, two buttons and a tip that shows where the controller points. It has no textures
    /// and no asset file.
    Plain,
    /// A glTF 2.0 model of the app's own. Its origin lies at the controller's grip pose. It uses the
    /// axes of the runtime's models: x to the right, y up and z toward the user, in meters. The
    /// model is not scaled or moved to fit a box, so a model with its origin in the middle of the
    /// hand sits in the hand. Needs the `gltf` feature.
    #[cfg(feature = "gltf")]
    Gltf(SpatialGltfModelSource),
}
