//! The codecs of a video stream.

/// The codec of a video stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoCodec {
    /// H.264 (AVC) in Annex-B form: NAL units after start codes.
    H264,
    /// H.265 (HEVC) in Annex-B form.
    Hevc,
    /// AV1: one temporal unit of OBUs in each access unit.
    Av1,
}

impl VideoCodec {
    // The MIME type that Android's media codecs use.
    #[cfg(target_os = "android")]
    pub(super) fn mime(self) -> &'static str {
        match self {
            Self::H264 => "video/avc",
            Self::Hevc => "video/hevc",
            Self::Av1 => "video/av01",
        }
    }
}
