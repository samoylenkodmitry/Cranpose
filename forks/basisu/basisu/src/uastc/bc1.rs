//! UASTC -> BC1 (DXT1) transcode.
//!
//! Three paths, cheapest first:
//! - solid-color blocks go through the single-color match tables
//!   (`encode_bc1_solid_block`),
//! - blocks carrying the `bc1_hint0` flag (when high quality is off) scale the
//!   UASTC endpoints and weights straight to BC1,
//! - everything else unpacks to pixels and runs `encode_bc1` (PCA plus
//!   least-squares), with `bc1_hint1` seeding the selectors from the UASTC
//!   weights.
//!
//! `high_quality` (decode flag 32) disables the hint0 shortcut and doubles the
//! least-squares refinement passes.
//!
//! The per-texel loops index the 16-pixel block directly; iteration order and
//! integer rounding are load-bearing, since the conformance gate compares the
//! encoded block byte for byte.
#![allow(clippy::needless_range_loop)]

use super::bise::astc_unquant;
use super::tables::{COMPS, ENDPOINT_RANGES, PLANES, WEIGHT_BITS};
use super::unpack::{unpack_to_block, unpack_uastc, UnpackedUastcBlock};
use super::UASTC_MODE_INDEX_SOLID_COLOR;
use crate::color::Color32;
use crate::mathf::fabsf;
use crate::tables::bc1::tables as bc1_tables;

pub(crate) const C_HIGH_QUALITY: u32 = 1;
const C_USE_SELECTORS: u32 = 4;

/// UASTC weight -> BC1 selector maps, one table per weight bit width (1..=5).
/// BC1 wire selectors run 0, 2, 3, 1 along the low-to-high color ramp.
const UASTC1_TO_BC1: [u8; 2] = [0, 1];
const UASTC2_TO_BC1: [u8; 4] = [0, 2, 3, 1];
const UASTC3_TO_BC1: [u8; 8] = [0, 0, 2, 2, 3, 3, 1, 1];
const UASTC4_TO_BC1: [u8; 16] = [0, 0, 0, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 1, 1, 1];
const UASTC5_TO_BC1: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 1, 1, 1, 1, 1, 1,
];

/// Map a UASTC weight `w` to its BC1 selector, picking the lookup table for the
/// mode's `weight_bits` (1..=5). Out-of-range widths return 0.
#[inline]
fn uastc_to_bc1_weight(weight_bits: usize, w: usize) -> u32 {
    (match weight_bits {
        1 => UASTC1_TO_BC1[w],
        2 => UASTC2_TO_BC1[w],
        3 => UASTC3_TO_BC1[w],
        4 => UASTC4_TO_BC1[w],
        5 => UASTC5_TO_BC1[w],
        _ => 0,
    }) as u32
}

/// Pack already-quantized 5/6/5 channels into a BC1 565 endpoint word.
#[inline]
fn pack_unscaled_color(r: u32, g: u32, b: u32) -> u32 {
    b | (g << 5) | (r << 11)
}

/// Quantize 8-bit RGB to a 565 endpoint word, scaling each channel with the
/// caller's rounding bias and clamping to the field width.
#[inline]
fn pack_color_scaled(r: u32, g: u32, b: u32, bias: u32) -> u32 {
    let r = ((r * 31 + bias) / 255).min(31);
    let g = ((g * 63 + bias) / 255).min(63);
    let b = ((b * 31 + bias) / 255).min(31);
    b | (g << 5) | (r << 11)
}

/// Scale an 8-bit channel to 5 bits with round-to-nearest (v * 31 / 255,
/// computed without a divide); used for the BC1 red and blue endpoints.
#[inline]
fn to_5(v: u32) -> u32 {
    let v = v * 31 + 128;
    (v + (v >> 8)) >> 8
}
/// Scale an 8-bit channel to 6 bits with round-to-nearest (BC1 green endpoint).
#[inline]
fn to_6(v: u32) -> u32 {
    let v = v * 63 + 128;
    (v + (v >> 8)) >> 8
}

/// A BC1 block under construction: low/high 565 endpoints + 4 selector bytes.
struct Dxt1Block {
    low: u16,
    high: u16,
    sels: [u8; 4],
}

impl Dxt1Block {
    /// Zeroed block: both endpoints 0, all selectors 0.
    fn new() -> Self {
        Self {
            low: 0,
            high: 0,
            sels: [0; 4],
        }
    }
    /// Serialize to the 8-byte BC1 layout: low endpoint, high endpoint (both
    /// little-endian 565), then the four selector bytes.
    fn to_bytes(&self) -> [u8; 8] {
        [
            self.low as u8,
            (self.low >> 8) as u8,
            self.high as u8,
            (self.high >> 8) as u8,
            self.sels[0],
            self.sels[1],
            self.sels[2],
            self.sels[3],
        ]
    }
}

/// Encode a solid-color BC1 block from the single-color match tables. Always
/// emits the 4-color endpoint ordering (low word > high word), never the
/// 3-color-plus-punchthrough mode, so the block decodes fully opaque.
fn encode_bc1_solid_block(fr: usize, fg: usize, fb: usize) -> [u8; 8] {
    let t = bc1_tables();
    // 0xAA packs wire selector 2 (the two-thirds interpolant the match tables
    // were solved for) into all 16 texels.
    let mut mask = 0xAAu32;
    let mut max16 = ((t.match5_equals_1[fr].m_hi as u32) << 11)
        | ((t.match6_equals_1[fg].m_hi as u32) << 5)
        | t.match5_equals_1[fb].m_hi as u32;
    let mut min16 = ((t.match5_equals_1[fr].m_lo as u32) << 11)
        | ((t.match6_equals_1[fg].m_lo as u32) << 5)
        | t.match5_equals_1[fb].m_lo as u32;

    if min16 == max16 {
        // identical endpoints would decode as 3-color mode; nudge one apart and
        // point every texel at whichever endpoint kept the exact color.
        mask = 0;
        if min16 > 0 {
            min16 -= 1;
        } else {
            max16 = 1;
            min16 = 0;
            mask = 0x55;
        }
    }
    if max16 < min16 {
        core::mem::swap(&mut max16, &mut min16);
        mask ^= 0x55;
    }
    let m = mask as u8;
    Dxt1Block {
        low: max16 as u16,
        high: min16 as u16,
        sels: [m, m, m, m],
    }
    .to_bytes()
}

/// `bc1_find_sels`: pick the 4-color BC1 selector for each of the 16 pixels by
/// projecting onto the endpoint axis and thresholding against the three
/// midpoints between the four interpolated colors.
#[allow(clippy::too_many_arguments)]
fn bc1_find_sels(
    pixels: &[Color32; 16],
    lr: u32,
    lg: u32,
    lb: u32,
    hr: u32,
    hg: u32,
    hb: u32,
    sels: &mut [u8; 16],
) {
    let mut block_r = [0i32; 4];
    let mut block_g = [0i32; 4];
    let mut block_b = [0i32; 4];
    block_r[0] = ((lr << 3) | (lr >> 2)) as i32;
    block_g[0] = ((lg << 2) | (lg >> 4)) as i32;
    block_b[0] = ((lb << 3) | (lb >> 2)) as i32;
    block_r[3] = ((hr << 3) | (hr >> 2)) as i32;
    block_g[3] = ((hg << 2) | (hg >> 4)) as i32;
    block_b[3] = ((hb << 3) | (hb >> 2)) as i32;
    block_r[1] = (block_r[0] * 2 + block_r[3]) / 3;
    block_g[1] = (block_g[0] * 2 + block_g[3]) / 3;
    block_b[1] = (block_b[0] * 2 + block_b[3]) / 3;
    block_r[2] = (block_r[3] * 2 + block_r[0]) / 3;
    block_g[2] = (block_g[3] * 2 + block_g[0]) / 3;
    block_b[2] = (block_b[3] * 2 + block_b[0]) / 3;

    let mut ar = block_r[3] - block_r[0];
    let mut ag = block_g[3] - block_g[0];
    let mut ab = block_b[3] - block_b[0];

    let mut dots = [0i32; 4];
    for i in 0..4 {
        dots[i] = block_r[i] * ar + block_g[i] * ag + block_b[i] * ab;
    }
    let t0 = dots[0] + dots[1];
    let t1 = dots[1] + dots[2];
    let t2 = dots[2] + dots[3];

    // the thresholds above are sums of adjacent dots (twice the midpoint
    // projections), so the axis is doubled to keep the per-pixel dot on the
    // same scale.
    ar *= 2;
    ag *= 2;
    ab *= 2;

    const S_SELS: [u8; 4] = [3, 2, 1, 0];
    for i in 0..16 {
        let d = pixels[i].r() as i32 * ar + pixels[i].g() as i32 * ag + pixels[i].b() as i32 * ab;
        let idx = (d <= t0) as usize + (d < t1) as usize + (d < t2) as usize;
        sels[i] = S_SELS[idx];
    }
}

/// Solve the 2x2 least-squares system for the two RGB endpoints given the
/// current selectors. Returns the float endpoint pair `(xl, xh)`, or `None`
/// when the system is singular.
fn compute_least_squares_endpoints_rgb(
    pixels: &[Color32; 16],
    sels: &[u8; 16],
) -> Option<([f32; 3], [f32; 3])> {
    // per-selector entries of the symmetric 2x2 normal matrix, byte-packed as
    // z00 in bits 16..24, z10 in bits 8..16, z11 in bits 0..8, so a single add
    // per texel accumulates all three sums. 16 texels peak at 16 * 9 = 144,
    // which fits each byte lane without carries.
    const WEIGHT_VALS: [u32; 4] = [0x000009, 0x010204, 0x040201, 0x090000];

    let (mut ut_r, mut ut_g, mut ut_b) = (0u32, 0u32, 0u32);
    let (mut uq00_r, mut uq00_g, mut uq00_b) = (0u32, 0u32, 0u32);
    let mut weight_accum = 0u32;
    for i in 0..16 {
        let r = pixels[i].c[0] as u32;
        let g = pixels[i].c[1] as u32;
        let b = pixels[i].c[2] as u32;
        let sel = sels[i] as u32;
        ut_r += r;
        ut_g += g;
        ut_b += b;
        weight_accum += WEIGHT_VALS[sel as usize];
        uq00_r += sel * r;
        uq00_g += sel * g;
        uq00_b += sel * b;
    }

    let q00_r = uq00_r as f32;
    let q00_g = uq00_g as f32;
    let q00_b = uq00_b as f32;
    let t_r = ut_r as f32;
    let t_g = ut_g as f32;
    let t_b = ut_b as f32;

    let q10_r = t_r * 3.0 - q00_r;
    let q10_g = t_g * 3.0 - q00_g;
    let q10_b = t_b * 3.0 - q00_b;

    let z00 = ((weight_accum >> 16) & 0xFF) as f32;
    let z10 = ((weight_accum >> 8) & 0xFF) as f32;
    let z11 = (weight_accum & 0xFF) as f32;
    let z01 = z10;

    let mut det = z00 * z11 - z01 * z10;
    if fabsf(det) < 1e-8 {
        return None;
    }
    det = 3.0 / det;

    let iz00 = z11 * det;
    let iz01 = -z01 * det;
    let iz10 = -z10 * det;
    let iz11 = z00 * det;

    let mut xl = [0f32; 3];
    let mut xh = [0f32; 3];
    xl[0] = iz00 * q00_r + iz01 * q10_r;
    xh[0] = iz10 * q00_r + iz11 * q10_r;
    xl[1] = iz00 * q00_g + iz01 * q10_g;
    xh[1] = iz10 * q00_g + iz11 * q10_g;
    xl[2] = iz00 * q00_b + iz01 * q10_b;
    xh[2] = iz10 * q00_b + iz11 * q10_b;

    // if a channel's solution escapes [0, 255] and that channel is actually
    // constant across the block, pin both endpoints to the constant value.
    for c in 0..3 {
        if xl[c] < 0.0 || xh[c] > 255.0 {
            let mut lo_v = u32::MAX;
            let mut hi_v = 0u32;
            for i in 0..16 {
                lo_v = lo_v.min(pixels[i].c[c] as u32);
                hi_v = hi_v.max(pixels[i].c[c] as u32);
            }
            if lo_v == hi_v {
                xl[c] = lo_v as f32;
                xh[c] = hi_v as f32;
            }
        }
    }
    Some((xl, xh))
}

/// Scale a float channel, round to nearest via the `+0.5` truncation, and clamp
/// to `[lo, hi]` inclusive. Used to quantize the least-squares endpoints to
/// 5/6-bit.
#[inline]
fn clampf_to_int(v: f32, scale: f32, lo: i32, hi: i32) -> u32 {
    ((v * scale + 0.5) as i32).clamp(lo, hi) as u32
}

/// Encode 16 RGBA pixels as a BC1 block. `pre_sels`, if `Some`, are
/// caller-supplied packed wire selectors (required with `C_USE_SELECTORS`, the
/// hint1 path); otherwise a PCA power iteration seeds the endpoints and
/// selectors. Either way the endpoints are then refined by least squares.
pub(crate) fn encode_bc1(
    pixels: &[Color32; 16],
    flags: u32,
    pre_sels: Option<&[u8; 4]>,
) -> [u8; 8] {
    let mut avg_r = -1i32;
    let mut avg_g = 0i32;
    let mut avg_b = 0i32;
    let mut lr = 0u32;
    let mut lg = 0u32;
    let mut lb = 0u32;
    let mut hr = 0u32;
    let mut hg = 0u32;
    let mut hb = 0u32;
    let mut sels = [0u8; 16];

    let use_sels = (flags & C_USE_SELECTORS) != 0;
    if use_sels {
        let pre = pre_sels.expect("use_sels requires selectors");
        let s = pre[0] as u32
            | ((pre[1] as u32) << 8)
            | ((pre[2] as u32) << 16)
            | ((pre[3] as u32) << 24);
        // map wire selectors (ramp order 0, 2, 3, 1) to the linear 0..3 ramp
        // order the least-squares solver works in.
        const SEL_TRAN: [u8; 4] = [0, 3, 1, 2];
        for i in 0..16 {
            sels[i] = SEL_TRAN[((s >> (i * 2)) & 3) as usize];
        }
    } else {
        let fr = pixels[0].r() as i32;
        let fg = pixels[0].g() as i32;
        let fb = pixels[0].b() as i32;

        let mut j = 1;
        while j < 16 {
            if pixels[j].r() as i32 != fr
                || pixels[j].g() as i32 != fg
                || pixels[j].b() as i32 != fb
            {
                break;
            }
            j += 1;
        }
        if j == 16 {
            return encode_bc1_solid_block(fr as usize, fg as usize, fb as usize);
        }

        let mut total_r = fr;
        let mut total_g = fg;
        let mut total_b = fb;
        let mut max_r = fr;
        let mut max_g = fg;
        let mut max_b = fb;
        let mut min_r = fr;
        let mut min_g = fg;
        let mut min_b = fb;
        for i in 1..16 {
            let r = pixels[i].r() as i32;
            let g = pixels[i].g() as i32;
            let b = pixels[i].b() as i32;
            max_r = max_r.max(r);
            max_g = max_g.max(g);
            max_b = max_b.max(b);
            min_r = min_r.min(r);
            min_g = min_g.min(g);
            min_b = min_b.min(b);
            total_r += r;
            total_g += g;
            total_b += b;
        }

        avg_r = (total_r + 8) >> 4;
        avg_g = (total_g + 8) >> 4;
        avg_b = (total_b + 8) >> 4;

        let mut icov = [0i32; 6];
        for i in 0..16 {
            let r = pixels[i].r() as i32 - avg_r;
            let g = pixels[i].g() as i32 - avg_g;
            let b = pixels[i].b() as i32 - avg_b;
            icov[0] += r * r;
            icov[1] += r * g;
            icov[2] += r * b;
            icov[3] += g * g;
            icov[4] += g * b;
            icov[5] += b * b;
        }
        let mut cov = [0f32; 6];
        for i in 0..6 {
            cov[i] = icov[i] as f32 * (1.0 / 255.0);
        }

        // four power-iteration rounds push the channel-range seed toward the
        // covariance matrix's principal axis.
        let mut xr = (max_r - min_r) as f32;
        let mut xg = (max_g - min_g) as f32;
        let mut xb = (max_b - min_b) as f32;
        for _ in 0..4 {
            let r = xr * cov[0] + xg * cov[1] + xb * cov[2];
            let g = xr * cov[1] + xg * cov[3] + xb * cov[4];
            let b = xr * cov[2] + xg * cov[4] + xb * cov[5];
            xr = r;
            xg = g;
            xb = b;
        }

        // when the iterated axis is too small to normalize reliably, fall back
        // to a Rec. 601 luma axis (0.299, 0.587, 0.114 in 10-bit fixed point).
        let k = fabsf(xr).max(fabsf(xg)).max(fabsf(xb));
        let mut saxis_r = 306i32;
        let mut saxis_g = 601i32;
        let mut saxis_b = 117i32;
        if k >= 2.0 {
            let m = 1024.0 / k;
            saxis_r = (xr * m) as i32;
            saxis_g = (xg * m) as i32;
            saxis_b = (xb * m) as i32;
        }

        let mut low_dot = i32::MAX;
        let mut high_dot = i32::MIN;
        let mut low_c = 0usize;
        let mut high_c = 0usize;
        for i in 0..16 {
            let dot = pixels[i].r() as i32 * saxis_r
                + pixels[i].g() as i32 * saxis_g
                + pixels[i].b() as i32 * saxis_b;
            if dot < low_dot {
                low_dot = dot;
                low_c = i;
            }
            if dot > high_dot {
                high_dot = dot;
                high_c = i;
            }
        }

        lr = to_5(pixels[low_c].r() as u32);
        lg = to_6(pixels[low_c].g() as u32);
        lb = to_5(pixels[low_c].b() as u32);
        hr = to_5(pixels[high_c].r() as u32);
        hg = to_6(pixels[high_c].g() as u32);
        hb = to_5(pixels[high_c].b() as u32);

        bc1_find_sels(pixels, lr, lg, lb, hr, hg, hb, &mut sels);
    }

    let total_ls_passes = if (flags & C_HIGH_QUALITY) != 0 { 2 } else { 1 };
    let t = bc1_tables();
    for _ in 0..total_ls_passes {
        match compute_least_squares_endpoints_rgb(pixels, &sels) {
            None => {
                // singular system: encode the block average as a single color
                // via the match tables. avg_r is -1 when the pre-selector path
                // skipped the averaging pass, so compute it here on demand.
                if avg_r < 0 {
                    let mut total_r = 0i32;
                    let mut total_g = 0i32;
                    let mut total_b = 0i32;
                    for i in 0..16 {
                        total_r += pixels[i].r() as i32;
                        total_g += pixels[i].g() as i32;
                        total_b += pixels[i].b() as i32;
                    }
                    avg_r = (total_r + 8) >> 4;
                    avg_g = (total_g + 8) >> 4;
                    avg_b = (total_b + 8) >> 4;
                }
                lr = t.match5_equals_1[avg_r as usize].m_hi as u32;
                lg = t.match6_equals_1[avg_g as usize].m_hi as u32;
                lb = t.match5_equals_1[avg_b as usize].m_hi as u32;
                hr = t.match5_equals_1[avg_r as usize].m_lo as u32;
                hg = t.match6_equals_1[avg_g as usize].m_lo as u32;
                hb = t.match5_equals_1[avg_b as usize].m_lo as u32;
            }
            Some((xl, xh)) => {
                lr = clampf_to_int(xl[0], 31.0 / 255.0, 0, 31);
                lg = clampf_to_int(xl[1], 63.0 / 255.0, 0, 63);
                lb = clampf_to_int(xl[2], 31.0 / 255.0, 0, 31);
                hr = clampf_to_int(xh[0], 31.0 / 255.0, 0, 31);
                hg = clampf_to_int(xh[1], 63.0 / 255.0, 0, 63);
                hb = clampf_to_int(xh[2], 31.0 / 255.0, 0, 31);
            }
        }
        bc1_find_sels(pixels, lr, lg, lb, hr, hg, hb, &mut sels);
    }

    let mut lc16 = pack_unscaled_color(lr, lg, lb);
    let mut hc16 = pack_unscaled_color(hr, hg, hb);

    let mut blk = Dxt1Block::new();
    if lc16 == hc16 {
        let mut mask = 0u8;
        if hc16 > 0 {
            hc16 -= 1;
        } else {
            hc16 = 0;
            lc16 = 1;
            mask = 0x55;
        }
        blk.low = lc16 as u16;
        blk.high = hc16 as u16;
        blk.sels = [mask, mask, mask, mask];
    } else {
        let mut invert_mask = 0u8;
        if lc16 < hc16 {
            core::mem::swap(&mut lc16, &mut hc16);
            invert_mask = 0x55;
        }
        blk.low = lc16 as u16;
        blk.high = hc16 as u16;

        let mut packed_sels = 0u32;
        // linear 0..3 ramp order back to BC1 wire selector order.
        const SEL_TRANS: [u8; 4] = [0, 2, 3, 1];
        for i in 0..16 {
            packed_sels |= (SEL_TRANS[sels[i] as usize] as u32) << (i * 2);
        }
        blk.sels[0] = (packed_sels as u8) ^ invert_mask;
        blk.sels[1] = ((packed_sels >> 8) as u8) ^ invert_mask;
        blk.sels[2] = ((packed_sels >> 16) as u8) ^ invert_mask;
        blk.sels[3] = ((packed_sels >> 24) as u8) ^ invert_mask;
    }
    blk.to_bytes()
}

/// The bc1_hint0 fast path: quantize the first subset's unquantized endpoints
/// straight to 565 and map the first plane's weights to BC1 selectors,
/// skipping the pixel unpack and the encoder entirely.
fn transcode_uastc_to_bc1_hint0(u: &UnpackedUastcBlock) -> [u8; 8] {
    let mode = u.mode as usize;
    let endpoint_range = ENDPOINT_RANGES[mode] as usize;
    let total_comps = COMPS[mode] as u32;
    let unq = astc_unquant();
    let e = &u.astc.endpoints;
    let uq = |i: usize| unq[endpoint_range][e[i] as usize].m_unquant as u32;

    let mut blk = Dxt1Block::new();
    if total_comps == 2 {
        let l = uq(0);
        let h = uq(1);
        blk.low = pack_color_scaled(l, l, l, 127) as u16;
        blk.high = pack_color_scaled(h, h, h, 127) as u16;
    } else {
        blk.low = pack_color_scaled(uq(0), uq(2), uq(4), 127) as u16;
        blk.high = pack_color_scaled(uq(1), uq(3), uq(5), 127) as u16;
    }

    if blk.low == blk.high {
        let mut lc16 = blk.low;
        let mut hc16 = blk.high;
        let mut mask = 0u8;
        if hc16 > 0 {
            hc16 -= 1;
        } else {
            hc16 = 0;
            lc16 = 1;
            mask = 0x55;
        }
        blk.low = lc16;
        blk.high = hc16;
        blk.sels = [mask, mask, mask, mask];
    } else {
        let mut invert = false;
        if blk.low < blk.high {
            core::mem::swap(&mut blk.low, &mut blk.high);
            invert = true;
        }
        let weight_bits = WEIGHT_BITS[mode] as usize;
        let plane_shift = (PLANES[mode] - 1) as usize;
        let w = &u.astc.weights;
        let mut sels = 0u32;
        for i in (0..16).rev() {
            let mut s = uastc_to_bc1_weight(weight_bits, w[i << plane_shift] as usize);
            if invert {
                s ^= 1;
            }
            sels = (sels << 2) | s;
        }
        blk.sels[0] = (sels & 0xFF) as u8;
        blk.sels[1] = ((sels >> 8) & 0xFF) as u8;
        blk.sels[2] = ((sels >> 16) & 0xFF) as u8;
        blk.sels[3] = ((sels >> 24) & 0xFF) as u8;
    }
    blk.to_bytes()
}

/// The bc1_hint1 path: derive the BC1 selectors from the UASTC weights and let
/// `encode_bc1` fit only the endpoints around them by least squares.
fn transcode_uastc_to_bc1_hint1(
    u: &UnpackedUastcBlock,
    pixels: &[Color32; 16],
    high_quality: bool,
) -> [u8; 8] {
    let mode = u.mode as usize;
    let weight_bits = WEIGHT_BITS[mode] as usize;
    let plane_shift = (PLANES[mode] - 1) as usize;
    let w = &u.astc.weights;

    // only selectors are derived here (from the first plane's weight of each
    // texel); encode_bc1 computes the endpoints from them by least squares.
    let mut sels = 0u32;
    for i in (0..16).rev() {
        sels <<= 2;
        sels |= uastc_to_bc1_weight(weight_bits, w[i << plane_shift] as usize);
    }
    let pre_sels = [
        (sels & 0xFF) as u8,
        ((sels >> 8) & 0xFF) as u8,
        ((sels >> 16) & 0xFF) as u8,
        ((sels >> 24) & 0xFF) as u8,
    ];
    let flags = if high_quality { C_HIGH_QUALITY } else { 0 } | C_USE_SELECTORS;
    encode_bc1(pixels, flags, Some(&pre_sels))
}

/// Transcode one 16-byte UASTC block to an 8-byte BC1 block, picking the
/// cheapest path the block's mode and hints allow. `None` when the block fails
/// to decode.
pub fn transcode_uastc_to_bc1(src: &[u8; 16], high_quality: bool) -> Option<[u8; 8]> {
    let u = unpack_to_block(src, false, true)?;

    if u.mode == UASTC_MODE_INDEX_SOLID_COLOR {
        return Some(encode_bc1_solid_block(
            u.solid_color.r() as usize,
            u.solid_color.g() as usize,
            u.solid_color.b() as usize,
        ));
    }

    if !high_quality && u.bc1_hint0 {
        return Some(transcode_uastc_to_bc1_hint0(&u));
    }

    let pixels = unpack_uastc(src, false)?;
    if u.bc1_hint1 {
        Some(transcode_uastc_to_bc1_hint1(&u, &pixels, high_quality))
    } else {
        let flags = if high_quality { C_HIGH_QUALITY } else { 0 };
        Some(encode_bc1(&pixels, flags, None))
    }
}
