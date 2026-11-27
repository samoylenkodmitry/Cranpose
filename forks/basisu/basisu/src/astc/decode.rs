//! Logical ASTC block decode to texels: CEM endpoint decode, partition
//! selection, weight upsampling, and the four output modes (LDR8 and SRGB8,
//! both 8-bit RGBA; HDR16, half-float RGBA; and RGB9E5). Block-size-generic
//! (4x4 through 12x12): the UASTC HDR codec decodes 4x4 blocks and the raw
//! ASTC sources decode their own footprint.

use super::dequant::dequant_tables;
use super::half::{half_from_unorm16, is_half_inf_or_nan, qlog16_to_half};
use super::rgb9e5::{pack_rgb9e5, pack_rgb9e5_hdr_astc, pack_rgb9e5_ldr_astc};
use super::unpack::{ise_levels, LogAstcBlock, MAX_ENDPOINTS};

/// The largest block footprint (12x12), sizing every per-texel scratch array.
pub const MAX_BLOCK_TEXELS: usize = 144;

/// Whether a CEM is one of the LDR endpoint modes.
fn is_cem_ldr(cem: u32) -> bool {
    matches!(cem, 0 | 1 | 4 | 5 | 6 | 8 | 9 | 10 | 12 | 13)
}

/// BISE values a CEM's endpoints occupy (`get_num_cem_values`).
#[inline]
fn num_cem_values(cem: u32) -> u32 {
    2 + 2 * (cem >> 2)
}

/// `weight_interpolate`: lerp two 16-bit-expanded endpoints by a [0,64]
/// weight.
#[inline]
fn weight_interpolate(l: i32, h: i32, w: i32) -> i32 {
    (l * (64 - w) + h * w + 32) >> 6
}

/// The ASTC partition hash (`hash52`), uint32 wrapping arithmetic.
fn hash52(v: u32) -> u32 {
    let mut p = v;
    p ^= p >> 15;
    p = p.wrapping_sub(p << 17);
    p = p.wrapping_add(p << 7);
    p = p.wrapping_add(p << 4);
    p ^= p >> 5;
    p = p.wrapping_add(p << 16);
    p ^= p >> 7;
    p ^= p >> 3;
    p ^= p << 6;
    p ^= p >> 17;
    p
}

/// The texel partition (subset) index for a block coordinate
/// (`compute_texel_partition`). `small_block` doubles the coordinates; a 4x4
/// block (16 texels < 31) is always small.
pub fn compute_texel_partition(
    seed_in: u32,
    x_in: u32,
    y_in: u32,
    z_in: u32,
    num_partitions: u32,
    small_block: bool,
) -> u32 {
    let x = if small_block { x_in << 1 } else { x_in };
    let y = if small_block { y_in << 1 } else { y_in };
    let z = if small_block { z_in << 1 } else { z_in };
    let seed = seed_in + 1024 * (num_partitions - 1);
    let rnum = hash52(seed);

    let mut s = [
        (rnum & 0xF) as u8,
        ((rnum >> 4) & 0xF) as u8,
        ((rnum >> 8) & 0xF) as u8,
        ((rnum >> 12) & 0xF) as u8,
        ((rnum >> 16) & 0xF) as u8,
        ((rnum >> 20) & 0xF) as u8,
        ((rnum >> 24) & 0xF) as u8,
        ((rnum >> 28) & 0xF) as u8,
        ((rnum >> 18) & 0xF) as u8,
        ((rnum >> 22) & 0xF) as u8,
        ((rnum >> 26) & 0xF) as u8,
        (rnum.rotate_left(2) & 0xF) as u8,
    ];
    for v in s.iter_mut() {
        *v = v.wrapping_mul(*v);
    }

    let sh_a = if seed & 2 != 0 { 4 } else { 5 };
    let sh_b = if num_partitions == 3 { 6 } else { 5 };
    let sh1 = if seed & 1 != 0 { sh_a } else { sh_b };
    let sh2 = if seed & 1 != 0 { sh_b } else { sh_a };
    let sh3 = if seed & 0x10 != 0 { sh1 } else { sh2 };

    // Seeds 1-8 alternate sh1/sh2; seeds 9-12 use sh3.
    for (i, v) in s.iter_mut().enumerate() {
        let sh = match i {
            0 | 2 | 4 | 6 => sh1,
            1 | 3 | 5 | 7 => sh2,
            _ => sh3,
        };
        *v >>= sh;
    }
    let s = s.map(u32::from);

    let a = 0x3F
        & (s[0].wrapping_mul(x))
            .wrapping_add(s[1].wrapping_mul(y))
            .wrapping_add(s[10].wrapping_mul(z))
            .wrapping_add(rnum >> 14);
    let b = 0x3F
        & (s[2].wrapping_mul(x))
            .wrapping_add(s[3].wrapping_mul(y))
            .wrapping_add(s[11].wrapping_mul(z))
            .wrapping_add(rnum >> 10);
    let c = if num_partitions >= 3 {
        0x3F & (s[4].wrapping_mul(x))
            .wrapping_add(s[5].wrapping_mul(y))
            .wrapping_add(s[8].wrapping_mul(z))
            .wrapping_add(rnum >> 6)
    } else {
        0
    };
    let d = if num_partitions >= 4 {
        0x3F & (s[6].wrapping_mul(x))
            .wrapping_add(s[7].wrapping_mul(y))
            .wrapping_add(s[9].wrapping_mul(z))
            .wrapping_add(rnum >> 2)
    } else {
        0
    };

    if a >= b && a >= c && a >= d {
        0
    } else if b >= c && b >= d {
        1
    } else if c >= d {
        2
    } else {
        3
    }
}

/// `bit_transfer_signed`: move `a`'s low bit into `b` and sign-extend `a` to
/// six bits, per the base-plus-offset CEM decodes.
fn bit_transfer_signed(a: &mut i32, b: &mut i32) {
    *b >>= 1;
    *b |= *a & 0x80;
    *a >>= 1;
    *a &= 0x3F;
    if *a & 0x20 != 0 {
        *a -= 0x40;
    }
}

/// `blue_contract`: halve red and green toward blue.
fn blue_contract(r: i32, g: i32, b: i32, a: i32) -> [i32; 4] {
    [(r + b) >> 1, (g + b) >> 1, b, a]
}

/// Sign-extend the low `num_src_bits` of `src`.
fn sign_extend(src: i32, num_src_bits: u32) -> i32 {
    if src & (1 << (num_src_bits - 1)) != 0 {
        src | !((1 << num_src_bits) - 1)
    } else {
        src & ((1 << num_src_bits) - 1)
    }
}

/// Clamp `a` into the inclusive range `[l, h]`.
#[inline]
fn clamp(a: i32, l: i32, h: i32) -> i32 {
    a.clamp(l, h)
}

/// Decode one subset's endpoints from its dequantized [0,255] values `e`.
/// Returns `[comp][low/high]`: LDR CEMs produce [0,255] components, HDR CEMs
/// produce qlog12 [0,0xFFF] components (with alpha `0x780` = 1.0 unless the
/// CEM carries alpha). The CEM 7 and 11 arms double as the BC6H qlog12
/// endpoint decode, which the bc6h module reuses from here.
pub(crate) fn decode_endpoint(cem: u32, e: &[u8]) -> [[i32; 2]; 4] {
    let v0 = e[0] as i32;
    let v1 = e[1] as i32;
    let mut out = [[0i32; 2]; 4];

    match cem {
        // CEM_LDR_LUM_DIRECT
        0 => {
            for ep in out.iter_mut().take(3) {
                *ep = [v0, v1];
            }
            out[3] = [0xFF, 0xFF];
        }
        // CEM_LDR_LUM_BASE_PLUS_OFS
        1 => {
            let l0 = (v0 >> 2) | (v1 & 0xC0);
            let l1 = (l0 + (v1 & 0x3F)).min(0xFF);
            for ep in out.iter_mut().take(3) {
                *ep = [l0, l1];
            }
            out[3] = [0xFF, 0xFF];
        }
        // CEM_HDR_LUM_LARGE_RANGE
        2 => {
            let (y0, y1) = if v1 >= v0 {
                (v0 << 4, v1 << 4)
            } else {
                ((v1 << 4) + 8, (v0 << 4) - 8)
            };
            for ep in out.iter_mut().take(3) {
                *ep = [y0, y1];
            }
            out[3] = [0x780, 0x780];
        }
        // CEM_HDR_LUM_SMALL_RANGE
        3 => {
            let (y0, d) = if v0 & 0x80 != 0 {
                (((v1 & 0xE0) << 4) | ((v0 & 0x7F) << 2), (v1 & 0x1F) << 2)
            } else {
                (((v1 & 0xF0) << 4) | ((v0 & 0x7F) << 1), (v1 & 0x0F) << 1)
            };
            let y1 = (y0 + d).min(0xFFF);
            for ep in out.iter_mut().take(3) {
                *ep = [y0, y1];
            }
            out[3] = [0x780, 0x780];
        }
        // CEM_LDR_LUM_ALPHA_DIRECT
        4 => {
            let (v2, v3) = (e[2] as i32, e[3] as i32);
            for ep in out.iter_mut().take(3) {
                *ep = [v0, v1];
            }
            out[3] = [v2, v3];
        }
        // CEM_LDR_LUM_ALPHA_BASE_PLUS_OFS
        5 => {
            let (mut v0, mut v1) = (v0, v1);
            let (mut v2, mut v3) = (e[2] as i32, e[3] as i32);
            bit_transfer_signed(&mut v1, &mut v0);
            bit_transfer_signed(&mut v3, &mut v2);
            for ep in out.iter_mut().take(3) {
                *ep = [v0, v0 + v1];
            }
            out[3] = [v2, v2 + v3];
            for ep in out.iter_mut() {
                ep[0] = clamp(ep[0], 0, 255);
                ep[1] = clamp(ep[1], 0, 255);
            }
        }
        // CEM_LDR_RGB_BASE_SCALE
        6 => {
            let (v2, v3) = (e[2] as i32, e[3] as i32);
            out[0] = [(v0 * v3) >> 8, v0];
            out[1] = [(v1 * v3) >> 8, v1];
            out[2] = [(v2 * v3) >> 8, v2];
            out[3] = [0xFF, 0xFF];
        }
        // CEM_HDR_RGB_BASE_SCALE
        7 => {
            let (v2, v3) = (e[2] as i32, e[3] as i32);

            let modeval = ((v0 & 0xC0) >> 6) | ((v1 & 0x80) >> 5) | ((v2 & 0x80) >> 4);
            let (majcomp, mode) = if modeval & 0xC != 0xC {
                (modeval >> 2, modeval & 3)
            } else if modeval != 0xF {
                (modeval & 3, 4)
            } else {
                (0, 5)
            };

            let mut red = v0 & 0x3F;
            let mut green = v1 & 0x1F;
            let mut blue = v2 & 0x1F;
            let mut scale = v3 & 0x1F;

            let x0 = (v1 >> 6) & 1;
            let x1 = (v1 >> 5) & 1;
            let x2 = (v2 >> 6) & 1;
            let x3 = (v2 >> 5) & 1;
            let x4 = (v3 >> 7) & 1;
            let x5 = (v3 >> 6) & 1;
            let x6 = (v3 >> 5) & 1;

            let ohm = 1 << mode;
            if ohm & 0x30 != 0 {
                green |= x0 << 6;
            }
            if ohm & 0x3A != 0 {
                green |= x1 << 5;
            }
            if ohm & 0x30 != 0 {
                blue |= x2 << 6;
            }
            if ohm & 0x3A != 0 {
                blue |= x3 << 5;
            }
            if ohm & 0x3D != 0 {
                scale |= x6 << 5;
            }
            if ohm & 0x2D != 0 {
                scale |= x5 << 6;
            }
            if ohm & 0x04 != 0 {
                scale |= x4 << 7;
            }
            if ohm & 0x3B != 0 {
                red |= x4 << 6;
            }
            if ohm & 0x04 != 0 {
                red |= x3 << 6;
            }
            if ohm & 0x10 != 0 {
                red |= x5 << 7;
            }
            if ohm & 0x0F != 0 {
                red |= x2 << 7;
            }
            if ohm & 0x05 != 0 {
                red |= x1 << 8;
            }
            if ohm & 0x0A != 0 {
                red |= x0 << 8;
            }
            if ohm & 0x05 != 0 {
                red |= x0 << 9;
            }
            if ohm & 0x02 != 0 {
                red |= x6 << 9;
            }
            if ohm & 0x01 != 0 {
                red |= x3 << 10;
            }
            if ohm & 0x02 != 0 {
                red |= x5 << 10;
            }

            const SHAMTS: [i32; 6] = [1, 1, 2, 3, 4, 5];
            let shamt = SHAMTS[mode as usize];
            red <<= shamt;
            green <<= shamt;
            blue <<= shamt;
            scale <<= shamt;

            if mode != 5 {
                green = red - green;
                blue = red - blue;
            }
            if majcomp == 1 {
                core::mem::swap(&mut red, &mut green);
            }
            if majcomp == 2 {
                core::mem::swap(&mut red, &mut blue);
            }

            out[0] = [clamp(red - scale, 0, 0xFFF), clamp(red, 0, 0xFFF)];
            out[1] = [clamp(green - scale, 0, 0xFFF), clamp(green, 0, 0xFFF)];
            out[2] = [clamp(blue - scale, 0, 0xFFF), clamp(blue, 0, 0xFFF)];
            out[3] = [0x780, 0x780];
        }
        // CEM_LDR_RGB_DIRECT
        8 => {
            let (v2, v3, v4, v5) = (e[2] as i32, e[3] as i32, e[4] as i32, e[5] as i32);
            if v1 + v3 + v5 >= v0 + v2 + v4 {
                out[0] = [v0, v1];
                out[1] = [v2, v3];
                out[2] = [v4, v5];
                out[3] = [0xFF, 0xFF];
            } else {
                let lo = blue_contract(v1, v3, v5, 0xFF);
                let hi = blue_contract(v0, v2, v4, 0xFF);
                for c in 0..4 {
                    out[c] = [lo[c], hi[c]];
                }
            }
        }
        // CEM_LDR_RGB_BASE_PLUS_OFFSET
        9 => {
            let (mut v0, mut v1) = (v0, v1);
            let (mut v2, mut v3) = (e[2] as i32, e[3] as i32);
            let (mut v4, mut v5) = (e[4] as i32, e[5] as i32);
            bit_transfer_signed(&mut v1, &mut v0);
            bit_transfer_signed(&mut v3, &mut v2);
            bit_transfer_signed(&mut v5, &mut v4);
            if v1 + v3 + v5 >= 0 {
                out[0] = [v0, v0 + v1];
                out[1] = [v2, v2 + v3];
                out[2] = [v4, v4 + v5];
                out[3] = [0xFF, 0xFF];
            } else {
                let lo = blue_contract(v0 + v1, v2 + v3, v4 + v5, 0xFF);
                let hi = blue_contract(v0, v2, v4, 0xFF);
                for c in 0..4 {
                    out[c] = [lo[c], hi[c]];
                }
            }
            for ep in out.iter_mut() {
                ep[0] = clamp(ep[0], 0, 255);
                ep[1] = clamp(ep[1], 0, 255);
            }
        }
        // CEM_LDR_RGB_BASE_SCALE_PLUS_TWO_A
        10 => {
            let (v2, v3, v4, v5) = (e[2] as i32, e[3] as i32, e[4] as i32, e[5] as i32);
            out[0] = [(v0 * v3) >> 8, v0];
            out[1] = [(v1 * v3) >> 8, v1];
            out[2] = [(v2 * v3) >> 8, v2];
            out[3] = [v4, v5];
        }
        // CEM_LDR_RGBA_DIRECT
        12 => {
            let (v2, v3, v4, v5) = (e[2] as i32, e[3] as i32, e[4] as i32, e[5] as i32);
            let (v6, v7) = (e[6] as i32, e[7] as i32);
            if v1 + v3 + v5 >= v0 + v2 + v4 {
                out[0] = [v0, v1];
                out[1] = [v2, v3];
                out[2] = [v4, v5];
                out[3] = [v6, v7];
            } else {
                let lo = blue_contract(v1, v3, v5, v7);
                let hi = blue_contract(v0, v2, v4, v6);
                for c in 0..4 {
                    out[c] = [lo[c], hi[c]];
                }
            }
        }
        // CEM_LDR_RGBA_BASE_PLUS_OFFSET
        13 => {
            let (mut v0, mut v1) = (v0, v1);
            let (mut v2, mut v3) = (e[2] as i32, e[3] as i32);
            let (mut v4, mut v5) = (e[4] as i32, e[5] as i32);
            let (mut v6, mut v7) = (e[6] as i32, e[7] as i32);
            bit_transfer_signed(&mut v1, &mut v0);
            bit_transfer_signed(&mut v3, &mut v2);
            bit_transfer_signed(&mut v5, &mut v4);
            bit_transfer_signed(&mut v7, &mut v6);
            if v1 + v3 + v5 >= 0 {
                out[0] = [v0, v0 + v1];
                out[1] = [v2, v2 + v3];
                out[2] = [v4, v4 + v5];
                out[3] = [v6, v6 + v7];
            } else {
                let lo = blue_contract(v0 + v1, v2 + v3, v4 + v5, v6 + v7);
                let hi = blue_contract(v0, v2, v4, v6);
                for c in 0..4 {
                    out[c] = [lo[c], hi[c]];
                }
            }
            for ep in out.iter_mut() {
                ep[0] = clamp(ep[0], 0, 255);
                ep[1] = clamp(ep[1], 0, 255);
            }
        }
        // CEM_HDR_RGB (11), CEM_HDR_RGB_LDR_ALPHA (14), CEM_HDR_RGB_HDR_ALPHA (15)
        _ => {
            let (v2, v3, v4, v5) = (e[2] as i32, e[3] as i32, e[4] as i32, e[5] as i32);

            let majcomp = ((v4 & 0x80) >> 7) | ((v5 & 0x80) >> 6);

            out[3] = [0x780, 0x780];

            if majcomp == 3 {
                out[0] = [v0 << 4, v1 << 4];
                out[1] = [v2 << 4, v3 << 4];
                out[2] = [(v4 & 0x7F) << 5, (v5 & 0x7F) << 5];
            } else {
                let mode = ((v1 & 0x80) >> 7) | ((v2 & 0x80) >> 6) | ((v3 & 0x80) >> 5);
                let mut va = v0 | ((v1 & 0x40) << 2);
                let mut vb0 = v2 & 0x3F;
                let mut vb1 = v3 & 0x3F;
                let mut vc = v1 & 0x3F;

                const DBITS: [u32; 8] = [7, 6, 7, 6, 5, 6, 5, 6];
                let mut vd0 = sign_extend(v4 & 0x7F, DBITS[mode as usize]);
                let mut vd1 = sign_extend(v5 & 0x7F, DBITS[mode as usize]);

                let x0 = (v2 >> 6) & 1;
                let x1 = (v3 >> 6) & 1;
                let x2 = (v4 >> 6) & 1;
                let x3 = (v5 >> 6) & 1;
                let x4 = (v4 >> 5) & 1;
                let x5 = (v5 >> 5) & 1;

                let ohm = 1 << mode;
                if ohm & 0xA4 != 0 {
                    va |= x0 << 9;
                }
                if ohm & 0x08 != 0 {
                    va |= x2 << 9;
                }
                if ohm & 0x50 != 0 {
                    va |= x4 << 9;
                }
                if ohm & 0x50 != 0 {
                    va |= x5 << 10;
                }
                if ohm & 0xA0 != 0 {
                    va |= x1 << 10;
                }
                if ohm & 0xC0 != 0 {
                    va |= x2 << 11;
                }
                if ohm & 0x04 != 0 {
                    vc |= x1 << 6;
                }
                if ohm & 0xE8 != 0 {
                    vc |= x3 << 6;
                }
                if ohm & 0x20 != 0 {
                    vc |= x2 << 7;
                }
                if ohm & 0x5B != 0 {
                    vb0 |= x0 << 6;
                    vb1 |= x1 << 6;
                }
                if ohm & 0x12 != 0 {
                    vb0 |= x2 << 7;
                    vb1 |= x3 << 7;
                }

                let shamt = (mode >> 1) ^ 3;
                // Shift through u32 to sidestep signed-shift UB on the
                // sign-extended deltas; only the low bits survive the clamp.
                va = ((va as u32) << shamt) as i32;
                vb0 = ((vb0 as u32) << shamt) as i32;
                vb1 = ((vb1 as u32) << shamt) as i32;
                vc = ((vc as u32) << shamt) as i32;
                vd0 = ((vd0 as u32) << shamt) as i32;
                vd1 = ((vd1 as u32) << shamt) as i32;

                out[0] = [clamp(va - vc, 0, 0xFFF), clamp(va, 0, 0xFFF)];
                out[1] = [
                    clamp(va - vb0 - vc - vd0, 0, 0xFFF),
                    clamp(va - vb0, 0, 0xFFF),
                ];
                out[2] = [
                    clamp(va - vb1 - vc - vd1, 0, 0xFFF),
                    clamp(va - vb1, 0, 0xFFF),
                ];

                if majcomp == 1 {
                    out.swap(0, 1);
                } else if majcomp == 2 {
                    out.swap(0, 2);
                }
            }

            if cem == 14 {
                // LDR alpha: [0,255] direct.
                out[3] = [e[6] as i32, e[7] as i32];
            } else if cem == 15 {
                // HDR alpha: qlog12.
                let (mut v6, mut v7) = (e[6] as i32, e[7] as i32);
                let mode = ((v6 >> 7) & 1) | ((v7 >> 6) & 2);
                v6 &= 0x7F;
                v7 &= 0x7F;
                if mode == 3 {
                    out[3] = [v6 << 5, v7 << 5];
                } else {
                    v6 |= (v7 << (mode + 1)) & 0x780;
                    v7 &= 0x3F >> mode;
                    v7 ^= 0x20 >> mode;
                    v7 -= 0x20 >> mode;
                    v6 = ((v6 as u32) << (4 - mode)) as i32;
                    v7 = ((v7 as u32) << (4 - mode)) as i32;
                    v7 += v6;
                    v7 = clamp(v7, 0, 0xFFF);
                    out[3] = [v6, v7];
                }
            }
        }
    }
    out
}

/// Bilinear-upsample a `wx` x `wy` grid of [0,64] weights to the `bx` x `by`
/// block footprint; identity copy when the grid already matches. The
/// fixed-point sample positions depend only on the block and grid dimensions
/// and are computed inline per texel.
fn upsample_weight_grid(bx: u32, by: u32, wx: u32, wy: u32, src: &[u8], dst: &mut [u8]) {
    let n = (bx * by) as usize;
    if wx == bx && wy == by {
        dst[..n].copy_from_slice(&src[..n]);
        return;
    }
    let scale_x = (1024 + bx / 2) / (bx - 1);
    let scale_y = (1024 + by / 2) / (by - 1);
    for ty in 0..by {
        for tx in 0..bx {
            let gx = (scale_x * tx * (wx - 1) + 32) >> 6;
            let gy = (scale_y * ty * (wy - 1) + 32) >> 6;
            let (jx, jy) = (gx >> 4, gy >> 4);
            let (fx, fy) = (gx & 0xF, gy & 0xF);
            let w11 = (fx * fy + 8) >> 4;
            let w10 = fy - w11;
            let w01 = fx - w11;
            // Computed in unsigned arithmetic: the intermediate
            // `16 - fx - fy` may wrap below zero before `+ w11` brings it back,
            // so the wrap is done explicitly. The final weight is always in
            // [0, 16] (the four bilinear weights sum to 16).
            let w00 = 16u32.wrapping_sub(fx).wrapping_sub(fy).wrapping_add(w11);

            let mut total = 8u32;
            if w00 != 0 {
                total += src[(jx + jy * wx) as usize] as u32 * w00;
            }
            if w01 != 0 {
                total += src[(jx + 1 + jy * wx) as usize] as u32 * w01;
            }
            if w10 != 0 {
                total += src[(jx + (jy + 1) * wx) as usize] as u32 * w10;
            }
            if w11 != 0 {
                total += src[(jx + 1 + (jy + 1) * wx) as usize] as u32 * w11;
            }
            dst[(tx + ty * bx) as usize] = (total >> 4) as u8;
        }
    }
}

/// Shared decode state for the non-solid texel loops: per-subset decoded
/// endpoints, upsampled per-plane weights, and the partition config.
struct Prepared {
    is_ldr: [bool; 4],
    cems: [u8; 4],
    /// `[subset][comp][low/high]`.
    endpoints: [[[i32; 2]; 4]; 4],
    /// `[plane][texel]`, [0,64] weights at block resolution.
    weights: [[u8; MAX_BLOCK_TEXELS]; 2],
    /// Component using plane 1, or `u32::MAX` when single-plane.
    ccs: u32,
    num_partitions: u32,
    partition_id: u32,
    /// `num_texels < 31`: partition coordinates double for small blocks.
    small_block: bool,
}

/// Validate a non-solid logical block and dequantize/decode everything the
/// texel loops need. Returns `None` for any config the format rejects; a
/// rejected block aborts the slice upstream, so the error color it would
/// otherwise paint is never observable output.
fn prepare(log: &LogAstcBlock, bw: u32, bh: u32) -> Option<Prepared> {
    if log.grid_width < 2
        || log.grid_height < 2
        || log.grid_width > bw
        || log.grid_height > bh
        || !(4..=20).contains(&log.endpoint_ise_range)
        || log.weight_ise_range > 11
        || !(1..=4).contains(&log.num_partitions)
        || (log.dual_plane && log.num_partitions > 3)
        || log.partition_id >= 1024
        || (log.num_partitions == 1 && log.partition_id > 0)
        || log.color_component_selector > 3
    {
        return None;
    }

    let total_endpoint_levels = ise_levels(log.endpoint_ise_range);
    let total_weight_levels = ise_levels(log.weight_ise_range);

    let mut is_ldr = [false; 4];
    let mut total_cem_vals = 0u32;
    for (flag, &cem) in is_ldr
        .iter_mut()
        .zip(&log.color_endpoint_modes)
        .take(log.num_partitions as usize)
    {
        let cem = cem as u32;
        if cem > 15 {
            return None;
        }
        total_cem_vals += num_cem_values(cem);
        *flag = is_cem_ldr(cem);
    }
    if total_cem_vals as usize > MAX_ENDPOINTS {
        return None;
    }

    let tables = dequant_tables();
    let ep_dequant = &tables.endpoints[(log.endpoint_ise_range - 4) as usize];
    let w_dequant = &tables.weights[log.weight_ise_range as usize];

    let mut dequantized_endpoints = [0u8; MAX_ENDPOINTS];
    for i in 0..total_cem_vals as usize {
        if log.endpoints[i] as u32 >= total_endpoint_levels {
            return None;
        }
        dequantized_endpoints[i] = ep_dequant[log.endpoints[i] as usize];
    }

    let mut dequantized_weights = [[0u8; MAX_BLOCK_TEXELS]; 2];
    let total_weight_vals = (if log.dual_plane { 2 } else { 1 }) * log.grid_width * log.grid_height;
    for i in 0..total_weight_vals as usize {
        if log.weights[i] as u32 >= total_weight_levels {
            return None;
        }
        let (plane, grid) = if log.dual_plane {
            (i & 1, i >> 1)
        } else {
            (0, i)
        };
        dequantized_weights[plane][grid] = w_dequant[log.weights[i] as usize];
    }

    let mut weights = [[0u8; MAX_BLOCK_TEXELS]; 2];
    upsample_weight_grid(
        bw,
        bh,
        log.grid_width,
        log.grid_height,
        &dequantized_weights[0],
        &mut weights[0],
    );
    if log.dual_plane {
        upsample_weight_grid(
            bw,
            bh,
            log.grid_width,
            log.grid_height,
            &dequantized_weights[1],
            &mut weights[1],
        );
    }

    let mut endpoints = [[[0i32; 2]; 4]; 4];
    let mut val_index = 0usize;
    for (ep, &cem) in endpoints
        .iter_mut()
        .zip(&log.color_endpoint_modes)
        .take(log.num_partitions as usize)
    {
        *ep = decode_endpoint(cem as u32, &dequantized_endpoints[val_index..]);
        val_index += num_cem_values(cem as u32) as usize;
    }

    Some(Prepared {
        is_ldr,
        cems: log.color_endpoint_modes,
        endpoints,
        weights,
        ccs: if log.dual_plane {
            log.color_component_selector
        } else {
            u32::MAX
        },
        num_partitions: log.num_partitions,
        partition_id: log.partition_id,
        small_block: bw * bh < 31,
    })
}

/// The subset (partition) a texel belongs to. Calling `compute_texel_partition`
/// per texel is value-identical to indexing a precomputed 2-3 subset table,
/// since such tables are generated from that same function. `small_block`
/// (fewer than 31 texels) doubles the coordinates.
#[inline]
fn texel_subset(p: &Prepared, x: u32, y: u32) -> usize {
    if p.num_partitions > 1 {
        compute_texel_partition(p.partition_id, x, y, 0, p.num_partitions, p.small_block) as usize
    } else {
        0
    }
}

/// Decode a logical `bw` x `bh` block to half-float RGBA texels into
/// `out[..bw*bh]` (`[texel][component]` halves, row-major). `None` for a
/// config the format rejects (which aborts the slice).
pub fn decode_block_hdr16(
    log: &LogAstcBlock,
    bw: u32,
    bh: u32,
    out: &mut [[u16; 4]],
) -> Option<()> {
    let n = (bw * bh) as usize;
    if log.solid_color_flag_ldr {
        // LDR void extent: 16-bit UNORM components to halves, truncating.
        let mut h = [0u16; 4];
        for (hc, &c) in h.iter_mut().zip(&log.solid_color) {
            *hc = if c == 0xFFFF {
                0x3C00
            } else {
                half_from_unorm16(c as u32)
            };
        }
        out[..n].fill(h);
        return Some(());
    }
    if log.solid_color_flag_hdr {
        // HDR void extent: components already are halves.
        out[..n].fill(log.solid_color);
        return Some(());
    }

    let p = prepare(log, bw, bh)?;
    for y in 0..bh {
        for x in 0..bw {
            let i = (x + y * bw) as usize;
            let subset = texel_subset(&p, x, y);
            for (c, out_c) in out[i].iter_mut().enumerate() {
                let w = p.weights[usize::from(c as u32 == p.ccs)][i] as i32;
                let ldr_channel = p.is_ldr[subset] || (p.cems[subset] == 14 && c == 3);
                *out_c = if ldr_channel {
                    let le = p.endpoints[subset][c][0];
                    let he = p.endpoints[subset][c][1];
                    let k = weight_interpolate((le << 8) | le, (he << 8) | he, w);
                    if k == 0xFFFF {
                        0x3C00
                    } else {
                        half_from_unorm16(k as u32)
                    }
                } else {
                    let le = p.endpoints[subset][c][0] << 4;
                    let he = p.endpoints[subset][c][1] << 4;
                    let o = qlog16_to_half(weight_interpolate(le, he, w));
                    if is_half_inf_or_nan(o) {
                        0x7BFF
                    } else {
                        o
                    }
                };
            }
        }
    }
    Some(())
}

/// Decode a logical `bw` x `bh` block to packed RGB9E5 texels into
/// `out[..bw*bh]`, row-major. `None` for a config the format rejects.
pub fn decode_block_9e5(log: &LogAstcBlock, bw: u32, bh: u32, out: &mut [u32]) -> Option<()> {
    let n = (bw * bh) as usize;
    if log.solid_color_flag_ldr {
        // LDR void extent packs through the float path.
        let f = |c: u16| {
            if c == 0xFFFF {
                1.0f32
            } else {
                c as f32 * (1.0 / 65536.0)
            }
        };
        let packed = pack_rgb9e5(
            f(log.solid_color[0]),
            f(log.solid_color[1]),
            f(log.solid_color[2]),
        );
        out[..n].fill(packed);
        return Some(());
    }
    if log.solid_color_flag_hdr {
        let packed = pack_rgb9e5(
            super::half::half_to_float(log.solid_color[0]),
            super::half::half_to_float(log.solid_color[1]),
            super::half::half_to_float(log.solid_color[2]),
        );
        out[..n].fill(packed);
        return Some(());
    }

    let p = prepare(log, bw, bh)?;
    for y in 0..bh {
        for x in 0..bw {
            let i = (x + y * bw) as usize;
            let subset = texel_subset(&p, x, y);
            let mut comp = [0i32; 3];
            for (c, comp_c) in comp.iter_mut().enumerate() {
                let w = p.weights[usize::from(c as u32 == p.ccs)][i] as i32;
                let le = p.endpoints[subset][c][0];
                let he = p.endpoints[subset][c][1];
                *comp_c = if p.is_ldr[subset] {
                    weight_interpolate((le << 8) | le, (he << 8) | he, w)
                } else {
                    let o = qlog16_to_half(weight_interpolate(le << 4, he << 4, w));
                    if is_half_inf_or_nan(o) {
                        0x7BFF
                    } else {
                        o as i32
                    }
                };
            }
            out[i] = if p.is_ldr[subset] {
                pack_rgb9e5_ldr_astc(comp[0], comp[1], comp[2])
            } else {
                pack_rgb9e5_hdr_astc(comp[0], comp[1], comp[2])
            };
        }
    }
    Some(())
}

/// Decode a logical `bw` x `bh` block to 8-bit RGBA texels (linear LDR8, or
/// sRGB8 when `srgb` is set) into `out[..bw*bh]`, row-major. `None` for a
/// config the format rejects, including an HDR void extent and any texel whose
/// subset uses an HDR CEM; such a block aborts the slice, so the magenta error
/// color it would otherwise paint is never observable output.
pub fn decode_block_ldr8(
    log: &LogAstcBlock,
    bw: u32,
    bh: u32,
    srgb: bool,
    out: &mut [[u8; 4]],
) -> Option<()> {
    let n = (bw * bh) as usize;
    if log.solid_color_flag_ldr {
        // LDR void extent: the high byte of each 16-bit component.
        let mut c8 = [0u8; 4];
        for (dst, &c) in c8.iter_mut().zip(&log.solid_color) {
            *dst = (c >> 8) as u8;
        }
        out[..n].fill(c8);
        return Some(());
    }
    if log.solid_color_flag_hdr {
        // An HDR void extent cannot decode to 8-bit, so reject it and abort
        // the slice.
        return None;
    }

    let p = prepare(log, bw, bh)?;
    for y in 0..bh {
        for x in 0..bw {
            let i = (x + y * bw) as usize;
            let subset = texel_subset(&p, x, y);
            if !p.is_ldr[subset] {
                return None;
            }
            for (c, out_c) in out[i].iter_mut().enumerate() {
                let w = p.weights[usize::from(c as u32 == p.ccs)][i] as i32;
                let le = p.endpoints[subset][c][0];
                let he = p.endpoints[subset][c][1];
                // The sRGB profile fills the low byte with 0x80 to center it;
                // the linear LDR path replicates the endpoint byte instead.
                let (le, he) = if srgb {
                    ((le << 8) | 0x80, (he << 8) | 0x80)
                } else {
                    ((le << 8) | le, (he << 8) | he)
                };
                *out_c = (weight_interpolate(le, he, w) >> 8) as u8;
            }
        }
    }
    Some(())
}
