//! Container-independent per-target transcode dispatch, shared by both the KTX2
//! and `.basis` front-ends. Each front-end parses its own container framing to
//! recover the same inputs: for ETC1S, the decoded codebooks plus a color slice
//! and optional alpha slice; for UASTC, the per-image raw block bytes. Once
//! those are in hand the per-target conversion is identical, so it lives here.

use crate::basislz::etc1s::Etc1sTranscoder;
use crate::etc::uastc_to_etc1::transcode_uastc_to_etc1;
use crate::etc::uastc_to_etc2::transcode_uastc_to_etc2_rgba;
use crate::uastc::astc_pack::transcode_uastc_to_astc;
use crate::uastc::bc1::transcode_uastc_to_bc1;
use crate::uastc::bc3_bc5::{transcode_uastc_to_bc3, transcode_uastc_to_bc5};
use crate::uastc::bc4::transcode_uastc_to_bc4;
use crate::uastc::bc7::transcode_uastc_to_bc7;
use crate::uastc::eac::{transcode_uastc_to_etc2_eac_r11, transcode_uastc_to_etc2_eac_rg11};
use crate::uastc::unpack::unpack_uastc;

/// Target `transcoder_texture_format` ids (the subset this crate can emit).
pub mod target {
    pub const ETC1_RGB: i32 = 0;
    pub const ETC2_RGBA: i32 = 1;
    pub const BC1_RGB: i32 = 2;
    pub const BC3_RGBA: i32 = 3;
    pub const BC4_R: i32 = 4;
    pub const BC5_RG: i32 = 5;
    pub const BC7_RGBA: i32 = 6;
    pub const PVRTC1_4_RGB: i32 = 8;
    pub const PVRTC1_4_RGBA: i32 = 9;
    pub const ASTC_4X4_RGBA: i32 = 10;
    pub const ATC_RGB: i32 = 11;
    pub const ATC_RGBA: i32 = 12;
    pub const PVRTC2_4_RGB: i32 = 18;
    pub const PVRTC2_4_RGBA: i32 = 19;
    pub const RGBA32: i32 = 13;
    pub const RGB565: i32 = 14;
    pub const BGR565: i32 = 15;
    pub const RGBA4444: i32 = 16;
    pub const FXT1_RGB: i32 = 17;
    pub const EAC_R11: i32 = 20;
    pub const EAC_RG11: i32 = 21;
    pub const BC6H: i32 = 22;
    pub const ASTC_HDR_4X4_RGBA: i32 = 23;
    pub const RGB_HALF: i32 = 24;
    pub const RGBA_HALF: i32 = 25;
    pub const RGB_9E5: i32 = 26;
    pub const ASTC_HDR_6X6_RGBA: i32 = 27;
}

/// The `basisd_decode_flags` bits the dispatch honors.
pub mod decode_flags {
    pub const BC1_FORBID_THREE_COLOR_BLOCKS: u32 = 8;
    pub const HIGH_QUALITY: u32 = 32;
    pub const NO_ETC1S_CHROMA_FILTERING: u32 = 64;
}

/// `mul_8(v, q)`: rounded requantize of an 8-bit `v` to a q-scaled value, used
/// by the UASTC 565/4444 packs. `q` is the target maximum (31, 63, or 15).
#[inline]
fn mul_8(v: u32, q: u32) -> u32 {
    let v = v * q + 128;
    (v + (v >> 8)) >> 8
}

/// Transcode one ETC1S image to `fmt`, writing into `out` (sized per the
/// target's own layout). `rgb` is the BasisLZ color slice and `alpha`, if
/// present, the paired alpha slice (both already decoded from the container
/// framing). `(bx, by)` are the level's 4x4 block dimensions and `(lw, lh)`
/// its pixel dimensions. `video`, if present, is the ETC1S video state plus
/// the mip level index whose previous-frame slots this image uses. `None` for
/// an unsupported target.
#[allow(clippy::too_many_arguments)]
pub fn transcode_etc1s(
    t: &Etc1sTranscoder,
    rgb: &[u8],
    alpha: Option<&[u8]>,
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    fmt: i32,
    flags: u32,
    video: Option<(&mut crate::basislz::etc1s::VideoState, u32)>,
    out: &mut [u8],
) -> Option<()> {
    let vid = video.map(|(state, level)| state.slots(level as usize, (bx * by) as usize));
    match fmt {
        target::ASTC_4X4_RGBA => match alpha {
            Some(a) => t.transcode_image_astc_rgba(rgb, a, bx, by, vid, out),
            None => t.transcode_slice_astc(rgb, bx, by, vid.map(|(c, _)| c), out),
        },
        target::BC1_RGB => {
            // opaque target: an alpha slice, if present, is ignored
            // (the alpha-to-opaque decode flag is not supported here)
            let forbid = (flags & decode_flags::BC1_FORBID_THREE_COLOR_BLOCKS) != 0;
            t.transcode_slice_bc1(rgb, bx, by, forbid, vid.map(|(c, _)| c), out)
        }
        target::BC3_RGBA => match alpha {
            Some(a) => t.transcode_image_bc3_rgba(rgb, a, bx, by, vid, out),
            None => t.transcode_slice_bc3_opaque(rgb, bx, by, vid.map(|(c, _)| c), out),
        },
        // PVRTC1_4_RGBA is image-global with alpha; without an alpha slice it
        // falls back to the RGB path.
        target::PVRTC1_4_RGBA => match alpha {
            Some(a) => t.transcode_slice_pvrtc1_4_rgba(rgb, a, bx, by, vid, out),
            None => t.transcode_slice_pvrtc1_4_rgb(rgb, bx, by, vid.map(|(c, _)| c), out),
        },
        // PVRTC1 is image-global: per-block bounding-box endpoints, then a
        // whole-image modulation pass. Opaque (alpha ignored).
        target::PVRTC1_4_RGB => {
            t.transcode_slice_pvrtc1_4_rgb(rgb, bx, by, vid.map(|(c, _)| c), out)
        }
        target::BC4_R => t.transcode_slice_bc4(rgb, bx, by, vid.map(|(c, _)| c), out),
        target::BC5_RG => match alpha {
            Some(a) => t.transcode_image_bc5_rg(rgb, a, bx, by, vid, out),
            None => t.transcode_slice_bc5_opaque(rgb, bx, by, vid.map(|(c, _)| c), out),
        },
        target::BC7_RGBA => {
            // cross-block BC7 mode-5 chroma filtering is on by default;
            // NO_ETC1S_CHROMA_FILTERING opts out.
            let chroma = (flags & decode_flags::NO_ETC1S_CHROMA_FILTERING) == 0;
            match alpha {
                Some(a) => t.transcode_image_bc7_rgba(rgb, a, bx, by, chroma, vid, out),
                None => t.transcode_slice_bc7(rgb, bx, by, chroma, vid.map(|(c, _)| c), out),
            }
        }
        target::ETC2_RGBA => match alpha {
            Some(a) => t.transcode_image_etc2_rgba(rgb, a, bx, by, vid, out),
            None => t.transcode_slice_etc2_rgba_opaque(rgb, bx, by, vid.map(|(c, _)| c), out),
        },
        target::EAC_R11 => t.transcode_slice_eac_r11(rgb, bx, by, vid.map(|(c, _)| c), out),
        target::EAC_RG11 => match alpha {
            Some(a) => t.transcode_image_eac_rg11(rgb, a, bx, by, vid, out),
            None => t.transcode_slice_eac_rg11_opaque(rgb, bx, by, vid.map(|(c, _)| c), out),
        },
        target::ATC_RGB => t.transcode_slice_atc(rgb, bx, by, vid.map(|(c, _)| c), out),
        target::ATC_RGBA => match alpha {
            Some(a) => t.transcode_image_atc_rgba(rgb, a, bx, by, vid, out),
            None => t.transcode_slice_atc_rgba_opaque(rgb, bx, by, vid.map(|(c, _)| c), out),
        },
        target::PVRTC2_4_RGB => t.transcode_slice_pvrtc2_rgb(rgb, bx, by, vid.map(|(c, _)| c), out),
        target::PVRTC2_4_RGBA => match alpha {
            Some(a) => t.transcode_image_pvrtc2_rgba(rgb, a, bx, by, vid, out),
            None => t.transcode_slice_pvrtc2_rgba_opaque(rgb, bx, by, vid.map(|(c, _)| c), out),
        },
        target::RGBA32 => match alpha {
            Some(a) => t.transcode_image_rgba32(rgb, a, bx, by, lw, lh, vid, out),
            None => t.transcode_slice_rgba32(rgb, bx, by, lw, lh, vid.map(|(c, _)| c), out),
        },
        target::ETC1_RGB => t.transcode_slice_etc1(rgb, bx, by, vid.map(|(c, _)| c), out),
        // FXT1 has an 8x4 block (the only such target): output grid is
        // ceil(w/8)*ceil(h/4), two ETC1S 4x4 blocks per FXT1 block.
        target::FXT1_RGB => t.transcode_slice_fxt1(rgb, bx, by, lw, lh, vid.map(|(c, _)| c), out),
        target::RGB565 => {
            t.transcode_slice_565(rgb, bx, by, lw, lh, false, vid.map(|(c, _)| c), out)
        }
        target::BGR565 => {
            t.transcode_slice_565(rgb, bx, by, lw, lh, true, vid.map(|(c, _)| c), out)
        }
        target::RGBA4444 => match alpha {
            Some(a) => t.transcode_image_rgba4444(rgb, a, bx, by, lw, lh, vid, out),
            None => {
                t.transcode_slice_rgba4444_opaque(rgb, bx, by, lw, lh, vid.map(|(c, _)| c), out)
            }
        },
        _ => None,
    }
}

/// Transcode one UASTC image to `fmt`, writing into `out` (sized per the
/// target's own layout). `img` is the level's per-image raw UASTC block bytes
/// (`bx*by*16`); `has_alpha` reports whether the source carries an alpha
/// channel (it only affects the PVRTC1 RGBA fallback). `(bx, by)` are the 4x4
/// block dimensions and `(lw, lh)` the pixel dimensions. `None` for an
/// unsupported target or a malformed block.
#[allow(clippy::too_many_arguments)]
pub fn transcode_uastc(
    img: &[u8],
    has_alpha: bool,
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    fmt: i32,
    flags: u32,
    out: &mut [u8],
) -> Option<()> {
    let n = (bx * by) as usize;

    // PVRTC1 is image-global: it can't go through the per-block loop. Build the
    // whole image at once (per-block bounding-box endpoints, then a whole-image
    // modulation pass). `None` for non-pow2 dimensions.
    if fmt == target::PVRTC1_4_RGB {
        return crate::pvrtc::transcode_uastc_pvrtc1_4_rgb(img, bx, by, out);
    }
    if fmt == target::PVRTC1_4_RGBA {
        // a source with no alpha channel is encoded as PVRTC1_4_RGB instead;
        // both variants produce the same block layout and output size
        if has_alpha {
            return crate::pvrtc::transcode_uastc_pvrtc1_4_rgba(img, bx, by, out);
        }
        return crate::pvrtc::transcode_uastc_pvrtc1_4_rgb(img, bx, by, out);
    }

    // Raster (per-pixel) targets: unpack each block to 16 pixels and write them
    // into a width*height image, packing per the target's pixel layout.
    if matches!(
        fmt,
        target::RGBA32 | target::RGB565 | target::BGR565 | target::RGBA4444
    ) {
        let bpp = if fmt == target::RGBA32 { 4 } else { 2 };
        if out.len() != (lw * lh) as usize * bpp {
            return None;
        }
        out.fill(0);
        for (b, blk) in img.chunks_exact(16).enumerate() {
            let src: [u8; 16] = blk.try_into().ok()?;
            let px = unpack_uastc(&src, false)?;
            let block_x = b as u32 % bx;
            let block_y = b as u32 / bx;
            let max_x = (lw as i32 - block_x as i32 * 4).clamp(0, 4) as u32;
            let max_y = (lh as i32 - block_y as i32 * 4).clamp(0, 4) as u32;
            for y in 0..max_y {
                for x in 0..max_x {
                    let c = px[(y * 4 + x) as usize];
                    let o = ((block_x * 4 + x) + (block_y * 4 + y) * lw) as usize * bpp;
                    match fmt {
                        target::RGBA32 => {
                            out[o] = c.r();
                            out[o + 1] = c.g();
                            out[o + 2] = c.b();
                            out[o + 3] = c.a();
                        }
                        target::RGB565 => {
                            let p = (mul_8(c.r() as u32, 31) << 11)
                                | (mul_8(c.g() as u32, 63) << 5)
                                | mul_8(c.b() as u32, 31);
                            out[o..o + 2].copy_from_slice(&(p as u16).to_le_bytes());
                        }
                        target::BGR565 => {
                            let p = (mul_8(c.b() as u32, 31) << 11)
                                | (mul_8(c.g() as u32, 63) << 5)
                                | mul_8(c.r() as u32, 31);
                            out[o..o + 2].copy_from_slice(&(p as u16).to_le_bytes());
                        }
                        _ => {
                            let p = (mul_8(c.r() as u32, 15) << 12)
                                | (mul_8(c.g() as u32, 15) << 8)
                                | (mul_8(c.b() as u32, 15) << 4)
                                | mul_8(c.a() as u32, 15);
                            out[o..o + 2].copy_from_slice(&(p as u16).to_le_bytes());
                        }
                    }
                }
            }
        }
        return Some(());
    }

    if fmt == target::BC1_RGB {
        // BC1 is an 8-byte-per-block opaque target.
        let high_quality = (flags & decode_flags::HIGH_QUALITY) != 0;
        if out.len() != n * 8 {
            return None;
        }
        out.fill(0);
        for b in 0..n {
            let src: [u8; 16] = img[b * 16..b * 16 + 16].try_into().ok()?;
            let block = transcode_uastc_to_bc1(&src, high_quality)?;
            out[b * 8..b * 8 + 8].copy_from_slice(&block);
        }
        return Some(());
    }
    if fmt == target::BC4_R {
        // BC4 is an 8-byte-per-block single-channel format.
        if out.len() != n * 8 {
            return None;
        }
        out.fill(0);
        for b in 0..n {
            let src: [u8; 16] = img[b * 16..b * 16 + 16].try_into().ok()?;
            let block = transcode_uastc_to_bc4(&src, false, 0)?;
            out[b * 8..b * 8 + 8].copy_from_slice(&block);
        }
        return Some(());
    }
    // ETC1 is an 8-byte-per-block target; everything else here is 16.
    if fmt == target::ETC1_RGB {
        if out.len() != n * 8 {
            return None;
        }
        out.fill(0);
        for b in 0..n {
            let src: [u8; 16] = img[b * 16..b * 16 + 16].try_into().ok()?;
            let block = transcode_uastc_to_etc1(&src)?;
            out[b * 8..b * 8 + 8].copy_from_slice(&block);
        }
        return Some(());
    }
    // EAC R11 is an 8-byte-per-block single-channel format (chan0 = R).
    if fmt == target::EAC_R11 {
        let high_quality = (flags & decode_flags::HIGH_QUALITY) != 0;
        if out.len() != n * 8 {
            return None;
        }
        out.fill(0);
        for b in 0..n {
            let src: [u8; 16] = img[b * 16..b * 16 + 16].try_into().ok()?;
            let block = transcode_uastc_to_etc2_eac_r11(&src, high_quality, 0)?;
            out[b * 8..b * 8 + 8].copy_from_slice(&block);
        }
        return Some(());
    }

    let high_quality = (flags & decode_flags::HIGH_QUALITY) != 0;
    if out.len() != n * 16 {
        return None;
    }
    out.fill(0);
    for b in 0..n {
        let src: [u8; 16] = img[b * 16..b * 16 + 16].try_into().ok()?;
        let block = match fmt {
            target::ASTC_4X4_RGBA => transcode_uastc_to_astc(&src)?,
            target::BC7_RGBA => transcode_uastc_to_bc7(&src)?,
            target::ETC2_RGBA => transcode_uastc_to_etc2_rgba(&src)?,
            // BC3 = BC4(alpha) + BC1(color); BC5 = BC4(R) + BC4(A).
            target::BC3_RGBA => transcode_uastc_to_bc3(&src, high_quality)?,
            target::BC5_RG => transcode_uastc_to_bc5(&src, high_quality, 0, 3)?,
            target::EAC_RG11 => transcode_uastc_to_etc2_eac_rg11(&src, high_quality, 0, 3)?,
            _ => return None,
        };
        out[b * 16..b * 16 + 16].copy_from_slice(&block);
    }
    Some(())
}

/// Transcode one UASTC HDR 4x4 image to `fmt`, writing into `out`. `img` is
/// the level's per-image raw block bytes (`bx*by*16`, each block a restricted
/// ASTC HDR block) and `(lw, lh)` the level's pixel dimensions, which the
/// uncompressed targets use as the output row pitch and row count. The HDR
/// path takes no decode flags. `None` for an unsupported target (only HDR
/// targets are valid here), a short input, or a block the ASTC decode rejects.
pub fn transcode_uastc_hdr(
    img: &[u8],
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    fmt: i32,
    out: &mut [u8],
) -> Option<()> {
    match fmt {
        target::ASTC_HDR_4X4_RGBA => crate::uastc_hdr::transcode_astc_hdr_4x4(img, bx, by, out),
        target::RGB_HALF => crate::uastc_hdr::transcode_rgb_half(img, bx, by, lw, lh, out),
        target::RGBA_HALF => crate::uastc_hdr::transcode_rgba_half(img, bx, by, lw, lh, out),
        target::RGB_9E5 => crate::uastc_hdr::transcode_rgb_9e5(img, bx, by, lw, lh, out),
        target::BC6H => crate::uastc_hdr::transcode_bc6h(img, bx, by, out),
        _ => None,
    }
}

/// Verbatim ASTC block pass-through: the source already is standard ASTC of
/// the destination's block size, so the image copies through unchanged
/// (`bx * by * 16` bytes) into `out`. The payload is not validated; the
/// target is what declares it ASTC.
fn astc_passthrough(img: &[u8], bx: u32, by: u32, out: &mut [u8]) -> Option<()> {
    let total = (bx * by) as usize * 16;
    let src = img.get(..total)?;
    if out.len() != total {
        return None;
    }
    out.copy_from_slice(src);
    Some(())
}

/// Transcode one raw ASTC LDR image to `fmt`, writing into `out`. `img` is
/// the level's per-image raw ASTC block bytes (`bx*by*16`, blocks of
/// `block`'s footprint), `(lw, lh)` the level's pixel dimensions, `is_srgb`
/// the vkFormat- or header-flag-derived decode profile, and `has_alpha`
/// whether the container reports alpha. Targets: the matching-block-size ASTC
/// pass-through, the four uncompressed targets (with deblock filtering per
/// the decode flags), and the compressed re-encode targets (`has_alpha` takes
/// effect there, via the alpha-to-opaque flag).
#[allow(clippy::too_many_arguments)]
pub fn transcode_astc_ldr(
    img: &[u8],
    block: crate::api::AstcBlock,
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    fmt: i32,
    flags: u32,
    is_srgb: bool,
    has_alpha: bool,
    out: &mut [u8],
) -> Option<()> {
    use crate::astc::slice::{transcode_ldr, Target4x4};
    if fmt == block.passthrough_target().as_i32() {
        return astc_passthrough(img, bx, by, out);
    }
    let t4 = match fmt {
        target::RGBA32 => Target4x4::Rgba32,
        target::RGB565 => Target4x4::Rgb565,
        target::BGR565 => Target4x4::Bgr565,
        target::RGBA4444 => Target4x4::Rgba4444,
        target::BC1_RGB => Target4x4::Bc1,
        target::BC3_RGBA => Target4x4::Bc3,
        target::BC4_R => Target4x4::Bc4,
        target::BC5_RG => Target4x4::Bc5,
        target::EAC_R11 => Target4x4::EacR11,
        target::EAC_RG11 => Target4x4::EacRg11,
        target::ETC1_RGB => Target4x4::Etc1,
        target::ETC2_RGBA => Target4x4::Etc2Rgba,
        // Unlike the UASTC path, the raw-ASTC RGBA target never downgrades
        // to RGB for opaque files; it always encodes RGBA endpoints.
        target::PVRTC1_4_RGB => Target4x4::Pvrtc1Rgb,
        target::PVRTC1_4_RGBA => Target4x4::Pvrtc1Rgba,
        target::BC7_RGBA => Target4x4::Bc7,
        _ => return None,
    };
    let (sbw, sbh) = block.dims();
    transcode_ldr(
        img, sbw, sbh, bx, by, lw, lh, t4, flags, is_srgb, has_alpha, out,
    )
}

/// Transcode one raw ASTC HDR 6x6 image to `fmt`, writing into `out`: the 6x6
/// ASTC pass-through, BC6H (through the 12x12-tile re-encoder, where
/// HIGH_QUALITY enables the encoder's 2-subset search), or the uncompressed
/// half-float/9E5 rasters.
#[allow(clippy::too_many_arguments)]
pub fn transcode_astc_hdr_6x6(
    img: &[u8],
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    fmt: i32,
    flags: u32,
    out: &mut [u8],
) -> Option<()> {
    if fmt == target::ASTC_HDR_6X6_RGBA {
        return astc_passthrough(img, bx, by, out);
    }
    use crate::astc::hdr6x6;
    match fmt {
        target::BC6H => {
            let high_quality = (flags & decode_flags::HIGH_QUALITY) != 0;
            hdr6x6::transcode_bc6h(img, bx, by, lw, lh, high_quality, out)
        }
        target::RGB_HALF => hdr6x6::transcode_half(img, bx, by, lw, lh, 3, out),
        target::RGBA_HALF => hdr6x6::transcode_half(img, bx, by, lw, lh, 4, out),
        target::RGB_9E5 => hdr6x6::transcode_9e5(img, bx, by, lw, lh, out),
        _ => None,
    }
}
