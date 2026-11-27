//! The `.basis` container front-end. `.basis` is Basis Universal's own
//! container format; it wraps the same ETC1S and UASTC payload as KTX2, so the
//! codebook decode and per-target transcode are shared with the KTX2 path via
//! `crate::dispatch`. Only the container framing differs: a fixed 77-byte
//! `basis_file_header` followed by an array of 23-byte `basis_slice_desc`
//! entries, with each slice's compressed data addressed by an absolute file
//! offset (KTX2 instead groups slices into per-level data blocks).
//!
//! Scope: ETC1S (`tex_format` 0), UASTC_LDR_4x4 (`tex_format` 1),
//! UASTC_HDR_4x4 (`tex_format` 2, raw slices routed like UASTC LDR), raw ASTC
//! HDR 6x6 (`tex_format` 3), the UASTC HDR 6x6 intermediate (`tex_format` 4,
//! each slice a compressed stream that decodes to ASTC HDR 6x6 blocks), the
//! XUASTC LDR family (`tex_format` 5..18, each slice an intermediate stream
//! that decodes to ASTC LDR blocks), and the raw ASTC LDR family (`tex_format`
//! 19..32, raw slices of the format's own block size). A `tex_format` value
//! outside those ranges yields `None` so the caller can skip the file. ETC1S
//! video (`tex_type` 3) transcodes through the stateful video entry point, one
//! frame at a time in order.

use crate::api::AstcBlock;
use crate::basislz::etc1s::{
    Endpoint, Etc1sTranscoder, Selector, VideoState, MAX_PREV_FRAME_LEVELS,
};
use crate::dispatch;
use alloc::vec::Vec;

/// `.basis` signature: file bytes [0]=0x73 ('s'), [1]=0x42 ('B'), i.e. the
/// little-endian u16 `0x4273`.
const BASIS_SIG: u16 = 0x4273;
/// The only baseline `m_ver` accepted (`BASISD_SUPPORTED_BASIS_VERSION`).
const BASIS_VERSION: u16 = 0x13;
/// `sizeof(basis_file_header)`; `m_header_size` must equal this.
const HEADER_SIZE: u16 = 77;
/// `sizeof(basis_slice_desc)`.
const SLICE_DESC_SIZE: usize = 23;

/// `basis_tex_format` values this crate transcodes.
const TEX_FORMAT_ETC1S: u8 = 0;
const TEX_FORMAT_UASTC_LDR_4X4: u8 = 1;
const TEX_FORMAT_UASTC_HDR_4X4: u8 = 2;
const TEX_FORMAT_ASTC_HDR_6X6: u8 = 3;
const TEX_FORMAT_UASTC_HDR_6X6_INTERMEDIATE: u8 = 4;
const TEX_FORMAT_XUASTC_LDR_FIRST: u8 = 5;
const TEX_FORMAT_XUASTC_LDR_LAST: u8 = 18;
/// `cASTC_LDR_4x4` .. `cASTC_LDR_12x12`, in [`AstcBlock::ALL`] order.
const TEX_FORMAT_ASTC_LDR_FIRST: u8 = 19;
const TEX_FORMAT_ASTC_LDR_LAST: u8 = 32;

/// `basis_header_flags` bits we read.
const FLAG_HAS_ALPHA_SLICES: u16 = 4;
const FLAG_USES_GLOBAL_CODEBOOK: u16 = 8;
/// `cBASISHeaderFlagSRGB`: for raw ASTC LDR files, selects the sRGB decode
/// profile (KTX2 derives the same bit from the vkFormat instead).
const FLAG_SRGB: u16 = 16;

/// `basis_slice_desc_flags` bits we read.
const SLICE_FLAG_HAS_ALPHA: u8 = 1;
const SLICE_FLAG_IS_IFRAME: u8 = 2;

/// `basis_texture_type::cBASISTexTypeVideoFrames`.
const TEX_TYPE_VIDEO_FRAMES: u8 = 3;

/// The Basis source codec a `.basis` file carries (the subset we support).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BasisSourceFormat {
    /// ETC1S (`tex_format` 0), BasisLZ-coded slices.
    Etc1s,
    /// UASTC LDR 4x4 (`tex_format` 1), raw 16-byte-block slices.
    Uastc,
    /// UASTC HDR 4x4 (`tex_format` 2), raw 16-byte-block slices.
    UastcHdr4x4,
    /// Raw ASTC LDR (`tex_format` 19..32), raw 16-byte-block slices of the
    /// named footprint. In practice these footprints are carried in KTX2 rather
    /// than `.basis`, but the container format permits them, so the decode path
    /// still accepts them.
    AstcLdr(AstcBlock),
    /// Raw ASTC HDR 6x6 (`tex_format` 3), raw 16-byte-block slices.
    AstcHdr6x6,
    /// UASTC HDR 6x6 intermediate (`tex_format` 4); each slice is one
    /// bitwise-compressed stream that decompresses to ASTC HDR 6x6 blocks.
    UastcHdr6x6,
    /// XUASTC LDR of the given block size (`tex_format` 5..=18); each slice
    /// is one intermediate stream that decompresses to ASTC LDR blocks.
    XuastcLdr(AstcBlock),
}

/// One parsed `basis_slice_desc` entry (the fields we use).
#[derive(Clone, Copy, Debug)]
struct SliceDesc {
    image_index: u32,
    level_index: u32,
    flags: u8,
    orig_width: u32,
    orig_height: u32,
    num_blocks_x: u32,
    num_blocks_y: u32,
    file_ofs: u32,
    file_size: u32,
}

/// Little-endian u16 at byte offset `o`, widened to u32. The header and
/// slice-desc fields are stored LE regardless of host endianness.
fn rd16(d: &[u8], o: usize) -> u32 {
    u16::from_le_bytes([d[o], d[o + 1]]) as u32
}
/// Little-endian 24-bit value at byte offset `o`. Several count fields
/// (total_slices, total_images, codebook sizes) are packed as 3 bytes.
fn rd24(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], 0])
}
/// Little-endian u32 at byte offset `o`.
fn rd32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

/// A parsed + prepared `.basis` file, ready to transcode any image level.
pub struct BasisTranscoder<'a> {
    data: &'a [u8],
    format: BasisSourceFormat,
    /// Whether the texture carries alpha (ETC1S: the header alpha-slices flag;
    /// UASTC: any slice's per-slice alpha flag).
    has_alpha: bool,
    /// `cBASISTexTypeVideoFrames`: frames decode through the stateful video
    /// entry point with cross-frame conditional replenishment.
    is_video: bool,
    /// `cBASISHeaderFlagSRGB`: raw ASTC LDR slices decode with the sRGB
    /// profile on the re-encode targets.
    astc_is_srgb: bool,
    /// Base (image 0, level 0) pixel dimensions.
    base_width: u32,
    base_height: u32,
    /// Mip levels of image 0 (the 2D case the engine uses).
    level_count: u32,
    total_images: u32,
    slices: Vec<SliceDesc>,
    /// Decoded ETC1S codebooks + Huffman tables (None for UASTC).
    etc1s: Option<Etc1sTranscoder>,
}

impl<'a> BasisTranscoder<'a> {
    /// True if `data` starts with the `.basis` signature. Cheap pre-check used
    /// by the public `Transcoder::new` container sniff.
    pub fn is_basis(data: &[u8]) -> bool {
        data.len() >= 2 && rd16(data, 0) as u16 == BASIS_SIG
    }

    /// Parse + prepare a self-contained `.basis` file. `None` if it is not a
    /// supported `.basis` payload (bad signature/version, an unrecognized
    /// `tex_format`, or malformed framing), or if it is a global-codebook
    /// file (its shared codebook is not available here; use
    /// [`Self::new_with_codebook`]).
    pub fn new(data: &'a [u8]) -> Option<Self> {
        Self::new_with_codebook(data, None)
    }

    /// The decoded ETC1S endpoint + selector codebook, for use as the shared
    /// codebook of a global-codebook file (see [`Self::new_with_codebook`]).
    /// `None` unless this is a self-contained ETC1S file.
    pub fn codebook(&self) -> Option<(&[Endpoint], &[Selector])> {
        self.etc1s
            .as_ref()
            .map(|d| (d.endpoints.as_slice(), d.selectors.as_slice()))
    }

    /// Parse + prepare a `.basis` file, optionally supplying an external ETC1S
    /// codebook for a global-codebook file (a file that carries only its
    /// Huffman tables and slices, referencing a codebook stored in a separate
    /// file). The shared `(endpoints, selectors)` stand in for the palettes the
    /// file omits, and their counts must match the file's header. The codebook
    /// is ignored for a self-contained file. `None` on the same
    /// failures as [`Self::new`], plus a global-codebook file with no codebook
    /// supplied or a codebook whose size does not match the header.
    pub fn new_with_codebook(
        data: &'a [u8],
        codebook: Option<(&[Endpoint], &[Selector])>,
    ) -> Option<Self> {
        // buffer must be strictly larger than the header.
        if data.len() <= HEADER_SIZE as usize {
            return None;
        }
        if rd16(data, 0) as u16 != BASIS_SIG
            || rd16(data, 2) as u16 != BASIS_VERSION
            || rd16(data, 4) as u16 != HEADER_SIZE
        {
            return None;
        }

        let data_size = rd32(data, 8) as usize;
        if data.len() < HEADER_SIZE as usize + data_size {
            return None;
        }

        let total_slices = rd24(data, 14);
        let total_images = rd24(data, 17);
        if total_slices == 0 || total_images == 0 {
            return None;
        }

        let tex_format = data[20];
        let flags = rd16(data, 21) as u16;
        let tex_type = data[23];

        let slice_desc_ofs = rd32(data, 65) as usize;
        // the slice-desc array must fit in the buffer.
        let descs_bytes = SLICE_DESC_SIZE.checked_mul(total_slices as usize)?;
        if slice_desc_ofs >= data.len() || data.len() - slice_desc_ofs < descs_bytes {
            return None;
        }

        // Map the container tex_format to a source codec. An unrecognized
        // value returns None, which the caller treats as a feature skip rather
        // than an error.
        let format = match tex_format {
            TEX_FORMAT_ETC1S => BasisSourceFormat::Etc1s,
            TEX_FORMAT_UASTC_LDR_4X4 => BasisSourceFormat::Uastc,
            TEX_FORMAT_UASTC_HDR_4X4 => BasisSourceFormat::UastcHdr4x4,
            // The ASTC LDR range indexes AstcBlock::ALL directly: the
            // basis_tex_format declaration order matches it.
            TEX_FORMAT_ASTC_LDR_FIRST..=TEX_FORMAT_ASTC_LDR_LAST => BasisSourceFormat::AstcLdr(
                AstcBlock::ALL[(tex_format - TEX_FORMAT_ASTC_LDR_FIRST) as usize],
            ),
            TEX_FORMAT_ASTC_HDR_6X6 => BasisSourceFormat::AstcHdr6x6,
            TEX_FORMAT_UASTC_HDR_6X6_INTERMEDIATE => BasisSourceFormat::UastcHdr6x6,
            TEX_FORMAT_XUASTC_LDR_FIRST..=TEX_FORMAT_XUASTC_LDR_LAST => {
                BasisSourceFormat::XuastcLdr(
                    AstcBlock::ALL[(tex_format - TEX_FORMAT_XUASTC_LDR_FIRST) as usize],
                )
            }
            _ => return None,
        };

        // A file referencing a codebook from another .basis file carries no
        // palettes of its own; it needs the external codebook supplied. The
        // codebook decode below takes that path when the flag is set.
        let uses_global_codebook =
            format == BasisSourceFormat::Etc1s && (flags & FLAG_USES_GLOBAL_CODEBOOK) != 0;

        // Parse the slice-desc array.
        let mut slices = Vec::with_capacity(total_slices as usize);
        for i in 0..total_slices as usize {
            let o = slice_desc_ofs + i * SLICE_DESC_SIZE;
            slices.push(SliceDesc {
                image_index: rd24(data, o),
                level_index: data[o + 3] as u32,
                flags: data[o + 4],
                orig_width: rd16(data, o + 5),
                orig_height: rd16(data, o + 7),
                num_blocks_x: rd16(data, o + 9),
                num_blocks_y: rd16(data, o + 11),
                file_ofs: rd32(data, o + 13),
                file_size: rd32(data, o + 17),
            });
        }

        let has_alpha = match format {
            // ETC1S: the file-level alpha-slices flag governs; if anything has
            // alpha, every image reports alpha.
            BasisSourceFormat::Etc1s => (flags & FLAG_HAS_ALPHA_SLICES) != 0,
            // UASTC: a per-slice alpha flag (each block already carries alpha).
            BasisSourceFormat::Uastc => slices.iter().any(|s| s.flags & SLICE_FLAG_HAS_ALPHA != 0),
            // The HDR codecs never report alpha: the HDR targets have no
            // meaningful alpha channel, so it is always off.
            BasisSourceFormat::UastcHdr4x4
            | BasisSourceFormat::AstcHdr6x6
            | BasisSourceFormat::UastcHdr6x6 => false,
            // XUASTC carries its alpha in the stream headers, not the
            // container. The container-level query still reports the header
            // alpha-slices flag; the transcode itself uses the stream bit.
            BasisSourceFormat::XuastcLdr(_) => (flags & FLAG_HAS_ALPHA_SLICES) != 0,
            // Raw ASTC LDR reports alpha from the header alpha-slices flag,
            // which its low-level transcode also consumes.
            BasisSourceFormat::AstcLdr(_) => (flags & FLAG_HAS_ALPHA_SLICES) != 0,
        };
        // Raw ASTC LDR decodes use the sRGB profile when the header says so
        // (the KTX2 container derives the same bit from its vkFormat).
        let astc_is_srgb = (flags & FLAG_SRGB) != 0;

        let is_video = tex_type == TEX_TYPE_VIDEO_FRAMES
            || slices.iter().any(|s| s.flags & SLICE_FLAG_IS_IFRAME != 0);

        // Base dimensions + level count come from image 0 (the 2D case). Level 0
        // of image 0 is the first color slice for that image.
        let base = slices
            .iter()
            .find(|s| s.image_index == 0 && s.level_index == 0)?;
        let base_width = base.orig_width;
        let base_height = base.orig_height;

        // level count for image 0: scan from the first image-0 slice and take
        // max(level_index)+1 over the contiguous run of image-0 slices.
        let level_count = Self::image_levels(&slices, 0);

        // Decode the ETC1S codebooks + Huffman tables once.
        let etc1s = if format == BasisSourceFormat::Etc1s {
            let t_ofs = rd32(data, 57) as usize;
            let t_size = rd32(data, 61) as usize;
            let num_e = rd16(data, 39);
            let num_s = rd16(data, 48);

            // Both paths need the Huffman slice-decode tables in range.
            if t_size == 0 {
                return None;
            }
            let t = data.get(t_ofs..t_ofs.checked_add(t_size)?)?;

            if uses_global_codebook {
                // The file omits its palettes; use the supplied codebook, whose
                // entry counts must match the header. A size mismatch or a
                // missing codebook is rejected outright.
                let (endpoints, selectors) = codebook?;
                if endpoints.len() as u32 != num_e || selectors.len() as u32 != num_s {
                    return None;
                }
                Some(Etc1sTranscoder::with_global_codebook(
                    endpoints.to_vec(),
                    selectors.to_vec(),
                    t,
                )?)
            } else {
                // Self-contained file: decode its own palettes. The codebook
                // ranges must be non-empty and lie within the buffer.
                let e_ofs = rd32(data, 41) as usize;
                let e_size = rd24(data, 45) as usize;
                let s_ofs = rd32(data, 50) as usize;
                let s_size = rd24(data, 54) as usize;
                if e_size == 0 || s_size == 0 {
                    return None;
                }
                let e = data.get(e_ofs..e_ofs.checked_add(e_size)?)?;
                let s = data.get(s_ofs..s_ofs.checked_add(s_size)?)?;

                let mut dec = Etc1sTranscoder::new();
                if !dec.decode_palettes(num_e, e, num_s, s) || !dec.decode_tables(t) {
                    return None;
                }
                Some(dec)
            }
        } else {
            None
        };

        Some(Self {
            data,
            format,
            has_alpha,
            is_video,
            astc_is_srgb,
            base_width,
            base_height,
            level_count,
            total_images,
            slices,
            etc1s,
        })
    }

    /// Level count for `image_index`: 1 + the highest `level_index` over
    /// the contiguous run of slices for that image. The scan stops at the first
    /// slice belonging to a different image, and the count defaults to 1 when
    /// the image has no level-0 slice.
    fn image_levels(slices: &[SliceDesc], image_index: u32) -> u32 {
        let Some(start) = slices
            .iter()
            .position(|s| s.image_index == image_index && s.level_index == 0)
        else {
            return 1;
        };
        let mut total = 1;
        for s in &slices[start + 1..] {
            if s.image_index == image_index {
                total = total.max(s.level_index + 1);
            } else {
                break;
            }
        }
        total
    }

    /// The Basis source codec the file carries.
    pub fn source_format(&self) -> BasisSourceFormat {
        self.format
    }

    /// Whether the texture carries an alpha channel.
    pub fn has_alpha(&self) -> bool {
        self.has_alpha
    }

    /// Whether this is a `.basis` video (cross-frame conditional replenishment).
    /// Its frames transcode through [`Self::transcode_image_level_video`] in
    /// ascending order so each P-frame sees the right previous frame.
    pub fn is_video(&self) -> bool {
        self.is_video
    }

    /// Base (image 0, level 0) pixel dimensions.
    pub fn base_dimensions(&self) -> (u32, u32) {
        (self.base_width, self.base_height)
    }

    /// Mip levels of image 0.
    pub fn level_count(&self) -> u32 {
        self.level_count
    }

    /// Number of images in the file (a 2D `.basis` uses image 0).
    pub fn image_count(&self) -> u32 {
        self.total_images
    }

    /// Pixel dimensions and block counts of `(image, level)`, the block counts
    /// being in the source format's own footprint (4x4 for ETC1S and UASTC,
    /// the named footprint for the raw ASTC and 6x6 families). `None` if the
    /// slice does not exist.
    pub fn image_level_info(&self, image: u32, level: u32) -> Option<(u32, u32, u32, u32)> {
        let s = self.find_first_slice(image, level)?;
        (s.orig_width, s.orig_height, s.num_blocks_x, s.num_blocks_y).into()
    }

    /// The first slice matching `(image, level)`. For
    /// an ETC1S alpha file this is the color slice (the alpha slice carries the
    /// same image/level but is filtered out by ordering: the color slice always
    /// precedes its alpha slice).
    fn find_first_slice(&self, image: u32, level: u32) -> Option<&SliceDesc> {
        self.slices
            .iter()
            .find(|s| s.image_index == image && s.level_index == level)
    }

    /// Raw color-slice bytes (and the paired alpha slice for ETC1S alpha files)
    /// for `(image, level)`. Each slice's compressed data lives at its absolute
    /// `m_file_ofs`; the alpha slice immediately follows its color slice in the
    /// slice-desc array.
    fn etc1s_slices(&self, image: u32, level: u32) -> Option<(&'a [u8], Option<&'a [u8]>)> {
        let idx = self
            .slices
            .iter()
            .position(|s| s.image_index == image && s.level_index == level)?;
        let color = &self.slices[idx];
        // A properly formed alpha file has the color slice first (no alpha flag).
        if color.flags & SLICE_FLAG_HAS_ALPHA != 0 {
            return None;
        }
        let co = color.file_ofs as usize;
        let rgb = self
            .data
            .get(co..co.checked_add(color.file_size as usize)?)?;

        let alpha = if self.has_alpha {
            let a = self.slices.get(idx + 1)?;
            // Sanity: the next slice must be this image/level's alpha slice with
            // the same block dimensions.
            if a.flags & SLICE_FLAG_HAS_ALPHA == 0
                || a.num_blocks_x != color.num_blocks_x
                || a.num_blocks_y != color.num_blocks_y
            {
                return None;
            }
            let ao = a.file_ofs as usize;
            Some(self.data.get(ao..ao.checked_add(a.file_size as usize)?)?)
        } else {
            None
        };
        Some((rgb, alpha))
    }

    /// Raw UASTC block bytes for `(image, level)`: the slice's `m_file_size`
    /// bytes at its absolute `m_file_ofs` (one slice per image/level, blocks
    /// stored contiguously, no codebooks).
    fn uastc_slice(&self, image: u32, level: u32) -> Option<&'a [u8]> {
        let s = self.find_first_slice(image, level)?;
        let o = s.file_ofs as usize;
        self.data.get(o..o.checked_add(s.file_size as usize)?)
    }

    /// Transcode `(image, level)` to a target `transcoder_texture_format` `fmt`
    /// with `basisd_decode_flags` `flags`, into `out`. `None` for an
    /// unsupported target, a missing image/level, or malformed framing.
    pub fn transcode_image_level(
        &self,
        image: u32,
        level: u32,
        fmt: i32,
        flags: u32,
        out: &mut [u8],
    ) -> Option<()> {
        self.transcode_image_level_inner(image, level, fmt, flags, None, out)
    }

    /// Transcode one video frame (`image` is the frame index) with cross-frame
    /// state, into `out`. Frames must be transcoded in ascending image order
    /// for the conditional-replenishment decode to see the right previous
    /// frame. On a non-video file this behaves exactly like
    /// [`Self::transcode_image_level`].
    pub fn transcode_image_level_video(
        &self,
        image: u32,
        level: u32,
        fmt: i32,
        flags: u32,
        state: &mut VideoState,
        out: &mut [u8],
    ) -> Option<()> {
        self.transcode_image_level_inner(image, level, fmt, flags, Some(state), out)
    }

    /// Shared body behind the transcode entry points; `video` carries the
    /// cross-frame state for the video path.
    fn transcode_image_level_inner(
        &self,
        image: u32,
        level: u32,
        fmt: i32,
        flags: u32,
        video: Option<&mut VideoState>,
        out: &mut [u8],
    ) -> Option<()> {
        let s = self.find_first_slice(image, level)?;
        let (bx, by) = (s.num_blocks_x, s.num_blocks_y);
        let (lw, lh) = (s.orig_width, s.orig_height);
        match self.format {
            BasisSourceFormat::Etc1s => {
                let t = self.etc1s.as_ref()?;
                let (rgb, alpha) = self.etc1s_slices(image, level)?;
                // Cross-frame state only applies to video, and only up to
                // `MAX_PREV_FRAME_LEVELS` mip levels; anything else decodes
                // stateless.
                let vid = if self.is_video && (level as usize) < MAX_PREV_FRAME_LEVELS {
                    video.map(|st| (st, level))
                } else {
                    None
                };
                dispatch::transcode_etc1s(t, rgb, alpha, bx, by, lw, lh, fmt, flags, vid, out)
            }
            BasisSourceFormat::Uastc => {
                let img = self.uastc_slice(image, level)?;
                dispatch::transcode_uastc(img, self.has_alpha, bx, by, lw, lh, fmt, flags, out)
            }
            // Same raw-slice framing as UASTC LDR, HDR per-target dispatch.
            BasisSourceFormat::UastcHdr4x4 => {
                let img = self.uastc_slice(image, level)?;
                dispatch::transcode_uastc_hdr(img, bx, by, lw, lh, fmt, out)
            }
            // Same raw-slice framing again; the slice's recorded block counts
            // are in the source's own footprint.
            BasisSourceFormat::AstcLdr(b) => {
                let img = self.uastc_slice(image, level)?;
                dispatch::transcode_astc_ldr(
                    img,
                    b,
                    bx,
                    by,
                    lw,
                    lh,
                    fmt,
                    flags,
                    self.astc_is_srgb,
                    self.has_alpha,
                    out,
                )
            }
            // Raw 6x6 HDR blocks, same slice framing, HDR per-target dispatch.
            BasisSourceFormat::AstcHdr6x6 => {
                let img = self.uastc_slice(image, level)?;
                dispatch::transcode_astc_hdr_6x6(img, bx, by, lw, lh, fmt, flags, out)
            }
            // The slice is one intermediate stream; decompress it to raw ASTC
            // HDR 6x6 blocks, then the raw 6x6 paths take over. The decoded
            // dimensions must match the slice's recorded dimensions.
            BasisSourceFormat::UastcHdr6x6 => {
                let stream = self.uastc_slice(image, level)?;
                let (blocks, dec_w, dec_h) = crate::uastc_hdr_6x6::decode_6x6_hdr(stream)?;
                if dec_w != lw || dec_h != lh {
                    return None;
                }
                dispatch::transcode_astc_hdr_6x6(&blocks, bx, by, lw, lh, fmt, flags, out)
            }
            // The slice is one XUASTC stream; the transcode layer validates
            // the stream header against the slice's block counts.
            BasisSourceFormat::XuastcLdr(b) => {
                let stream = self.uastc_slice(image, level)?;
                crate::xuastc::transcode::transcode_xuastc(
                    stream, b, bx, by, lw, lh, fmt, flags, out,
                )
            }
        }
    }
}
