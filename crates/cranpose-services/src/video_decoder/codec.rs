//! The codecs of a video stream.

/// The codec of a video stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    /// H.264 (AVC) in Annex-B form: NAL units after start codes.
    H264,
    /// H.265 (HEVC) in Annex-B form.
    Hevc,
    /// VP9: one frame or superframe in each access unit.
    Vp9,
    /// AV1: one temporal unit of OBUs in each access unit.
    Av1,
}

impl VideoCodec {
    /// The codec's MIME type, which Android's media codecs name it by: `video/avc` for H.264,
    /// `video/hevc` for HEVC, `video/x-vnd.on2.vp9` for VP9, and `video/av01` for AV1.
    pub fn mime(self) -> &'static str {
        match self {
            Self::H264 => "video/avc",
            Self::Hevc => "video/hevc",
            Self::Vp9 => "video/x-vnd.on2.vp9",
            Self::Av1 => "video/av01",
        }
    }
}
