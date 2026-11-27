//! UASTC -> BC4 (DXT5A / single-channel) transcode
//! (`transcode_uastc_to_bc4`), built on the generic `encode_bc4` /
//! `write_bc4_solid_block` block writers.
//!
//! A BC4 (`dxt5a_block`) is 8 bytes: two 8-bit endpoints (`max`, `min`) followed
//! by sixteen 3-bit selectors packed LSB-first in raster order. `encode_bc4` is
//! also reused for the alpha half of BC3 and both halves of BC5.

use super::unpack::unpack_uastc;

/// `write_bc4_solid_block`: a constant-`a` BC4 block (both endpoints `a`, all
/// selectors 0).
pub fn write_bc4_solid_block(dst: &mut [u8; 8], a: u8) {
    dst[0] = a;
    dst[1] = a;
    dst[2..8].fill(0);
}

/// `encode_bc4`: pack 16 single-channel values (`pixels[i]`) into an 8-byte BC4
/// block. `pixels` are in raster order (row-major).
pub fn encode_bc4(dst: &mut [u8; 8], pixels: &[u8; 16]) {
    // Overall min/max via four interleaved (index mod 4) accumulator lanes,
    // reduced at the end. The independent lanes break the sequential dependency
    // chain so the comparisons can be scheduled or vectorized; min/max is
    // order-independent, so the result matches a single straight pass.
    let (mut min0, mut max0) = (pixels[0] as u32, pixels[0] as u32);
    let (mut min1, mut max1) = (pixels[1] as u32, pixels[1] as u32);
    let (mut min2, mut max2) = (pixels[2] as u32, pixels[2] as u32);
    let (mut min3, mut max3) = (pixels[3] as u32, pixels[3] as u32);
    let acc = |i: usize, mn: &mut u32, mx: &mut u32| {
        let v = pixels[i] as u32;
        *mn = (*mn).min(v);
        *mx = (*mx).max(v);
    };
    acc(4, &mut min0, &mut max0);
    acc(5, &mut min1, &mut max1);
    acc(6, &mut min2, &mut max2);
    acc(7, &mut min3, &mut max3);
    acc(8, &mut min0, &mut max0);
    acc(9, &mut min1, &mut max1);
    acc(10, &mut min2, &mut max2);
    acc(11, &mut min3, &mut max3);
    acc(12, &mut min0, &mut max0);
    acc(13, &mut min1, &mut max1);
    acc(14, &mut min2, &mut max2);
    acc(15, &mut min3, &mut max3);

    let min_v = min0.min(min1).min(min2).min(min3);
    let max_v = max0.max(max1).max(max2).max(max3);

    dst[0] = max_v as u8;
    dst[1] = min_v as u8;

    if max_v == min_v {
        dst[2..8].fill(0);
        return;
    }

    let delta = (max_v - min_v) as i32;

    // Midpoint thresholds between the 8 reconstructed values, scaled by 14
    // (levels step by delta/7, and midpoints halve that spacing again). `bias`
    // folds the -min_v offset and a +4 rounding term into each pixel's scaled
    // value.
    let t0 = delta * 13;
    let t1 = delta * 11;
    let t2 = delta * 9;
    let t3 = delta * 7;
    let t4 = delta * 5;
    let t5 = delta * 3;
    let t6 = delta;
    let bias = 4 - min_v as i32 * 14;

    // Map the level index (0 = nearest min, 7 = nearest max) to the BC4
    // selector code (0 decodes the max endpoint, 1 the min, 2..7 interpolants
    // descending from max to min), pre-shifted 3 bits per pixel lane so the
    // four lanes OR together.
    const S_TRAN0: [u64; 8] = [1, 7, 6, 5, 4, 3, 2, 0];
    const S_TRAN1: [u64; 8] = [1 << 3, 7 << 3, 6 << 3, 5 << 3, 4 << 3, 3 << 3, 2 << 3, 0];
    const S_TRAN2: [u64; 8] = [1 << 6, 7 << 6, 6 << 6, 5 << 6, 4 << 6, 3 << 6, 2 << 6, 0];
    const S_TRAN3: [u64; 8] = [1 << 9, 7 << 9, 6 << 9, 5 << 9, 4 << 9, 3 << 9, 2 << 9, 0];

    // Count the midpoint thresholds `v` passes: the level index, 0..=7.
    #[inline]
    fn sel(v: i32, t: &[i32; 7]) -> usize {
        ((v >= t[0]) as usize)
            + (v >= t[1]) as usize
            + (v >= t[2]) as usize
            + (v >= t[3]) as usize
            + (v >= t[4]) as usize
            + (v >= t[5]) as usize
            + (v >= t[6]) as usize
    }
    let t = [t0, t1, t2, t3, t4, t5, t6];

    let mut a0: u64 = 0;
    let mut a1: u64 = 0;
    let mut a2: u64 = 0;
    let mut a3: u64 = 0;
    for group in 0..4 {
        let base = group * 4;
        let v0 = pixels[base] as i32 * 14 + bias;
        let v1 = pixels[base + 1] as i32 * 14 + bias;
        let v2 = pixels[base + 2] as i32 * 14 + bias;
        let v3 = pixels[base + 3] as i32 * 14 + bias;
        let shift = (group * 12) as u32;
        a0 |= S_TRAN0[sel(v0, &t)] << shift;
        a1 |= S_TRAN1[sel(v1, &t)] << shift;
        a2 |= S_TRAN2[sel(v2, &t)] << shift;
        a3 |= S_TRAN3[sel(v3, &t)] << shift;
    }

    let f = a0 | a1 | a2 | a3;
    dst[2] = f as u8;
    dst[3] = (f >> 8) as u8;
    dst[4] = (f >> 16) as u8;
    dst[5] = (f >> 24) as u8;
    dst[6] = (f >> 32) as u8;
    dst[7] = (f >> 40) as u8;
}

/// `transcode_uastc_to_bc4`: decode a UASTC block to pixels and BC4-encode a
/// single channel (`chan0`; 0 selects R). `high_quality` is accepted for
/// signature parity with the other block transcoders but unused: BC4 output
/// does not vary with the quality flag.
pub fn transcode_uastc_to_bc4(
    src: &[u8; 16],
    _high_quality: bool,
    chan0: usize,
) -> Option<[u8; 8]> {
    let px = unpack_uastc(src, false)?;
    let mut chan = [0u8; 16];
    for (i, p) in px.iter().enumerate() {
        chan[i] = p.c[chan0];
    }
    let mut out = [0u8; 8];
    encode_bc4(&mut out, &chan);
    Some(out)
}
