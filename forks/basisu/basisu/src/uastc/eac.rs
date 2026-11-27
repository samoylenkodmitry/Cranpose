//! UASTC -> ETC2 EAC R11 / RG11 transcode: unpack the UASTC block to pixels,
//! then re-encode one channel plane per 8-byte EAC block via `pack_eac`,
//! `pack_eac_high_quality`, `pack_eac_solid_block`, or the alpha hint path.
//!
//! An `eac_block` is 8 bytes: base (byte 0), `table | (multiplier << 4)`
//! (byte 1), then 48 selector bits packed MSB-first (bytes 2..8). R11 is one
//! such block (R plane); RG11 is two (R + G/alpha planes).
//!
//! Do not rewrite the float endpoint math with `mul_add`: fusing the
//! `a + (b - a) * c` lerp changes rounding, which can shift the rounded
//! base/multiplier picks and thus the encoded block. Every intermediate must
//! round as a separate IEEE f32 operation.

use super::tables::HAS_ALPHA;
use super::unpack::{unpack_pixels, unpack_to_block, UnpackedUastcBlock};
use super::UASTC_MODE_INDEX_SOLID_COLOR;
use crate::color::Color32;
use crate::etc::tables::{EAC_MODIFIER_TABLE, ETC2_EAC_A8_SEL4};

const ETC2_EAC_MIN_VALUE_SELECTOR: usize = 3;
const ETC2_EAC_MAX_VALUE_SELECTOR: usize = 7;

/// Where pixel `i` (raster order) lands in the 48-bit selector field
/// (MSB-first).
const S_ETC2_EAC_BIT_OFS: [u8; 16] = [45, 33, 21, 9, 42, 30, 18, 6, 39, 27, 15, 3, 36, 24, 12, 0];

/// Clamp an int to the inclusive `[0, 255]` byte range.
#[inline]
fn clamp255(v: i32) -> i32 {
    v.clamp(0, 255)
}

/// Clamp `v` to the inclusive range `[lo, hi]`.
#[inline]
fn clampi(v: i32, lo: i32, hi: i32) -> i32 {
    v.clamp(lo, hi)
}

/// Pack base/table/multiplier/selectors into an 8-byte `eac_block`.
#[inline]
fn pack_block(base: u8, table: u8, multiplier: u8, selector_bits: u64) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0] = base;
    out[1] = (table & 0xF) | ((multiplier & 0xF) << 4);
    out[2] = (selector_bits >> 40) as u8;
    out[3] = (selector_bits >> 32) as u8;
    out[4] = (selector_bits >> 24) as u8;
    out[5] = (selector_bits >> 16) as u8;
    out[6] = (selector_bits >> 8) as u8;
    out[7] = selector_bits as u8;
    out
}

/// `pack_eac_solid_block`: constant value `a` (table 13, multiplier 0, all-4
/// selectors).
fn pack_eac_solid_block(a: u8) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0] = a;
    out[1] = 13; // table 13, multiplier 0
    out[2..8].copy_from_slice(&ETC2_EAC_A8_SEL4);
    out
}

/// `pack_eac`: low-quality EAC encode (checks 4 tables). `pixels` are the
/// channel values in raster order.
// The selector index `s` doubles as the packed selector value (`| s`), and the
// best-table search uses strict `<` so ties go to the lowest-numbered table;
// the explicit index loops keep both orderings visible.
#[allow(clippy::needless_range_loop)]
pub(crate) fn pack_eac(pixels: &[u8; 16]) -> [u8; 8] {
    let mut min_alpha = 255u32;
    let mut max_alpha = 0u32;
    for &p in pixels {
        let a = p as u32;
        if a < min_alpha {
            min_alpha = a;
        }
        if a > max_alpha {
            max_alpha = a;
        }
    }

    if min_alpha == max_alpha {
        return pack_eac_solid_block(min_alpha as u8);
    }

    let alpha_range = max_alpha - min_alpha;

    const SINGLE_TABLE_THRESH: u32 = 5;
    if alpha_range <= SINGLE_TABLE_THRESH {
        // table 13 is lossless when alpha_range <= 5
        let base = clamp255(max_alpha as i32 - 2);
        let base_minus = base - 3;
        let mut packed_sels = 0u64;
        for (i, &p) in pixels.iter().enumerate() {
            const S_SELS: [u8; 6] = [2, 1, 0, 4, 5, 6];
            let sel = p as i32 - base_minus;
            packed_sels |= (S_SELS[sel as usize] as u64) << S_ETC2_EAC_BIT_OFS[i];
        }
        return pack_block(base as u8, 13, 1, packed_sels);
    }

    const T0: usize = 2;
    const T1: usize = 8;
    const T2: usize = 11;
    const T3: usize = 13;
    let s_tables = [T0, T1, T2, T3];

    let mut base = [0i32; 4];
    let mut mul = [0i32; 4];
    let mut mul_or = 0u32;
    for i in 0..4 {
        let table = s_tables[i];
        let range = (EAC_MODIFIER_TABLE[table][ETC2_EAC_MAX_VALUE_SELECTOR] as i32
            - EAC_MODIFIER_TABLE[table][ETC2_EAC_MIN_VALUE_SELECTOR] as i32)
            as f32;
        let t = (0 - EAC_MODIFIER_TABLE[table][ETC2_EAC_MIN_VALUE_SELECTOR] as i32) as f32 / range;
        // a + (b - a) * c lerp; keep the multiply and add separate (module doc).
        let center =
            crate::mathf::roundf(min_alpha as f32 + (max_alpha as f32 - min_alpha as f32) * t)
                as i32;
        base[i] = clamp255(center);
        mul[i] = clampi(
            crate::mathf::roundf(alpha_range as f32 / range) as i32,
            1,
            15,
        );
        mul_or |= mul[i] as u32;
    }

    let mut total_err = [0u32; 4];
    let mut sels = [[0u8; 16]; 4];

    for i in 0..16usize {
        let a = pixels[i] as i32;
        // per-table running best, packed as (error << 3) | selector so one min
        // tracks both at once
        let mut l = [u32::MAX; 4];

        // only pixel values near 0 or 255 can make the winning interpolant
        // clamp; elsewhere the cheaper unclamped paths pick the same selector
        if !(7..=(255 - 7)).contains(&a) {
            for s in 0..8usize {
                let v0 = clamp255(mul[0] * EAC_MODIFIER_TABLE[T0][s] as i32 + base[0]);
                let v1 = clamp255(mul[1] * EAC_MODIFIER_TABLE[T1][s] as i32 + base[1]);
                let v2 = clamp255(mul[2] * EAC_MODIFIER_TABLE[T2][s] as i32 + base[2]);
                let v3 = clamp255(mul[3] * EAC_MODIFIER_TABLE[T3][s] as i32 + base[3]);
                l[0] = l[0].min(((v0 - a).unsigned_abs() << 3) | s as u32);
                l[1] = l[1].min(((v1 - a).unsigned_abs() << 3) | s as u32);
                l[2] = l[2].min(((v2 - a).unsigned_abs() << 3) | s as u32);
                l[3] = l[3].min(((v3 - a).unsigned_abs() << 3) | s as u32);
            }
        } else if mul_or == 1 {
            // every multiplier is 1: add the modifier directly, no multiply
            let a0 = base[0] - a;
            let a1 = base[1] - a;
            let a2 = base[2] - a;
            let a3 = base[3] - a;
            for s in 0..8usize {
                let v0 = EAC_MODIFIER_TABLE[T0][s] as i32 + a0;
                let v1 = EAC_MODIFIER_TABLE[T1][s] as i32 + a1;
                let v2 = EAC_MODIFIER_TABLE[T2][s] as i32 + a2;
                let v3 = EAC_MODIFIER_TABLE[T3][s] as i32 + a3;
                l[0] = l[0].min((v0.unsigned_abs() << 3) | s as u32);
                l[1] = l[1].min((v1.unsigned_abs() << 3) | s as u32);
                l[2] = l[2].min((v2.unsigned_abs() << 3) | s as u32);
                l[3] = l[3].min((v3.unsigned_abs() << 3) | s as u32);
            }
        } else {
            // general multipliers, still no clamping needed for interior values
            let a0 = base[0] - a;
            let a1 = base[1] - a;
            let a2 = base[2] - a;
            let a3 = base[3] - a;
            for s in 0..8usize {
                let v0 = mul[0] * EAC_MODIFIER_TABLE[T0][s] as i32 + a0;
                let v1 = mul[1] * EAC_MODIFIER_TABLE[T1][s] as i32 + a1;
                let v2 = mul[2] * EAC_MODIFIER_TABLE[T2][s] as i32 + a2;
                let v3 = mul[3] * EAC_MODIFIER_TABLE[T3][s] as i32 + a3;
                l[0] = l[0].min((v0.unsigned_abs() << 3) | s as u32);
                l[1] = l[1].min((v1.unsigned_abs() << 3) | s as u32);
                l[2] = l[2].min((v2.unsigned_abs() << 3) | s as u32);
                l[3] = l[3].min((v3.unsigned_abs() << 3) | s as u32);
            }
        }

        sels[0][i] = (l[0] & 7) as u8;
        sels[1][i] = (l[1] & 7) as u8;
        sels[2][i] = (l[2] & 7) as u8;
        sels[3][i] = (l[3] & 7) as u8;

        total_err[0] += (l[0] >> 3) * (l[0] >> 3);
        total_err[1] += (l[1] >> 3) * (l[1] >> 3);
        total_err[2] += (l[2] >> 3) * (l[2] >> 3);
        total_err[3] += (l[3] >> 3) * (l[3] >> 3);
    }

    let mut min_err = total_err[0];
    let mut min_index = 0usize;
    for i in 1..4 {
        if total_err[i] < min_err {
            min_err = total_err[i];
            min_index = i;
        }
    }

    let mut packed_sels = 0u64;
    for i in 0..16 {
        packed_sels |= (sels[min_index][i] as u64) << S_ETC2_EAC_BIT_OFS[i];
    }
    pack_block(
        base[min_index] as u8,
        s_tables[min_index] as u8,
        mul[min_index] as u8,
        packed_sels,
    )
}

/// `pack_eac_high_quality`: checks all 16 tables.
#[allow(clippy::needless_range_loop)]
pub(crate) fn pack_eac_high_quality(pixels: &[u8; 16]) -> [u8; 8] {
    let mut min_alpha = 255u32;
    let mut max_alpha = 0u32;
    for &p in pixels {
        let a = p as u32;
        if a < min_alpha {
            min_alpha = a;
        }
        if a > max_alpha {
            max_alpha = a;
        }
    }

    if min_alpha == max_alpha {
        return pack_eac_solid_block(min_alpha as u8);
    }

    let alpha_range = max_alpha - min_alpha;

    const SINGLE_TABLE_THRESH: u32 = 5;
    if alpha_range <= SINGLE_TABLE_THRESH {
        let base = clamp255(max_alpha as i32 - 2);
        let base_minus = base - 3;
        let mut packed_sels = 0u64;
        for (i, &p) in pixels.iter().enumerate() {
            const S_SELS: [u8; 6] = [2, 1, 0, 4, 5, 6];
            let sel = p as i32 - base_minus;
            packed_sels |= (S_SELS[sel as usize] as u64) << S_ETC2_EAC_BIT_OFS[i];
        }
        return pack_block(base as u8, 13, 1, packed_sels);
    }

    let mut base = [0i32; 16];
    let mut mul = [0i32; 16];
    for table in 0..16usize {
        let range = (EAC_MODIFIER_TABLE[table][ETC2_EAC_MAX_VALUE_SELECTOR] as i32
            - EAC_MODIFIER_TABLE[table][ETC2_EAC_MIN_VALUE_SELECTOR] as i32)
            as f32;
        let t = (0 - EAC_MODIFIER_TABLE[table][ETC2_EAC_MIN_VALUE_SELECTOR] as i32) as f32 / range;
        let center =
            crate::mathf::roundf(min_alpha as f32 + (max_alpha as f32 - min_alpha as f32) * t)
                as i32;
        base[table] = clamp255(center);
        mul[table] = clampi(
            crate::mathf::roundf(alpha_range as f32 / range) as i32,
            1,
            15,
        );
    }

    let mut total_err = [0u32; 16];
    let mut sels = [[0u8; 16]; 16];

    for table in 0..16usize {
        let m = mul[table];
        let b = base[table];
        let mut prev_l = 0u32;
        let mut prev_a = u32::MAX;

        for i in 0..16usize {
            let a = pixels[i] as i32;
            if a as u32 == prev_a {
                // same value as the previous pixel: reuse its selector and error
                sels[table][i] = (prev_l & 7) as u8;
                total_err[table] += (prev_l >> 3) * (prev_l >> 3);
            } else {
                let mut l =
                    (clamp255(m * EAC_MODIFIER_TABLE[table][0] as i32 + b) - a).unsigned_abs() << 3;
                for (s, ofs_extra) in (1..8usize).map(|s| (s, s as u32)) {
                    let cand = (((clamp255(m * EAC_MODIFIER_TABLE[table][s] as i32 + b) - a)
                        .unsigned_abs())
                        << 3)
                        | ofs_extra;
                    l = l.min(cand);
                }
                sels[table][i] = (l & 7) as u8;
                total_err[table] += (l >> 3) * (l >> 3);
                prev_l = l;
                prev_a = a as u32;
            }
        }
    }

    let mut min_err = total_err[0];
    let mut min_index = 0usize;
    for i in 1..16 {
        if total_err[i] < min_err {
            min_err = total_err[i];
            min_index = i;
        }
    }

    let mut packed_sels = 0u64;
    for i in 0..16 {
        packed_sels |= (sels[min_index][i] as u64) << S_ETC2_EAC_BIT_OFS[i];
    }
    pack_block(
        base[min_index] as u8,
        min_index as u8,
        mul[min_index] as u8,
        packed_sels,
    )
}

/// Hint-based EAC encode of the alpha plane (used when `chan == 3`): the
/// block's `etc2_hints` byte supplies the table and multiplier directly, so
/// only the base and selectors are searched. `pixels` are in raster order.
fn pack_eac_a8_hint(u: &UnpackedUastcBlock, pixels: &[Color32; 16]) -> [u8; 8] {
    if HAS_ALPHA[u.mode as usize] == 0 || u.mode == UASTC_MODE_INDEX_SOLID_COLOR {
        let a = if u.mode == UASTC_MODE_INDEX_SOLID_COLOR {
            u.solid_color.a() as u32
        } else {
            255
        };
        let mut out = [0u8; 8];
        out[0] = a as u8;
        out[1] = 13 | (1 << 4); // table 13, multiplier 1
        out[2..8].copy_from_slice(&ETC2_EAC_A8_SEL4);
        return out;
    }

    let mut min_a = 255u32;
    let mut max_a = 0u32;
    for p in pixels {
        min_a = min_a.min(p.a() as u32);
        max_a = max_a.max(p.a() as u32);
    }
    if min_a == max_a {
        let mut out = [0u8; 8];
        out[0] = min_a as u8;
        out[1] = 13 | (1 << 4);
        out[2..8].copy_from_slice(&ETC2_EAC_A8_SEL4);
        return out;
    }

    let table = u.etc2_hints & 0xF;
    let multiplier = u.etc2_hints >> 4;
    let tbl = &EAC_MODIFIER_TABLE[table as usize];
    let range =
        (tbl[ETC2_EAC_MAX_VALUE_SELECTOR] as i32 - tbl[ETC2_EAC_MIN_VALUE_SELECTOR] as i32) as f32;
    let t = (0 - tbl[ETC2_EAC_MIN_VALUE_SELECTOR] as i32) as f32 / range;
    let center = crate::mathf::roundf(min_a as f32 + (max_a as f32 - min_a as f32) * t) as i32;

    let mut vals = [0i32; 8];
    for (j, v) in vals.iter_mut().enumerate() {
        *v = clamp255(center + tbl[j] as i32 * multiplier as i32);
    }

    let mut sels = 0u64;
    for i in 0..16u32 {
        // `i` walks the selector slots in MSB-first order (bit `45 - i*3`), which
        // is column-major (`i == x*4 + y`); transpose back to the raster pixel.
        let a = pixels[((i & 3) * 4 + (i >> 2)) as usize].a() as i32;
        let mut min_err = u32::MAX;
        for (j, &vj) in vals.iter().enumerate() {
            let err = ((vj - a).unsigned_abs() << 3) | j as u32;
            min_err = min_err.min(err);
        }
        sels |= ((min_err & 7) as u64) << (45 - i * 3);
    }

    pack_block(center as u8, table as u8, multiplier as u8, sels)
}

/// Unpack a UASTC block into (`UnpackedUastcBlock`, raster pixels). Solid mode
/// yields a constant-color pixel array.
fn unpack(src: &[u8; 16]) -> Option<(UnpackedUastcBlock, [Color32; 16])> {
    let u = unpack_to_block(src, false, true)?;
    let pixels = if u.mode == UASTC_MODE_INDEX_SOLID_COLOR {
        [u.solid_color; 16]
    } else {
        unpack_pixels(u.mode, u.common_pattern, u.solid_color, &u.astc, false)
    };
    Some((u, pixels))
}

/// Encode one channel plane to an 8-byte EAC R11 block, mirroring the dispatch
/// in `transcode_uastc_to_etc2_eac_r11` (solid fast-path, alpha hint path, or
/// the float search).
fn encode_plane(
    u: &UnpackedUastcBlock,
    pixels: &[Color32; 16],
    high_quality: bool,
    chan: usize,
) -> [u8; 8] {
    if u.mode == UASTC_MODE_INDEX_SOLID_COLOR {
        return pack_eac_solid_block(u.solid_color.c[chan]);
    }
    if chan == 3 {
        pack_eac_a8_hint(u, pixels)
    } else {
        let mut plane = [0u8; 16];
        for (i, p) in pixels.iter().enumerate() {
            plane[i] = p.c[chan];
        }
        if high_quality {
            pack_eac_high_quality(&plane)
        } else {
            pack_eac(&plane)
        }
    }
}

/// `transcode_uastc_to_etc2_eac_r11`: 16-byte UASTC block -> 8-byte EAC R11
/// block (channel `chan0`).
pub fn transcode_uastc_to_etc2_eac_r11(
    src: &[u8; 16],
    high_quality: bool,
    chan0: usize,
) -> Option<[u8; 8]> {
    let (u, pixels) = unpack(src)?;
    Some(encode_plane(&u, &pixels, high_quality, chan0))
}

/// `transcode_uastc_to_etc2_eac_rg11`: 16-byte UASTC block -> 16-byte EAC RG11
/// block (R = `chan0` in bytes 0..8, G = `chan1` in bytes 8..16).
pub fn transcode_uastc_to_etc2_eac_rg11(
    src: &[u8; 16],
    high_quality: bool,
    chan0: usize,
    chan1: usize,
) -> Option<[u8; 16]> {
    let (u, pixels) = unpack(src)?;
    let r = encode_plane(&u, &pixels, high_quality, chan0);
    let g = encode_plane(&u, &pixels, high_quality, chan1);
    let mut out = [0u8; 16];
    out[0..8].copy_from_slice(&r);
    out[8..16].copy_from_slice(&g);
    Some(out)
}
