//! Where a glTF model comes from.

use std::{
    borrow::Cow,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::gltf_error::GltfError;

/// Where a [`SpatialGltfModel`](crate::SpatialGltfModel) loads its model from. Jetpack XR's
/// `SpatialGltfModelSource`.
///
/// Jetpack XR loads only binary `.glb` files, from the app's `assets/` folder, from a content
/// `Uri` or from an Android resource. Cranpose loads `.glb` and `.gltf` files, with the buffers
/// and images of a `.gltf` embedded as `data:` URIs or next to it in files. A source is a file
/// system path or bytes. It has no `fromUri` and no `fromResource`: the Android assets of an APK
/// are not files, so an Android app ships a model with `include_bytes!` and
/// [`from_bytes`](Self::from_bytes).
///
/// Two sources are equal when they name the same path or share the same bytes, so a state keyed by
/// a source loads again only when the source changes. A source made from bytes is a new source each
/// time it is made, so a composable keeps it with `remember`.
#[derive(Clone)]
pub struct SpatialGltfModelSource(Origin);

#[derive(Clone)]
enum Origin {
    Path(PathBuf),
    Bytes(Arc<[u8]>),
}

// A source that holds bytes prints their number, because a model is megabytes.
impl fmt::Debug for SpatialGltfModelSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Origin::Path(path) => f.debug_tuple("SpatialGltfModelSource").field(path).finish(),
            Origin::Bytes(bytes) => write!(f, "SpatialGltfModelSource({} bytes)", bytes.len()),
        }
    }
}

impl PartialEq for SpatialGltfModelSource {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Origin::Path(a), Origin::Path(b)) => a == b,
            (Origin::Bytes(a), Origin::Bytes(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl SpatialGltfModelSource {
    /// A model in the file at `path`, a `.glb` or a `.gltf`. The buffers and images that a `.gltf`
    /// names by relative URI are read from files next to it. Jetpack XR's `fromPath`, which takes a
    /// path relative to the `assets/` folder: Cranpose takes the path as the file system has it.
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self(Origin::Path(path.into()))
    }

    /// A model in `bytes`, the content of a `.glb` or of a `.gltf` whose buffers and images are
    /// embedded. A `.gltf` that names an external file fails to load, because bytes have no
    /// directory to find it in. Jetpack XR has no counterpart.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self(Origin::Bytes(bytes.into()))
    }

    // The file's bytes and the directory that its relative URIs name files in, or the shared bytes
    // with no directory.
    pub(crate) fn read(&self) -> Result<(Cow<'_, [u8]>, Option<&Path>), GltfError> {
        match &self.0 {
            Origin::Path(path) => {
                let bytes = std::fs::read(path).map_err(|error| GltfError::Unreadable {
                    location: path.display().to_string(),
                    reason: error.to_string(),
                })?;
                Ok((Cow::Owned(bytes), path.parent()))
            }
            Origin::Bytes(bytes) => Ok((Cow::Borrowed(bytes), None)),
        }
    }
}
