//! XUASTC to BC7 fast tile arms and their packers: pure transcodes of solid
//! and single-subset ASTC blocks into BC7 modes 5, 6, and 1 without a
//! decode+re-encode round trip, falling back to the pixel encoder where the
//! source configuration is too complex. The 4x4 arm is per-block; the 8x6 and
//! 6x6 arms tile 2 (or 2x2) source blocks into 2x3 (3x3) BC7 blocks.

use super::endpoints::{cem_has_alpha, decode_endpoints, num_cem_values};
use crate::astc::decode::decode_block_ldr8;
use crate::astc::dequant::dequant_tables;
use crate::astc::unpack::{unpack_block, LogAstcBlock};
use crate::fastenc::bc7f::{
    self, determine_unique_pbits, encode_mode6_rgba_block, pack_mode5_solid, Rgba,
};
use alloc::vec::Vec;

/// Single-plane incremental fixed-point bilinear upsample of dequantized
/// [0,64] weights from a `wx` by `wy` grid onto a `bx` by `by` grid.
pub(crate) fn upsample_weight_grid_xuastc(
    bx: usize,
    by: usize,
    wx: usize,
    wy: usize,
    src: &[u8],
    dst: &mut [u8],
) {
    let scale_x = (1024 + bx as u32 / 2) / (bx as u32 - 1);
    let scale_y = (1024 + by as u32 / 2) / (by as u32 - 1);
    let gyu_inc = scale_y * (wy as u32 - 1);
    let gxu_inc = scale_x * (wx as u32 - 1);

    let mut gyu = 32u32;
    for ty in 0..by {
        let gy = gyu >> 6;
        gyu += gyu_inc;
        let jy = (gy >> 4) as usize;
        let fy = gy & 0xF;

        let mut gxu = 32u32;
        for tx in 0..bx {
            let gx = gxu >> 6;
            gxu += gxu_inc;
            let jx = (gx >> 4) as usize;
            let fx = gx & 0xF;

            let w11 = (fx * fy + 8) >> 4;
            let w10 = fy - w11;
            let w01 = fx - w11;
            let w00 = 16 - fx - fy + w11;

            let mut total = 8u32;
            if w00 != 0 {
                total += src[jx + jy * wx] as u32 * w00;
            }
            if w01 != 0 {
                total += src[jx + 1 + jy * wx] as u32 * w01;
            }
            if w10 != 0 {
                total += src[jx + (jy + 1) * wx] as u32 * w10;
            }
            if w11 != 0 {
                total += src[jx + 1 + (jy + 1) * wx] as u32 * w11;
            }
            dst[tx + ty * bx] = (total >> 4) as u8;
        }
    }
}

/// Dequantize a logical block's (single-plane) weights to [0,64].
fn dequantize_weights(log: &LogAstcBlock, out: &mut [u8]) {
    let tab = &dequant_tables().weights[log.weight_ise_range as usize];
    let n = (log.grid_width * log.grid_height) as usize;
    for i in 0..n {
        out[i] = tab[log.weights[i] as usize];
    }
}

/// Pack a single-subset non-dual-plane 4x4-source block into BC7 mode 6.
fn pack_from_astc_4x4_single_subset(dst: &mut [u8; 16], log: &LogAstcBlock) {
    let (l, h) = decode_endpoints(
        log.color_endpoint_modes[0] as u32,
        &log.endpoints,
        log.endpoint_ise_range,
    );

    let mut dequantized = [0u8; 16];
    dequantize_weights(log, &mut dequantized);

    let mut upsampled = [0u8; 16];
    let weights: &[u8] = if log.grid_width < 4 || log.grid_height < 4 {
        upsample_weight_grid_xuastc(
            4,
            4,
            log.grid_width as usize,
            log.grid_height as usize,
            &dequantized,
            &mut upsampled,
        );
        &upsampled
    } else {
        &dequantized
    };

    let q = 1.0f32 / 255.0;
    let sxl = [
        l[0] as f32 * q,
        l[1] as f32 * q,
        l[2] as f32 * q,
        l[3] as f32 * q,
    ];
    let sxh = [
        h[0] as f32 * q,
        h[1] as f32 * q,
        h[2] as f32 * q,
        h[3] as f32 * q,
    ];

    let mut best_min = [0u8; 4];
    let mut best_max = [0u8; 4];
    let mut best_pbits = [0u32; 2];
    determine_unique_pbits(
        4,
        7,
        &sxl,
        &sxh,
        &mut best_min,
        &mut best_max,
        &mut best_pbits,
    );

    let mut bc7_weights = [0u8; 16];
    for (dst_w, &w) in bc7_weights.iter_mut().zip(weights.iter()) {
        *dst_w = ((w as u32 * 15 + 32) >> 6) as u8;
    }

    encode_mode6_rgba_block(
        dst,
        best_min[0] as u32,
        best_min[1] as u32,
        best_min[2] as u32,
        best_min[3] as u32,
        best_pbits[0],
        best_max[0] as u32,
        best_max[1] as u32,
        best_max[2] as u32,
        best_max[3] as u32,
        best_pbits[1],
        &bc7_weights,
    );
}

/// The 4x4-source arm, per block: solid maps to BC7 mode 5, a single-subset
/// non-dual-plane block to a mode 6 pure transcode, and anything else falls
/// back to a full decode plus the pixel encoder (honoring the stream's
/// has_alpha).
fn transcode_bc7_fast_4x4(
    blocks: &[u8],
    bx: u32,
    by: u32,
    srgb: bool,
    has_alpha: bool,
    out: &mut [u8],
) -> Option<()> {
    if out.len() != (bx * by) as usize * 16 {
        return None;
    }
    out.fill(0);
    let bc7f_flags = bc7f::flags::DEFAULT;

    for i in 0..(bx * by) as usize {
        let src: &[u8; 16] = blocks[i * 16..i * 16 + 16].try_into().ok()?;
        let log = unpack_block(src, 4, 4)?;
        let dst: &mut [u8; 16] = (&mut out[i * 16..i * 16 + 16]).try_into().ok()?;

        if log.solid_color_flag_ldr {
            let sc: Rgba = core::array::from_fn(|c| (log.solid_color[c] >> 8) as u8);
            pack_mode5_solid(dst, sc);
            continue;
        }
        if !log.dual_plane && log.num_partitions == 1 {
            pack_from_astc_4x4_single_subset(dst, &log);
            continue;
        }

        // Fallback: full decode + the pixel encoder.
        let mut pixels = [[0u8; 4]; 16];
        decode_block_ldr8(&log, 4, 4, srgb, &mut pixels)?;
        if has_alpha {
            bc7f::fast_pack_bc7_auto_rgba(dst, &pixels, bc7f_flags);
        } else {
            bc7f::fast_pack_bc7_auto_rgb(dst, &pixels, bc7f_flags);
        }
    }
    Some(())
}

/// True when the block can emit non-opaque alpha: a solid block whose alpha is
/// below 255, or a non-solid block whose CEM carries alpha.
pub(crate) fn block_has_alpha(log: &LogAstcBlock) -> bool {
    if log.solid_color_flag_ldr {
        (log.solid_color[3] >> 8) != 255
    } else {
        cem_has_alpha(log.color_endpoint_modes[0] as u32)
    }
}

/// Whether both blocks are solid with matching color: at `tol` 0 the full
/// 16-bit solid colors must be equal, at `tol` > 0 the 8-bit components must
/// be within `tol`.
pub(crate) fn blocks_same_solid_colors(a: &LogAstcBlock, b: &LogAstcBlock, tol: i32) -> bool {
    if !a.solid_color_flag_ldr || !b.solid_color_flag_ldr {
        return false;
    }
    if tol == 0 {
        a.solid_color == b.solid_color
    } else {
        (0..4)
            .all(|c| ((a.solid_color[c] >> 8) as i32 - (b.solid_color[c] >> 8) as i32).abs() <= tol)
    }
}

/// Whether both blocks are non-solid single-subset single-plane with the same
/// CEM and ISE range and matching endpoints: at `tol` 0 the raw endpoint
/// symbols must be equal, at `tol` > 0 the decoded channels must be within
/// `tol`.
pub(crate) fn blocks_same_single_subset_endpoints(
    a: &LogAstcBlock,
    b: &LogAstcBlock,
    tol: i32,
) -> bool {
    if a.solid_color_flag_ldr
        || b.solid_color_flag_ldr
        || a.dual_plane
        || b.dual_plane
        || a.num_partitions != 1
        || b.num_partitions != 1
        || a.color_endpoint_modes[0] != b.color_endpoint_modes[0]
        || a.endpoint_ise_range != b.endpoint_ise_range
    {
        return false;
    }
    let n = num_cem_values(a.color_endpoint_modes[0] as u32);
    if tol == 0 {
        return a.endpoints[..n] == b.endpoints[..n];
    }
    let (al, ah) = decode_endpoints(
        a.color_endpoint_modes[0] as u32,
        &a.endpoints,
        a.endpoint_ise_range,
    );
    let (bl, bh) = decode_endpoints(
        b.color_endpoint_modes[0] as u32,
        &b.endpoints,
        b.endpoint_ise_range,
    );
    (0..4).all(|c| {
        (al[c] as i32 - bl[c] as i32).abs() <= tol && (ah[c] as i32 - bh[c] as i32).abs() <= tol
    })
}

/// Entry point: route to the fast arm for the source block size (4x4, 8x6, or
/// 6x6), writing the BC7 blocks into `out`. Any other block size returns
/// `None`.
#[allow(clippy::too_many_arguments)]
pub fn transcode_bc7_fast(
    blocks: &[u8],
    sbw: u32,
    sbh: u32,
    bx: u32,
    by: u32,
    _lw: u32,
    _lh: u32,
    srgb: bool,
    has_alpha: bool,
    out: &mut [u8],
) -> Option<()> {
    match (sbw, sbh) {
        (4, 4) => transcode_bc7_fast_4x4(blocks, bx, by, srgb, has_alpha, out),
        (8, 6) => transcode_bc7_fast_8x6(blocks, bx, by, _lw, _lh, srgb, has_alpha, out),
        (6, 6) => transcode_bc7_fast_6x6(blocks, bx, by, _lw, _lh, srgb, has_alpha, out),
        _ => None,
    }
}

/// Pack one single-subset block into BC7 mode 6, with its weights already
/// upsampled to the full block grid.
fn pack_from_astc_single_subset(
    dst: &mut [u8; 16],
    log: &LogAstcBlock,
    upsampled_weights: &[u8],
    weight_ofs_x: usize,
    weight_ofs_y: usize,
    block_width: usize,
) {
    let (l, h) = decode_endpoints(
        log.color_endpoint_modes[0] as u32,
        &log.endpoints,
        log.endpoint_ise_range,
    );
    let q = 1.0f32 / 255.0;
    let sxl = core::array::from_fn(|c| l[c] as f32 * q);
    let sxh = core::array::from_fn(|c| h[c] as f32 * q);
    let mut best_min = [0u8; 4];
    let mut best_max = [0u8; 4];
    let mut best_pbits = [0u32; 2];
    determine_unique_pbits(
        4,
        7,
        &sxl,
        &sxh,
        &mut best_min,
        &mut best_max,
        &mut best_pbits,
    );

    let mut bc7_weights = [0u8; 16];
    for y in 0..4 {
        for x in 0..4 {
            let w = upsampled_weights[(weight_ofs_x + x) + (weight_ofs_y + y) * block_width] as u32;
            bc7_weights[x + y * 4] = ((w * 15 + 32) >> 6) as u8;
        }
    }
    encode_mode6_rgba_block(
        dst,
        best_min[0] as u32,
        best_min[1] as u32,
        best_min[2] as u32,
        best_min[3] as u32,
        best_pbits[0],
        best_max[0] as u32,
        best_max[1] as u32,
        best_max[2] as u32,
        best_max[3] as u32,
        best_pbits[1],
        &bc7_weights,
    );
}

/// Decoded endpoints, or the replicated solid color (`l == h`).
fn endpoints_or_solid(b: &LogAstcBlock) -> ([u8; 4], [u8; 4]) {
    if b.solid_color_flag_ldr {
        let c: [u8; 4] = core::array::from_fn(|i| (b.solid_color[i] >> 8) as u8);
        (c, c)
    } else {
        decode_endpoints(
            b.color_endpoint_modes[0] as u32,
            &b.endpoints,
            b.endpoint_ise_range,
        )
    }
}

/// The two-subset weight fetch shared by the same-endpoints and
/// two-subsets packers: which of the pair's upsampled grids a BC7 texel
/// reads, per the tile geometry.
#[inline]
#[allow(clippy::too_many_arguments)]
fn fetch_pair_weight(
    is_6x6: bool,
    top_or_bottom: bool,
    dx: i32,
    dy: i32,
    x: usize,
    y: usize,
    w0: &[u8],
    w1: &[u8],
    b0_solid: bool,
    b1_solid: bool,
) -> u32 {
    if is_6x6 {
        if top_or_bottom {
            let row = y + if dy == 2 { 2 } else { 0 };
            if x < 2 {
                if b0_solid {
                    0
                } else {
                    w0[(x + 4) + row * 6] as u32
                }
            } else if b1_solid {
                0
            } else {
                w1[(x - 2) + row * 6] as u32
            }
        } else {
            let col = x + if dx == 2 { 2 } else { 0 };
            if y < 2 {
                if b0_solid {
                    0
                } else {
                    w0[col + (y + 4) * 6] as u32
                }
            } else if b1_solid {
                0
            } else {
                w1[col + (y - 2) * 6] as u32
            }
        }
    } else {
        // 8x6.
        let col = dx as usize * 4 + x;
        if y < 2 {
            if b0_solid {
                0
            } else {
                w0[col + (y + 4) * 8] as u32
            }
        } else if b1_solid {
            0
        } else {
            w1[col + (y - 2) * 8] as u32
        }
    }
}

/// Pack two blocks that share endpoints into BC7 mode 6, averaging their
/// endpoint pairs.
#[allow(clippy::too_many_arguments)]
fn pack_same_endpoints(
    dst: &mut [u8; 16],
    b0: &LogAstcBlock,
    w0: &[u8],
    b1: &LogAstcBlock,
    w1: &[u8],
    dx: i32,
    dy: i32,
    is_6x6: bool,
) {
    let (mut l, mut h) = decode_endpoints(
        b0.color_endpoint_modes[0] as u32,
        &b0.endpoints,
        b0.endpoint_ise_range,
    );
    let (al, ah) = decode_endpoints(
        b1.color_endpoint_modes[0] as u32,
        &b1.endpoints,
        b1.endpoint_ise_range,
    );
    for c in 0..4 {
        l[c] = ((l[c] as u32 + al[c] as u32 + 1) >> 1) as u8;
        h[c] = ((h[c] as u32 + ah[c] as u32 + 1) >> 1) as u8;
    }

    let q = 1.0f32 / 255.0;
    let sxl = core::array::from_fn(|c| l[c] as f32 * q);
    let sxh = core::array::from_fn(|c| h[c] as f32 * q);
    let mut best_min = [0u8; 4];
    let mut best_max = [0u8; 4];
    let mut best_pbits = [0u32; 2];
    determine_unique_pbits(
        4,
        7,
        &sxl,
        &sxh,
        &mut best_min,
        &mut best_max,
        &mut best_pbits,
    );

    let top_or_bottom = is_6x6 && (dy == 0 || dy == 2);
    let mut bc7_weights = [0u8; 16];
    for y in 0..4 {
        for x in 0..4 {
            let w = fetch_pair_weight(is_6x6, top_or_bottom, dx, dy, x, y, w0, w1, false, false);
            bc7_weights[x + y * 4] = ((w * 15 + 32) >> 6) as u8;
        }
    }
    encode_mode6_rgba_block(
        dst,
        best_min[0] as u32,
        best_min[1] as u32,
        best_min[2] as u32,
        best_min[3] as u32,
        best_pbits[0],
        best_max[0] as u32,
        best_max[1] as u32,
        best_max[2] as u32,
        best_max[3] as u32,
        best_pbits[1],
        &bc7_weights,
    );
}

/// Shared-pbit fit of one subset's endpoints for the mode-1 packers.
fn shared_pbits_of(l: [u8; 4], h: [u8; 4]) -> ([u8; 4], [u8; 4], u32) {
    let q = 1.0f32 / 255.0;
    let sxl = core::array::from_fn(|c| l[c] as f32 * q);
    let sxh = core::array::from_fn(|c| h[c] as f32 * q);
    let mut best_min = [0u8; 4];
    let mut best_max = [0u8; 4];
    let mut best_pbits = [0u32; 2];
    crate::fastenc::bc7f::determine_shared_pbits(
        3,
        6,
        &sxl,
        &sxh,
        &mut best_min,
        &mut best_max,
        &mut best_pbits,
    );
    (best_min, best_max, best_pbits[0])
}

/// Assemble + emit a mode-1 block from two subsets' fits and weights.
fn emit_mode1(
    dst: &mut [u8; 16],
    part_id: u32,
    mins: [[u8; 4]; 2],
    maxs: [[u8; 4]; 2],
    p0: u32,
    p1: u32,
    bc7_weights: &[u8; 16],
) {
    let mut lr = [mins[0][0] as u32, mins[1][0] as u32];
    let mut lg = [mins[0][1] as u32, mins[1][1] as u32];
    let mut lb = [mins[0][2] as u32, mins[1][2] as u32];
    let mut hr = [maxs[0][0] as u32, maxs[1][0] as u32];
    let mut hg = [maxs[0][1] as u32, maxs[1][1] as u32];
    let mut hb = [maxs[0][2] as u32, maxs[1][2] as u32];
    crate::fastenc::bc7f::encode_mode1_rgb_block(
        dst,
        part_id,
        &mut lr,
        &mut lg,
        &mut lb,
        &mut hr,
        &mut hg,
        &mut hb,
        p0,
        p1,
        bc7_weights,
    );
}

/// Pack two 6x6 source blocks into a BC7 mode 1 block, one subset per source
/// block.
#[allow(clippy::too_many_arguments)]
fn pack_6x6_two_subsets(
    dst: &mut [u8; 16],
    b0: &LogAstcBlock,
    w0: &[u8],
    b1: &LogAstcBlock,
    w1: &[u8],
    dx: i32,
    dy: i32,
) -> bool {
    let (l0, h0) = endpoints_or_solid(b0);
    let (l1, h1) = endpoints_or_solid(b1);
    let (min0, max0, p0) = shared_pbits_of(l0, h0);
    let (min1, max1, p1) = shared_pbits_of(l1, h1);

    let top_or_bottom = dy == 0 || dy == 2;
    let part_id = if dx == 0 || dx == 2 { 13 } else { 0 };

    let mut bc7_weights = [0u8; 16];
    for y in 0..4 {
        for x in 0..4 {
            let w = fetch_pair_weight(
                true,
                top_or_bottom,
                dx,
                dy,
                x,
                y,
                w0,
                w1,
                b0.solid_color_flag_ldr,
                b1.solid_color_flag_ldr,
            );
            bc7_weights[x + y * 4] = ((w * 7 + 32) >> 6) as u8;
        }
    }
    emit_mode1(
        dst,
        part_id,
        [min0, min1],
        [max0, max1],
        p0,
        p1,
        &bc7_weights,
    );
    true
}

/// Pack two 8x6 source blocks into BC7 mode 1, narrowing each subset's
/// endpoints to its used weight range. When both subsets are near-flat, decode
/// the texels and re-encode analytically instead.
#[allow(clippy::too_many_arguments)]
fn pack_8x6_two_subsets_hq(
    dst: &mut [u8; 16],
    b0: &LogAstcBlock,
    w0: &[u8],
    b1: &LogAstcBlock,
    w1: &[u8],
    dx: i32,
    srgb: bool,
) -> bool {
    let b_solid = [b0.solid_color_flag_ldr, b1.solid_color_flag_ldr];
    let (l0, h0) = endpoints_or_solid(b0);
    let (l1, h1) = endpoints_or_solid(b1);
    let mut l = [l0, l1];
    let mut h = [h0, h1];

    // Weight span per subset over this 4x4 tile.
    let mut low_w = [u32::MAX; 2];
    let mut high_w = [0u32; 2];
    for y in 0..4usize {
        for x in 0..4usize {
            let s = usize::from(y >= 2);
            let w = fetch_pair_weight(false, false, dx, 0, x, y, w0, w1, b_solid[0], b_solid[1]);
            low_w[s] = low_w[s].min(w);
            high_w[s] = high_w[s].max(w);
        }
    }

    // Interpolate one channel: expand the endpoints to 16 bits, lerp by the
    // weight, then narrow back to 8 bits.
    let channel_interpolate = |le: u32, he: u32, w: u32| -> u32 {
        let (le16, he16) = if srgb {
            ((le << 8) | 0x80, (he << 8) | 0x80)
        } else {
            ((le << 8) | le, (he << 8) | he)
        };
        ((le16 * (64 - w) + he16 * w + 32) >> 6) >> 8
    };

    let orig_l = l;
    let orig_h = h;
    let mut num_low_stddev = 0u32;
    for s in 0..2 {
        if b_solid[s] {
            continue;
        }
        if low_w[s] > 0 || high_w[s] < 64 {
            for c in 0..3 {
                l[s][c] =
                    channel_interpolate(orig_l[s][c] as u32, orig_h[s][c] as u32, low_w[s]) as u8;
                h[s][c] =
                    channel_interpolate(orig_l[s][c] as u32, orig_h[s][c] as u32, high_w[s]) as u8;
            }
        }
        let e_delta = (0..3)
            .map(|c| {
                let d = h[s][c] as i32 - l[s][c] as i32;
                (d * d) as u32
            })
            .sum::<u32>();
        const E_DELTA_THRESH: u32 = 60;
        num_low_stddev += u32::from(e_delta < E_DELTA_THRESH);
    }

    if num_low_stddev == 2 {
        // Both subsets nearly flat: decode the 16 texels analytically and
        // re-encode with the trivial-mode-6 + pbit-opt flag set.
        let mut ep_l = [[0i32; 3]; 2];
        let mut ep_h = [[0i32; 3]; 2];
        for s in 0..2 {
            for c in 0..3 {
                let (le, he) = (l[s][c] as i32, h[s][c] as i32);
                if srgb {
                    ep_l[s][c] = (le << 8) | 0x80;
                    ep_h[s][c] = (he << 8) | 0x80;
                } else {
                    ep_l[s][c] = (le << 8) | le;
                    ep_h[s][c] = (he << 8) | he;
                }
            }
        }
        let mut dec_pixels = [[0u8; 4]; 16];
        for y in 0..4usize {
            for x in 0..4usize {
                let s = usize::from(y >= 2);
                let w = fetch_pair_weight(false, false, dx, 0, x, y, w0, w1, false, false);
                for c in 0..3 {
                    dec_pixels[x + y * 4][c] =
                        (((ep_l[s][c] as u32 * (64 - w) + ep_h[s][c] as u32 * w + 32) >> 6) >> 8)
                            as u8;
                }
                dec_pixels[x + y * 4][3] = 255;
            }
        }
        const FLAGS: u32 = crate::fastenc::bc7f::flags::USE_TRIVIAL_MODE6
            | crate::fastenc::bc7f::flags::PBIT_OPT_MODE6;
        crate::fastenc::bc7f::fast_pack_bc7_rgb_analytical(dst, &dec_pixels, FLAGS);
        return true;
    }

    let (min0, max0, p0) = shared_pbits_of(l[0], h[0]);
    let (min1, max1, p1) = shared_pbits_of(l[1], h[1]);

    let mut one_over_range = [0f32; 2];
    for s in 0..2 {
        if low_w[s] != high_w[s] {
            one_over_range[s] = 7.0 / (high_w[s] - low_w[s]) as f32;
        }
    }

    let mut bc7_weights = [0u8; 16];
    for y in 0..4usize {
        for x in 0..4usize {
            let s = usize::from(y >= 2);
            let mut qw = 0i32;
            if !b_solid[s] && low_w[s] != high_w[s] {
                let w = fetch_pair_weight(false, false, dx, 0, x, y, w0, w1, false, false);
                let f = (w as f32 - low_w[s] as f32) * one_over_range[s];
                qw = (f + 0.5) as i32;
                if qw as u32 > 7 {
                    qw = qw.clamp(0, 7);
                }
            }
            bc7_weights[x + y * 4] = qw as u8;
        }
    }
    emit_mode1(dst, 13, [min0, min1], [max0, max1], p0, p1, &bc7_weights);
    true
}

/// Pack the center BC7 block of a 2x2 6x6 tile, split top/bottom or left/right
/// into BC7 mode 1.
fn pack_middle_block(
    dst: &mut [u8; 16],
    blocks: &[[&LogAstcBlock; 2]; 2], // [x][y]
    weights: &[[[u8; 36]; 2]; 2],     // [x][y]
    do_left_right: bool,
) {
    // Subset membership: left/right pairs or top/bottom pairs.
    let (p, q_) = if do_left_right {
        ([blocks[0][0], blocks[0][1]], [blocks[1][0], blocks[1][1]])
    } else {
        ([blocks[0][0], blocks[1][0]], [blocks[0][1], blocks[1][1]])
    };

    let mut el = [[0u8; 4]; 2];
    let mut eh = [[0u8; 4]; 2];
    for i in 0..2 {
        let (l1, h1) = endpoints_or_solid(p[i]);
        let (l2, h2) = endpoints_or_solid(q_[i]);
        for c in 0..3 {
            el[i][c] = ((l1[c] as u32 + l2[c] as u32 + 1) >> 1) as u8;
            eh[i][c] = ((h1[c] as u32 + h2[c] as u32 + 1) >> 1) as u8;
        }
        el[i][3] = ((l1[3] as u32 + l2[3] as u32 + 1) >> 1) as u8;
        eh[i][3] = ((h1[3] as u32 + h2[3] as u32 + 1) >> 1) as u8;
    }

    let (min0, max0, p0) = shared_pbits_of(el[0], eh[0]);
    let (min1, max1, p1) = shared_pbits_of(el[1], eh[1]);

    let mut bc7_weights = [0u8; 16];
    for y in 0..4usize {
        for x in 0..4usize {
            let (ibx, iby) = (usize::from(x >= 2), usize::from(y >= 2));
            let w = if blocks[ibx][iby].solid_color_flag_ldr {
                0
            } else {
                let sx = if ibx == 0 { x + 4 } else { x - 2 };
                let sy = if iby == 0 { y + 4 } else { y - 2 };
                weights[ibx][iby][sx + sy * 6] as u32
            };
            bc7_weights[x + y * 4] = ((w * 7 + 32) >> 6) as u8;
        }
    }

    let part_id = if do_left_right { 0 } else { 13 };
    emit_mode1(
        dst,
        part_id,
        [min0, min1],
        [max0, max1],
        p0,
        p1,
        &bc7_weights,
    );
}

/// Dequantize a non-solid block's plane-0 weights and upsample them to the
/// full block grid. Solid blocks are skipped, since their weights are never
/// read.
fn astc_upsample_grid_weights(log: &LogAstcBlock, dst: &mut [u8], bw: usize, bh: usize) {
    if log.solid_color_flag_ldr {
        return;
    }
    let mut dequantized = [0u8; 144];
    dequantize_weights(log, &mut dequantized);
    let (gw, gh) = (log.grid_width as usize, log.grid_height as usize);
    if gw < bw || gh < bh {
        upsample_weight_grid_xuastc(bw, bh, gw, gh, &dequantized, dst);
    } else {
        dst[..bw * bh].copy_from_slice(&dequantized[..bw * bh]);
    }
}

/// Decode a full 6x6 or 8x6 block to pixels through the standard block decoder.
fn decode_full(
    log: &LogAstcBlock,
    bw: u32,
    bh: u32,
    srgb: bool,
    out: &mut [[u8; 4]],
) -> Option<()> {
    decode_block_ldr8(log, bw, bh, srgb, out)
}

/// The 8x6-source arm: pairs of vertically-adjacent blocks map onto up to 2x3
/// BC7 blocks.
#[allow(clippy::too_many_arguments)]
fn transcode_bc7_fast_8x6(
    blocks: &[u8],
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    srgb: bool,
    has_alpha: bool,
    out: &mut [u8],
) -> Option<()> {
    let dst_nx = lw.div_ceil(4);
    let dst_ny = lh.div_ceil(4);
    if out.len() != (dst_nx * dst_ny) as usize * 16 {
        return None;
    }
    out.fill(0);
    let bc7f_flags = bc7f::flags::DEFAULT;
    const ENDPOINT_TOL: i32 = 0;

    // Unpack every physical block back to its logical form once.
    let mut logs = Vec::with_capacity((bx * by) as usize);
    for i in 0..(bx * by) as usize {
        let src: &[u8; 16] = blocks[i * 16..i * 16 + 16].try_into().ok()?;
        logs.push(unpack_block(src, 8, 6)?);
    }

    let mut base_by = 0u32;
    while base_by < by {
        let num_buffered = 2.min(by - base_by);
        let buffered_src_pixel_y = base_by * 6;
        let rows_to_emit = (num_buffered * 6 + 3) >> 2;

        for src_bx in 0..bx {
            let b: [&LogAstcBlock; 2] = [
                &logs[(base_by * bx + src_bx) as usize],
                &logs[((base_by + 1.min(num_buffered - 1)) * bx + src_bx) as usize],
            ];

            // Emit one packed block to every in-bounds tile position.
            macro_rules! for_each_dst {
                (|$dst:ident, $dy:ident, $dx:ident| $body:block) => {
                    for $dy in 0..rows_to_emit {
                        let dst_by = (buffered_src_pixel_y >> 2) + $dy;
                        if dst_by >= dst_ny {
                            break;
                        }
                        for $dx in 0..2u32 {
                            let dst_bx = (src_bx << 1) + $dx;
                            if dst_bx >= dst_nx {
                                break;
                            }
                            let o = ((dst_by * dst_nx + dst_bx) as usize) * 16;
                            let $dst: &mut [u8; 16] = (&mut out[o..o + 16]).try_into().unwrap();
                            $body
                        }
                    }
                };
            }

            if blocks_same_solid_colors(b[0], b[1], 0) {
                let sc: Rgba = core::array::from_fn(|c| (b[0].solid_color[c] >> 8) as u8);
                let mut temp = [0u8; 16];
                pack_mode5_solid(&mut temp, sc);
                for_each_dst!(|dst, _dy, _dx| {
                    dst.copy_from_slice(&temp);
                });
                continue;
            }

            let num_hard = b
                .iter()
                .filter(|l| l.dual_plane || l.num_partitions > 1)
                .count();

            let mut unpacked = [[[0u8; 4]; 48]; 2];
            let mut has_unpacked = [false; 2];

            if num_hard == 2 {
                for (row, l) in b.iter().enumerate() {
                    decode_full(l, 8, 6, srgb, &mut unpacked[row])?;
                }
                for_each_dst!(|dst, dy, dx| {
                    let mut pixels = [[0u8; 4]; 16];
                    for y in 0..4u32 {
                        let sy = dy * 4 + y;
                        for x in 0..4u32 {
                            let sx = dx * 4 + x;
                            pixels[(x + y * 4) as usize] =
                                unpacked[(sy / 6) as usize][(sx + (sy % 6) * 8) as usize];
                        }
                    }
                    if has_alpha {
                        bc7f::fast_pack_bc7_auto_rgba(dst, &pixels, bc7f_flags);
                    } else {
                        bc7f::fast_pack_bc7_auto_rgb(dst, &pixels, bc7f_flags);
                    }
                });
                continue;
            }

            let mut weights = [[0u8; 48]; 2];
            for (row, l) in b.iter().enumerate() {
                astc_upsample_grid_weights(l, &mut weights[row], 8, 6);
            }

            for_each_dst!(|dst, dy, dx| {
                let top_by = ((dy * 4) / 6) as usize;
                let bot_by = ((dy * 4 + 3) / 6) as usize;
                let (b0, b1) = (b[top_by], b[bot_by]);
                let single = top_by == bot_by;

                let mut full_encode = false;
                if b0.dual_plane || b1.dual_plane || b0.num_partitions > 1 || b1.num_partitions > 1
                {
                    full_encode = true;
                } else if single {
                    if b0.solid_color_flag_ldr {
                        let sc: Rgba = core::array::from_fn(|c| (b0.solid_color[c] >> 8) as u8);
                        pack_mode5_solid(dst, sc);
                    } else {
                        pack_from_astc_single_subset(
                            dst,
                            b0,
                            &weights[top_by],
                            (dx * 4) as usize,
                            ((dy * 4) % 6) as usize,
                            8,
                        );
                    }
                } else if blocks_same_single_subset_endpoints(b0, b1, ENDPOINT_TOL) {
                    pack_same_endpoints(
                        dst,
                        b0,
                        &weights[top_by],
                        b1,
                        &weights[bot_by],
                        dx as i32,
                        dy as i32,
                        false,
                    );
                } else if !block_has_alpha(b0) && !block_has_alpha(b1) {
                    if !pack_8x6_two_subsets_hq(
                        dst,
                        b0,
                        &weights[top_by],
                        b1,
                        &weights[bot_by],
                        dx as i32,
                        srgb,
                    ) {
                        full_encode = true;
                    }
                } else {
                    full_encode = true;
                }

                if full_encode {
                    for row in [top_by, bot_by] {
                        if !has_unpacked[row] {
                            decode_full(b[row], 8, 6, srgb, &mut unpacked[row])?;
                            has_unpacked[row] = true;
                        }
                    }
                    let mut pixels = [[0u8; 4]; 16];
                    for y in 0..4u32 {
                        let sy = dy * 4 + y;
                        for x in 0..4u32 {
                            let sx = dx * 4 + x;
                            pixels[(x + y * 4) as usize] =
                                unpacked[(sy / 6) as usize][(sx + (sy % 6) * 8) as usize];
                        }
                    }
                    if has_alpha {
                        bc7f::fast_pack_bc7_auto_rgba(dst, &pixels, bc7f_flags);
                    } else {
                        bc7f::fast_pack_bc7_auto_rgb(dst, &pixels, bc7f_flags);
                    }
                }
            });
        }
        base_by += 2;
    }
    Some(())
}

/// The 6x6-source arm: 2x2 tiles of blocks map onto up to 3x3 BC7 blocks, with
/// the center block overlapping all four sources.
#[allow(clippy::too_many_arguments)]
fn transcode_bc7_fast_6x6(
    blocks: &[u8],
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    srgb: bool,
    has_alpha: bool,
    out: &mut [u8],
) -> Option<()> {
    let dst_nx = lw.div_ceil(4);
    let dst_ny = lh.div_ceil(4);
    if out.len() != (dst_nx * dst_ny) as usize * 16 {
        return None;
    }
    out.fill(0);
    let bc7f_flags = bc7f::flags::DEFAULT;
    const ENDPOINT_TOL: i32 = 1;

    let mut logs = Vec::with_capacity((bx * by) as usize);
    for i in 0..(bx * by) as usize {
        let src: &[u8; 16] = blocks[i * 16..i * 16 + 16].try_into().ok()?;
        logs.push(unpack_block(src, 6, 6)?);
    }

    let mut base_by = 0u32;
    while base_by < by {
        let num_buffered = 2.min(by - base_by);
        let buffered_src_pixel_y = base_by * 6;
        let rows_to_emit = (num_buffered * 6 + 3) >> 2;

        let mut src_bx = 0u32;
        while src_bx < bx {
            // 2x2 block pointers with edge duplication, indexed [x][y].
            let col = |x: u32| (src_bx + x).min(bx - 1);
            let row = |y: u32| base_by + y.min(num_buffered - 1);
            let blk = |x: u32, y: u32| &logs[(row(y) * bx + col(x)) as usize];
            let b: [[&LogAstcBlock; 2]; 2] = [[blk(0, 0), blk(0, 1)], [blk(1, 0), blk(1, 1)]];

            macro_rules! for_each_dst {
                (|$dst:ident, $dy:ident, $dx:ident| $body:block) => {
                    for $dy in 0..rows_to_emit {
                        let dst_by = (buffered_src_pixel_y >> 2) + $dy;
                        if dst_by >= dst_ny {
                            break;
                        }
                        for $dx in 0..3u32 {
                            let dst_bx = ((src_bx * 6) >> 2) + $dx;
                            if dst_bx >= dst_nx {
                                break;
                            }
                            let o = ((dst_by * dst_nx + dst_bx) as usize) * 16;
                            let $dst: &mut [u8; 16] = (&mut out[o..o + 16]).try_into().unwrap();
                            $body
                        }
                    }
                };
            }

            // All four solid and byte-identical: nine identical mode-5 blocks.
            let all_solid = b.iter().flatten().all(|l| l.solid_color_flag_ldr)
                && b.iter()
                    .flatten()
                    .all(|l| l.solid_color == b[0][0].solid_color);
            if all_solid {
                let sc: Rgba = core::array::from_fn(|c| (b[0][0].solid_color[c] >> 8) as u8);
                let mut temp = [0u8; 16];
                pack_mode5_solid(&mut temp, sc);
                for_each_dst!(|dst, _dy, _dx| {
                    dst.copy_from_slice(&temp);
                });
                src_bx += 2;
                continue;
            }

            let num_hard = b
                .iter()
                .flatten()
                .filter(|l| l.dual_plane || l.num_partitions > 1)
                .count();

            let mut unpacked = [[[[0u8; 4]; 36]; 2]; 2]; // [x][y]
            let mut has_unpacked = [[false; 2]; 2];

            let gather = |unpacked: &[[[[u8; 4]; 36]; 2]; 2], dx: u32, dy: u32| -> [[u8; 4]; 16] {
                let mut pixels = [[0u8; 4]; 16];
                for y in 0..4u32 {
                    let sy = dy * 4 + y;
                    for x in 0..4u32 {
                        let sx = dx * 4 + x;
                        pixels[(x + y * 4) as usize] = unpacked[(sx / 6) as usize]
                            [(sy / 6) as usize][((sx % 6) + (sy % 6) * 6) as usize];
                    }
                }
                pixels
            };

            if num_hard == 4 {
                for x in 0..2 {
                    for y in 0..2 {
                        decode_full(b[x][y], 6, 6, srgb, &mut unpacked[x][y])?;
                    }
                }
                for_each_dst!(|dst, dy, dx| {
                    let pixels = gather(&unpacked, dx, dy);
                    if has_alpha {
                        bc7f::fast_pack_bc7_auto_rgba(dst, &pixels, bc7f_flags);
                    } else {
                        bc7f::fast_pack_bc7_auto_rgb(dst, &pixels, bc7f_flags);
                    }
                });
                src_bx += 2;
                continue;
            }

            let mut weights = [[[0u8; 36]; 2]; 2]; // [x][y]
            for x in 0..2 {
                for y in 0..2 {
                    astc_upsample_grid_weights(b[x][y], &mut weights[x][y], 6, 6);
                }
            }

            // The eight non-center BC7 blocks.
            for_each_dst!(|dst, dy, dx| {
                if dx == 1 && dy == 1 {
                    continue;
                }
                let top_bx = ((dx * 4) / 6) as usize;
                let bot_bx = ((dx * 4 + 3) / 6) as usize;
                let top_by = ((dy * 4) / 6) as usize;
                let bot_by = ((dy * 4 + 3) / 6) as usize;
                let (b0, b1) = (b[top_bx][top_by], b[bot_bx][bot_by]);
                let single = top_bx == bot_bx && top_by == bot_by;

                let mut full_encode = false;
                if b0.dual_plane || b1.dual_plane || b0.num_partitions > 1 || b1.num_partitions > 1
                {
                    full_encode = true;
                } else if single {
                    if b0.solid_color_flag_ldr {
                        let sc: Rgba = core::array::from_fn(|c| (b0.solid_color[c] >> 8) as u8);
                        pack_mode5_solid(dst, sc);
                    } else {
                        pack_from_astc_single_subset(
                            dst,
                            b0,
                            &weights[top_bx][top_by],
                            ((dx * 4) % 6) as usize,
                            ((dy * 4) % 6) as usize,
                            6,
                        );
                    }
                } else if blocks_same_solid_colors(b0, b1, 0) {
                    let sc: Rgba = core::array::from_fn(|c| (b0.solid_color[c] >> 8) as u8);
                    pack_mode5_solid(dst, sc);
                } else if blocks_same_single_subset_endpoints(b0, b1, ENDPOINT_TOL) {
                    pack_same_endpoints(
                        dst,
                        b0,
                        &weights[top_bx][top_by],
                        b1,
                        &weights[bot_bx][bot_by],
                        dx as i32,
                        dy as i32,
                        true,
                    );
                } else if !block_has_alpha(b0) && !block_has_alpha(b1) {
                    if !pack_6x6_two_subsets(
                        dst,
                        b0,
                        &weights[top_bx][top_by],
                        b1,
                        &weights[bot_bx][bot_by],
                        dx as i32,
                        dy as i32,
                    ) {
                        full_encode = true;
                    }
                } else {
                    full_encode = true;
                }

                if full_encode {
                    for (ix, iy) in [(top_bx, top_by), (bot_bx, bot_by)] {
                        if !has_unpacked[ix][iy] {
                            decode_full(b[ix][iy], 6, 6, srgb, &mut unpacked[ix][iy])?;
                            has_unpacked[ix][iy] = true;
                        }
                    }
                    let pixels = gather(&unpacked, dx, dy);
                    if has_alpha {
                        bc7f::fast_pack_bc7_auto_rgba(dst, &pixels, bc7f_flags);
                    } else {
                        bc7f::fast_pack_bc7_auto_rgb(dst, &pixels, bc7f_flags);
                    }
                }
            });

            // The center BC7 block, overlapping all four sources.
            let dst_bx = ((src_bx * 6) >> 2) + 1;
            let dst_by = (buffered_src_pixel_y >> 2) + 1;
            if dst_bx < dst_nx && dst_by < dst_ny {
                let o = ((dst_by * dst_nx + dst_bx) as usize) * 16;
                let mut center = [0u8; 16];
                let mut skip_full_encode = false;

                if num_hard == 0 {
                    let top_ok = (blocks_same_solid_colors(b[0][0], b[1][0], 1)
                        || blocks_same_single_subset_endpoints(b[0][0], b[1][0], ENDPOINT_TOL))
                        && !block_has_alpha(b[0][0])
                        && !block_has_alpha(b[1][0]);
                    let bot_ok = (blocks_same_solid_colors(b[0][1], b[1][1], 1)
                        || blocks_same_single_subset_endpoints(b[0][1], b[1][1], ENDPOINT_TOL))
                        && !block_has_alpha(b[0][1])
                        && !block_has_alpha(b[1][1]);
                    let left_ok = (blocks_same_solid_colors(b[0][0], b[0][1], 1)
                        || blocks_same_single_subset_endpoints(b[0][0], b[0][1], ENDPOINT_TOL))
                        && !block_has_alpha(b[0][0])
                        && !block_has_alpha(b[0][1]);
                    let right_ok = (blocks_same_solid_colors(b[1][0], b[1][1], 1)
                        || blocks_same_single_subset_endpoints(b[1][0], b[1][1], ENDPOINT_TOL))
                        && !block_has_alpha(b[1][0])
                        && !block_has_alpha(b[1][1]);

                    if top_ok && bot_ok {
                        pack_middle_block(&mut center, &b, &weights, false);
                        skip_full_encode = true;
                    } else if left_ok && right_ok {
                        pack_middle_block(&mut center, &b, &weights, true);
                        skip_full_encode = true;
                    }
                }

                if !skip_full_encode {
                    // Decode whichever sources aren't unpacked yet. A full
                    // decode of a block yields the same pixels as decoding only
                    // the region the center block needs.
                    for x in 0..2 {
                        for y in 0..2 {
                            if !has_unpacked[x][y] {
                                decode_full(b[x][y], 6, 6, srgb, &mut unpacked[x][y])?;
                                has_unpacked[x][y] = true;
                            }
                        }
                    }
                    let pixels = gather(&unpacked, 1, 1);
                    if has_alpha {
                        bc7f::fast_pack_bc7_auto_rgba(&mut center, &pixels, bc7f_flags);
                    } else {
                        bc7f::fast_pack_bc7_auto_rgb(&mut center, &pixels, bc7f_flags);
                    }
                }
                out[o..o + 16].copy_from_slice(&center);
            }

            src_bx += 2;
        }
        base_by += 2;
    }
    Some(())
}
