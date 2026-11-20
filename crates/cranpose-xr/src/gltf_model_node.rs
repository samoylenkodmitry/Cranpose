//! A node of a glTF model that a ray hit.

use std::sync::Arc;

/// A node of a glTF model: Jetpack XR's `GltfModelNode`, with the `index` and the `name` that it
/// has. [`SpatialGltfModelState::node_of`](crate::SpatialGltfModelState::node_of) gives the node
/// whose mesh a [`HitInfo`](crate::HitInfo) names. Jetpack XR's node also holds the node's poses
/// and scale and takes a material override, which this one does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GltfModelNode {
    /// The index of the node in the file's list of nodes. Jetpack XR counts the nodes of the
    /// model as one flattened list, as the file does.
    pub index: usize,
    /// The name of the node in the file, or `None` for a node with no name.
    pub name: Option<Arc<str>>,
}
