//! Why a glTF model does not load.

/// Why a glTF model did not load. It is the reason in
/// [`SpatialGltfModelStatus::Failed`](crate::SpatialGltfModelStatus::Failed), where Jetpack XR
/// carries the exception that the load threw.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GltfError {
    /// The file of a model, or a file that it names, could not be read.
    #[error("cannot read {location}: {reason}")]
    Unreadable {
        /// The path or the URI that failed.
        location: String,
        /// What the system or the decoder said.
        reason: String,
    },
    /// The bytes are not a glTF 2.0 file, or the document points at things that do not exist.
    #[error("not a valid glTF 2.0 file: {0}")]
    Invalid(String),
    /// A buffer has no data, or less than it says. A model from bytes has no directory to find an
    /// external buffer in.
    #[error("buffer {index} has no usable data: {reason}")]
    MissingBuffer {
        /// The buffer's index in the document.
        index: usize,
        /// Why the data is missing.
        reason: String,
    },
    /// An image could not be read or decoded.
    #[error("image {index}: {reason}")]
    Image {
        /// The image's index in the document.
        index: usize,
        /// What the decoder said.
        reason: String,
    },
    /// The vertices and indices of a mesh primitive do not make a mesh.
    #[error("mesh {mesh}, primitive {primitive}: {reason}")]
    Mesh {
        /// The mesh's index in the document.
        mesh: usize,
        /// The primitive's index in the mesh.
        primitive: usize,
        /// Which part is wrong.
        reason: String,
    },
    /// An animation's keyframes are missing or out of order.
    #[error("animation {index}: {reason}")]
    Animation {
        /// The animation's index in the document.
        index: usize,
        /// Which part is wrong.
        reason: String,
    },
    /// The worker thread that reads the model stopped before it finished.
    #[error("the load did not finish: {0}")]
    Aborted(String),
    /// The scene holds no triangle to draw.
    #[error("the model has no triangles to draw")]
    Empty,
}
