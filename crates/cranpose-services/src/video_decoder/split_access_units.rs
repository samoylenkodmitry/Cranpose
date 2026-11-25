//! The access units of a stream that an app reads from a file or a network.

use super::{VideoCodec, annex_b::AccessUnitNals};

/// Splits an Annex-B stream of H.264 or HEVC into access units, as [`AccessUnit::data`] takes
/// them: each holds the NAL units of one picture with their start codes, and the parameter sets
/// before the picture that they precede.
///
/// An access unit begins at an access unit delimiter, at a parameter set that follows a slice,
/// and at a picture's first slice that follows another slice, so a stream with no delimiters
/// splits as well. Bytes before the first start code belong to no unit. A stream of AV1 has no
/// start codes, and comes back as one unit.
///
/// [`AccessUnit::data`]: crate::AccessUnit::data
pub fn split_access_units(codec: VideoCodec, stream: &[u8]) -> Vec<&[u8]> {
    if matches!(codec, VideoCodec::Av1 | VideoCodec::Vp9) {
        return vec![stream];
    }
    let mut nals = AccessUnitNals::default();
    nals.parse(codec, stream);
    let starts = nals.unit_starts(stream);
    let ends = starts.iter().skip(1).copied().chain([stream.len()]);
    starts
        .iter()
        .zip(ends)
        .map(|(start, end)| &stream[*start..end])
        .collect()
}
