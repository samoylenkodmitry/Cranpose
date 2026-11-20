//! The KTX2 images of a glTF model.
//!
//! A texture of the `KHR_texture_basisu` extension holds a Basis Universal texture in a KTX2 file
//! in place of a PNG or a JPEG. The controller models of a Meta Quest use it. The transcoder turns
//! the file into pixels.

use basisu::{DecodeFlags, TargetFormat, Transcoder};

use crate::gltf_material::{MAX_SIDE, Texture};

// The first twelve bytes of every KTX2 file.
const IDENTIFIER: [u8; 12] = [
    0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB, b'\r', b'\n', 0x1A, b'\n',
];

// Whether `bytes` start as a KTX2 file.
pub(crate) fn is_ktx2(bytes: &[u8]) -> bool {
    bytes.starts_with(&IDENTIFIER)
}

// The pixels of the KTX2 file in `bytes`, from the first mip level that fits the longest side that a
// texture may have.
pub(crate) fn decode(bytes: &[u8]) -> Result<Texture, String> {
    let transcoder = Transcoder::new(bytes).map_err(|error| format!("{error:?}"))?;
    let level = (0..transcoder.level_count())
        .find(|&level| {
            transcoder
                .image_level_info(level)
                .is_ok_and(|info| info.width <= MAX_SIDE && info.height <= MAX_SIDE)
        })
        .ok_or_else(|| format!("no mip level is within {MAX_SIDE} pixels"))?;
    let info = transcoder
        .image_level_info(level)
        .map_err(|error| format!("{error:?}"))?;
    let rgba = transcoder
        .transcode(level, TargetFormat::Rgba32, DecodeFlags::NONE)
        .map_err(|error| format!("{error:?}"))?;
    Ok(Texture {
        width: info.width,
        height: info.height,
        rgba,
    })
}
