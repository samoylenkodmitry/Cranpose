//! XUASTC LDR endpoint machinery: the full per-CEM endpoint decode, the
//! blue-contraction / base+offset-preserving requantizer (distinct from the
//! 6x6 codec's), and the mini-CEM encoder that predicts one CEM's endpoints
//! from another's. All integer math, so the decode stays deterministic across
//! compilers; floating point is deliberately avoided here.

// The index-based loops here walk parallel endpoint arrays by position, which
// tracks the ISE value layout more directly than iterator adaptors would.
#![allow(clippy::needless_range_loop, clippy::explicit_counter_loop)]

use crate::astc::dequant::{dequant_tables, quant_tables};
use crate::astc::unpack::ise_levels;
use crate::once::OnceBox;
use crate::uastc_hdr_6x6::preserve_tables;
use alloc::boxed::Box;

// ASTC color endpoint mode numbers. The values are the ASTC CEM indices; the
// two LDR base-plus-offset luminance modes (1 and 5) are decoded inline by
// their number instead of through a named constant, and the HDR CEMs (2, 3, 7,
// 11) never appear in this LDR stream.

/// LDR luminance, both endpoints stored directly.
pub const CEM_LDR_LUM_DIRECT: u32 = 0;
/// LDR luminance plus alpha, all four values stored directly.
pub const CEM_LDR_LUM_ALPHA_DIRECT: u32 = 4;
/// LDR RGB expressed as a high color plus a scale factor for the low color.
pub const CEM_LDR_RGB_BASE_SCALE: u32 = 6;
/// LDR RGB, both endpoints stored directly.
pub const CEM_LDR_RGB_DIRECT: u32 = 8;
/// LDR RGB as a base color plus a signed per-channel offset.
pub const CEM_LDR_RGB_BASE_PLUS_OFFSET: u32 = 9;
/// LDR RGB base-plus-scale with two directly stored alpha endpoints.
pub const CEM_LDR_RGB_BASE_SCALE_PLUS_TWO_A: u32 = 10;
/// LDR RGBA, both endpoints stored directly.
pub const CEM_LDR_RGBA_DIRECT: u32 = 12;
/// LDR RGBA as a base color plus a signed per-channel offset.
pub const CEM_LDR_RGBA_BASE_PLUS_OFFSET: u32 = 13;

/// Number of ISE endpoint values a CEM encodes: 2 for the luminance modes, up
/// to 8 for RGBA.
#[inline]
pub fn num_cem_values(cem: u32) -> usize {
    (2 + 2 * (cem >> 2)) as usize
}

/// True for the LDR CEMs that carry an alpha channel.
#[inline]
pub fn cem_has_alpha(cem: u32) -> bool {
    matches!(cem, 4 | 5 | 10 | 12 | 13)
}

/// The CEMs whose encoding can express blue contraction.
#[inline]
pub fn cem_supports_bc(cem: u32) -> bool {
    matches!(cem, 8 | 9 | 12 | 13)
}

/// Map an alpha-carrying CEM to its RGB or luminance base CEM; CEMs that have
/// no separate alpha are returned unchanged.
#[inline]
pub fn base_cem_without_alpha(cem: u32) -> u32 {
    match cem {
        4 => 0,  // LUM_ALPHA -> LUM
        12 => 8, // RGBA -> RGB
        10 => 6, // BASE_SCALE_2A -> BASE_SCALE
        13 => 9, // RGBA_BASE_OFS -> RGB_BASE_OFS
        _ => cem,
    }
}

/// Dense 0..=7 context index for the CEM probability model.
#[inline]
pub fn cem_to_ldrcem_index(cem: u32) -> usize {
    match cem {
        0 => 0,
        4 => 1,
        6 => 2,
        8 => 3,
        9 => 4,
        10 => 5,
        12 => 6,
        13 => 7,
        _ => 0,
    }
}

/// Undo the signed bit transfer: move `b`'s stashed MSB back into place and
/// sign-extend the 6-bit delta in `a`.
#[inline]
fn bit_transfer_signed_dec(a: &mut i32, b: &mut i32) {
    *b >>= 1;
    *b |= *a & 0x80;
    *a >>= 1;
    *a &= 0x3F;
    if *a & 0x20 != 0 {
        *a -= 0x40;
    }
}

/// Signed bit transfer: stash `b`'s MSB into `a`'s top bit, turning `a` in
/// [-32,31] into an 8-bit code.
#[inline]
fn bit_transfer_signed_enc(a: &mut i32, b: &mut i32) {
    let bit_to_transfer = (*b & 0x80) != 0;
    *b = (*b << 1) & 0xFF;
    *a &= 0x3F;
    *a <<= 1;
    if bit_to_transfer {
        *a |= 0x80;
    }
}

/// Blue contraction, decode side: the halving that reconstructs r and g from
/// the stored blue.
#[inline]
fn blue_contract(r: i32, g: i32, b: i32, a: i32) -> [i32; 4] {
    [(r + b) >> 1, (g + b) >> 1, b, a]
}

/// Blue contraction, encode side: the doubling against an already-quantized
/// blue; sets `did_clamp` when r or g leaves [0,255].
fn blue_contract_enc(orig: [u8; 4], did_clamp: &mut bool, encoded_b: i32) -> [u8; 4] {
    let tr = orig[0] as i32 * 2 - encoded_b;
    let tg = orig[1] as i32 * 2 - encoded_b;
    if !(0..=255).contains(&tr) || !(0..=255).contains(&tg) {
        *did_clamp = true;
    }
    [
        tr.clamp(0, 255) as u8,
        tg.clamp(0, 255) as u8,
        orig[2],
        orig[3],
    ]
}

/// Decode one subset's endpoints for the LDR CEMs: dequantized 8-bit values
/// in, `(low, high)` RGBA out.
fn decode_endpoint_vals(cem: u32, v: &[u8]) -> ([u8; 4], [u8; 4]) {
    let vi = |i: usize| v[i] as i32;
    let clamp8 = |x: i32| x.clamp(0, 255) as u8;
    match cem {
        CEM_LDR_LUM_DIRECT => {
            let (v0, v1) = (v[0], v[1]);
            ([v0, v0, v0, 0xFF], [v1, v1, v1, 0xFF])
        }
        1 => {
            // LUM_BASE_PLUS_OFS
            let l0 = (vi(0) >> 2) | (vi(1) & 0xC0);
            let l1 = (l0 + (vi(1) & 0x3F)).min(0xFF);
            let (l0, l1) = (l0 as u8, l1 as u8);
            ([l0, l0, l0, 0xFF], [l1, l1, l1, 0xFF])
        }
        CEM_LDR_LUM_ALPHA_DIRECT => ([v[0], v[0], v[0], v[2]], [v[1], v[1], v[1], v[3]]),
        5 => {
            // LUM_ALPHA_BASE_PLUS_OFS
            let (mut v0, mut v1, mut v2, mut v3) = (vi(0), vi(1), vi(2), vi(3));
            bit_transfer_signed_dec(&mut v1, &mut v0);
            bit_transfer_signed_dec(&mut v3, &mut v2);
            (
                [clamp8(v0), clamp8(v0), clamp8(v0), clamp8(v2)],
                [
                    clamp8(v0 + v1),
                    clamp8(v0 + v1),
                    clamp8(v0 + v1),
                    clamp8(v2 + v3),
                ],
            )
        }
        CEM_LDR_RGB_BASE_SCALE => {
            let s = vi(3);
            (
                [
                    ((vi(0) * s) >> 8) as u8,
                    ((vi(1) * s) >> 8) as u8,
                    ((vi(2) * s) >> 8) as u8,
                    0xFF,
                ],
                [v[0], v[1], v[2], 0xFF],
            )
        }
        CEM_LDR_RGB_BASE_SCALE_PLUS_TWO_A => {
            let s = vi(3);
            (
                [
                    ((vi(0) * s) >> 8) as u8,
                    ((vi(1) * s) >> 8) as u8,
                    ((vi(2) * s) >> 8) as u8,
                    v[4],
                ],
                [v[0], v[1], v[2], v[5]],
            )
        }
        CEM_LDR_RGB_DIRECT | CEM_LDR_RGBA_DIRECT => {
            let (la, ha) = if cem == CEM_LDR_RGBA_DIRECT {
                (vi(6), vi(7))
            } else {
                (0xFF, 0xFF)
            };
            if vi(1) + vi(3) + vi(5) >= vi(0) + vi(2) + vi(4) {
                ([v[0], v[2], v[4], la as u8], [v[1], v[3], v[5], ha as u8])
            } else {
                let e0 = blue_contract(vi(1), vi(3), vi(5), ha);
                let e1 = blue_contract(vi(0), vi(2), vi(4), la);
                (
                    [e0[0] as u8, e0[1] as u8, e0[2] as u8, e0[3] as u8],
                    [e1[0] as u8, e1[1] as u8, e1[2] as u8, e1[3] as u8],
                )
            }
        }
        CEM_LDR_RGB_BASE_PLUS_OFFSET | CEM_LDR_RGBA_BASE_PLUS_OFFSET => {
            let (mut v0, mut v1, mut v2, mut v3, mut v4, mut v5) =
                (vi(0), vi(1), vi(2), vi(3), vi(4), vi(5));
            bit_transfer_signed_dec(&mut v1, &mut v0);
            bit_transfer_signed_dec(&mut v3, &mut v2);
            bit_transfer_signed_dec(&mut v5, &mut v4);
            let (mut v6, mut v7) = (0xFF, 0);
            if cem == CEM_LDR_RGBA_BASE_PLUS_OFFSET {
                v6 = vi(6);
                v7 = vi(7);
                bit_transfer_signed_dec(&mut v7, &mut v6);
            }
            let (la, ha) = if cem == CEM_LDR_RGBA_BASE_PLUS_OFFSET {
                (v6, v6 + v7)
            } else {
                (0xFF, 0xFF)
            };
            if v1 + v3 + v5 >= 0 {
                (
                    [clamp8(v0), clamp8(v2), clamp8(v4), clamp8(la)],
                    [
                        clamp8(v0 + v1),
                        clamp8(v2 + v3),
                        clamp8(v4 + v5),
                        clamp8(ha),
                    ],
                )
            } else {
                let e0 = blue_contract(v0 + v1, v2 + v3, v4 + v5, ha);
                let e1 = blue_contract(v0, v2, v4, la);
                (
                    [clamp8(e0[0]), clamp8(e0[1]), clamp8(e0[2]), clamp8(e0[3])],
                    [clamp8(e1[0]), clamp8(e1[1]), clamp8(e1[2]), clamp8(e1[3])],
                )
            }
        }
        _ => ([0; 4], [0; 4]),
    }
}

/// Dequantize the raw ISE symbols, then decode them. Returns `(low, high)`
/// RGBA.
pub fn decode_endpoints(cem: u32, endpoints: &[u8], ise_range: u32) -> ([u8; 4], [u8; 4]) {
    let total = num_cem_values(cem);
    let mut dequant = [0u8; 8];
    if ise_range == 20 {
        dequant[..total].copy_from_slice(&endpoints[..total]);
    } else {
        let tab = &dequant_tables().endpoints[(ise_range - 4) as usize];
        for i in 0..total {
            dequant[i] = tab[endpoints[i] as usize];
        }
    }
    decode_endpoint_vals(cem, &dequant)
}

/// Whether these endpoints would trigger blue contraction on decode: the
/// second endpoint's RGB sum falls below the first's (read per CEM). CEMs
/// without blue contraction return false.
pub fn used_blue_contraction(cem: u32, endpoints: &[u8], ise_range: u32) -> bool {
    let dq = |v: u8| -> i32 {
        if ise_range == 20 {
            v as i32
        } else {
            dequant_tables().endpoints[(ise_range - 4) as usize][v as usize] as i32
        }
    };
    match cem {
        8 | 12 => {
            let s0 = dq(endpoints[0]) + dq(endpoints[2]) + dq(endpoints[4]);
            let s1 = dq(endpoints[1]) + dq(endpoints[3]) + dq(endpoints[5]);
            s1 < s0
        }
        9 | 13 => {
            let mut sum = 0;
            for i in 0..3 {
                let (mut a, mut b) = (dq(endpoints[1 + i * 2]), dq(endpoints[i * 2]));
                bit_transfer_signed_dec(&mut a, &mut b);
                sum += a;
            }
            sum < 0
        }
        _ => false,
    }
}

/// Step an endpoint symbol by `delta` ranks within its ISE range, clamped to
/// the range's rank span.
pub fn apply_delta_to_bise_endpoint_val(ise_range: u32, ise_val: u8, delta: i32) -> u8 {
    if delta == 0 {
        return ise_val;
    }
    let n = ise_levels(ise_range) as i32;
    let qt = quant_tables();
    let r = (ise_range - 4) as usize;
    let cur_rank = qt.endpoint_ise_to_rank[r][ise_val as usize] as i32;
    let new_rank = (cur_rank + delta).clamp(0, n - 1);
    qt.endpoint_rank_to_ise[r][new_rank as usize]
}

/// Quantizer that preserves the value's top 2 MSBs, using the shared preserve
/// tables. Identity at 256 levels (ISE range 20).
#[inline]
fn quant_preserve2(ise_range: u32, v: u8) -> u8 {
    if ise_range == 20 {
        v
    } else {
        preserve_tables().preserve2[(ise_range - 4) as usize][v as usize]
    }
}

/// For each endpoint range and nudge direction, the nearest ISE symbol whose
/// decoded 6-bit delta moves in the wanted direction without disturbing the
/// transferred MSB. Indexed `[range - 4][neg][symbol]`.
struct BaseOfsNudges {
    tabs: [[[u8; 256]; 2]; 17],
}

/// Build (once) and return the base+offset nudge tables.
fn base_ofs_nudges() -> &'static BaseOfsNudges {
    static TABLES: OnceBox<BaseOfsNudges> = OnceBox::new();
    TABLES.get_or_init(|| {
        let dq = dequant_tables();
        let mut t = Box::new(BaseOfsNudges {
            tabs: [[[0; 256]; 2]; 17],
        });
        for range in 4..=20u32 {
            let n = ise_levels(range) as usize;
            let tab = &dq.endpoints[(range - 4) as usize];
            for (pos_or_neg, delta) in [(0usize, 1i32), (1, -1)] {
                for cur_ise in 0..n {
                    let (mut cur_a, mut cur_b) = (tab[cur_ise] as i32, 0i32);
                    bit_transfer_signed_dec(&mut cur_a, &mut cur_b);
                    let mut best_err = i32::MAX;
                    let mut best_trial = cur_ise;
                    for trial_ise in 0..n {
                        let (mut ta, mut tb) = (tab[trial_ise] as i32, 0i32);
                        bit_transfer_signed_dec(&mut ta, &mut tb);
                        if cur_b != tb || ta == cur_a {
                            continue;
                        }
                        if delta < 0 {
                            if ta > cur_a {
                                continue;
                            }
                        } else if ta < cur_a {
                            continue;
                        }
                        let e = (ta - cur_a).abs();
                        if e < best_err {
                            best_err = e;
                            best_trial = trial_ise;
                        }
                    }
                    t.tabs[(range - 4) as usize][pos_or_neg][cur_ise] = best_trial as u8;
                }
            }
        }
        t
    })
}

/// Requantize endpoints from `src_range` to `dst_range` while preserving the
/// blue-contraction / base+offset semantics (top MSBs, delta-sum sign).
pub fn requantize_ise_endpoints(
    cem: u32,
    src_range: u32,
    src: &[u8],
    dst_range: u32,
    dst: &mut [u8],
) -> bool {
    let n = num_cem_values(cem);
    if src_range == dst_range {
        dst[..n].copy_from_slice(&src[..n]);
        return true;
    }

    let mut temp = [0u8; 8];
    let dequant_src: &[u8] = if src_range != 20 {
        let tab = &dequant_tables().endpoints[(src_range - 4) as usize];
        for i in 0..n {
            temp[i] = tab[src[i] as usize];
        }
        &temp[..n]
    } else {
        &src[..n]
    };

    if dst_range == 20 {
        dst[..n].copy_from_slice(dequant_src);
        return true;
    }

    let qt = quant_tables();
    let dst_quant = &qt.endpoint_val_to_ise[(dst_range - 4) as usize];
    let dst_dequant = &dequant_tables().endpoints[(dst_range - 4) as usize];

    if cem == CEM_LDR_RGB_BASE_PLUS_OFFSET || cem == CEM_LDR_RGBA_BASE_PLUS_OFFSET {
        for i in 0..n {
            // Preserve v1,v3,v5,v7's 2 MSBs across the requant.
            dst[i] = if i & 1 != 0 {
                quant_preserve2(dst_range, dequant_src[i])
            } else {
                dst_quant[dequant_src[i] as usize]
            };
        }

        let src_used_bc = used_blue_contraction(cem, src, src_range);

        let deltas_sum = |dst: &[u8]| -> i32 {
            let mut s = 0;
            for i in 0..3 {
                let (mut a, mut b) = (
                    dst_dequant[dst[1 + i * 2] as usize] as i32,
                    dst_dequant[dst[i * 2] as usize] as i32,
                );
                bit_transfer_signed_dec(&mut a, &mut b);
                s += a;
            }
            s
        };

        let mut quant_used_bc = deltas_sum(dst) < 0;

        const MAX_TRIES: u32 = 5;
        if src_used_bc != quant_used_bc {
            let nudge_delta: i32 = if quant_used_bc { 1 } else { -1 };
            let nudges = base_ofs_nudges();
            let nt = &nudges.tabs[(dst_range - 4) as usize][usize::from(nudge_delta < 0)];
            let mut cur_c_rover = 2usize; // blue first
            for _ in 0..MAX_TRIES {
                for j in 0..3 {
                    let i = (cur_c_rover + j) % 3;
                    let new_ise = nt[dst[1 + i * 2] as usize];
                    if new_ise != dst[1 + i * 2] {
                        dst[1 + i * 2] = new_ise;
                        break;
                    }
                }
                quant_used_bc = deltas_sum(dst) < 0;
                if src_used_bc == quant_used_bc {
                    break;
                }
                cur_c_rover += 1;
            }
            // If the retries run out, keep the endpoints as they are; the
            // residual blue-contraction mismatch is harmless.
        }
    } else if cem == CEM_LDR_RGB_DIRECT || cem == CEM_LDR_RGBA_DIRECT {
        let s0 = dequant_src[0] as u32 + dequant_src[2] as u32 + dequant_src[4] as u32;
        let s1 = dequant_src[1] as u32 + dequant_src[3] as u32 + dequant_src[5] as u32;
        let orig_used_bc = s1 < s0;

        for i in 0..n {
            dst[i] = dst_quant[dequant_src[i] as usize];
        }

        let dq_s0 = dst_dequant[dst[0] as usize] as u32
            + dst_dequant[dst[2] as usize] as u32
            + dst_dequant[dst[4] as usize] as u32;
        let dq_s1 = dst_dequant[dst[1] as usize] as u32
            + dst_dequant[dst[3] as usize] as u32
            + dst_dequant[dst[5] as usize] as u32;
        let quant_used_bc = dq_s1 < dq_s0;

        if orig_used_bc != quant_used_bc {
            if dq_s0 == dq_s1 {
                if dq_s1 != 0 {
                    // Decrease s1.
                    for i in 0..3 {
                        let new_ise =
                            apply_delta_to_bise_endpoint_val(dst_range, dst[1 + i * 2], -1);
                        if new_ise != dst[1 + i * 2] {
                            dst[1 + i * 2] = new_ise;
                            break;
                        }
                    }
                } else {
                    // Both zero: increase s0.
                    for i in 0..3 {
                        let new_ise = apply_delta_to_bise_endpoint_val(dst_range, dst[i * 2], 1);
                        if new_ise != dst[i * 2] {
                            dst[i * 2] = new_ise;
                            break;
                        }
                    }
                }
            } else {
                dst.swap(0, 1);
                dst.swap(2, 3);
                dst.swap(4, 5);
                if cem == CEM_LDR_RGBA_DIRECT {
                    dst.swap(6, 7);
                }
            }
        }
    } else {
        for i in 0..n {
            dst[i] = dst_quant[dequant_src[i] as usize];
        }
    }

    true
}

/// Encode decoded endpoints as CEM 9 or 13 with the requested blue-contraction
/// state, swapping and clamping until the delta-sum sign matches.
#[allow(clippy::too_many_arguments)]
fn pack_base_offset(
    cem: u32,
    dst_range: u32,
    packed: &mut [u8],
    l: [u8; 4],
    h: [u8; 4],
    mut use_blue_contraction: bool,
    auto_disable_bc_if_clamped: bool,
    bc_clamped: &mut bool,
    base_ofs_clamped: &mut bool,
) -> bool {
    *bc_clamped = false;
    *base_ofs_clamped = false;

    let mut pack_l = l;
    let mut pack_h = h;

    if use_blue_contraction {
        let enc_l = blue_contract_enc(pack_l, bc_clamped, pack_l[2] as i32);
        let enc_h = blue_contract_enc(pack_h, bc_clamped, pack_h[2] as i32);
        if *bc_clamped && auto_disable_bc_if_clamped {
            use_blue_contraction = false;
        } else {
            pack_h = enc_l;
            pack_l = enc_h;
        }
    }

    let (mut dr, mut dg, mut db, mut da) = (0i32, 0i32, 0i32, 0i32);
    let mut low_clamp = -32i32;

    for pass in 0..4u32 {
        let orig_dr = pack_h[0] as i32 - pack_l[0] as i32;
        let orig_dg = pack_h[1] as i32 - pack_l[1] as i32;
        let orig_db = pack_h[2] as i32 - pack_l[2] as i32;
        let orig_da = pack_h[3] as i32 - pack_l[3] as i32;

        *base_ofs_clamped = false;
        dr = orig_dr.clamp(low_clamp, 31);
        if dr != orig_dr {
            *base_ofs_clamped = true;
        }
        dg = orig_dg.clamp(low_clamp, 31);
        if dg != orig_dg {
            *base_ofs_clamped = true;
        }
        db = orig_db.clamp(low_clamp, 31);
        if db != orig_db {
            *base_ofs_clamped = true;
        }
        da = orig_da.clamp(low_clamp, 31);
        if da != orig_da {
            *base_ofs_clamped = true;
        }

        let s = dr + dg + db;
        let pack_uses_bc = s < 0;
        if pack_uses_bc == use_blue_contraction {
            break;
        }

        if s == 0 {
            // Force the sum negative (blue first, then red, then green).
            if db > -32 {
                db -= 1;
            } else if dr > -32 {
                dr -= 1;
            } else if dg > -32 {
                dg -= 1;
            }
            break;
        }

        if pass == 3 {
            break; // theoretically unreachable
        }
        if pass == 1 {
            low_clamp = -31;
        }
        core::mem::swap(&mut pack_l, &mut pack_h);
    }

    let (mut v0, mut v2, mut v4) = (pack_l[0] as i32, pack_l[1] as i32, pack_l[2] as i32);
    let (mut v1, mut v3, mut v5) = (dr, dg, db);
    bit_transfer_signed_enc(&mut v1, &mut v0);
    bit_transfer_signed_enc(&mut v3, &mut v2);
    bit_transfer_signed_enc(&mut v5, &mut v4);

    let mut new8 = [0u8; 8];
    new8[0] = v0 as u8;
    new8[1] = v1 as u8;
    new8[2] = v2 as u8;
    new8[3] = v3 as u8;
    new8[4] = v4 as u8;
    new8[5] = v5 as u8;
    if cem_has_alpha(cem) {
        let (mut v6, mut v7) = (pack_l[3] as i32, da);
        bit_transfer_signed_enc(&mut v7, &mut v6);
        new8[6] = v6 as u8;
        new8[7] = v7 as u8;
    }

    requantize_ise_endpoints(cem, 20, &new8, dst_range, packed)
}

/// Predict and encode `dst_cem` endpoints from a source block's endpoints of
/// any LDR CEM.
#[allow(clippy::too_many_arguments)]
pub fn convert_endpoints_across_cems(
    prev_cem: u32,
    prev_range: u32,
    prev_endpoints: &[u8],
    dst_cem: u32,
    dst_range: u32,
    dst: &mut [u8],
    always_repack: bool,
    mut use_blue_contraction: bool,
    auto_disable_bc_if_clamped: bool,
    bc_clamped: &mut bool,
    base_ofs_clamped: &mut bool,
) -> bool {
    *bc_clamped = false;
    *base_ofs_clamped = false;

    let num_dst_vals = num_cem_values(dst_cem);
    let qt = quant_tables();
    let dst_quant = &qt.endpoint_val_to_ise[(dst_range - 4) as usize];
    let dst_dequant = &dequant_tables().endpoints[(dst_range - 4) as usize];

    if prev_cem == dst_cem && !always_repack {
        return requantize_ise_endpoints(prev_cem, prev_range, prev_endpoints, dst_range, dst);
    }

    if !always_repack {
        let prev_base = base_cem_without_alpha(prev_cem);
        let dst_base = base_cem_without_alpha(dst_cem);

        if prev_base == dst_base && !cem_has_alpha(dst_cem) {
            // Alpha stripped: requantize the base values only.
            return requantize_ise_endpoints(prev_base, prev_range, prev_endpoints, dst_range, dst);
        }

        if prev_base == dst_base && cem_has_alpha(dst_cem) {
            // Alpha added: requantize base, then plug in opaque alpha.
            if !requantize_ise_endpoints(prev_base, prev_range, prev_endpoints, dst_range, dst) {
                return false;
            }
            let ise_a = dst_quant[255];
            match dst_cem {
                CEM_LDR_LUM_ALPHA_DIRECT => {
                    dst[2] = ise_a;
                    dst[3] = ise_a;
                }
                CEM_LDR_RGBA_DIRECT => {
                    dst[6] = ise_a;
                    dst[7] = ise_a;
                }
                CEM_LDR_RGB_BASE_SCALE_PLUS_TWO_A => {
                    dst[4] = ise_a;
                    dst[5] = ise_a;
                }
                CEM_LDR_RGBA_BASE_PLUS_OFFSET => {
                    dst[6] = ise_a; // base
                    dst[7] = dst_quant[128]; // offset: +0 delta, MSB preserved
                }
                _ => return false,
            }
            return true;
        }
    }

    // Cross-class: fully decode, then re-encode into the destination CEM.
    let (prev_l, prev_h) = decode_endpoints(prev_cem, prev_endpoints, prev_range);
    let mut new8 = [0u8; 8];

    match dst_cem {
        CEM_LDR_LUM_DIRECT | CEM_LDR_LUM_ALPHA_DIRECT => {
            new8[0] = ((prev_l[0] as u32 + prev_l[1] as u32 + prev_l[2] as u32 + 1) / 3) as u8;
            new8[1] = ((prev_h[0] as u32 + prev_h[1] as u32 + prev_h[2] as u32 + 1) / 3) as u8;
            if dst_cem == CEM_LDR_LUM_ALPHA_DIRECT {
                new8[2] = prev_l[3];
                new8[3] = prev_h[3];
            }
            if prev_cem != CEM_LDR_LUM_DIRECT
                && prev_cem != CEM_LDR_LUM_ALPHA_DIRECT
                && new8[0] > new8[1]
            {
                new8.swap(0, 1);
                new8.swap(2, 3);
            }
            requantize_ise_endpoints(dst_cem, 20, &new8, dst_range, dst)
        }
        CEM_LDR_RGB_DIRECT | CEM_LDR_RGBA_DIRECT => {
            new8[0] = prev_l[0];
            new8[1] = prev_h[0];
            new8[2] = prev_l[1];
            new8[3] = prev_h[1];
            new8[4] = prev_l[2];
            new8[5] = prev_h[2];
            if dst_cem == CEM_LDR_RGBA_DIRECT {
                new8[6] = prev_l[3];
                new8[7] = prev_h[3];
            }

            if use_blue_contraction {
                let enc_l = blue_contract_enc(
                    prev_l,
                    bc_clamped,
                    dst_dequant[dst_quant[prev_l[2] as usize] as usize] as i32,
                );
                let enc_h = blue_contract_enc(
                    prev_h,
                    bc_clamped,
                    dst_dequant[dst_quant[prev_h[2] as usize] as usize] as i32,
                );
                if auto_disable_bc_if_clamped && *bc_clamped {
                    use_blue_contraction = false;
                } else {
                    new8[0] = enc_h[0];
                    new8[1] = enc_l[0];
                    new8[2] = enc_h[1];
                    new8[3] = enc_l[1];
                    new8[4] = enc_h[2];
                    new8[5] = enc_l[2];
                    if dst_cem == CEM_LDR_RGBA_DIRECT {
                        new8[6] = prev_h[3];
                        new8[7] = prev_l[3];
                    }
                }
            }

            let s0 = new8[0] as u32 + new8[2] as u32 + new8[4] as u32;
            let s1 = new8[1] as u32 + new8[3] as u32 + new8[5] as u32;
            let pack_used_bc = s1 < s0;

            if pack_used_bc != use_blue_contraction {
                if s0 == s1 {
                    if s1 != 0 {
                        for i in 0..3 {
                            let new_ise = apply_delta_to_bise_endpoint_val(20, new8[1 + i * 2], -1);
                            if new_ise != new8[1 + i * 2] {
                                new8[1 + i * 2] = new_ise;
                                break;
                            }
                        }
                    } else {
                        for i in 0..3 {
                            let new_ise = apply_delta_to_bise_endpoint_val(20, new8[i * 2], 1);
                            if new_ise != new8[i * 2] {
                                new8[i * 2] = new_ise;
                                break;
                            }
                        }
                    }
                } else {
                    let mut i = 0;
                    while i < num_dst_vals {
                        new8.swap(i, i + 1);
                        i += 2;
                    }
                }
            }

            requantize_ise_endpoints(dst_cem, 20, &new8, dst_range, dst)
        }
        CEM_LDR_RGB_BASE_SCALE | CEM_LDR_RGB_BASE_SCALE_PLUS_TWO_A => {
            let mut lc = prev_l;
            let mut hc = prev_h;
            let prev_is_base_scale =
                prev_cem == CEM_LDR_RGB_BASE_SCALE || prev_cem == CEM_LDR_RGB_BASE_SCALE_PLUS_TWO_A;
            if !prev_is_base_scale
                && lc[0] as u32 + lc[1] as u32 + lc[2] as u32
                    > hc[0] as u32 + hc[1] as u32 + hc[2] as u32
            {
                core::mem::swap(&mut lc, &mut hc);
            }
            new8[0] = hc[0];
            new8[1] = hc[1];
            new8[2] = hc[2];
            {
                let id = lc[0] as i32 * hc[0] as i32
                    + lc[1] as i32 * hc[1] as i32
                    + lc[2] as i32 * hc[2] as i32;
                let inrm = hc[0] as i32 * hc[0] as i32
                    + hc[1] as i32 * hc[1] as i32
                    + hc[2] as i32 * hc[2] as i32;
                const IMAX_S: i32 = (1024 * 255) / 256;
                let mut iscale = if inrm > 0 { (id * 1024) / inrm } else { IMAX_S };
                iscale = iscale.clamp(0, IMAX_S);
                iscale = (iscale + 2) >> 2;
                new8[3] = iscale.clamp(0, 255) as u8;
            }
            if dst_cem == CEM_LDR_RGB_BASE_SCALE_PLUS_TWO_A {
                new8[4] = lc[3];
                new8[5] = hc[3];
                if !prev_is_base_scale && new8[4] > new8[5] {
                    new8.swap(4, 5);
                }
            }
            requantize_ise_endpoints(dst_cem, 20, &new8, dst_range, dst)
        }
        CEM_LDR_RGB_BASE_PLUS_OFFSET | CEM_LDR_RGBA_BASE_PLUS_OFFSET => pack_base_offset(
            dst_cem,
            dst_range,
            dst,
            prev_l,
            prev_h,
            use_blue_contraction,
            auto_disable_bc_if_clamped,
            bc_clamped,
            base_ofs_clamped,
        ),
        _ => false,
    }
}
