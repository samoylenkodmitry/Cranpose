//! The raw ASTC LDR slice driver. Decodes source blocks to 8-bit RGBA (LDR8 or
//! SRGB8 per the container's profile) and emits destination blocks/pixels
//! through one of three code paths: the 4x4 fast path (source blocks map 1:1
//! onto destination 4x4 blocks), the row-buffered reblocker (non-4x4 sources),
//! and the whole-image path (used whenever the deblocking filter runs, and also
//! for PVRTC1). The verbatim ASTC pass-through lives in `dispatch`.

use super::decode::{decode_block_ldr8, MAX_BLOCK_TEXELS};
use super::unpack::unpack_block;
use crate::fastenc::etc1f::PackEtc1State;
use alloc::vec::Vec;

/// One decoded texel, RGBA byte order.
pub type Rgba = [u8; 4];

/// The decode-flag bits this driver reads (values mirror the public
/// `DecodeFlags`).
const FLAG_TRANSCODE_ALPHA: u32 = 4;
const FLAG_HIGH_QUALITY: u32 = 32;
const FLAG_NO_DEBLOCK: u32 = 128;
const FLAG_STRONGER_DEBLOCK: u32 = 256;
const FLAG_FORCE_DEBLOCK: u32 = 512;

/// Deblock skip threshold: boundary texel pairs whose per-channel delta
/// exceeds this are left unfiltered (doubled under stronger filtering).
const DEBLOCK_SKIP_THRESH: i32 = 24;

/// Rounded requantize of an 8-bit `v` to a `q`-scaled value, used by the
/// 565/4444 packers.
#[inline]
fn mul_8(v: u32, q: u32) -> u32 {
    let v = v * q + 128;
    (v + (v >> 8)) >> 8
}

/// The destination formats the per-4x4-block emitter handles.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target4x4 {
    Rgba32,
    Rgb565,
    Bgr565,
    Rgba4444,
    Bc1,
    Bc3,
    Bc4,
    Bc5,
    EacR11,
    EacRg11,
    Etc1,
    Etc2Rgba,
    Pvrtc1Rgb,
    Pvrtc1Rgba,
    Bc7,
    /// BC7 with the source known opaque (from the XUASTC stream header's
    /// has_alpha bit): skips the per-block alpha scan the general BC7 arm runs.
    Bc7Opaque,
}

impl Target4x4 {
    /// Raster (per-pixel) output rather than per-block.
    fn is_uncompressed(self) -> bool {
        matches!(
            self,
            Target4x4::Rgba32 | Target4x4::Rgb565 | Target4x4::Bgr565 | Target4x4::Rgba4444
        )
    }

    /// Bytes per output pixel (raster targets) or per 4x4 block.
    fn bytes_per(self) -> usize {
        match self {
            Target4x4::Rgba32 => 4,
            Target4x4::Rgb565 | Target4x4::Bgr565 | Target4x4::Rgba4444 => 2,
            Target4x4::Bc1
            | Target4x4::Bc4
            | Target4x4::EacR11
            | Target4x4::Etc1
            | Target4x4::Pvrtc1Rgb
            | Target4x4::Pvrtc1Rgba => 8,
            Target4x4::Bc3
            | Target4x4::Bc5
            | Target4x4::EacRg11
            | Target4x4::Etc2Rgba
            | Target4x4::Bc7
            | Target4x4::Bc7Opaque => 16,
        }
    }
}

/// One channel of a 4x4 pixel block gathered into the contiguous 16 bytes the
/// BC4/EAC packers consume.
fn channel_bytes(pixels: &[Rgba; 16], c: usize) -> [u8; 16] {
    core::array::from_fn(|i| pixels[i][c])
}

/// The pixels as `Color32`s for the BC1 encoder.
fn as_color32(pixels: &[Rgba; 16]) -> [crate::color::Color32; 16] {
    core::array::from_fn(|i| {
        crate::color::Color32::new(
            pixels[i][0] as u32,
            pixels[i][1] as u32,
            pixels[i][2] as u32,
            pixels[i][3] as u32,
        )
    })
}

/// Emit one decoded 4x4 block of pixels. Raster targets write pixels clamped
/// at the right/bottom edges (`max_x/max_y = min(4, pitch_or_rows - block * 4)`,
/// `pitch` in pixels, `rows` = output rows in pixels); block targets write one
/// block at `(block_y * pitch + block_x) * stride` with `pitch` in blocks. The
/// BC4/BC5/EAC channel selects use the raw-ASTC defaults (channel0 0,
/// channel1 3).
#[allow(clippy::too_many_arguments)]
fn emit_4x4(
    fmt: Target4x4,
    block_x: u32,
    block_y: u32,
    pixels: &[Rgba; 16],
    out: &mut [u8],
    pitch: u32,
    rows: u32,
    high_quality: bool,
    from_alpha: bool,
    etc1_state: &mut PackEtc1State,
) {
    use crate::fastenc::etc1f::{pack_etc1, pack_etc1_grayscale};
    use crate::uastc::bc1::{encode_bc1, C_HIGH_QUALITY};
    use crate::uastc::bc4::encode_bc4;
    use crate::uastc::eac::{pack_eac, pack_eac_high_quality};

    if !fmt.is_uncompressed() {
        let stride = fmt.bytes_per();
        let o = ((block_y * pitch + block_x) as usize) * stride;
        let eac = if high_quality {
            pack_eac_high_quality
        } else {
            pack_eac
        };
        let bc1_flags = if high_quality { C_HIGH_QUALITY } else { 0 };
        match fmt {
            Target4x4::Bc1 => {
                out[o..o + 8].copy_from_slice(&encode_bc1(&as_color32(pixels), bc1_flags, None));
            }
            Target4x4::Bc3 => {
                let mut a = [0u8; 8];
                encode_bc4(&mut a, &channel_bytes(pixels, 3));
                out[o..o + 8].copy_from_slice(&a);
                out[o + 8..o + 16].copy_from_slice(&encode_bc1(
                    &as_color32(pixels),
                    bc1_flags,
                    None,
                ));
            }
            Target4x4::Bc4 => {
                // Alpha-to-opaque selects the alpha channel (the container
                // path passes channel0 = 3 in that case, else the default 0).
                let ch = if from_alpha { 3 } else { 0 };
                let mut b = [0u8; 8];
                encode_bc4(&mut b, &channel_bytes(pixels, ch));
                out[o..o + 8].copy_from_slice(&b);
            }
            Target4x4::Bc5 => {
                let mut b = [0u8; 8];
                encode_bc4(&mut b, &channel_bytes(pixels, 0));
                out[o..o + 8].copy_from_slice(&b);
                encode_bc4(&mut b, &channel_bytes(pixels, 3));
                out[o + 8..o + 16].copy_from_slice(&b);
            }
            Target4x4::EacR11 => {
                // Same alpha-to-opaque channel selection as BC4.
                let ch = if from_alpha { 3 } else { 0 };
                out[o..o + 8].copy_from_slice(&eac(&channel_bytes(pixels, ch)));
            }
            Target4x4::EacRg11 => {
                out[o..o + 8].copy_from_slice(&eac(&channel_bytes(pixels, 0)));
                out[o + 8..o + 16].copy_from_slice(&eac(&channel_bytes(pixels, 3)));
            }
            Target4x4::Etc1 => {
                let mut blk = [0u8; 8];
                if from_alpha {
                    // Alpha-to-opaque: pack the alpha channel as grayscale.
                    pack_etc1_grayscale(&mut blk, &channel_bytes(pixels, 3), etc1_state);
                } else {
                    pack_etc1(&mut blk, pixels, etc1_state);
                }
                out[o..o + 8].copy_from_slice(&blk);
            }
            Target4x4::Etc2Rgba => {
                // EAC alpha block first, then the ETC1 color block.
                out[o..o + 8].copy_from_slice(&eac(&channel_bytes(pixels, 3)));
                let mut blk = [0u8; 8];
                pack_etc1(&mut blk, pixels, etc1_state);
                out[o + 8..o + 16].copy_from_slice(&blk);
            }
            Target4x4::Bc7 | Target4x4::Bc7Opaque => {
                // The raw-ASTC caller's has_alpha is always "unknown", so the
                // auto-RGBA entry runs unconditionally; the XUASTC generic
                // path (whose stream header is authoritative) picks the
                // known-opaque entry instead. HIGH_QUALITY selects the
                // partially-analytical flag set.
                let bc7f_flags = if high_quality {
                    crate::fastenc::bc7f::flags::DEFAULT_PARTIALLY_ANALYTICAL
                } else {
                    crate::fastenc::bc7f::flags::DEFAULT
                };
                let mut blk = [0u8; 16];
                if fmt == Target4x4::Bc7Opaque {
                    crate::fastenc::bc7f::fast_pack_bc7_auto_rgb(&mut blk, pixels, bc7f_flags);
                } else {
                    crate::fastenc::bc7f::fast_pack_bc7_auto_rgba(&mut blk, pixels, bc7f_flags);
                }
                out[o..o + 16].copy_from_slice(&blk);
            }
            _ => unreachable!(),
        }
        return;
    }

    let bpp = fmt.bytes_per();
    let max_x = 4.min(pitch as i32 - block_x as i32 * 4).max(0) as u32;
    let max_y = 4.min(rows as i32 - block_y as i32 * 4).max(0) as u32;
    for y in 0..max_y {
        for x in 0..max_x {
            let c = pixels[(y * 4 + x) as usize];
            let o = ((block_x * 4 + x) + (block_y * 4 + y) * pitch) as usize * bpp;
            match fmt {
                Target4x4::Rgba32 => out[o..o + 4].copy_from_slice(&c),
                Target4x4::Rgb565 => {
                    let p = (mul_8(c[0] as u32, 31) << 11)
                        | (mul_8(c[1] as u32, 63) << 5)
                        | mul_8(c[2] as u32, 31);
                    out[o..o + 2].copy_from_slice(&(p as u16).to_le_bytes());
                }
                Target4x4::Bgr565 => {
                    let p = (mul_8(c[2] as u32, 31) << 11)
                        | (mul_8(c[1] as u32, 63) << 5)
                        | mul_8(c[0] as u32, 31);
                    out[o..o + 2].copy_from_slice(&(p as u16).to_le_bytes());
                }
                Target4x4::Rgba4444 => {
                    let p = (mul_8(c[0] as u32, 15) << 12)
                        | (mul_8(c[1] as u32, 15) << 8)
                        | (mul_8(c[2] as u32, 15) << 4)
                        | mul_8(c[3] as u32, 15);
                    out[o..o + 2].copy_from_slice(&(p as u16).to_le_bytes());
                }
                _ => unreachable!(),
            }
        }
    }
}

/// Unpack + LDR8/SRGB8-decode one source block into `px[..sbw*sbh]`.
fn decode_src_block(
    blk: &[u8],
    sbw: u32,
    sbh: u32,
    srgb: bool,
    px: &mut [Rgba; MAX_BLOCK_TEXELS],
) -> Option<()> {
    let blk: &[u8; 16] = blk.try_into().ok()?;
    let log = unpack_block(blk, sbw, sbh)?;
    decode_block_ldr8(&log, sbw, sbh, srgb, px)
}

/// Extract a 4x4 block with edge clamping: reads past the image width or past
/// `override_h` rows clamp to the last valid column/row.
fn extract_4x4_clamped(
    img: &[Rgba],
    img_w: u32,
    img_h: u32,
    sx: u32,
    sy: u32,
    override_h: u32,
) -> [Rgba; 16] {
    let mut out = [[0u8; 4]; 16];
    let eff_h = img_h.min(override_h);
    if sx + 4 > img_w || sy + 4 > eff_h {
        for y in 0..4u32 {
            for x in 0..4u32 {
                let cx = (sx + x).min(img_w - 1);
                let cy = (sy + y).min(override_h - 1);
                out[(y * 4 + x) as usize] = img[(cx + cy * img_w) as usize];
            }
        }
    } else {
        for y in 0..4u32 {
            let row = ((sx + (sy + y) * img_w) as usize)..((sx + (sy + y) * img_w) as usize + 4);
            out[(y * 4) as usize..(y * 4 + 4) as usize].copy_from_slice(&img[row]);
        }
    }
    out
}

/// Soften source-block boundaries in a decoded (padded) whole image. A
/// horizontal pass reads the pristine input and writes a temp image; the
/// vertical pass reads the temp (so it sees the horizontal results) and writes
/// the output. Boundary pairs whose per-channel delta exceeds the skip
/// threshold (24, doubled under stronger filtering) are left alone; all four
/// channels filter, alpha included.
fn deblock_filter(img: &[Rgba], w: u32, h: u32, fbw: u32, fbh: u32, stronger: bool) -> Vec<Rgba> {
    let skip_thresh = if stronger {
        DEBLOCK_SKIP_THRESH * 2
    } else {
        DEBLOCK_SKIP_THRESH
    };
    let at = |buf: &[Rgba], x: i32, y: i32| -> Rgba {
        let cx = x.clamp(0, w as i32 - 1);
        let cy = y.clamp(0, h as i32 - 1);
        buf[(cx + cy * w as i32) as usize]
    };
    let mut temp: Vec<Rgba> = img.to_vec();
    // Horizontal pass over every source-block boundary column.
    for y in 0..h as i32 {
        let mut x = fbw as i32;
        while x < w as i32 {
            let ll = at(img, x - 2, y);
            let l = at(img, x - 1, y);
            let r = at(img, x, y);
            let rr = at(img, x + 1, y);
            let skip = (0..4).any(|c| (l[c] as i32 - r[c] as i32).abs() > skip_thresh);
            if !skip {
                let mut ml = [0u8; 4];
                let mut mr = [0u8; 4];
                for c in 0..4 {
                    let (lc, rc, llc, rrc) = (l[c] as u32, r[c] as u32, ll[c] as u32, rr[c] as u32);
                    if stronger {
                        ml[c] = ((3 * lc + 2 * rc + llc + 3) / 6) as u8;
                        mr[c] = ((3 * rc + 2 * lc + rrc + 3) / 6) as u8;
                    } else {
                        ml[c] = ((5 * lc + 2 * rc + llc + 4) / 8) as u8;
                        mr[c] = ((5 * rc + 2 * lc + rrc + 4) / 8) as u8;
                    }
                }
                temp[((x - 1) + y * w as i32) as usize] = ml;
                temp[(x + y * w as i32) as usize] = mr;
            }
            x += fbw as i32;
        }
    }

    let mut dst = temp.clone();
    // Vertical pass over every source-block boundary row, reading the
    // horizontally filtered temp image.
    for x in 0..w as i32 {
        let mut y = fbh as i32;
        while y < h as i32 {
            let uu = at(&temp, x, y - 2);
            let u = at(&temp, x, y - 1);
            let d = at(&temp, x, y);
            let dd = at(&temp, x, y + 1);
            let skip = (0..4).any(|c| (u[c] as i32 - d[c] as i32).abs() > skip_thresh);
            if !skip {
                let mut mu = [0u8; 4];
                let mut md = [0u8; 4];
                for c in 0..4 {
                    let (uc, dc, uuc, ddc) = (u[c] as u32, d[c] as u32, uu[c] as u32, dd[c] as u32);
                    if stronger {
                        mu[c] = ((3 * uc + 2 * dc + uuc + 3) / 6) as u8;
                        md[c] = ((3 * dc + 2 * uc + ddc + 3) / 6) as u8;
                    } else {
                        mu[c] = ((5 * uc + 2 * dc + uuc + 4) / 8) as u8;
                        md[c] = ((5 * dc + 2 * uc + ddc + 4) / 8) as u8;
                    }
                }
                dst[(x + (y - 1) * w as i32) as usize] = mu;
                dst[(x + y * w as i32) as usize] = md;
            }
            y += fbh as i32;
        }
    }
    dst
}

/// Transcode one raw ASTC LDR image to a 4x4-block or raster target: the
/// selection and loops for the three code paths. `img` is `bx*by` source
/// blocks of `sbw` x `sbh` texels; raster targets emit an `orig_w` x `orig_h`
/// pixel image, block targets a `ceil(orig_w/4)` x `ceil(orig_h/4)` block
/// grid, both with the default pitch semantics, written into `out` (which
/// must be exactly the output length). Returns `None` if any source block
/// fails to unpack or decode, which aborts the slice.
#[allow(clippy::too_many_arguments)]
pub fn transcode_ldr(
    img: &[u8],
    sbw: u32,
    sbh: u32,
    bx: u32,
    by: u32,
    orig_w: u32,
    orig_h: u32,
    fmt: Target4x4,
    flags: u32,
    srgb: bool,
    has_alpha: bool,
    out: &mut [u8],
) -> Option<()> {
    let total = (bx as usize).checked_mul(by as usize)?.checked_mul(16)?;
    if img.len() < total {
        return None;
    }
    let is_pvrtc1 = matches!(fmt, Target4x4::Pvrtc1Rgb | Target4x4::Pvrtc1Rgba);
    // PVRTC1 requires power-of-two texture dimensions (checked on the
    // original dimensions, before any block padding).
    if is_pvrtc1 && (!orig_w.is_power_of_two() || !orig_h.is_power_of_two()) {
        return None;
    }
    // Deblock decision, computed before path selection.
    let disable = flags & FLAG_NO_DEBLOCK != 0;
    let stronger = (flags & FLAG_STRONGER_DEBLOCK != 0) || sbw > 8 || sbh > 8;
    let force = flags & FLAG_FORCE_DEBLOCK != 0;
    let deblock = !disable && (force || sbw > 8 || sbh > 6);
    let high_quality = flags & FLAG_HIGH_QUALITY != 0;
    // Alpha-to-opaque: only honored when the container reports alpha.
    let from_alpha = has_alpha && flags & FLAG_TRANSCODE_ALPHA != 0;
    // One packer state per slice: ETC1 output depends on block order.
    let mut etc1_state = PackEtc1State::new();

    let dst_nx = orig_w.div_ceil(4);
    let dst_ny = orig_h.div_ceil(4);

    // Default pitch semantics: raster targets use pitch = orig_width pixels
    // and rows = orig_height; block targets use the destination block count
    // per row.
    let (pitch, rows, out_len) = if fmt.is_uncompressed() {
        (
            orig_w,
            orig_h,
            (orig_w as usize) * (orig_h as usize) * fmt.bytes_per(),
        )
    } else {
        (dst_nx, 0, (dst_nx * dst_ny) as usize * fmt.bytes_per())
    };
    if out.len() != out_len {
        return None;
    }
    out.fill(0);
    let mut px = [[0u8; 4]; MAX_BLOCK_TEXELS];

    if sbw == 4 && sbh == 4 && !deblock && !is_pvrtc1 {
        // Fast path: source blocks map 1:1 onto destination 4x4 blocks.
        for block_y in 0..by {
            for block_x in 0..bx {
                let b = ((block_x + block_y * bx) as usize) * 16;
                decode_src_block(&img[b..b + 16], 4, 4, srgb, &mut px)?;
                let block: [Rgba; 16] = px[..16].try_into().ok()?;
                emit_4x4(
                    fmt,
                    block_x,
                    block_y,
                    &block,
                    &mut *out,
                    pitch,
                    rows,
                    high_quality,
                    from_alpha,
                    &mut etc1_state,
                );
            }
        }
        return Some(());
    }

    if !deblock && !is_pvrtc1 {
        // Reblocker path: buffer the smallest number of source block rows
        // whose scanline total is a multiple of 4, then emit destination 4x4
        // rows from the buffer with edge clamping.
        let mut n_buf_rows = 1u32;
        while (n_buf_rows * sbh) & 3 != 0 {
            n_buf_rows += 1;
        }
        let buf_w = bx * sbw;
        let buf_h = n_buf_rows * sbh;
        let mut buffered = vec![[0u8; 4]; (buf_w * buf_h) as usize];

        for src_by in 0..by {
            let buf_row = src_by % n_buf_rows;
            for src_bx in 0..bx {
                let b = ((src_bx + src_by * bx) as usize) * 16;
                decode_src_block(&img[b..b + 16], sbw, sbh, srgb, &mut px)?;
                for y in 0..sbh {
                    let dst0 = (src_bx * sbw + (buf_row * sbh + y) * buf_w) as usize;
                    let src0 = (y * sbw) as usize;
                    buffered[dst0..dst0 + sbw as usize]
                        .copy_from_slice(&px[src0..src0 + sbw as usize]);
                }
            }

            let final_row = src_by == by - 1;
            if buf_row != n_buf_rows - 1 && !final_row {
                continue;
            }
            // Flush: emit the destination block rows covered by the buffer.
            let buffered_src_pixel_y = (src_by / n_buf_rows) * n_buf_rows * sbh;
            let num_buffered_rows = buf_row + 1;
            let override_h = (orig_h - buffered_src_pixel_y).min(num_buffered_rows * sbh);
            let total_dst_rows = (num_buffered_rows * sbh + 3) >> 2;
            for dst_ofs_by in 0..total_dst_rows {
                let dst_by = (buffered_src_pixel_y >> 2) + dst_ofs_by;
                if dst_by >= dst_ny {
                    break;
                }
                for dst_bx in 0..dst_nx {
                    let block = extract_4x4_clamped(
                        &buffered,
                        buf_w,
                        buf_h,
                        dst_bx * 4,
                        dst_ofs_by * 4,
                        override_h,
                    );
                    emit_4x4(
                        fmt,
                        dst_bx,
                        dst_by,
                        &block,
                        &mut *out,
                        pitch,
                        rows,
                        high_quality,
                        from_alpha,
                        &mut etc1_state,
                    );
                }
            }
        }
        return Some(());
    }

    // Whole-image path: decode the entire padded image, deblock it, then emit
    // destination 4x4 blocks with plain edge clamping.
    let img_w = bx * sbw;
    let img_h = by * sbh;
    let mut temp = vec![[0u8; 4]; (img_w * img_h) as usize];
    for src_by in 0..by {
        for src_bx in 0..bx {
            let b = ((src_bx + src_by * bx) as usize) * 16;
            decode_src_block(&img[b..b + 16], sbw, sbh, srgb, &mut px)?;
            for y in 0..sbh {
                let dst0 = (src_bx * sbw + (src_by * sbh + y) * img_w) as usize;
                let src0 = (y * sbw) as usize;
                temp[dst0..dst0 + sbw as usize].copy_from_slice(&px[src0..src0 + sbw as usize]);
            }
        }
    }
    let filtered = if deblock {
        deblock_filter(&temp, img_w, img_h, sbw, sbh, stronger)
    } else {
        temp
    };
    if is_pvrtc1 {
        // PVRTC1 is whole-image: per-block bounding-box endpoints then the
        // shared modulation/swizzle pass over the (possibly deblocked) image.
        return crate::pvrtc::encode_pvrtc1_raw(
            dst_nx,
            dst_ny,
            fmt == Target4x4::Pvrtc1Rgba,
            from_alpha,
            |bx, by| {
                let px = extract_4x4_clamped(&filtered, img_w, img_h, bx * 4, by * 4, img_h);
                core::array::from_fn(|i| {
                    crate::color::Color32::new(
                        px[i][0] as u32,
                        px[i][1] as u32,
                        px[i][2] as u32,
                        px[i][3] as u32,
                    )
                })
            },
            out,
        );
    }
    for dst_by in 0..dst_ny {
        for dst_bx in 0..dst_nx {
            let block = extract_4x4_clamped(&filtered, img_w, img_h, dst_bx * 4, dst_by * 4, img_h);
            emit_4x4(
                fmt,
                dst_bx,
                dst_by,
                &block,
                &mut *out,
                pitch,
                rows,
                high_quality,
                from_alpha,
                &mut etc1_state,
            );
        }
    }
    Some(())
}
