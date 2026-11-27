//! The default cross-block BC7 mode-5 chroma post-pass
//! (`chroma_filter_bc7_mode5`).
//!
//! After the ETC1S-to-BC7 color pass records each block's decoded endpoint index
//! into a `[bx][by]` grid, this pass converts every endpoint to CoCg and, per
//! block, checks whether any of the eight neighbours (the 3x3 stencil minus the
//! center) differs in chroma by more than a threshold. When one does, the
//! block's chroma is re-derived from an edge-clamped bilinear blend of the
//! neighbour endpoints' CoCg while keeping the block's per-texel luma, then the
//! BC7 mode-5 block is re-encoded.
//!
//! All float math keeps every product and sum a separate operation, with no use
//! of `f32::mul_add`. Fusing a multiply and add would change the rounding, so
//! fp contraction is avoided to keep the output bit-exact.

use super::etc1s::Endpoint;
use crate::mathf::{fabs, fabsf};
use alloc::vec::Vec;

/// Per-channel CoCg delta between a block and one of its neighbours past which
/// the block counts as a chroma edge and gets filtered.
const CHROMA_THRESH: f32 = 10.0;
/// Per-texel luma variance below which a flagged block is left untouched.
const Y_VAR_SKIP_THRESH: f32 = 3.0;

/// BC7 2-bit weight table: selector `w` blends the two
/// endpoints as `(64 - weight, weight) / 64`.
const G_BC7_WEIGHTS2: [u32; 4] = [0, 21, 43, 64];

/// Clamp `v` to the inclusive range `[l, h]`.
#[inline]
fn clampf(v: f32, l: f32, h: f32) -> f32 {
    if v < l {
        l
    } else if v > h {
        h
    } else {
        v
    }
}

/// Integer clamp of `v` to `[l, h]`.
#[inline]
fn clampi(v: i32, l: i32, h: i32) -> i32 {
    if v < l {
        l
    } else if v > h {
        h
    } else {
        v
    }
}

/// Linear interpolation `a + (b - a) * s` (no fused multiply-add).
#[inline]
fn lerp(a: f32, b: f32, s: f32) -> f32 {
    a + (b - a) * s
}

/// Square of `v`, used for the per-block luma variance.
#[inline]
fn squaref(v: f32) -> f32 {
    v * v
}

/// Three-component dot product, summed left to right: c0*o0 + c1*o1 + c2*o2.
#[inline]
fn dot3(c: [f32; 3], o: [f32; 3]) -> f32 {
    c[0] * o[0] + c[1] * o[1] + c[2] * o[2]
}

/// RGB to chroma: Co = dot(rgb, (0.5, 0, -0.5)); Cg = dot(rgb, (-0.25, 0.5, -0.25)).
#[inline]
fn rgb_to_cocg(rgb: [f32; 3]) -> [f32; 2] {
    [dot3(rgb, [0.5, 0.0, -0.5]), dot3(rgb, [-0.25, 0.5, -0.25])]
}

/// YCoCg to RGB: R = dot(ycocg, (1,1,-1)); G = dot(ycocg, (1,0,1)); B = dot(ycocg, (1,-1,-1)).
#[inline]
fn ycocg_to_rgb(ycocg: [f32; 3]) -> [f32; 3] {
    [
        dot3(ycocg, [1.0, 1.0, -1.0]),
        dot3(ycocg, [1.0, 0.0, 1.0]),
        dot3(ycocg, [1.0, -1.0, -1.0]),
    ]
}

/// `color5_to_cocg`: expand each 5-bit channel to 8 bits, then to CoCg.
#[inline]
fn color5_to_cocg(e: &Endpoint) -> [f32; 2] {
    let c0 = e.color5.r() as i32;
    let c1 = e.color5.g() as i32;
    let c2 = e.color5.b() as i32;
    let r = (c0 << 3) | (c0 >> 2);
    let g = (c1 << 3) | (c1 >> 2);
    let b = (c2 << 3) | (c2 >> 2);
    rgb_to_cocg([r as f32, g as f32, b as f32])
}

/// Expand a 7-bit BC7 endpoint channel to 8 bits (shift up one bit and fold the
/// top bit into the new low bit).
#[inline]
fn bc7_7_to_8(v: u32) -> u32 {
    (v << 1) | (v >> 6)
}

/// Expand a 7-bit value to 8 bits (shift up one bit, fold the top bit into the
/// new low bit).
#[inline]
fn from_7(v: u32) -> u32 {
    (v << 1) | (v >> 6)
}

/// 2-bit BC7 endpoint interpolation: blend `l` and `h` by selector `w`'s
/// weight, the +32 rounding the divide by 64.
#[inline]
fn bc7_interp2(l: u32, h: u32, w: usize) -> u32 {
    (l * (64 - G_BC7_WEIGHTS2[w]) + h * G_BC7_WEIGHTS2[w] + 32) >> 6
}

/// Row-major grid of the decoded ETC1S endpoint index for each block, used by
/// the chroma pass to look up neighbour chroma.
pub struct EndpointGrid {
    /// Grid width in blocks.
    w: i32,
    /// Grid height in blocks.
    h: i32,
    /// Row-major endpoint indices, one per block.
    data: Vec<u16>,
}

impl EndpointGrid {
    /// Allocate a zero-filled grid sized to the block dimensions.
    fn new(num_blocks_x: u32, num_blocks_y: u32) -> Self {
        EndpointGrid {
            w: num_blocks_x as i32,
            h: num_blocks_y as i32,
            data: vec![0u16; (num_blocks_x * num_blocks_y) as usize],
        }
    }

    /// Store the endpoint index for block `(bx, by)`.
    #[inline]
    fn set(&mut self, bx: u32, by: u32, ep: u16) {
        self.data[(by * self.w as u32 + bx) as usize] = ep;
    }

    /// Endpoint index at `(bx, by)`, unclamped. The caller guarantees the
    /// coordinates are in bounds.
    #[inline]
    fn at(&self, bx: i32, by: i32) -> u16 {
        self.data[(by * self.w + bx) as usize]
    }

    /// Endpoint index at `(bx, by)`, with the coordinates clamped to the grid
    /// edge so out-of-range neighbours read the nearest border block.
    #[inline]
    fn at_clamped(&self, bx: i32, by: i32) -> u16 {
        let cx = clampi(bx, 0, self.w - 1);
        let cy = clampi(by, 0, self.h - 1);
        self.data[(cy * self.w + cx) as usize]
    }
}

/// One opaque RGB pixel fed to the mode-5 encoder. Alpha is implicit (the
/// re-encode path is always opaque), so only the three color channels are kept.
#[derive(Clone, Copy, Default)]
struct Color32Pack {
    r: u8,
    g: u8,
    b: u8,
}

// BC7 mode-5 block field access. The offsets must agree with the LSB-first
// bitfield layout the mode-5 packer below writes.

/// Read `num_bits` starting at bit `ofs` (LSB-first) from a packed block.
#[inline]
fn read_bits(block: &[u8; 16], ofs: u32, num_bits: u32) -> u64 {
    let mut val = 0u64;
    let mut got = 0u32;
    let mut o = ofs;
    while got < num_bits {
        let byte = (o >> 3) as usize;
        let bit = o & 7;
        let take = (8 - bit).min(num_bits - got);
        let mask = ((1u32 << take) - 1) as u8;
        let chunk = ((block[byte] >> bit) & mask) as u64;
        val |= chunk << got;
        got += take;
        o += take;
    }
    val
}

/// The seven-bit RGB endpoint fields of a BC7 mode-5 block:
/// r0@8, r1@15, g0@22, g1@29, b0@36, b1@43.
#[inline]
fn read_endpoints(block: &[u8; 16]) -> [u32; 6] {
    [
        read_bits(block, 8, 7) as u32,
        read_bits(block, 15, 7) as u32,
        read_bits(block, 22, 7) as u32,
        read_bits(block, 29, 7) as u32,
        read_bits(block, 36, 7) as u32,
        read_bits(block, 43, 7) as u32,
    ]
}

/// The 64 high bits of the block, i.e. bytes [8..16] as a u64.
#[inline]
fn read_hi_bits(block: &[u8; 16]) -> u64 {
    u64::from_le_bytes(block[8..16].try_into().unwrap())
}

// BC7 mode-5 RGB encoder, used to re-pack a block after its chroma has been
// rewritten.

/// The per-value decision thresholds used to round a
/// float channel to a 7-bit endpoint. Built once, deterministically.
struct Mode5Midpoints {
    m: [f32; 128],
}

impl Mode5Midpoints {
    /// Precompute the 128 rounding thresholds. Slot `i` holds the midpoint
    /// between the decoded values of 7-bit endpoints `i` and `i + 1`, so a float
    /// channel rounds up to `i + 1` once it passes that midpoint. The last slot
    /// is set to a huge value so index 127 never rounds further up.
    fn new() -> Self {
        let mut m = [0.0f32; 128];
        for (i, slot) in m.iter_mut().enumerate() {
            let vl = (i << 1) | ((i << 1) >> 7);
            let lo = vl as f32 / 255.0;

            let vh_base = 127.min(i + 1) << 1;
            let vh = vh_base | (vh_base >> 7);
            let hi = vh as f32 / 255.0;

            *slot = if i == 127 { 1e+15 } else { (lo + hi) / 2.0 };
        }
        Mode5Midpoints { m }
    }

    /// Quantize a normalized float channel in `[0, 1]` to a 7-bit endpoint,
    /// rounding up past the precomputed midpoint and clamping to `[0, 127]`.
    #[inline]
    fn to_7(&self, c: f32) -> i32 {
        let mut vl = (c * 127.0) as i32;
        if c > self.m[vl as usize] {
            vl += 1;
        }
        clampi(vl, 0, 127)
    }
}

/// Maps an 8-bit value to the BC7 mode-5 7-bit endpoint
/// pair (stored `[m_hi, m_lo]`) that best reproduces it at selector 1. Used by
/// the solid-color path of the encoder.
use crate::tables::bc7_m5_equals_1::G_BC7_M5_EQUALS_1;

/// Packed per-selector contributions to the least-squares normal matrix, one
/// byte per matrix term so a single accumulator sums all four terms at once.
const G_WEIGHT_VALS4: [u32; 4] = [0x000009, 0x010204, 0x040201, 0x090000];

/// Assign each of the 16 pixels to one of the four mode-5 selector levels by
/// projecting onto the endpoint axis and thresholding against the three
/// inter-level boundaries.
fn eval_weights(
    pixels: &[Color32Pack; 16],
    lr: u32,
    lg: u32,
    lb: u32,
    hr: u32,
    hg: u32,
    hb: u32,
) -> [u8; 16] {
    let lr = from_7(lr);
    let lg = from_7(lg);
    let lb = from_7(lb);
    let hr = from_7(hr);
    let hg = from_7(hg);
    let hb = from_7(hb);

    let mut cr = [0i32; 4];
    let mut cg = [0i32; 4];
    let mut cb = [0i32; 4];
    for i in 0..4 {
        cr[i] = bc7_interp2(lr, hr, i) as u8 as i32;
        cg[i] = bc7_interp2(lg, hg, i) as u8 as i32;
        cb[i] = bc7_interp2(lb, hb, i) as u8 as i32;
    }

    let ar = cr[3] - cr[0];
    let ag = cg[3] - cg[0];
    let ab = cb[3] - cb[0];

    let mut dots = [0i32; 4];
    for i in 0..4 {
        dots[i] = cr[i] * ar + cg[i] * ag + cb[i] * ab;
    }

    let t0 = dots[0] + dots[1];
    let t1 = dots[1] + dots[2];
    let t2 = dots[2] + dots[3];

    let ar = ar * 2;
    let ag = ag * 2;
    let ab = ab * 2;

    // Each pixel's selector is the count of level boundaries its dot product
    // passes; the lowest boundary is exclusive, the upper two inclusive.
    let mut weights = [0u8; 16];
    let mut i = 0usize;
    while i < 16 {
        for k in 0..4 {
            let p = &pixels[i + k];
            let d = p.r as i32 * ar + p.g as i32 * ag + p.b as i32 * ab;
            weights[i + k] = ((d > t0) as u8) + ((d >= t1) as u8) + ((d >= t2) as u8);
        }
        i += 4;
    }
    weights
}

/// `compute_least_squares_endpoints4_rgb`: refine the endpoints from the
/// current selector assignment by solving the 2x2 least-squares normal
/// equations per channel. Returns the refined 7-bit endpoints
/// (lr, lg, lb, hr, hg, hb), or `None` when the matrix is near singular.
#[allow(clippy::too_many_arguments)]
fn compute_least_squares_endpoints4_rgb(
    pixels: &[Color32Pack; 16],
    sels: &[u8; 16],
    mids: &Mode5Midpoints,
    total_r: i32,
    total_g: i32,
    total_b: i32,
) -> Option<(i32, i32, i32, i32, i32, i32)> {
    let mut uq00_r = 0u32;
    let mut uq00_g = 0u32;
    let mut uq00_b = 0u32;
    let mut weight_accum = 0u32;
    for i in 0..16 {
        let r = pixels[i].r as u32;
        let g = pixels[i].g as u32;
        let b = pixels[i].b as u32;
        let sel = sels[i] as u32;
        weight_accum = weight_accum.wrapping_add(G_WEIGHT_VALS4[sel as usize]);
        uq00_r += sel * r;
        uq00_g += sel * g;
        uq00_b += sel * b;
    }

    let q10_r = total_r * 3 - uq00_r as i32;
    let q10_g = total_g * 3 - uq00_g as i32;
    let q10_b = total_b * 3 - uq00_b as i32;

    let z00 = ((weight_accum >> 16) & 0xFF) as f32;
    let z10 = ((weight_accum >> 8) & 0xFF) as f32;
    let z11 = (weight_accum & 0xFF) as f32;
    let z01 = z10;

    let mut det = z00 * z11 - z01 * z10;
    if fabsf(det) < 1e-8 {
        return None;
    }

    det = (3.0 / 255.0) / det;

    let iz00 = z11 * det;
    let iz01 = -z01 * det;
    let iz10 = -z10 * det;
    let iz11 = z00 * det;

    let fhr = clampf(iz00 * uq00_r as f32 + iz01 * q10_r as f32, 0.0, 1.0);
    let flr = clampf(iz10 * uq00_r as f32 + iz11 * q10_r as f32, 0.0, 1.0);

    let fhg = clampf(iz00 * uq00_g as f32 + iz01 * q10_g as f32, 0.0, 1.0);
    let flg = clampf(iz10 * uq00_g as f32 + iz11 * q10_g as f32, 0.0, 1.0);

    let fhb = clampf(iz00 * uq00_b as f32 + iz01 * q10_b as f32, 0.0, 1.0);
    let flb = clampf(iz10 * uq00_b as f32 + iz11 * q10_b as f32, 0.0, 1.0);

    let lr = mids.to_7(flr);
    let lg = mids.to_7(flg);
    let lb = mids.to_7(flb);
    let hr = mids.to_7(fhr);
    let hg = mids.to_7(fhg);
    let hb = mids.to_7(fhb);

    Some((lr, lg, lb, hr, hg, hb))
}

/// Write the 7-bit RGB endpoints (swapping low and high if the anchor weight has
/// bit 2 set), opaque alpha, and the 31-bit selector field into a fresh packed
/// block.
#[allow(clippy::too_many_arguments)]
fn pack_bc7_mode5_rgb_block(
    block: &mut [u8; 16],
    mut lr: i32,
    mut lg: i32,
    mut lb: i32,
    mut hr: i32,
    mut hg: i32,
    mut hb: i32,
    weights: &[u8; 16],
) {
    let mut weight_inv = 0u8;
    if weights[0] & 2 != 0 {
        core::mem::swap(&mut lr, &mut hr);
        core::mem::swap(&mut lg, &mut hg);
        core::mem::swap(&mut lb, &mut hb);
        weight_inv = 3;
    }

    // m_lo_bits laid out LSB-first:
    //   m_mode : 6  -> 32 (mode 5: bit 5 set)
    //   m_rot  : 2  -> 0
    //   r0,r1,g0,g1,b0,b1 : 7 each
    //   m_a0 : 8 -> 255
    //   m_a1_0 : 6 -> 63
    // (m_a1_1 : 2 -> 3 lives in m_hi_bits at bit 0..2; we set it via sel_bits.)
    let mut lo_bits = 0u64;
    let mut ofs = 0u32;
    let mut put = |val: u64, n: u32, ofs: &mut u32| {
        lo_bits |= (val & ((1u64 << n) - 1)) << *ofs;
        *ofs += n;
    };
    put(32, 6, &mut ofs); // m_mode
    put(0, 2, &mut ofs); // m_rot
    put(lr as u64, 7, &mut ofs);
    put(hr as u64, 7, &mut ofs);
    put(lg as u64, 7, &mut ofs);
    put(hg as u64, 7, &mut ofs);
    put(lb as u64, 7, &mut ofs);
    put(hb as u64, 7, &mut ofs);
    put(255, 8, &mut ofs); // m_a0
    put(63, 6, &mut ofs); // m_a1_0 -> ofs now 64
    block[0..8].copy_from_slice(&lo_bits.to_le_bytes());

    // m_hi_bits: m_a1_1 (2 bits) = 3, then the 16 selectors (anchor 1 bit).
    let mut sel_bits = 3u64; // m_a1_1 = 3
    let mut cur_ofs = 2u32;
    for (i, &w) in weights.iter().enumerate() {
        sel_bits |= ((weight_inv ^ w) as u64) << cur_ofs;
        cur_ofs += if i != 0 { 2 } else { 1 };
    }
    block[8..16].copy_from_slice(&sel_bits.to_le_bytes());
}

/// `encode_bc7_mode_5_block` (non-HQ path), re-encoding 16 RGB pixels (opaque)
/// back into the packed block.
fn encode_bc7_mode_5_block(
    block: &mut [u8; 16],
    pixels: &[Color32Pack; 16],
    mids: &Mode5Midpoints,
) {
    let mut total_r = 0i32;
    let mut total_g = 0i32;
    let mut total_b = 0i32;
    let mut min_r = 255i32;
    let mut min_g = 255i32;
    let mut min_b = 255i32;
    let mut max_r = 0i32;
    let mut max_g = 0i32;
    let mut max_b = 0i32;
    for p in pixels.iter() {
        let r = p.r as i32;
        let g = p.g as i32;
        let b = p.b as i32;
        total_r += r;
        total_g += g;
        total_b += b;
        min_r = min_r.min(r);
        min_g = min_g.min(g);
        min_b = min_b.min(b);
        max_r = max_r.max(r);
        max_g = max_g.max(g);
        max_b = max_b.max(b);
    }

    if min_r == max_r && min_g == max_g && min_b == max_b {
        // Table entries are [m_hi, m_lo].
        let er = G_BC7_M5_EQUALS_1[min_r as usize];
        let eg = G_BC7_M5_EQUALS_1[min_g as usize];
        let eb = G_BC7_M5_EQUALS_1[min_b as usize];
        let lr = er[1] as i32;
        let lg = eg[1] as i32;
        let lb = eb[1] as i32;
        let hr = er[0] as i32;
        let hg = eg[0] as i32;
        let hb = eb[0] as i32;
        let solid_weights = [1u8; 16];
        pack_bc7_mode5_rgb_block(block, lr, lg, lb, hr, hg, hb, &solid_weights);
        return;
    }

    let mean_r = (total_r + 8) >> 4;
    let mean_g = (total_g + 8) >> 4;
    let mean_b = (total_b + 8) >> 4;

    // covariance (scaled by 16)
    let mut icov = [0i32; 6];
    for p in pixels.iter() {
        let r = p.r as i32 - mean_r;
        let g = p.g as i32 - mean_g;
        let b = p.b as i32 - mean_b;
        icov[0] += r * r;
        icov[1] += r * g;
        icov[2] += r * b;
        icov[3] += g * g;
        icov[4] += g * b;
        icov[5] += b * b;
    }

    let block_max_var = icov[0].max(icov[3]).max(icov[5]);

    const SIMPLE_BLOCK_THRESH: i32 = 10 * 16;

    let (lr, lg, lb, hr, hg, hb);
    if block_max_var < SIMPLE_BLOCK_THRESH {
        // Low-variance block: skip the PCA axis search and place the endpoints
        // by lerping between the channel min and max at 16/255 and 239/255.
        const L: i32 = 16;
        const H: i32 = 239;
        lr = mids_to_7_from_lerp8(min_r, max_r, L, mids);
        lg = mids_to_7_from_lerp8(min_g, max_g, L, mids);
        lb = mids_to_7_from_lerp8(min_b, max_b, L, mids);
        hr = mids_to_7_from_lerp8(min_r, max_r, H, mids);
        hg = mids_to_7_from_lerp8(min_g, max_g, H, mids);
        hb = mids_to_7_from_lerp8(min_b, max_b, H, mids);

        let weights = eval_weights(
            pixels, lr as u32, lg as u32, lb as u32, hr as u32, hg as u32, hb as u32,
        );
        pack_bc7_mode5_rgb_block(block, lr, lg, lb, hr, hg, hb, &weights);
        return;
    }

    let mut cov = [0.0f32; 6];
    for i in 0..6 {
        cov[i] = icov[i] as f32;
    }

    let sc = 1.0f32 / block_max_var as f32;
    let wx = sc * cov[0];
    let wy = sc * cov[3];
    let wz = sc * cov[5];

    let alt_xr = cov[0] * wx + cov[1] * wy + cov[2] * wz;
    let alt_xg = cov[1] * wx + cov[3] * wy + cov[4] * wz;
    let alt_xb = cov[2] * wx + cov[4] * wy + cov[5] * wz;

    let mut saxis_r = 306i32;
    let mut saxis_g = 601i32;
    let mut saxis_b = 117i32;

    let k = fabsf(alt_xr).max(fabsf(alt_xg)).max(fabsf(alt_xb));
    const SMALL_FLOAT_VAL: f32 = 0.0000125;
    // The magnitude compare widens both sides to f64; the result is identical
    // to the f32 compare because f32-to-f64 conversion is exact.
    if fabs(k as f64) >= SMALL_FLOAT_VAL as f64 {
        let m = 2048.0f32 / k;
        saxis_r = (alt_xr * m) as i32;
        saxis_g = (alt_xg * m) as i32;
        saxis_b = (alt_xb * m) as i32;
    }

    saxis_r = ((saxis_r as u32) << 4) as i32;
    saxis_g = ((saxis_g as u32) << 4) as i32;
    saxis_b = ((saxis_b as u32) << 4) as i32;

    let mut low_dot = i32::MAX;
    let mut high_dot = i32::MIN;

    let mut i = 0usize;
    while i < 16 {
        let dot = |j: usize| -> i32 {
            let p = &pixels[j];
            ((p.r as i32 * saxis_r + p.g as i32 * saxis_g + p.b as i32 * saxis_b) & !0xF) + j as i32
        };
        let dot0 = dot(i);
        let dot1 = dot(i + 1);
        let dot2 = dot(i + 2);
        let dot3 = dot(i + 3);

        let min_d01 = dot0.min(dot1);
        let max_d01 = dot0.max(dot1);
        let min_d23 = dot2.min(dot3);
        let max_d23 = dot2.max(dot3);

        let min_d = min_d01.min(min_d23);
        let max_d = max_d01.max(max_d23);

        low_dot = low_dot.min(min_d);
        high_dot = high_dot.max(max_d);
        i += 4;
    }
    let low_c = (low_dot & 15) as usize;
    let high_c = (high_dot & 15) as usize;

    let mut lr = mids.to_7(pixels[low_c].r as f32 * (1.0 / 255.0));
    let mut lg = mids.to_7(pixels[low_c].g as f32 * (1.0 / 255.0));
    let mut lb = mids.to_7(pixels[low_c].b as f32 * (1.0 / 255.0));
    let mut hr = mids.to_7(pixels[high_c].r as f32 * (1.0 / 255.0));
    let mut hg = mids.to_7(pixels[high_c].g as f32 * (1.0 / 255.0));
    let mut hb = mids.to_7(pixels[high_c].b as f32 * (1.0 / 255.0));

    let mut weights = eval_weights(
        pixels, lr as u32, lg as u32, lb as u32, hr as u32, hg as u32, hb as u32,
    );

    if let Some((nlr, nlg, nlb, nhr, nhg, nhb)) =
        compute_least_squares_endpoints4_rgb(pixels, &weights, mids, total_r, total_g, total_b)
    {
        lr = nlr;
        lg = nlg;
        lb = nlb;
        hr = nhr;
        hg = nhg;
        hb = nhb;
        weights = eval_weights(
            pixels, lr as u32, lg as u32, lb as u32, hr as u32, hg as u32, hb as u32,
        );
    }

    pack_bc7_mode5_rgb_block(block, lr, lg, lb, hr, hg, hb, &weights);
}

/// `to_7(lerp_8bit(a, b, s))` for the simple-block path. `lerp_8bit(a,b,s) =
/// a + mul_8bit(b-a, s)`, `mul_8bit(a,b) = (a*b + 128 + ((a*b+128)>>8)) >> 8`.
#[inline]
fn mids_to_7_from_lerp8(a: i32, b: i32, s: i32, mids: &Mode5Midpoints) -> i32 {
    let v = lerp_8bit(a, b, s);
    mids.to_7(v as f32 * (1.0 / 255.0))
}

/// Multiply two 8-bit values and divide by 255 with rounding, using the
/// reciprocal-of-255 trick `(t + (t >> 8)) >> 8` where `t = a*b + 128`.
#[inline]
fn mul_8bit(a: i32, b: i32) -> i32 {
    let t = a * b + 128;
    (t + (t >> 8)) >> 8
}

/// Interpolate from `a` toward `b` by the 8-bit fraction `s` (0..255).
#[inline]
fn lerp_8bit(a: i32, b: i32, s: i32) -> i32 {
    a + mul_8bit(b - a, s)
}

/// `chroma_filter_bc7_mode5`: rewrite the chroma of selected BC7 mode-5 blocks.
///
/// `blocks` is the packed image (one 16-byte block per `(bx, by)` in raster
/// order, row pitch == `num_blocks_x`). `endpoints` is the ETC1S endpoint
/// codebook. `grid` maps each block to its decoded endpoint index.
pub fn chroma_filter_bc7_mode5(
    grid: &EndpointGrid,
    blocks: &mut [u8],
    num_blocks_x: u32,
    num_blocks_y: u32,
    endpoints: &[Endpoint],
) {
    let mids = Mode5Midpoints::new();
    let nbx = num_blocks_x as i32;
    let nby = num_blocks_y as i32;

    for by in 0..nby {
        for bx in 0..nbx {
            let center_cocg = color5_to_cocg(&endpoints[grid.at(bx, by) as usize]);

            // Decide whether to filter: any in-bounds 8-neighbour whose chroma
            // differs by more than the threshold triggers it.
            let mut do_filter = false;
            'outer: for dy in -1..=1i32 {
                let oy = by + dy;
                if oy < 0 || oy >= nby {
                    continue;
                }
                for dx in -1..=1i32 {
                    if (dx | dy) == 0 {
                        continue;
                    }
                    let ox = bx + dx;
                    if ox < 0 || ox >= nbx {
                        continue;
                    }
                    let nearby_cocg = color5_to_cocg(&endpoints[grid.at(ox, oy) as usize]);
                    let delta_co = fabsf(nearby_cocg[0] - center_cocg[0]);
                    let delta_cg = fabsf(nearby_cocg[1] - center_cocg[1]);
                    if delta_co > CHROMA_THRESH || delta_cg > CHROMA_THRESH {
                        do_filter = true;
                        break 'outer;
                    }
                }
            }
            if !do_filter {
                continue;
            }

            let bidx = (bx + by * nbx) as usize * 16;
            let block: &mut [u8; 16] = (&mut blocks[bidx..bidx + 16]).try_into().unwrap();

            let ep = read_endpoints(block);
            let lr = bc7_7_to_8(ep[0]) as i32;
            let hr = bc7_7_to_8(ep[1]) as i32;
            let lg = bc7_7_to_8(ep[2]) as i32;
            let hg = bc7_7_to_8(ep[3]) as i32;
            let lb = bc7_7_to_8(ep[4]) as i32;
            let hb = bc7_7_to_8(ep[5]) as i32;

            // Luma of the four interpolated block colors (Y = R/4 + G/2 + B/4).
            let mut y_vals = [0.0f32; 4];
            for (i, yv) in y_vals.iter_mut().enumerate() {
                let cr = bc7_interp2(lr as u32, hr as u32, i) as i32;
                let cg = bc7_interp2(lg as u32, hg as u32, i) as i32;
                let cb = bc7_interp2(lb as u32, hb as u32, i) as i32;
                *yv = cr as f32 * 0.25 + cg as f32 * 0.5 + cb as f32 * 0.25;
            }

            let mut sel_bits = read_hi_bits(block) >> 2;

            // Per-texel luma from the selectors. Texel 0 is the anchor and
            // stores its selector in 1 bit (high bit implied 0); the rest use 2.
            let mut block_y_vals = [0.0f32; 16];
            let mut y_sum = 0.0f32;
            let mut y_sum_sq = 0.0f32;
            for (i, byv) in block_y_vals.iter_mut().enumerate() {
                let mask: u64 = if i != 0 { 3 } else { 1 };
                let sel = (sel_bits & mask) as usize;
                sel_bits >>= if i != 0 { 2 } else { 1 };
                let y = y_vals[sel];
                *byv = y;
                y_sum += y;
                y_sum_sq += y * y;
            }

            let s = 1.0f32 / 16.0;
            let y_var = (y_sum_sq * s) - squaref(y_sum * s);
            if y_var < Y_VAR_SKIP_THRESH {
                continue;
            }

            // Rebuild the block's pixels: keep each texel's luma, replace its
            // chroma with a bilinear blend of the surrounding blocks' endpoint
            // CoCg. Texel centers sit half a block off the endpoint grid, so
            // (bp - 2) >> 2 picks the left/upper grid cell and
            // (((bp + 2) & 3) + 0.5) / 4 the fraction within it.
            let mut block_to_pack = [Color32Pack::default(); 16];
            for bpy in 0..4i32 {
                let uby = by + ((bpy - 2) >> 2);
                for bpx in 0..4i32 {
                    let fx = ((((bpx + 2) & 3) as f32) + 0.5) * (1.0 / 4.0);
                    let fy = ((((bpy + 2) & 3) as f32) + 0.5) * (1.0 / 4.0);
                    let ubx = bx + ((bpx - 2) >> 2);

                    let a = get_endpoint_cocg_clamped(ubx, uby, grid, endpoints);
                    let b = get_endpoint_cocg_clamped(ubx + 1, uby, grid, endpoints);
                    let c = get_endpoint_cocg_clamped(ubx, uby + 1, grid, endpoints);
                    let d = get_endpoint_cocg_clamped(ubx + 1, uby + 1, grid, endpoints);

                    let ab = [lerp(a[0], b[0], fx), lerp(a[1], b[1], fx)];
                    let cd = [lerp(c[0], d[0], fx), lerp(c[1], d[1], fx)];
                    let f = [lerp(ab[0], cd[0], fy), lerp(ab[1], cd[1], fy)];

                    let final_ycocg = [block_y_vals[(bpx + bpy * 4) as usize], f[0], f[1]];
                    let mut final_conv = ycocg_to_rgb(final_ycocg);
                    final_conv[0] = clampf(final_conv[0], 0.0, 255.0);
                    final_conv[1] = clampf(final_conv[1], 0.0, 255.0);
                    final_conv[2] = clampf(final_conv[2], 0.0, 255.0);

                    let dst = &mut block_to_pack[(bpx + bpy * 4) as usize];
                    dst.r = (0.5 + final_conv[0]) as i32 as u8;
                    dst.g = (0.5 + final_conv[1]) as i32 as u8;
                    dst.b = (0.5 + final_conv[2]) as i32 as u8;
                }
            }

            encode_bc7_mode_5_block(block, &block_to_pack, &mids);
        }
    }
}

/// CoCg of the endpoint at `(bx, by)`, clamping out-of-range coordinates to the
/// grid edge so the bilinear blend can sample past the image border.
#[inline]
fn get_endpoint_cocg_clamped(
    bx: i32,
    by: i32,
    grid: &EndpointGrid,
    endpoints: &[Endpoint],
) -> [f32; 2] {
    color5_to_cocg(&endpoints[grid.at_clamped(bx, by) as usize])
}

/// Build an empty endpoint grid for the convert pass.
pub fn new_endpoint_grid(num_blocks_x: u32, num_blocks_y: u32) -> EndpointGrid {
    EndpointGrid::new(num_blocks_x, num_blocks_y)
}

/// Record a block's decoded endpoint index during the convert pass.
pub fn record_endpoint(grid: &mut EndpointGrid, bx: u32, by: u32, endpoint_index: u16) {
    grid.set(bx, by, endpoint_index);
}
