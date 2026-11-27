//! KTX2 container parser for Basis Universal payloads (`vkFormat == 0` for
//! ETC1S/UASTC LDR, `VK_FORMAT_ASTC_4x4_SFLOAT_BLOCK` for UASTC HDR 4x4).
//! Parses the header, the level index, and the Data Format Descriptor, which
//! is what distinguishes the source codecs and reveals whether the texture
//! carries alpha. The supercompression global data (the ETC1S codebooks) is
//! decoded separately.

use crate::api::AstcBlock;
use alloc::vec::Vec;

/// The 12-byte KTX2 file identifier every valid file starts with.
const KTX2_MAGIC: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];

/// Data Format Descriptor color model number for a raw ASTC payload (LDR any
/// block size, or HDR 6x6 depending on the header's vkFormat).
const KTX2_KDF_DF_MODEL_ASTC: u32 = 162;
/// Data Format Descriptor color model number for an ETC1S payload.
const KTX2_KDF_DF_MODEL_ETC1S: u32 = 163;
/// Data Format Descriptor color model number for a UASTC LDR 4x4 payload.
const KTX2_KDF_DF_MODEL_UASTC: u32 = 166;
/// Data Format Descriptor color model number for a UASTC HDR 4x4 payload.
const KTX2_KDF_DF_MODEL_UASTC_HDR_4X4: u32 = 167;
/// Data Format Descriptor color model number for a UASTC HDR 6x6 intermediate
/// payload.
const KTX2_KDF_DF_MODEL_UASTC_HDR_6X6_INTERMEDIATE: u32 = 168;
/// Data Format Descriptor color model number for an XUASTC LDR intermediate
/// payload.
const KTX2_KDF_DF_MODEL_XUASTC_LDR_INTERMEDIATE: u32 = 169;

/// `VK_FORMAT_ASTC_4x4_SFLOAT_BLOCK`: the vkFormat a UASTC HDR 4x4 file must
/// declare (its payload is standard ASTC HDR 4x4 data).
const KTX2_FORMAT_ASTC_4X4_SFLOAT_BLOCK: u32 = 1000066000;
/// `VK_FORMAT_ASTC_6x6_SFLOAT_BLOCK`: the vkFormat a raw ASTC HDR 6x6 file
/// must declare. The only HDR presentation the model-162 DFD arm accepts.
const KTX2_FORMAT_ASTC_6X6_SFLOAT_BLOCK: u32 = 1000066004;

/// DFD transfer function ids: the only two `ktx2_transcoder::init` accepts.
const KTX2_KHR_DF_TRANSFER_LINEAR: u32 = 1;
const KTX2_KHR_DF_TRANSFER_SRGB: u32 = 2;

/// The ASTC LDR vkFormat range: `VK_FORMAT_ASTC_4x4_UNORM_BLOCK` (157) through
/// `VK_FORMAT_ASTC_12x12_SRGB_BLOCK` (184). UNORM/SRGB pairs alternate: UNORM
/// odd, SRGB even (so the low bit being clear means sRGB).
const KTX2_FORMAT_ASTC_LDR_FIRST: u32 = 157;
const KTX2_FORMAT_ASTC_LDR_LAST: u32 = 184;

/// Block size for an ASTC LDR *UNORM* vkFormat (the sRGB twin is `vk - 1`).
/// Vulkan's declaration order differs from `basis_tex_format` order: 8x8 sits
/// at 171/172, before 10x5, so this cannot be a simple index into
/// [`AstcBlock::ALL`].
fn astc_ldr_block_from_unorm_vk(vk: u32) -> Option<AstcBlock> {
    Some(match vk {
        157 => AstcBlock::B4x4,
        159 => AstcBlock::B5x4,
        161 => AstcBlock::B5x5,
        163 => AstcBlock::B6x5,
        165 => AstcBlock::B6x6,
        167 => AstcBlock::B8x5,
        169 => AstcBlock::B8x6,
        171 => AstcBlock::B8x8,
        173 => AstcBlock::B10x5,
        175 => AstcBlock::B10x6,
        177 => AstcBlock::B10x8,
        179 => AstcBlock::B10x10,
        181 => AstcBlock::B12x10,
        183 => AstcBlock::B12x12,
        _ => return None,
    })
}

/// Mip chains are capped at 16 levels (an implementation constraint, not a
/// KTX2 spec limit).
const KTX2_MAX_SUPPORTED_LEVEL_COUNT: u32 = 16;

/// `KTX2_SS_ZSTANDARD`: the highest supercompression scheme the shared
/// LDR/HDR level path decodes (0 none, 1 BasisLZ, 2 Zstandard).
const KTX2_SS_ZSTANDARD: u32 = 2;
/// `KTX2_SS_UASTC_HDR_6x6I`: allowed through header validation (its payload
/// is a source codec this crate does not transcode).
pub(crate) const KTX2_SS_UASTC_HDR_6X6I: u32 = 4;
/// `KTX2_SS_XUASTC_LDR`: allowed through header validation (its payload is a
/// source codec this crate does not transcode).
pub(crate) const KTX2_SS_XUASTC_LDR: u32 = 5;

/// UASTC DFD channel id for an RGBA (4-channel) payload, which carries alpha.
const KTX2_DF_CHANNEL_UASTC_RGBA: u32 = 3;
/// UASTC DFD channel id for an RRRG (luma + alpha) payload, which carries alpha.
const KTX2_DF_CHANNEL_UASTC_RRRG: u32 = 5;

/// The Basis source codec carried by a KTX2 file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BasisFormat {
    /// ETC1S: small codebooks plus the BasisLZ entropy-coded indices.
    Etc1s,
    /// UASTC: a fixed 128-bit-per-block intermediate format.
    Uastc,
    /// UASTC HDR 4x4: each 128-bit block is a restricted ASTC HDR block.
    UastcHdr4x4,
    /// Raw ASTC LDR blocks of the given footprint (DFD color model 162 with an
    /// ASTC LDR vkFormat).
    AstcLdr(AstcBlock),
    /// Raw ASTC HDR 6x6 blocks (DFD color model 162 with the 6x6 SFLOAT
    /// vkFormat, the only HDR presentation that model accepts).
    AstcHdr6x6,
    /// UASTC HDR 6x6 intermediate (DFD color model 168, supercompression
    /// scheme 4 or the old-style v1.6/v2.0 BasisLZ-scheme presentation).
    UastcHdr6x6,
    /// XUASTC LDR intermediate of the given block size (DFD color model
    /// 169, supercompression scheme 5 or the old-style scheme-1
    /// presentation; the block size comes from the DFD texel block
    /// dimensions).
    XuastcLdr(AstcBlock),
}

impl BasisFormat {
    /// Source block dimensions `(width, height)` in texels.
    pub fn block_dims(self) -> (u32, u32) {
        match self {
            BasisFormat::Etc1s | BasisFormat::Uastc | BasisFormat::UastcHdr4x4 => (4, 4),
            BasisFormat::AstcLdr(b) => b.dims(),
            BasisFormat::AstcHdr6x6 | BasisFormat::UastcHdr6x6 => (6, 6),
            BasisFormat::XuastcLdr(b) => b.dims(),
        }
    }
}

/// One KTX2 level-index entry: where a mip level's data lives in the file.
#[derive(Clone, Copy, Debug)]
pub struct LevelIndex {
    /// Offset of the level's data from the start of the file.
    pub byte_offset: u64,
    /// Length of the (possibly supercompressed) level data.
    pub byte_length: u64,
    /// Length the level data decompresses to under Zstandard supercompression.
    pub uncompressed_byte_length: u64,
}

/// Parsed KTX2 container metadata for a Basis Universal payload.
#[derive(Clone, Debug)]
pub struct Ktx2Header {
    /// Level-0 width in pixels.
    pub width: u32,
    /// Level-0 height in pixels.
    pub height: u32,
    /// Array layer count; 0 for a non-array texture.
    pub layer_count: u32,
    /// Face count: 6 for a cubemap, otherwise 1.
    pub face_count: u32,
    /// Mip level count; 0 means a single implied level.
    pub level_count: u32,
    /// Supercompression scheme: 0 none, 1 BasisLZ, 2 Zstandard.
    pub supercompression: u32,
    /// Source codec of the payload.
    pub format: BasisFormat,
    /// Whether the texture carries an alpha channel.
    pub has_alpha: bool,
    /// For a raw ASTC LDR source: whether the vkFormat is the sRGB twin, which
    /// selects the sRGB decode profile for every re-encode target. Always
    /// false for the other sources (they carry no vkFormat profile).
    pub astc_is_srgb: bool,
    /// Per-level data locations, one entry per level.
    pub levels: Vec<LevelIndex>,
    /// File offset of the supercompression global data (ETC1S codebooks).
    pub sgd_byte_offset: u64,
    /// Length of the supercompression global data.
    pub sgd_byte_length: u64,
    /// File offset of the key/value data block.
    pub kvd_byte_offset: u32,
    /// Length of the key/value data block.
    pub kvd_byte_length: u32,
    /// True if a `KTXanimData` key/value entry is present (a video marker). A
    /// plain layer array can also become video when an ETC1S image descriptor
    /// carries the P-frame flag, which is decided later in the transcoder once
    /// the image descriptors are parsed.
    pub has_anim_key: bool,
}

/// Read a little-endian u32 at byte offset `o`. KTX2 stores all integers LE.
fn rd32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}
/// Read a little-endian u64 at byte offset `o`.
fn rd64(d: &[u8], o: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[o..o + 8]);
    u64::from_le_bytes(b)
}

impl Ktx2Header {
    /// Parse a KTX2 file. Returns `None` if it is not a Basis Universal KTX2
    /// this crate transcodes (bad magic, an unsupported vkFormat or color
    /// model, or a texture shape this crate rejects: 1D/3D, DEFLATE
    /// supercompression, bad face/level counts).
    pub fn parse(data: &[u8]) -> Option<Ktx2Header> {
        if data.len() < 80 || data[..12] != KTX2_MAGIC {
            return None;
        }
        // vkFormat gate: 0 (UNDEFINED) for ETC1S/UASTC LDR payloads, the ASTC
        // 4x4 SFLOAT block format for UASTC HDR 4x4, the ASTC 6x6 SFLOAT block
        // format for raw ASTC HDR 6x6, or any ASTC LDR block format (raw ASTC
        // LDR sources).
        let vk_format = rd32(data, 12);
        let vk_is_astc_ldr =
            (KTX2_FORMAT_ASTC_LDR_FIRST..=KTX2_FORMAT_ASTC_LDR_LAST).contains(&vk_format);
        if vk_format != 0
            && vk_format != KTX2_FORMAT_ASTC_4X4_SFLOAT_BLOCK
            && vk_format != KTX2_FORMAT_ASTC_6X6_SFLOAT_BLOCK
            && !vk_is_astc_ldr
        {
            return None;
        }
        // KTX2 3.3: "When format is VK_FORMAT_UNDEFINED, typeSize must equal 1."
        if rd32(data, 16) != 1 {
            return None;
        }
        let width = rd32(data, 20);
        let height = rd32(data, 24);
        let depth = rd32(data, 28);
        let layer_count = rd32(data, 32);
        let face_count = rd32(data, 36);
        let level_count = rd32(data, 40);
        let supercompression = rd32(data, 44);
        // Only 2D textures (plain, cubemap, or array): KTX2 1D textures carry
        // pixelHeight == 0 and 3D textures a nonzero pixelDepth; both are
        // rejected.
        if width < 1 || height < 1 || depth > 0 {
            return None;
        }
        // Faces are 1 or 6, and cubemaps must be square.
        if face_count != 1 && face_count != 6 {
            return None;
        }
        if face_count > 1 && width != height {
            return None;
        }
        // KTX2 3.7: levelCount 0 is not allowed for block-compressed formats,
        // and the mip chain length is capped at KTX2_MAX_SUPPORTED_LEVEL_COUNT.
        if !(1..=KTX2_MAX_SUPPORTED_LEVEL_COUNT).contains(&level_count) {
            return None;
        }
        // Supercompression: DEFLATE (3) and unknown schemes fail at open. The
        // 6x6-intermediate and XUASTC schemes pass header validation here
        // (their color models are rejected below instead).
        if supercompression > KTX2_SS_ZSTANDARD
            && supercompression != KTX2_SS_UASTC_HDR_6X6I
            && supercompression != KTX2_SS_XUASTC_LDR
        {
            return None;
        }
        let dfd_byte_offset = rd32(data, 48) as usize;
        let dfd_byte_length = rd32(data, 52) as usize;
        let kvd_byte_offset = rd32(data, 56);
        let kvd_byte_length = rd32(data, 60);
        let sgd_byte_offset = rd64(data, 64);
        let sgd_byte_length = rd64(data, 72);

        // Level index: max(level_count, 1) entries of 24 bytes at offset 80.
        let num_levels = level_count.max(1) as usize;
        let li_end = 80 + num_levels * 24;
        if li_end > data.len() {
            return None;
        }
        let mut levels = Vec::with_capacity(num_levels);
        for i in 0..num_levels {
            let o = 80 + i * 24;
            let l = LevelIndex {
                byte_offset: rd64(data, o),
                byte_length: rd64(data, o + 8),
                uncompressed_byte_length: rd64(data, o + 16),
            };
            // Per-level `uncompressedByteLength` sanity: Zstandard levels must
            // record a nonzero uncompressed size; the BasisLZ and intermediate
            // schemes must record zero (their payloads are not per-level
            // frames).
            match supercompression {
                KTX2_SS_ZSTANDARD if l.uncompressed_byte_length == 0 => return None,
                1 | KTX2_SS_UASTC_HDR_6X6I | KTX2_SS_XUASTC_LDR
                    if l.uncompressed_byte_length != 0 =>
                {
                    return None
                }
                _ => {}
            }
            levels.push(l);
        }

        // Data Format Descriptor: color model + transfer function at word 3,
        // texel block dimensions at word 4, sample 0 at word 7. Only the two
        // DFD sizes the encoder writes are accepted.
        if dfd_byte_length != 44 && dfd_byte_length != 60 {
            return None;
        }
        if dfd_byte_offset + 32 > data.len() {
            return None;
        }
        let dfd_bits = rd32(data, dfd_byte_offset + 12);
        let color_model = dfd_bits & 255;
        let transfer_func = (dfd_bits >> 16) & 255;
        let texel_block_dimensions = rd32(data, dfd_byte_offset + 16);
        let dfd_block_width = (texel_block_dimensions & 0xFF) + 1;
        let dfd_block_height = ((texel_block_dimensions >> 8) & 0xFF) + 1;
        let sample_channel0 = rd32(data, dfd_byte_offset + 28);

        // KTX2 3.10.1 restrictions: only linear and sRGB transfer functions.
        if transfer_func != KTX2_KHR_DF_TRANSFER_LINEAR
            && transfer_func != KTX2_KHR_DF_TRANSFER_SRGB
        {
            return None;
        }

        // Source-format resolution: an ASTC LDR vkFormat decides the format by
        // itself (the DFD must agree it is ASTC); otherwise the DFD color model
        // chooses, with each model demanding its exact vkFormat.
        let (format, has_alpha, astc_is_srgb) = if vk_is_astc_ldr {
            if color_model != KTX2_KDF_DF_MODEL_ASTC {
                return None;
            }
            // UNORM/SRGB twins alternate; even means sRGB. Normalize to the
            // UNORM id to pick the block size.
            let is_srgb = (vk_format & 1) == 0;
            let unorm = if is_srgb { vk_format - 1 } else { vk_format };
            let block = astc_ldr_block_from_unorm_vk(unorm)?;
            // The vkFormat's block size and the DFD's must agree.
            if block.dims() != (dfd_block_width, dfd_block_height) {
                return None;
            }
            let chan0 = (sample_channel0 >> 24) & 15;
            (
                BasisFormat::AstcLdr(block),
                chan0 == KTX2_DF_CHANNEL_UASTC_RGBA || chan0 == KTX2_DF_CHANNEL_UASTC_RRRG,
                is_srgb,
            )
        } else if color_model == KTX2_KDF_DF_MODEL_ETC1S {
            if vk_format != 0 {
                return None;
            }
            // 2 slices (alpha) <=> a 60-byte DFD.
            (BasisFormat::Etc1s, dfd_byte_length == 60, false)
        } else if color_model == KTX2_KDF_DF_MODEL_UASTC {
            if vk_format != 0 {
                return None;
            }
            let chan0 = (sample_channel0 >> 24) & 15;
            (
                BasisFormat::Uastc,
                chan0 == KTX2_DF_CHANNEL_UASTC_RGBA || chan0 == KTX2_DF_CHANNEL_UASTC_RRRG,
                false,
            )
        } else if color_model == KTX2_KDF_DF_MODEL_UASTC_HDR_4X4 {
            // UASTC HDR 4x4 is standard ASTC HDR 4x4 texture data, so the
            // header must declare the matching vkFormat.
            if vk_format != KTX2_FORMAT_ASTC_4X4_SFLOAT_BLOCK {
                return None;
            }
            // has_alpha is forced off for this codec regardless of the DFD
            // channel id.
            (BasisFormat::UastcHdr4x4, false, false)
        } else if color_model == KTX2_KDF_DF_MODEL_ASTC {
            // Raw ASTC with a non-LDR vkFormat: the 6x6 SFLOAT block format is
            // the only HDR presentation accepted for this model.
            if vk_format != KTX2_FORMAT_ASTC_6X6_SFLOAT_BLOCK {
                return None;
            }
            (BasisFormat::AstcHdr6x6, false, false)
        } else if color_model == KTX2_KDF_DF_MODEL_UASTC_HDR_6X6_INTERMEDIATE {
            // Custom variable-block-size stream; the header must not claim a
            // vkFormat. The slice offset/length global data is parsed by the
            // transcoder (its shape depends on the supercompression scheme).
            if vk_format != 0 {
                return None;
            }
            (BasisFormat::UastcHdr6x6, false, false)
        } else if color_model == KTX2_KDF_DF_MODEL_XUASTC_LDR_INTERMEDIATE {
            // Same framing as the 6x6 intermediate, but the ASTC block size
            // comes from the DFD texel block dimensions (must be one of the
            // 14 valid sizes). The sRGB/alpha bits live in the stream
            // headers, not the container.
            if vk_format != 0 {
                return None;
            }
            let block = AstcBlock::ALL
                .into_iter()
                .find(|b| b.dims() == (dfd_block_width, dfd_block_height))?;
            (BasisFormat::XuastcLdr(block), false, false)
        } else {
            return None;
        };

        let has_anim_key = has_kvd_key(data, kvd_byte_offset, kvd_byte_length, b"KTXanimData");

        Some(Ktx2Header {
            width,
            height,
            layer_count,
            face_count,
            level_count,
            supercompression,
            format,
            has_alpha,
            astc_is_srgb,
            levels,
            sgd_byte_offset,
            sgd_byte_length,
            kvd_byte_offset,
            kvd_byte_length,
            has_anim_key,
        })
    }

    /// True if the payload is UASTC rather than ETC1S.
    pub fn is_uastc(&self) -> bool {
        self.format == BasisFormat::Uastc
    }
}

/// Scan the KTX2 key/value data for a NUL-terminated `key`. Each entry is a
/// `u32` byte length followed by `key\0value` padded up to a 4-byte boundary.
/// Used to detect the `KTXanimData` video marker. A malformed entry stops the
/// scan and returns false, so the marker is treated as absent.
fn has_kvd_key(data: &[u8], kvd_offset: u32, kvd_length: u32, key: &[u8]) -> bool {
    if kvd_length == 0 {
        return false;
    }
    let start = kvd_offset as usize;
    let end = start + kvd_length as usize;
    if end > data.len() {
        return false;
    }
    let mut p = start;
    while end - p > 4 {
        let l = u32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]) as usize;
        p += 4;
        if l < 2 || end - p < l {
            return false;
        }
        // The key is the NUL-terminated prefix of the kv blob.
        let blob = &data[p..p + l];
        if let Some(nul) = blob.iter().position(|&b| b == 0) {
            if &blob[..nul] == key {
                return true;
            }
        }
        p += l;
        // Advance to the next 4-byte boundary (relative to the file start).
        p += (4 - (p & 3)) & 3;
    }
    false
}
