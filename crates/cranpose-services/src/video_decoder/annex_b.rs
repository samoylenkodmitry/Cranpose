//! The NAL units of an Annex-B stream, sorted by what a decoder does with them.
//!
//! A decoder of Apple's takes the parameter sets apart from the pictures, and takes each
//! picture unit with a length in front instead of a start code.

use super::VideoCodec;

// What a decoder does with a NAL unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    // A slice of a picture. `key` tells that it starts a decodable sequence: an IDR slice, or
    // an HEVC IRAP slice. `first` tells that it is the picture's first slice.
    Slice { key: bool, first: bool },
    // An SEI message or another unit that the decoder reads and that holds no slice.
    Other,
    // A parameter set, which describes the stream's format.
    ParameterSet(Role),
    // An access unit delimiter, which begins an access unit.
    Delimiter,
    // A unit that no decoder needs: filler or the end of a sequence.
    Skipped,
}

// The part of a stream's format that a parameter set carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Video,
    Sequence,
    Picture,
}

// A NAL unit's bytes in the stream, after its start code.
#[derive(Clone, Copy, Debug)]
struct Nal {
    start: usize,
    #[cfg_attr(
        not(any(
            test,
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
            target_os = "windows",
            target_os = "linux",
            all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
        )),
        expect(
            dead_code,
            reason = "only targets with hardware decoders read NAL payload slices"
        )
    )]
    end: usize,
    kind: Kind,
}

// The size of the length that precedes a NAL unit in the decoder's input
#[cfg(any(test, target_os = "macos", target_os = "ios"))]
pub(super) const LENGTH_SIZE: usize = 4;

// The NAL units of one access unit or of a stream. The list is reused from one parse to the
// next.
#[derive(Default)]
pub(super) struct AccessUnitNals {
    nals: Vec<Nal>,
}

impl AccessUnitNals {
    // Splits `data` at its start codes, and classifies each unit for `codec`. Bytes before the
    // first start code belong to no unit.
    pub(super) fn parse(&mut self, codec: VideoCodec, data: &[u8]) {
        self.nals.clear();
        let mut start = None;
        let mut at = 0;
        // The third byte of a start code decides where the next one can begin, so most bytes
        // are read once in three.
        while at + 3 <= data.len() {
            match data[at + 2] {
                0 => at += 1,
                1 if data[at] == 0 && data[at + 1] == 0 => {
                    if let Some(start) = start {
                        self.push(codec, data, start, at);
                    }
                    start = Some(at + 3);
                    at += 3;
                }
                _ => at += 3,
            }
        }
        if let Some(start) = start {
            self.push(codec, data, start, data.len());
        }
    }

    // Adds the unit between `start` and `end`. Zero bytes before the next start code, and the
    // first zero of a start code of four bytes, do not belong to the unit.
    fn push(&mut self, codec: VideoCodec, data: &[u8], start: usize, mut end: usize) {
        while end > start && data[end - 1] == 0 {
            end -= 1;
        }
        if end > start {
            self.nals.push(Nal {
                start,
                end,
                kind: classify(codec, &data[start..end]),
            });
        }
    }

    // The offsets in `data` at which access units begin, after `parse`. A unit begins at an
    // access unit delimiter, at a parameter set after a slice, and at a picture's first slice
    // after another slice. An SEI message that nothing else separates from the slice before it
    // stays with that unit.
    pub(super) fn unit_starts(&self, data: &[u8]) -> Vec<usize> {
        let mut starts: Vec<usize> = Vec::new();
        let mut in_picture = false;
        for (index, nal) in self.nals.iter().enumerate() {
            let begins = index == 0
                || match nal.kind {
                    Kind::Delimiter | Kind::ParameterSet(_) => in_picture,
                    Kind::Slice { first, .. } => first && in_picture,
                    Kind::Other | Kind::Skipped => false,
                };
            if begins {
                // The start code has three bytes, and a fourth zero before them in a stream
                // that writes four.
                let code = if nal.start >= 4 && data[nal.start - 4] == 0 {
                    4
                } else {
                    3
                };
                let offset = nal.start - code;
                if starts.last() != Some(&offset) {
                    starts.push(offset);
                }
                in_picture = false;
            }
            in_picture |= matches!(nal.kind, Kind::Slice { .. });
        }
        starts
    }

    // Whether the access unit holds a slice, so that a decoder gives a picture for it.
    #[cfg(any(test, target_os = "macos", target_os = "ios"))]
    pub(super) fn has_picture(&self) -> bool {
        self.nals
            .iter()
            .any(|nal| matches!(nal.kind, Kind::Slice { .. }))
    }

    // Whether the access unit holds a slice that a decoder can start with.
    #[cfg(any(
        test,
        target_os = "macos",
        target_os = "ios",
        all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
    ))]
    pub(super) fn has_keyframe(&self) -> bool {
        self.nals
            .iter()
            .any(|nal| matches!(nal.kind, Kind::Slice { key: true, .. }))
    }

    // The first sequence parameter set NAL payload in the parsed access unit, if any.
    #[cfg(any(
        test,
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "linux",
        all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
    ))]
    pub(super) fn sequence_parameter_set<'a>(&self, data: &'a [u8]) -> Option<&'a [u8]> {
        self.nals
            .iter()
            .find(|nal| nal.kind == Kind::ParameterSet(Role::Sequence))
            .map(|nal| &data[nal.start..nal.end])
    }

    // The parameter sets in the order of the access unit, if it holds a full set: a sequence
    // set and a picture set, and for HEVC a video set too. A unit that holds a part of a set
    // has none.
    #[cfg(any(test, target_os = "macos", target_os = "ios"))]
    pub(super) fn parameter_sets<'a>(
        &'a self,
        codec: VideoCodec,
        data: &'a [u8],
    ) -> Option<impl Iterator<Item = &'a [u8]> + Clone> {
        let has = |role| {
            self.nals
                .iter()
                .any(|nal| nal.kind == Kind::ParameterSet(role))
        };
        let full = has(Role::Sequence)
            && has(Role::Picture)
            && (codec != VideoCodec::Hevc || has(Role::Video));
        full.then(|| {
            self.nals
                .iter()
                .filter(|nal| matches!(nal.kind, Kind::ParameterSet(_)))
                .map(|nal| &data[nal.start..nal.end])
        })
    }

    // The bytes that the units for the decoder take with a length before each.
    #[cfg(any(test, target_os = "macos", target_os = "ios"))]
    pub(super) fn length_prefixed_len(&self) -> usize {
        self.for_decoder()
            .map(|nal| LENGTH_SIZE + nal.end - nal.start)
            .sum()
    }

    // Writes the units of `data` for the decoder into `out`, each with its length before it as
    // four bytes. `out` has the size that `length_prefixed_len` gave.
    #[cfg(any(test, target_os = "macos", target_os = "ios"))]
    pub(super) fn write_length_prefixed(&self, data: &[u8], out: &mut [u8]) {
        let mut at = 0;
        for nal in self.for_decoder() {
            let size = nal.end - nal.start;
            let length = u32::try_from(size).unwrap_or(u32::MAX);
            out[at..at + LENGTH_SIZE].copy_from_slice(&length.to_be_bytes());
            at += LENGTH_SIZE;
            out[at..at + size].copy_from_slice(&data[nal.start..nal.end]);
            at += size;
        }
    }

    // The units that go to the decoder: slices and SEI messages. The parameter sets go to the
    // stream's format.
    #[cfg(any(test, target_os = "macos", target_os = "ios"))]
    fn for_decoder(&self) -> impl Iterator<Item = &Nal> {
        self.nals
            .iter()
            .filter(|nal| matches!(nal.kind, Kind::Slice { .. } | Kind::Other))
    }
}

// The kind of a unit from its bytes, which start with its header.
fn classify(codec: VideoCodec, nal: &[u8]) -> Kind {
    let header = nal[0];
    // The first bit after the header: for H.264 the first macroblock's number is zero, and for
    // HEVC the flag of the picture's first slice
    let first_bit = |header_len: usize| nal.get(header_len).is_some_and(|byte| byte & 0x80 != 0);
    match codec {
        VideoCodec::H264 => match header & 0x1f {
            kind @ 1..=5 => Kind::Slice {
                key: kind == 5,
                first: first_bit(1),
            },
            7 => Kind::ParameterSet(Role::Sequence),
            8 => Kind::ParameterSet(Role::Picture),
            9 => Kind::Delimiter,
            // The end of a sequence or of a stream, and filler
            10..=12 => Kind::Skipped,
            _ => Kind::Other,
        },
        VideoCodec::Hevc | VideoCodec::Av1 => match (header >> 1) & 0x3f {
            kind @ 0..=31 => Kind::Slice {
                // Slices of random access pictures
                key: (16..=23).contains(&kind),
                first: first_bit(2),
            },
            32 => Kind::ParameterSet(Role::Video),
            33 => Kind::ParameterSet(Role::Sequence),
            34 => Kind::ParameterSet(Role::Picture),
            35 => Kind::Delimiter,
            // Filler
            38 => Kind::Skipped,
            _ => Kind::Other,
        },
    }
}

#[cfg(test)]
#[path = "tests/annex_b_tests.rs"]
mod tests;
