//! Whether a glTF model has loaded.

use crate::gltf_error::GltfError;

/// How far the load of a glTF model has come. Jetpack XR's `SpatialGltfModelStatus`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpatialGltfModelStatus {
    /// The model is being read and decoded on a worker thread. It is not ready to show.
    Loading,
    /// The model is loaded and shows.
    Loaded,
    /// The model did not load. Jetpack XR's `Failed` carries the exception that the load threw.
    Failed(GltfError),
}
