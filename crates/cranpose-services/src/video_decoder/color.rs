//! Color metadata and display orientation of a decoded video picture.

/// Color primaries and YCbCr matrix standard of a video picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColorPrimaries {
    /// ITU-R BT.601 (standard-definition video).
    Bt601,
    /// ITU-R BT.709 (high-definition SDR video and sRGB gamut).
    #[default]
    Bt709,
    /// ITU-R BT.2020 (ultra-high-definition and HDR wide gamut).
    Bt2020,
}

impl ColorPrimaries {
    /// Parses an ITU-T H.273 colour primaries code.
    pub const fn from_h273(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Bt709),
            5 | 6 => Some(Self::Bt601),
            9 => Some(Self::Bt2020),
            _ => None,
        }
    }

    /// Parses an ITU-T H.273 YCbCr matrix coefficients code.
    pub const fn from_matrix_h273(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Bt709),
            5 | 6 => Some(Self::Bt601),
            9 | 10 => Some(Self::Bt2020),
            _ => None,
        }
    }

    /// Default primaries when a stream omits color metadata: BT.709 for pictures at least
    /// 720 pixels tall, and BT.601 for smaller pictures.
    pub const fn default_for_height(height: u32) -> Self {
        if height >= 720 {
            Self::Bt709
        } else {
            Self::Bt601
        }
    }
}

/// Sample value range of a video picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColorRange {
    /// Studio or video range (luma 16..=235 and chroma 16..=240 in 8-bit).
    #[default]
    Limited,
    /// Full range (0..=255 in 8-bit).
    Full,
}

/// Electro-optical transfer function of a video picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColorTransfer {
    /// Standard dynamic range video (BT.709 / sRGB gamma).
    #[default]
    Sdr,
    /// Perceptual Quantizer HDR (SMPTE ST 2084 / HDR10).
    Pq,
    /// Hybrid Log-Gamma HDR (ARIB STD-B67).
    Hlg,
}

impl ColorTransfer {
    /// Parses an ITU-T H.273 transfer characteristics code.
    pub const fn from_h273(code: u8) -> Option<Self> {
        match code {
            1 | 6 | 13 | 14 | 15 => Some(Self::Sdr),
            16 => Some(Self::Pq),
            18 => Some(Self::Hlg),
            _ => None,
        }
    }
}

/// Color description of a decoded video picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VideoColorInfo {
    /// Color primaries and YCbCr matrix standard.
    pub primaries: ColorPrimaries,
    /// Sample value range.
    pub range: ColorRange,
    /// Electro-optical transfer function.
    pub transfer: ColorTransfer,
}

impl VideoColorInfo {
    /// An SDR color description with the given primaries and sample range.
    pub const fn sdr(primaries: ColorPrimaries, range: ColorRange) -> Self {
        Self {
            primaries,
            range,
            transfer: ColorTransfer::Sdr,
        }
    }

    /// Default SDR color description for a picture `height` pixels tall.
    pub const fn default_for_height(height: u32) -> Self {
        Self::sdr(
            ColorPrimaries::default_for_height(height),
            ColorRange::Limited,
        )
    }
}

/// Clockwise rotation applied when displaying a decoded video picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VideoRotation {
    /// Upright, no rotation.
    #[default]
    Degrees0,
    /// Rotated 90 degrees clockwise.
    Degrees90,
    /// Rotated 180 degrees.
    Degrees180,
    /// Rotated 270 degrees clockwise (90 degrees counter-clockwise).
    Degrees270,
}

impl VideoRotation {
    /// Maps a clockwise angle in degrees (normalized modulo 360) to a rotation.
    pub const fn from_degrees(degrees: i32) -> Option<Self> {
        match degrees.rem_euclid(360) {
            0 => Some(Self::Degrees0),
            90 => Some(Self::Degrees90),
            180 => Some(Self::Degrees180),
            270 => Some(Self::Degrees270),
            _ => None,
        }
    }

    /// Clockwise angle in degrees: 0, 90, 180, or 270.
    pub const fn degrees(self) -> u16 {
        match self {
            Self::Degrees0 => 0,
            Self::Degrees90 => 90,
            Self::Degrees180 => 180,
            Self::Degrees270 => 270,
        }
    }

    /// Whether displaying this rotation swaps width and height.
    pub const fn swaps_axes(self) -> bool {
        matches!(self, Self::Degrees90 | Self::Degrees270)
    }
}
