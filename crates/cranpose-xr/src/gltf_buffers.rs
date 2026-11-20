//! The bytes that a glTF document points at: its buffers and its images.

use std::path::Path;

use base64::{Engine, engine::general_purpose::STANDARD};
use percent_encoding::percent_decode_str;

use crate::gltf_error::GltfError;

// The data of every buffer of a document, by the buffer's index.
pub(crate) struct Buffers(Vec<Vec<u8>>);

impl Buffers {
    // Reads each buffer: the binary chunk of a `.glb`, a `data:` URI, or a file named relative to
    // `base`.
    pub(crate) fn load(
        document: &gltf::Document,
        blob: Option<Vec<u8>>,
        base: Option<&Path>,
    ) -> Result<Self, GltfError> {
        let mut blob = blob;
        let mut data = Vec::new();
        for buffer in document.buffers() {
            let index = buffer.index();
            let missing = |reason: String| GltfError::MissingBuffer { index, reason };
            let bytes = match buffer.source() {
                gltf::buffer::Source::Bin => blob
                    .take()
                    .ok_or_else(|| missing("the file has no binary chunk for it".into()))?,
                gltf::buffer::Source::Uri(uri) => read_uri(uri, base).map_err(missing)?,
            };
            if bytes.len() < buffer.length() {
                return Err(missing(format!(
                    "it holds {} bytes and the document says {}",
                    bytes.len(),
                    buffer.length()
                )));
            }
            data.push(bytes);
        }
        Ok(Self(data))
    }

    // The data of `buffer`, for the readers of accessors.
    pub(crate) fn data(&self, buffer: gltf::Buffer<'_>) -> Option<&[u8]> {
        self.0.get(buffer.index()).map(Vec::as_slice)
    }

    // The encoded bytes of `image`: a view into a buffer, or what its URI names.
    pub(crate) fn image(
        &self,
        image: &gltf::Image<'_>,
        base: Option<&Path>,
    ) -> Result<Vec<u8>, GltfError> {
        let failed = |reason: String| GltfError::Image {
            index: image.index(),
            reason,
        };
        match image.source() {
            gltf::image::Source::View { view, .. } => {
                let data = self
                    .data(view.buffer())
                    .ok_or_else(|| failed("its buffer is missing".into()))?;
                let end = view.offset().saturating_add(view.length());
                data.get(view.offset()..end)
                    .map(<[u8]>::to_vec)
                    .ok_or_else(|| failed("its buffer view lies outside its buffer".into()))
            }
            gltf::image::Source::Uri { uri, .. } => read_uri(uri, base).map_err(failed),
        }
    }
}

// The bytes that `uri` names: the payload of a `data:` URI, or the file at a path relative to
// `base`. A URI with a scheme, such as `https:`, and an absolute path are not read.
fn read_uri(uri: &str, base: Option<&Path>) -> Result<Vec<u8>, String> {
    if let Some(data) = uri.strip_prefix("data:") {
        let (header, payload) = data
            .split_once(',')
            .ok_or("a data URI has no comma before its payload")?;
        return if header.ends_with(";base64") {
            STANDARD
                .decode(payload)
                .map_err(|error| format!("a data URI is not base64: {error}"))
        } else {
            Ok(percent_decode_str(payload).collect())
        };
    }
    let has_scheme = uri
        .split_once(':')
        .is_some_and(|(scheme, _)| !scheme.is_empty() && !scheme.contains('/'));
    if has_scheme || uri.starts_with('/') {
        return Err(format!("{uri} is not a relative path or a data URI"));
    }
    let base = base.ok_or_else(|| {
        format!("{uri} is a file, and a model from bytes has no directory to find it in")
    })?;
    let relative = percent_decode_str(uri)
        .decode_utf8()
        .map_err(|error| format!("{uri} is not UTF-8: {error}"))?;
    let path = base.join(&*relative);
    std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))
}
