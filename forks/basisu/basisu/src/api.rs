//! The stable, consumer-facing API: open a Basis Universal container, query it,
//! and transcode any image level to a GPU texture format. The numeric values of
//! [`TargetFormat`] and [`DecodeFlags`] match the Basis Universal
//! `transcoder_texture_format` and `basisd_decode_flags` enumerations, so raw
//! integer values interoperate with existing Basis tooling.

use crate::basis::{BasisSourceFormat, BasisTranscoder};
use crate::basislz::etc1s::{Endpoint, Selector, MAX_PREV_FRAME_LEVELS};
use crate::ktx2::BasisFormat;
use crate::transcoder::Ktx2Transcoder;

pub use crate::basislz::etc1s::VideoState;
use alloc::vec::Vec;

/// The Basis source codec a container carries.
///
/// Non-exhaustive: upstream Basis Universal adds codecs over time (XUASTC and
/// the 6x6 HDR formats arrived in v2), and tracking them must stay additive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum SourceFormat {
    /// ETC1S (BasisLZ-supercompressed).
    Etc1s,
    /// UASTC 4x4 LDR.
    UastcLdr,
    /// UASTC 4x4 HDR (`basis_tex_format::cUASTC_HDR_4x4`). Every block is a
    /// valid, restricted ASTC HDR 4x4 block, so it transcodes only to the HDR
    /// targets (BC6H, ASTC HDR, half-float, RGB 9E5).
    UastcHdr4x4,
    /// Raw (unsupercompressed) ASTC LDR blocks of the given block size
    /// (`basis_tex_format::cASTC_LDR_4x4 .. cASTC_LDR_12x12`). Transcodes to
    /// the matching-block-size ASTC pass-through target and to the LDR
    /// re-encode targets.
    AstcLdr(AstcBlock),
    /// Raw ASTC HDR 6x6 blocks (`basis_tex_format::cASTC_HDR_6x6`).
    AstcHdr6x6,
    /// UASTC HDR 6x6 (`basis_tex_format::cUASTC_HDR_6x6_INTERMEDIATE`): a
    /// bitwise-compressed intermediate stream that decompresses to ASTC HDR
    /// 6x6 blocks, then transcodes to the same HDR targets the raw 6x6
    /// source reaches.
    UastcHdr6x6,
    /// XUASTC LDR of the given block size
    /// (`basis_tex_format::cXUASTC_LDR_4x4 .. cXUASTC_LDR_12x12`): an
    /// arithmetic- or zstd-coded intermediate stream that decompresses to
    /// ASTC LDR blocks, then transcodes to the same targets the raw ASTC
    /// LDR source of that block size reaches.
    XuastcLdr(AstcBlock),
}

impl SourceFormat {
    /// Source block dimensions `(width, height)` in texels. The 4x4 codecs
    /// (ETC1S, UASTC LDR/HDR) are fixed; the raw ASTC sources carry theirs.
    pub fn block_dims(self) -> (u32, u32) {
        match self {
            SourceFormat::Etc1s | SourceFormat::UastcLdr | SourceFormat::UastcHdr4x4 => (4, 4),
            SourceFormat::AstcLdr(b) | SourceFormat::XuastcLdr(b) => b.dims(),
            SourceFormat::AstcHdr6x6 | SourceFormat::UastcHdr6x6 => (6, 6),
        }
    }
}

/// An ASTC LDR block footprint: the 14 block sizes the format defines, in
/// `basis_tex_format` declaration order (`cASTC_LDR_4x4` = 19 ..
/// `cASTC_LDR_12x12` = 32). Every block is 16 bytes regardless of footprint;
/// larger footprints trade quality for fewer bits per texel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AstcBlock {
    /// 4x4 texels (8.00 bpp).
    B4x4,
    /// 5x4 texels (6.40 bpp).
    B5x4,
    /// 5x5 texels (5.12 bpp).
    B5x5,
    /// 6x5 texels (4.27 bpp).
    B6x5,
    /// 6x6 texels (3.56 bpp).
    B6x6,
    /// 8x5 texels (3.20 bpp).
    B8x5,
    /// 8x6 texels (2.67 bpp).
    B8x6,
    /// 10x5 texels (2.56 bpp).
    B10x5,
    /// 10x6 texels (2.13 bpp).
    B10x6,
    /// 8x8 texels (2.00 bpp).
    B8x8,
    /// 10x8 texels (1.60 bpp).
    B10x8,
    /// 10x10 texels (1.28 bpp).
    B10x10,
    /// 12x10 texels (1.07 bpp).
    B12x10,
    /// 12x12 texels (0.89 bpp).
    B12x12,
}

impl AstcBlock {
    /// Every block size, in `basis_tex_format` declaration order.
    pub const ALL: [AstcBlock; 14] = [
        AstcBlock::B4x4,
        AstcBlock::B5x4,
        AstcBlock::B5x5,
        AstcBlock::B6x5,
        AstcBlock::B6x6,
        AstcBlock::B8x5,
        AstcBlock::B8x6,
        AstcBlock::B10x5,
        AstcBlock::B10x6,
        AstcBlock::B8x8,
        AstcBlock::B10x8,
        AstcBlock::B10x10,
        AstcBlock::B12x10,
        AstcBlock::B12x12,
    ];

    /// Block dimensions `(width, height)` in texels.
    pub fn dims(self) -> (u32, u32) {
        match self {
            AstcBlock::B4x4 => (4, 4),
            AstcBlock::B5x4 => (5, 4),
            AstcBlock::B5x5 => (5, 5),
            AstcBlock::B6x5 => (6, 5),
            AstcBlock::B6x6 => (6, 6),
            AstcBlock::B8x5 => (8, 5),
            AstcBlock::B8x6 => (8, 6),
            AstcBlock::B10x5 => (10, 5),
            AstcBlock::B10x6 => (10, 6),
            AstcBlock::B8x8 => (8, 8),
            AstcBlock::B10x8 => (10, 8),
            AstcBlock::B10x10 => (10, 10),
            AstcBlock::B12x10 => (12, 10),
            AstcBlock::B12x12 => (12, 12),
        }
    }

    /// The block size for the given texel dimensions, if it is one of the 14.
    pub fn from_dims(width: u32, height: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.dims() == (width, height))
    }

    /// The ASTC LDR pass-through target of this block size: value 10
    /// (`cTFASTC_LDR_4x4_RGBA`) for 4x4, else values 28..40 in `ALL` order.
    pub fn passthrough_target(self) -> TargetFormat {
        match self {
            AstcBlock::B4x4 => TargetFormat::Astc4x4Rgba,
            AstcBlock::B5x4 => TargetFormat::AstcLdr5x4Rgba,
            AstcBlock::B5x5 => TargetFormat::AstcLdr5x5Rgba,
            AstcBlock::B6x5 => TargetFormat::AstcLdr6x5Rgba,
            AstcBlock::B6x6 => TargetFormat::AstcLdr6x6Rgba,
            AstcBlock::B8x5 => TargetFormat::AstcLdr8x5Rgba,
            AstcBlock::B8x6 => TargetFormat::AstcLdr8x6Rgba,
            AstcBlock::B10x5 => TargetFormat::AstcLdr10x5Rgba,
            AstcBlock::B10x6 => TargetFormat::AstcLdr10x6Rgba,
            AstcBlock::B8x8 => TargetFormat::AstcLdr8x8Rgba,
            AstcBlock::B10x8 => TargetFormat::AstcLdr10x8Rgba,
            AstcBlock::B10x10 => TargetFormat::AstcLdr10x10Rgba,
            AstcBlock::B12x10 => TargetFormat::AstcLdr12x10Rgba,
            AstcBlock::B12x12 => TargetFormat::AstcLdr12x12Rgba,
        }
    }
}

/// The KTX2 supercompression scheme applied to level data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Supercompression {
    /// Level data is stored as-is.
    None,
    /// BasisLZ: the ETC1S endpoint/selector codebooks and slice indices are
    /// range-coded.
    BasisLz,
    /// Each level is a Zstandard frame.
    Zstandard,
    /// A scheme this transcoder does not recognize, carrying its raw KTX2 id.
    Other(u32),
}

/// A GPU texture format to transcode to. Values match
/// `basist::transcoder_texture_format`.
///
/// Non-exhaustive: upstream Basis Universal adds targets over time (the
/// non-4x4 ASTC pass-throughs arrived in v2), and tracking them must stay
/// additive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
#[non_exhaustive]
pub enum TargetFormat {
    /// ETC1 RGB (alpha dropped). 8 bytes per 4x4 block.
    Etc1Rgb = 0,
    /// ETC2 RGBA: ETC1 color plus an EAC alpha block. 16 bytes per 4x4 block.
    Etc2Rgba = 1,
    /// BC1 (DXT1) RGB. 8 bytes per 4x4 block.
    Bc1Rgb = 2,
    /// BC3 (DXT5) RGBA: BC1 color plus a BC4 alpha block. 16 bytes per block.
    Bc3Rgba = 3,
    /// BC4 single-channel (red). 8 bytes per 4x4 block.
    Bc4R = 4,
    /// BC5 two-channel (red, green): two BC4 blocks. 16 bytes per 4x4 block.
    Bc5Rg = 5,
    /// BC7 RGBA. 16 bytes per 4x4 block.
    Bc7Rgba = 6,
    /// PVRTC1 4bpp RGB. 8 bytes per 4x4 block.
    Pvrtc1_4Rgb = 8,
    /// PVRTC1 4bpp RGBA. 8 bytes per 4x4 block.
    Pvrtc1_4Rgba = 9,
    /// ASTC 4x4 LDR RGBA. 16 bytes per 4x4 block.
    Astc4x4Rgba = 10,
    /// ATC RGB. 8 bytes per 4x4 block.
    AtcRgb = 11,
    /// ATC RGBA: ATC color plus a BC4-style alpha block. 16 bytes per block.
    AtcRgba = 12,
    /// Uncompressed 32-bit RGBA, one pixel at a time (4 bytes per texel).
    Rgba32 = 13,
    /// Uncompressed 16-bit 5:6:5 RGB (2 bytes per texel).
    Rgb565 = 14,
    /// Uncompressed 16-bit 5:6:5 with red and blue swapped relative to `Rgb565`
    /// (blue in the high bits, red in the low). 2 bytes per texel.
    Bgr565 = 15,
    /// Uncompressed 16-bit 4:4:4:4 RGBA (2 bytes per texel).
    Rgba4444 = 16,
    /// FXT1 RGB. The only 8x4-block target, 16 bytes per block.
    Fxt1Rgb = 17,
    /// PVRTC2 4bpp RGB. 8 bytes per 4x4 block.
    Pvrtc2_4Rgb = 18,
    /// PVRTC2 4bpp RGBA. 8 bytes per 4x4 block.
    Pvrtc2_4Rgba = 19,
    /// EAC single-channel R11 (unsigned). 8 bytes per 4x4 block.
    EacR11 = 20,
    /// EAC two-channel RG11 (unsigned): two R11 blocks. 16 bytes per block.
    EacRg11 = 21,
    /// BC6H unsigned HDR RGB. 16 bytes per 4x4 block (like BC7).
    Bc6h = 22,
    /// ASTC 4x4 HDR RGBA. 16 bytes per 4x4 block. From a UASTC HDR 4x4 source
    /// this is a verbatim per-block copy (the source already is ASTC HDR).
    AstcHdr4x4Rgba = 23,
    /// Uncompressed half-float RGB, three 16-bit halves per texel (6 bytes).
    RgbHalf = 24,
    /// Uncompressed half-float RGBA, four 16-bit halves per texel (8 bytes).
    RgbaHalf = 25,
    /// Uncompressed shared-exponent RGB9E5, one 32-bit word per texel (4 bytes).
    Rgb9e5 = 26,
    /// ASTC 6x6 HDR RGBA. 16 bytes per 6x6 block. From a raw ASTC HDR 6x6
    /// source this is a verbatim per-block copy.
    AstcHdr6x6Rgba = 27,
    /// ASTC 5x4 LDR RGBA pass-through. 16 bytes per 5x4 block. The 4x4 LDR
    /// pass-through target is [`TargetFormat::Astc4x4Rgba`] (value 10); the
    /// non-4x4 sizes follow here in `basis_tex_format` block-size order.
    AstcLdr5x4Rgba = 28,
    /// ASTC 5x5 LDR RGBA pass-through. 16 bytes per 5x5 block.
    AstcLdr5x5Rgba = 29,
    /// ASTC 6x5 LDR RGBA pass-through. 16 bytes per 6x5 block.
    AstcLdr6x5Rgba = 30,
    /// ASTC 6x6 LDR RGBA pass-through. 16 bytes per 6x6 block.
    AstcLdr6x6Rgba = 31,
    /// ASTC 8x5 LDR RGBA pass-through. 16 bytes per 8x5 block.
    AstcLdr8x5Rgba = 32,
    /// ASTC 8x6 LDR RGBA pass-through. 16 bytes per 8x6 block.
    AstcLdr8x6Rgba = 33,
    /// ASTC 10x5 LDR RGBA pass-through. 16 bytes per 10x5 block.
    AstcLdr10x5Rgba = 34,
    /// ASTC 10x6 LDR RGBA pass-through. 16 bytes per 10x6 block.
    AstcLdr10x6Rgba = 35,
    /// ASTC 8x8 LDR RGBA pass-through. 16 bytes per 8x8 block.
    AstcLdr8x8Rgba = 36,
    /// ASTC 10x8 LDR RGBA pass-through. 16 bytes per 10x8 block.
    AstcLdr10x8Rgba = 37,
    /// ASTC 10x10 LDR RGBA pass-through. 16 bytes per 10x10 block.
    AstcLdr10x10Rgba = 38,
    /// ASTC 12x10 LDR RGBA pass-through. 16 bytes per 12x10 block.
    AstcLdr12x10Rgba = 39,
    /// ASTC 12x12 LDR RGBA pass-through. 16 bytes per 12x12 block.
    AstcLdr12x12Rgba = 40,
}

impl TargetFormat {
    /// The raw `transcoder_texture_format` integer.
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    /// Parse a raw `transcoder_texture_format` integer.
    pub fn from_i32(v: i32) -> Option<Self> {
        use TargetFormat::*;
        Some(match v {
            0 => Etc1Rgb,
            1 => Etc2Rgba,
            2 => Bc1Rgb,
            3 => Bc3Rgba,
            4 => Bc4R,
            5 => Bc5Rg,
            6 => Bc7Rgba,
            8 => Pvrtc1_4Rgb,
            9 => Pvrtc1_4Rgba,
            10 => Astc4x4Rgba,
            11 => AtcRgb,
            12 => AtcRgba,
            13 => Rgba32,
            14 => Rgb565,
            15 => Bgr565,
            16 => Rgba4444,
            17 => Fxt1Rgb,
            18 => Pvrtc2_4Rgb,
            19 => Pvrtc2_4Rgba,
            20 => EacR11,
            21 => EacRg11,
            22 => Bc6h,
            23 => AstcHdr4x4Rgba,
            24 => RgbHalf,
            25 => RgbaHalf,
            26 => Rgb9e5,
            27 => AstcHdr6x6Rgba,
            28 => AstcLdr5x4Rgba,
            29 => AstcLdr5x5Rgba,
            30 => AstcLdr6x5Rgba,
            31 => AstcLdr6x6Rgba,
            32 => AstcLdr8x5Rgba,
            33 => AstcLdr8x6Rgba,
            34 => AstcLdr10x5Rgba,
            35 => AstcLdr10x6Rgba,
            36 => AstcLdr8x8Rgba,
            37 => AstcLdr10x8Rgba,
            38 => AstcLdr10x10Rgba,
            39 => AstcLdr12x10Rgba,
            40 => AstcLdr12x12Rgba,
            _ => return None,
        })
    }

    /// Bytes per block, or per pixel for the uncompressed/packed formats.
    pub fn bytes_per_block_or_pixel(self) -> usize {
        use TargetFormat::*;
        match self {
            Etc1Rgb | Bc1Rgb | Bc4R | Pvrtc1_4Rgb | Pvrtc1_4Rgba | AtcRgb | Pvrtc2_4Rgb
            | Pvrtc2_4Rgba | EacR11 => 8,
            Etc2Rgba | Bc3Rgba | Bc5Rg | Bc7Rgba | Astc4x4Rgba | AtcRgba | Fxt1Rgb | EacRg11
            | Bc6h | AstcHdr4x4Rgba | AstcHdr6x6Rgba | AstcLdr5x4Rgba | AstcLdr5x5Rgba
            | AstcLdr6x5Rgba | AstcLdr6x6Rgba | AstcLdr8x5Rgba | AstcLdr8x6Rgba
            | AstcLdr10x5Rgba | AstcLdr10x6Rgba | AstcLdr8x8Rgba | AstcLdr10x8Rgba
            | AstcLdr10x10Rgba | AstcLdr12x10Rgba | AstcLdr12x12Rgba => 16,
            Rgba32 | Rgb9e5 => 4,
            Rgb565 | Bgr565 | Rgba4444 => 2,
            RgbHalf => 6,
            RgbaHalf => 8,
        }
    }

    /// Block dimensions `(width, height)` in texels for block-based targets:
    /// 4x4 except FXT1 (the only 8x4 format) and the non-4x4 ASTC targets.
    pub fn block_dims(self) -> (u32, u32) {
        use TargetFormat::*;
        match self {
            Fxt1Rgb => (8, 4),
            AstcHdr6x6Rgba | AstcLdr6x6Rgba => (6, 6),
            AstcLdr5x4Rgba => (5, 4),
            AstcLdr5x5Rgba => (5, 5),
            AstcLdr6x5Rgba => (6, 5),
            AstcLdr8x5Rgba => (8, 5),
            AstcLdr8x6Rgba => (8, 6),
            AstcLdr10x5Rgba => (10, 5),
            AstcLdr10x6Rgba => (10, 6),
            AstcLdr8x8Rgba => (8, 8),
            AstcLdr10x8Rgba => (10, 8),
            AstcLdr10x10Rgba => (10, 10),
            AstcLdr12x10Rgba => (12, 10),
            AstcLdr12x12Rgba => (12, 12),
            _ => (4, 4),
        }
    }

    /// Whether output is laid out as blocks (vs. raster pixels).
    pub fn is_block_based(self) -> bool {
        !matches!(
            self,
            TargetFormat::Rgba32
                | TargetFormat::Rgb565
                | TargetFormat::Bgr565
                | TargetFormat::Rgba4444
                | TargetFormat::RgbHalf
                | TargetFormat::RgbaHalf
                | TargetFormat::Rgb9e5
        )
    }
}

/// Decode behavior flags. Values match `basist::basisd_decode_flags`; combine
/// with `|`.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct DecodeFlags(u32);

impl DecodeFlags {
    /// No flags set (the default decode behavior).
    pub const NONE: Self = Self(0);
    /// `cDecodeFlagsPVRTCDecodeToNextPow2`.
    pub const PVRTC_DECODE_TO_NEXT_POW2: Self = Self(2);
    /// `cDecodeFlagsTranscodeAlphaDataToOpaqueFormats`: emit the alpha slice
    /// into an otherwise-opaque target (BC1/ETC1/...).
    pub const TRANSCODE_ALPHA_TO_OPAQUE: Self = Self(4);
    /// `cDecodeFlagsBC1ForbidThreeColorBlocks`.
    pub const BC1_FORBID_THREE_COLOR_BLOCKS: Self = Self(8);
    /// `cDecodeFlagsOutputHasAlphaIndices`.
    pub const OUTPUT_HAS_ALPHA_INDICES: Self = Self(16);
    /// `cDecodeFlagsHighQuality`: higher-quality UASTC to BCn transcodes (changes
    /// output bytes; must be matched for parity).
    pub const HIGH_QUALITY: Self = Self(32);
    /// `cDecodeFlagsNoETC1SChromaFiltering` (v2): disable the BC7 cross-block
    /// chroma-filtering post-pass that v2 applies to ETC1S to BC7 by default.
    pub const NO_ETC1S_CHROMA_FILTERING: Self = Self(64);
    /// `cDecodeFlagsNoDeblockFiltering` (v2): disable the deblocking filter
    /// that raw-ASTC decodes apply by default for block sizes above 8x6.
    pub const NO_DEBLOCK_FILTERING: Self = Self(128);
    /// `cDecodeFlagsStrongerDeblockFiltering` (v2): stronger deblock tap math
    /// (the default above 8x8).
    pub const STRONGER_DEBLOCK_FILTERING: Self = Self(256);
    /// `cDecodeFlagsForceDeblockFiltering` (v2): deblock even for the small
    /// block sizes that default to no filtering.
    pub const FORCE_DEBLOCK_FILTERING: Self = Self(512);

    /// Wrap a raw `basisd_decode_flags` bitmask.
    pub fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
    /// The raw `basisd_decode_flags` bitmask.
    pub fn bits(self) -> u32 {
        self.0
    }
    /// Whether every bit in `other` is also set in `self`.
    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl core::ops::BitOr for DecodeFlags {
    type Output = Self;
    /// Union of two decode-flag sets.
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Dimensions and block counts of one image level.
#[derive(Clone, Copy, Debug)]
pub struct ImageLevelInfo {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Number of source blocks across, `ceil(width / source block width)`.
    /// The source block is 4x4 for ETC1S and UASTC; the raw ASTC sources
    /// carry their own footprint ([`SourceFormat::block_dims`]).
    pub num_blocks_x: u32,
    /// Number of source blocks down, `ceil(height / source block height)`.
    pub num_blocks_y: u32,
}

/// Transcode failures.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Error {
    /// Not a recognized Basis Universal container, or corrupt.
    InvalidData,
    /// The container is truncated or an offset is out of range.
    Truncated,
    /// The `(source, target)` combination is not valid (per Basis' support matrix).
    Unsupported {
        source: SourceFormat,
        target: TargetFormat,
    },
    /// A container feature not yet implemented (multi-image levels, ...).
    UnsupportedFeature(&'static str),
    /// The container is an ETC1S video: frames carry cross-frame state, so
    /// they decode through [`Transcoder::transcode_video_frame`] in order,
    /// not through the stateless entry points.
    VideoRequiresState,
    /// `image`/`level` out of range.
    InvalidImageOrLevel,
    /// The caller-provided output buffer is too small.
    OutputTooSmall { needed: usize },
    /// A Zstandard-supercompressed level requires the `zstd` feature.
    ZstdRequired,
}

/// The parsed container behind a [`Transcoder`]: either of Basis Universal's
/// two wrappers around the same ETC1S/UASTC payload. Auto-detected from the
/// leading bytes by [`Transcoder::new`].
enum Container<'a> {
    Ktx2(Ktx2Transcoder<'a>),
    Basis(BasisTranscoder<'a>),
}

/// A shared ETC1S codebook lifted out of one Basis texture to decode
/// "global codebook" `.basis` files that reference it.
///
/// Basis Universal lets a set of `.basis` files share a single ETC1S
/// endpoint/selector codebook: one file carries the codebook, the rest carry
/// only their Huffman tables and slice data and set the global-codebook flag.
/// Decode the codebook-carrying file first, take its codebook with
/// [`Transcoder::etc1s_codebook`], then pass it to [`Transcoder::new_with_codebook`]
/// for each dependent file. KTX2 has no equivalent (its codebook is always
/// embedded), so this applies to the `.basis` container only.
#[derive(Clone)]
pub struct GlobalCodebook {
    endpoints: Vec<Endpoint>,
    selectors: Vec<Selector>,
}

/// A parsed + prepared Basis Universal texture, ready to transcode.
pub struct Transcoder<'a> {
    inner: Container<'a>,
}

impl<'a> Transcoder<'a> {
    /// Open a Basis Universal texture, auto-detecting the container: KTX2 (the
    /// 12-byte KTX2 identifier) or `.basis` (the `0x4273` signature). Returns
    /// [`Error::InvalidData`] if it is neither, or a corrupt/unsupported payload.
    pub fn new(data: &'a [u8]) -> Result<Self, Error> {
        if BasisTranscoder::is_basis(data) {
            return BasisTranscoder::new(data)
                .map(|t| Self {
                    inner: Container::Basis(t),
                })
                .ok_or(Error::InvalidData);
        }
        Ktx2Transcoder::new(data)
            .map(|t| Self {
                inner: Container::Ktx2(t),
            })
            .ok_or(Error::InvalidData)
    }

    /// Open a Basis texture, supplying a shared ETC1S codebook for a
    /// global-codebook `.basis` file (see [`GlobalCodebook`]). A self-contained
    /// file, or any KTX2 file, ignores the codebook and opens exactly as
    /// [`new`](Self::new) would. Returns [`Error::InvalidData`] if the file is
    /// not a supported payload, or if it is a global-codebook file whose header
    /// does not match the supplied codebook.
    pub fn new_with_codebook(data: &'a [u8], codebook: &GlobalCodebook) -> Result<Self, Error> {
        if BasisTranscoder::is_basis(data) {
            return BasisTranscoder::new_with_codebook(
                data,
                Some((&codebook.endpoints, &codebook.selectors)),
            )
            .map(|t| Self {
                inner: Container::Basis(t),
            })
            .ok_or(Error::InvalidData);
        }
        Self::new(data)
    }

    /// This texture's decoded ETC1S codebook, for decoding global-codebook
    /// `.basis` files that share it (see [`GlobalCodebook`]). `None` unless this
    /// is a self-contained ETC1S `.basis` file.
    pub fn etc1s_codebook(&self) -> Option<GlobalCodebook> {
        let Container::Basis(b) = &self.inner else {
            return None;
        };
        let (endpoints, selectors) = b.codebook()?;
        Some(GlobalCodebook {
            endpoints: endpoints.to_vec(),
            selectors: selectors.to_vec(),
        })
    }

    /// The Basis source codec the container carries.
    pub fn source_format(&self) -> SourceFormat {
        match &self.inner {
            Container::Ktx2(k) => match k.header.format {
                BasisFormat::Etc1s => SourceFormat::Etc1s,
                BasisFormat::Uastc => SourceFormat::UastcLdr,
                BasisFormat::UastcHdr4x4 => SourceFormat::UastcHdr4x4,
                BasisFormat::AstcLdr(b) => SourceFormat::AstcLdr(b),
                BasisFormat::AstcHdr6x6 => SourceFormat::AstcHdr6x6,
                BasisFormat::UastcHdr6x6 => SourceFormat::UastcHdr6x6,
                BasisFormat::XuastcLdr(b) => SourceFormat::XuastcLdr(b),
            },
            Container::Basis(b) => match b.source_format() {
                BasisSourceFormat::Etc1s => SourceFormat::Etc1s,
                BasisSourceFormat::Uastc => SourceFormat::UastcLdr,
                BasisSourceFormat::UastcHdr4x4 => SourceFormat::UastcHdr4x4,
                BasisSourceFormat::AstcLdr(blk) => SourceFormat::AstcLdr(blk),
                BasisSourceFormat::AstcHdr6x6 => SourceFormat::AstcHdr6x6,
                BasisSourceFormat::UastcHdr6x6 => SourceFormat::UastcHdr6x6,
                BasisSourceFormat::XuastcLdr(b) => SourceFormat::XuastcLdr(b),
            },
        }
    }

    /// Whether the texture carries an alpha channel.
    pub fn has_alpha(&self) -> bool {
        match &self.inner {
            Container::Ktx2(k) => k.header.has_alpha,
            Container::Basis(b) => b.has_alpha(),
        }
    }

    /// Number of mip levels (at least 1; a zero level count means a single base
    /// level).
    pub fn level_count(&self) -> u32 {
        match &self.inner {
            Container::Ktx2(k) => k.header.level_count.max(1),
            Container::Basis(b) => b.level_count().max(1),
        }
    }

    /// Number of array layers (0/1 for a plain 2D texture). For `.basis`, the
    /// image count (a flat 2D image array).
    pub fn layer_count(&self) -> u32 {
        match &self.inner {
            Container::Ktx2(k) => k.header.layer_count,
            Container::Basis(b) => b.image_count(),
        }
    }

    /// Number of cubemap faces (1 for a plain 2D texture, 6 for a cubemap).
    /// `.basis` does not expose faces, so this is always 1.
    pub fn face_count(&self) -> u32 {
        match &self.inner {
            Container::Ktx2(k) => k.header.face_count,
            Container::Basis(_) => 1,
        }
    }

    /// Whether this is a video (cross-frame conditional replenishment): for
    /// KTX2, an ETC1S `KTXanimData` key or a P-frame layer array; for `.basis`,
    /// a `cBASISTexTypeVideoFrames` file or any I-frame slice flag.
    pub fn is_video(&self) -> bool {
        match &self.inner {
            Container::Ktx2(k) => k.is_video(),
            Container::Basis(b) => b.is_video(),
        }
    }

    /// The level-data supercompression scheme. `.basis` has no separate
    /// supercompression field (ETC1S is intrinsically BasisLZ-coded, UASTC is
    /// stored raw), so it reports the codec's intrinsic scheme.
    pub fn supercompression(&self) -> Supercompression {
        match &self.inner {
            Container::Ktx2(k) => match k.header.supercompression {
                0 => Supercompression::None,
                1 => Supercompression::BasisLz,
                2 => Supercompression::Zstandard,
                other => Supercompression::Other(other),
            },
            Container::Basis(b) => match b.source_format() {
                BasisSourceFormat::Etc1s => Supercompression::BasisLz,
                // UASTC (LDR/HDR), raw ASTC, and intermediate-stream .basis
                // slices are stored raw (the 6x6 intermediate is its own
                // coding, not a KTX2 supercompression scheme).
                BasisSourceFormat::Uastc
                | BasisSourceFormat::UastcHdr4x4
                | BasisSourceFormat::AstcLdr(_)
                | BasisSourceFormat::AstcHdr6x6
                | BasisSourceFormat::UastcHdr6x6
                | BasisSourceFormat::XuastcLdr(_) => Supercompression::None,
            },
        }
    }

    /// Width and height of the base (level 0) image, in texels.
    pub fn base_dimensions(&self) -> (u32, u32) {
        match &self.inner {
            Container::Ktx2(k) => (k.header.width, k.header.height),
            Container::Basis(b) => b.base_dimensions(),
        }
    }

    /// Dimensions and source-block counts of mip `level` (base dimensions
    /// halved `level` times, clamped to a 1-texel floor). For `.basis`, the
    /// slice's recorded original dimensions for image 0.
    pub fn image_level_info(&self, level: u32) -> Result<ImageLevelInfo, Error> {
        if level >= self.level_count() {
            return Err(Error::InvalidImageOrLevel);
        }
        match &self.inner {
            Container::Ktx2(k) => {
                let width = (k.header.width >> level).max(1);
                let height = (k.header.height >> level).max(1);
                let (bw, bh) = self.source_format().block_dims();
                Ok(ImageLevelInfo {
                    width,
                    height,
                    num_blocks_x: width.div_ceil(bw),
                    num_blocks_y: height.div_ceil(bh),
                })
            }
            Container::Basis(b) => {
                let (width, height, num_blocks_x, num_blocks_y) = b
                    .image_level_info(0, level)
                    .ok_or(Error::InvalidImageOrLevel)?;
                Ok(ImageLevelInfo {
                    width,
                    height,
                    num_blocks_x,
                    num_blocks_y,
                })
            }
        }
    }

    /// Whether this texture can be transcoded to `target` (Basis' support matrix).
    pub fn supports(&self, target: TargetFormat) -> bool {
        crate::support::is_format_supported(target, self.source_format())
    }

    /// Bytes a transcode of a `width x height` image to `target` produces.
    fn size_for_dims(width: u32, height: u32, target: TargetFormat) -> usize {
        if target.is_block_based() {
            // FXT1 has an 8x4 block, but its output buffer is sized by the 4x4
            // `total_blocks` count. The FXT1 data occupies the front
            // (ceil(w/8)*ceil(h/4) blocks) and the rest is zero padding. Every
            // other block target is sized by its own block grid (4x4 for
            // everything but the non-4x4 ASTC targets).
            let (bw, bh) = if target == TargetFormat::Fxt1Rgb {
                (4, 4)
            } else {
                target.block_dims()
            };
            (width.div_ceil(bw) * height.div_ceil(bh)) as usize * target.bytes_per_block_or_pixel()
        } else {
            (width * height) as usize * target.bytes_per_block_or_pixel()
        }
    }

    /// Bytes a successful `transcode(level, target, _)` will produce.
    pub fn output_size(&self, level: u32, target: TargetFormat) -> Result<usize, Error> {
        let info = self.image_level_info(level)?;
        Ok(Self::size_for_dims(info.width, info.height, target))
    }

    /// Output size for a specific image: KTX2 images of a level all share the
    /// level's dimensions, but a `.basis` file's images are independent, so
    /// there `layer` selects the image whose recorded dimensions govern.
    fn image_output_size(
        &self,
        level: u32,
        layer: u32,
        target: TargetFormat,
    ) -> Result<usize, Error> {
        match &self.inner {
            Container::Ktx2(_) => self.output_size(level, target),
            Container::Basis(b) => {
                let (width, height, _, _) = b
                    .image_level_info(layer, level)
                    .ok_or(Error::InvalidImageOrLevel)?;
                Ok(Self::size_for_dims(width, height, target))
            }
        }
    }

    /// Transcode image `level` to `target` with the given decode `flags`. For
    /// `.basis`, this is image 0 (the 2D case).
    pub fn transcode(
        &self,
        level: u32,
        target: TargetFormat,
        flags: DecodeFlags,
    ) -> Result<Vec<u8>, Error> {
        if level >= self.level_count() {
            return Err(Error::InvalidImageOrLevel);
        }
        // P-frames reference the previous frame's block state; the stateless
        // entry points would decode them as garbage, so video files must go
        // through transcode_video_frame.
        if self.is_video() {
            return Err(Error::VideoRequiresState);
        }
        if !self.supports(target) {
            return Err(Error::Unsupported {
                source: self.source_format(),
                target,
            });
        }
        let mut out = alloc::vec![0u8; self.output_size(level, target)?];
        match &self.inner {
            Container::Ktx2(k) => {
                k.transcode_image_level_flags(level, target.as_i32(), flags.bits(), &mut out)
            }
            Container::Basis(b) => {
                b.transcode_image_level(0, level, target.as_i32(), flags.bits(), &mut out)
            }
        }
        .ok_or(Error::UnsupportedFeature(
            "target not yet implemented for this source codec",
        ))?;
        Ok(out)
    }

    /// Transcode a specific image `(level, layer, face)` to `target`. For KTX2
    /// 2D textures `layer` and `face` are 0; cubemaps select a face (0..6) and
    /// array textures select a layer (`0..layer_count`). For `.basis`, `layer`
    /// selects the image index and `face` must be 0.
    pub fn transcode_image(
        &self,
        level: u32,
        layer: u32,
        face: u32,
        target: TargetFormat,
        flags: DecodeFlags,
    ) -> Result<Vec<u8>, Error> {
        if level >= self.level_count()
            || layer >= self.layer_count().max(1)
            || face >= self.face_count().max(1)
        {
            return Err(Error::InvalidImageOrLevel);
        }
        // Same video guard as `transcode`: P-frames need the cross-frame
        // state that only transcode_video_frame carries.
        if self.is_video() {
            return Err(Error::VideoRequiresState);
        }
        if !self.supports(target) {
            return Err(Error::Unsupported {
                source: self.source_format(),
                target,
            });
        }
        let mut out = alloc::vec![0u8; self.image_output_size(level, layer, target)?];
        match &self.inner {
            Container::Ktx2(k) => {
                k.transcode_image(level, layer, face, target.as_i32(), flags.bits(), &mut out)
            }
            // `.basis` has no faces; `layer` is the image index.
            Container::Basis(b) => {
                b.transcode_image_level(layer, level, target.as_i32(), flags.bits(), &mut out)
            }
        }
        .ok_or(Error::UnsupportedFeature(
            "target not yet implemented for this source codec",
        ))?;
        Ok(out)
    }

    /// Transcode one frame of an ETC1S video. `frame` is the KTX2 layer or
    /// `.basis` image index; `state` carries the previous frame's block data
    /// that P-frames replenish from.
    ///
    /// Frames of a level must be transcoded in ascending frame order with the
    /// same `state`; out-of-order frames decode without error but reference
    /// the wrong previous frame. Start (or [`VideoState::reset`]) the state
    /// when seeking back to frame 0. On a non-video file this behaves like
    /// [`Self::transcode_image`] with `face` 0.
    pub fn transcode_video_frame(
        &self,
        state: &mut VideoState,
        level: u32,
        frame: u32,
        target: TargetFormat,
        flags: DecodeFlags,
    ) -> Result<Vec<u8>, Error> {
        if level >= self.level_count() || frame >= self.layer_count().max(1) {
            return Err(Error::InvalidImageOrLevel);
        }
        // Cross-frame state is tracked for at most MAX_PREV_FRAME_LEVELS mip
        // levels; a video deeper than that cannot decode correctly, so reject
        // rather than silently drop state.
        if self.is_video() && level as usize >= MAX_PREV_FRAME_LEVELS {
            return Err(Error::InvalidImageOrLevel);
        }
        if !self.supports(target) {
            return Err(Error::Unsupported {
                source: self.source_format(),
                target,
            });
        }
        let mut out = alloc::vec![0u8; self.image_output_size(level, frame, target)?];
        match &self.inner {
            Container::Ktx2(k) => k.transcode_image_video(
                level,
                frame,
                target.as_i32(),
                flags.bits(),
                state,
                &mut out,
            ),
            Container::Basis(b) => b.transcode_image_level_video(
                frame,
                level,
                target.as_i32(),
                flags.bits(),
                state,
                &mut out,
            ),
        }
        .ok_or(Error::UnsupportedFeature(
            "target not yet implemented for this source codec",
        ))?;
        Ok(out)
    }

    /// Transcode image `level` (like [`Self::transcode`]) into a
    /// caller-provided buffer, without allocating the output. `out` must hold
    /// at least [`Self::output_size`] bytes; exactly that many are written,
    /// and any bytes beyond them are left untouched. Intermediate scratch that
    /// a codec inherently needs (a Zstandard-inflated level, an intermediate
    /// stream's decompressed blocks) is still allocated internally, exactly as
    /// the reference transcoder does behind its caller-provided output
    /// pointer.
    pub fn transcode_into(
        &self,
        level: u32,
        target: TargetFormat,
        flags: DecodeFlags,
        out: &mut [u8],
    ) -> Result<(), Error> {
        let needed = self.output_size(level, target)?;
        if out.len() < needed {
            return Err(Error::OutputTooSmall { needed });
        }
        if self.is_video() {
            return Err(Error::VideoRequiresState);
        }
        if !self.supports(target) {
            return Err(Error::Unsupported {
                source: self.source_format(),
                target,
            });
        }
        let dst = &mut out[..needed];
        // The converters reproduce the allocating path's zeroed-buffer
        // semantics themselves (they zero-fill `dst` before writing), so a
        // dirty caller buffer produces identical bytes.
        match &self.inner {
            Container::Ktx2(k) => {
                k.transcode_image_level_flags(level, target.as_i32(), flags.bits(), dst)
            }
            Container::Basis(b) => {
                b.transcode_image_level(0, level, target.as_i32(), flags.bits(), dst)
            }
        }
        .ok_or(Error::UnsupportedFeature(
            "target not yet implemented for this source codec",
        ))
    }
}
