//! Top-level `Ktx2Transcoder` for Basis Universal KTX2 files. Parses the
//! container, decodes the ETC1S supercompression global data (codebooks +
//! per-image slice descriptors), and dispatches `transcode_image_level` to the
//! per-target converters. This is the public entry point callers use to
//! transcode a level.

use crate::basislz::etc1s::{Etc1sTranscoder, VideoState, MAX_PREV_FRAME_LEVELS};
use crate::dispatch;
use crate::ktx2::{BasisFormat, Ktx2Header, KTX2_SS_UASTC_HDR_6X6I, KTX2_SS_XUASTC_LDR};
use alloc::vec::Vec;

/// `KTX2_IMAGE_IS_P_FRAME`: an ETC1S image desc flag marking a predicted
/// (inter) video frame. Its presence on any image promotes a layer array to a
/// video, even without a `KTXanimData` key.
const KTX2_IMAGE_IS_P_FRAME: u32 = 2;

/// One ETC1S image's slice offsets/lengths (relative to its level data).
#[derive(Clone, Copy)]
struct Etc1sImageDesc {
    /// Per-image flag bits; `KTX2_IMAGE_IS_P_FRAME` here marks a predicted
    /// video frame.
    image_flags: u32,
    /// Byte offset of the RGB (color) slice within the level data.
    rgb_ofs: u32,
    /// Byte length of the RGB slice.
    rgb_len: u32,
    /// Byte offset of the alpha slice within the level data (only meaningful
    /// when the file has alpha).
    alpha_ofs: u32,
    /// Byte length of the alpha slice.
    alpha_len: u32,
}

/// A parsed + prepared Basis Universal KTX2, ready to transcode any level.
pub struct Ktx2Transcoder<'a> {
    /// The whole KTX2 file.
    data: &'a [u8],
    /// Parsed header, level index, and key/value-derived properties.
    pub header: Ktx2Header,
    /// Decoded ETC1S codebooks + Huffman tables (`None` for UASTC files).
    etc1s: Option<Etc1sTranscoder>,
    /// ETC1S per-image slice descriptors (empty for UASTC files).
    image_descs: Vec<Etc1sImageDesc>,
    /// UASTC HDR 6x6 intermediate per-image `(byte offset, byte length)`
    /// slice descriptors, relative to their level's byte offset (empty for
    /// every other format).
    slice_descs: Vec<(u32, u32)>,
    /// True for ETC1S video: a `KTXanimData` key, or any image desc carrying
    /// `KTX2_IMAGE_IS_P_FRAME` on a non-cubemap layer array.
    is_video: bool,
}

/// Inflate a Zstandard-supercompressed KTX2 level to exactly `want` bytes
/// (the level's `uncompressed_byte_length`). `None` if the stream is malformed
/// or does not produce exactly `want` bytes; a size mismatch means a corrupt
/// level. Decoding is pure Rust (`ruzstd`).
#[cfg(feature = "zstd")]
fn zstd_inflate(comp: &[u8], want: usize) -> Option<Vec<u8>> {
    use ruzstd::io::Read;
    let mut dec = ruzstd::StreamingDecoder::new(comp).ok()?;
    let mut out = vec![0u8; want];
    // Require an exact fill: read_exact errors if the stream yields fewer bytes.
    dec.read_exact(&mut out).ok()?;
    // Reject trailing data beyond `want`: a single extra byte means a
    // malformed or oversized stream.
    let mut extra = [0u8; 1];
    if dec.read(&mut extra).ok()? != 0 {
        return None;
    }
    Some(out)
}

/// Without the `zstd` feature, zstd-supercompressed levels are unsupported.
#[cfg(not(feature = "zstd"))]
fn zstd_inflate(_comp: &[u8], _want: usize) -> Option<Vec<u8>> {
    None
}

/// Read a little-endian u16 at byte offset `o`. The KTX2 supercompression
/// global data stores all integers little-endian.
fn rd16(d: &[u8], o: usize) -> u32 {
    u16::from_le_bytes([d[o], d[o + 1]]) as u32
}
/// Read a little-endian u32 at byte offset `o`.
fn rd32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

impl<'a> Ktx2Transcoder<'a> {
    /// Parse + prepare a KTX2 file. `None` if it is not a Basis payload or the
    /// supercompression global data is malformed.
    pub fn new(data: &'a [u8]) -> Option<Self> {
        let header = Ktx2Header::parse(data)?;
        let mut etc1s = None;
        let mut image_descs = Vec::new();
        let mut slice_descs = Vec::new();

        if header.format == BasisFormat::UastcHdr6x6
            || matches!(header.format, BasisFormat::XuastcLdr(_))
        {
            // The intermediate streams are variable-length per image, so the
            // supercompression global data is an array of per-image
            // offset/length descs: the Khronos schemes (4 for HDR 6x6, 5 for
            // XUASTC) use 12-byte structs (offset, length, profile), the
            // old-style v1.6/v2.0 presentation (BasisLZ scheme) 8-byte ones.
            // Any other scheme fails.
            let desc_size = match (header.supercompression, header.format) {
                (KTX2_SS_UASTC_HDR_6X6I, BasisFormat::UastcHdr6x6) => 12,
                (KTX2_SS_XUASTC_LDR, BasisFormat::XuastcLdr(_)) => 12,
                (1, _) => 8,
                _ => return None,
            };
            let image_count = (header.level_count.max(1)
                * header.layer_count.max(1)
                * header.face_count.max(1)) as usize;
            let sgd = header.sgd_byte_offset as usize;
            if header.sgd_byte_length as usize != image_count * desc_size
                || sgd + image_count * desc_size > data.len()
            {
                return None;
            }
            for i in 0..image_count {
                let o = sgd + i * desc_size;
                let ofs = rd32(data, o);
                let len = rd32(data, o + 4);
                if len == 0 {
                    return None;
                }
                slice_descs.push((ofs, len));
            }
        }

        if header.format == BasisFormat::Etc1s {
            // BasisLZ global data header (20 bytes): endpoint/selector counts
            // (u16 each), the endpoint/selector/table byte lengths (u32 each),
            // and an extendedByteLength u32 this decoder never needs.
            let sgd = header.sgd_byte_offset as usize;
            if sgd + 20 > data.len() {
                return None;
            }
            let num_e = rd16(data, sgd);
            let num_s = rd16(data, sgd + 2);
            let e_len = rd32(data, sgd + 4) as usize;
            let s_len = rd32(data, sgd + 8) as usize;
            let t_len = rd32(data, sgd + 12) as usize;

            let num_levels = header.level_count.max(1) as usize;
            let image_count =
                num_levels * header.layer_count.max(1) as usize * header.face_count.max(1) as usize;

            let descs_end = sgd + 20 + 20 * image_count;
            if descs_end > data.len() {
                return None;
            }
            for i in 0..image_count {
                let o = sgd + 20 + 20 * i;
                image_descs.push(Etc1sImageDesc {
                    image_flags: rd32(data, o),
                    rgb_ofs: rd32(data, o + 4),
                    rgb_len: rd32(data, o + 8),
                    alpha_ofs: rd32(data, o + 12),
                    alpha_len: rd32(data, o + 16),
                });
            }

            // The payloads follow the image descs in a fixed order: endpoint
            // codebook, selector codebook, Huffman tables.
            let e_ofs = descs_end;
            let s_ofs = e_ofs + e_len;
            let t_ofs = s_ofs + s_len;
            if t_ofs + t_len > data.len() {
                return None;
            }
            let mut t = Etc1sTranscoder::new();
            if !t.decode_palettes(
                num_e,
                &data[e_ofs..e_ofs + e_len],
                num_s,
                &data[s_ofs..s_ofs + s_len],
            ) || !t.decode_tables(&data[t_ofs..t_ofs + t_len])
            {
                return None;
            }
            etc1s = Some(t);
        }

        // Video detection: a `KTXanimData` key marks video; absent that, a
        // non-cubemap layer array is promoted to video if any ETC1S image desc
        // carries a P-frame flag.
        let mut is_video = header.has_anim_key;
        if !is_video
            && header.format == BasisFormat::Etc1s
            && header.face_count == 1
            && header.layer_count > 1
        {
            is_video = image_descs
                .iter()
                .any(|d| d.image_flags & KTX2_IMAGE_IS_P_FRAME != 0);
        }

        Some(Self {
            data,
            header,
            etc1s,
            image_descs,
            slice_descs,
            is_video,
        })
    }

    /// Whether this is an ETC1S video (P-frame conditional replenishment).
    pub fn is_video(&self) -> bool {
        self.is_video
    }

    /// `(num_blocks_x, num_blocks_y)` for a mip level, in the source format's
    /// own block size (4x4 for ETC1S/UASTC, the footprint for raw ASTC).
    fn level_blocks(&self, level: u32) -> (u32, u32) {
        let lw = (self.header.width >> level).max(1);
        let lh = (self.header.height >> level).max(1);
        let (bw, bh) = self.header.format.block_dims();
        (lw.div_ceil(bw), lh.div_ceil(bh))
    }

    /// ETC1S RGB (+ optional alpha) slice bytes for image (level, layer, face).
    fn etc1s_slices(&self, level: u32, layer: u32, face: u32) -> Option<(&[u8], Option<&[u8]>)> {
        let face_count = self.header.face_count.max(1);
        let idx = (level * self.header.layer_count.max(1) * face_count + layer * face_count + face)
            as usize;
        let d = self.image_descs.get(idx)?;
        let lo = self.header.levels[level as usize].byte_offset as usize;
        let rgb = self
            .data
            .get(lo + d.rgb_ofs as usize..lo + d.rgb_ofs as usize + d.rgb_len as usize)?;
        let alpha =
            if self.header.has_alpha {
                Some(self.data.get(
                    lo + d.alpha_ofs as usize..lo + d.alpha_ofs as usize + d.alpha_len as usize,
                )?)
            } else {
                None
            };
        Some((rgb, alpha))
    }

    /// Flags-aware transcode of image (level, layer 0, face 0) into `out`.
    /// `flags` are the `basisd_decode_flags` bits, forwarded to the per-target
    /// dispatch which interprets the bits each target honors (for example
    /// HIGH_QUALITY, BC1_FORBID_THREE_COLOR_BLOCKS, NO_ETC1S_CHROMA_FILTERING).
    pub fn transcode_image_level_flags(
        &self,
        level: u32,
        fmt: i32,
        flags: u32,
        out: &mut [u8],
    ) -> Option<()> {
        self.transcode_image_level_inner(level, 0, 0, fmt, flags, None, out)
    }

    /// Flags-aware transcode of a specific image `(level, layer, face)` into
    /// `out`. Cubemap faces and array layers select among the multiple images
    /// per level.
    pub fn transcode_image(
        &self,
        level: u32,
        layer: u32,
        face: u32,
        fmt: i32,
        flags: u32,
        out: &mut [u8],
    ) -> Option<()> {
        self.transcode_image_level_inner(level, layer, face, fmt, flags, None, out)
    }

    /// Transcode image (level, layer 0, face 0) to a target format with no
    /// decode flags, into `out`. `None` for unsupported (format, target)
    /// combinations.
    pub fn transcode_image_level(&self, level: u32, fmt: i32, out: &mut [u8]) -> Option<()> {
        self.transcode_image_level_inner(level, 0, 0, fmt, 0, None, out)
    }

    /// Transcode one video frame (`layer` is the frame index) with cross-frame
    /// state, into `out`. Frames of a level must be transcoded in ascending
    /// layer order for the conditional-replenishment decode to see the right
    /// previous frame. On a non-video file this behaves exactly like
    /// [`Self::transcode_image`].
    pub fn transcode_image_video(
        &self,
        level: u32,
        layer: u32,
        fmt: i32,
        flags: u32,
        state: &mut VideoState,
        out: &mut [u8],
    ) -> Option<()> {
        self.transcode_image_level_inner(level, layer, 0, fmt, flags, Some(state), out)
    }

    /// Shared body behind the public `transcode_*` entry points. Resolves the
    /// level's block and pixel dimensions, then dispatches to the ETC1S or
    /// UASTC path for the file's source format. `None` for an unsupported
    /// (source, target) pair or out-of-range image coordinates.
    #[allow(clippy::too_many_arguments)]
    fn transcode_image_level_inner(
        &self,
        level: u32,
        layer: u32,
        face: u32,
        fmt: i32,
        flags: u32,
        video: Option<&mut VideoState>,
        out: &mut [u8],
    ) -> Option<()> {
        let (bx, by) = self.level_blocks(level);
        let lw = (self.header.width >> level).max(1);
        let lh = (self.header.height >> level).max(1);
        match self.header.format {
            BasisFormat::Etc1s => {
                let t = self.etc1s.as_ref()?;
                let (rgb, alpha) = self.etc1s_slices(level, layer, face)?;
                // Cross-frame state only applies to video, and only for the
                // first MAX_PREV_FRAME_LEVELS levels; anything else decodes
                // stateless.
                let vid = if self.is_video && (level as usize) < MAX_PREV_FRAME_LEVELS {
                    video.map(|s| (s, level))
                } else {
                    None
                };
                dispatch::transcode_etc1s(t, rgb, alpha, bx, by, lw, lh, fmt, flags, vid, out)
            }
            // UASTC (LDR/HDR) and the raw ASTC sources share the level/slice
            // layout (contiguous raw 16-byte blocks per image); only the
            // per-target dispatch differs.
            BasisFormat::Uastc
            | BasisFormat::UastcHdr4x4
            | BasisFormat::AstcLdr(_)
            | BasisFormat::AstcHdr6x6 => {
                self.transcode_raw_level(level, layer, face, fmt, flags, bx, by, lw, lh, out)
            }
            // The XUASTC stream's slice comes from the SGD descs like the 6x6
            // intermediate; the transcode layer validates the stream header
            // against the level's block counts.
            BasisFormat::XuastcLdr(b) => {
                let face_count = self.header.face_count.max(1);
                let image_index = (level * self.header.layer_count.max(1) * face_count
                    + layer * face_count
                    + face) as usize;
                let &(ofs, len) = self.slice_descs.get(image_index)?;
                let lo = self.header.levels.get(level as usize)?.byte_offset as usize;
                let stream = self
                    .data
                    .get(lo + ofs as usize..lo + ofs as usize + len as usize)?;
                crate::xuastc::transcode::transcode_xuastc(
                    stream, b, bx, by, lw, lh, fmt, flags, out,
                )
            }
            // The intermediate stream decompresses to raw ASTC HDR 6x6 blocks
            // first; the per-target paths are then the raw 6x6 ones.
            BasisFormat::UastcHdr6x6 => {
                let face_count = self.header.face_count.max(1);
                let image_index = (level * self.header.layer_count.max(1) * face_count
                    + layer * face_count
                    + face) as usize;
                let &(ofs, len) = self.slice_descs.get(image_index)?;
                let lo = self.header.levels.get(level as usize)?.byte_offset as usize;
                let stream = self
                    .data
                    .get(lo + ofs as usize..lo + ofs as usize + len as usize)?;
                let (blocks, dec_w, dec_h) = crate::uastc_hdr_6x6::decode_6x6_hdr(stream)?;
                // The decoded dimensions must match the level's, or the stream
                // is inconsistent with the container.
                if dec_w != lw || dec_h != lh {
                    return None;
                }
                dispatch::transcode_astc_hdr_6x6(&blocks, bx, by, lw, lh, fmt, flags, out)
            }
        }
    }

    /// Raw-block (UASTC LDR/HDR or raw ASTC) image transcode into `out`. The
    /// level data is `layer_count*face_count` contiguous 2D images in
    /// layer-major order, each `num_blocks_x*num_blocks_y*16` bytes of the
    /// source's own block size. Uncompressed and Zstandard-supercompressed
    /// levels are handled; returns `None` for an unknown supercompression
    /// scheme, a zstd level when the `zstd` feature is off, or an unsupported
    /// target.
    #[allow(clippy::too_many_arguments)]
    fn transcode_raw_level(
        &self,
        level: u32,
        layer: u32,
        face: u32,
        fmt: i32,
        flags: u32,
        bx: u32,
        by: u32,
        lw: u32,
        lh: u32,
        out: &mut [u8],
    ) -> Option<()> {
        let l = &self.header.levels[level as usize];
        let comp = self
            .data
            .get(l.byte_offset as usize..(l.byte_offset + l.byte_length) as usize)?;
        // ss=2 (Zstandard): inflate the whole level to its uncompressed length
        // before indexing the per-image data. ss=1 (BasisLZ) never reaches the
        // raw-block path.
        let level_owned;
        let level_data: &[u8] = match self.header.supercompression {
            0 => comp,
            2 => {
                let want = l.uncompressed_byte_length as usize;
                level_owned = zstd_inflate(comp, want)?;
                &level_owned
            }
            _ => return None, // unknown supercompression scheme
        };
        // The raw ASTC LDR arm hard-checks the level size against the recorded
        // uncompressed length: for scheme 0 the two length fields must agree
        // (for zstd the inflate above already enforced it).
        if matches!(self.header.format, BasisFormat::AstcLdr(_))
            && self.header.supercompression == 0
            && l.byte_length != l.uncompressed_byte_length
        {
            return None;
        }
        let n = (bx * by) as usize;
        let total_2d = n * 16;
        let face_count = self.header.face_count.max(1) as usize;
        let img_ofs = (layer as usize * face_count + face as usize) * total_2d;
        let img = level_data.get(img_ofs..img_ofs + total_2d)?;

        match self.header.format {
            BasisFormat::UastcHdr4x4 => {
                dispatch::transcode_uastc_hdr(img, bx, by, lw, lh, fmt, out)
            }
            BasisFormat::AstcLdr(b) => dispatch::transcode_astc_ldr(
                img,
                b,
                bx,
                by,
                lw,
                lh,
                fmt,
                flags,
                self.header.astc_is_srgb,
                self.header.has_alpha,
                out,
            ),
            BasisFormat::AstcHdr6x6 => {
                dispatch::transcode_astc_hdr_6x6(img, bx, by, lw, lh, fmt, flags, out)
            }
            _ => dispatch::transcode_uastc(
                img,
                self.header.has_alpha,
                bx,
                by,
                lw,
                lh,
                fmt,
                flags,
                out,
            ),
        }
    }
}
