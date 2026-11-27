//! Real-time unsigned BC6H encoder for 4x4 blocks of RGB half-floats, used by
//! the ASTC HDR 6x6 BC6H transcode. The fast bit-trick half/float conversions
//! and the `inv_sqrt` approximation are part of the encoding itself: they
//! define the exact quantization, so the packed block depends on using these
//! forms rather than exact library math. Every `f32` step keeps a fixed
//! evaluation order and avoids fused multiply-add, so the output is the same on
//! every platform.
// Index-based loops are kept where they map directly onto the block layout.
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

use crate::fastenc::bc7f::{BC7_WEIGHTS3, BC7_WEIGHTS4};
use crate::mathf::{fabsf, roundf};
use crate::once::OnceBox;
use crate::uastc::bc7_tables::{G_BC7_ANCHOR_SECOND, G_BC7_PARTITION2};
use crate::uastc_hdr::bc6h::{half_to_blog, pack_block, LogicalBlock};
use crate::uastc_hdr::bc6h_tables::BC6H_MODE_SIG_BITS;
use alloc::boxed::Box;

/// Encoder configuration. The 6x6 transcode path only varies
/// `max_2subset_pats_to_try` (0, or 1 under HIGH_QUALITY); the other fields
/// keep their defaults.
pub struct FastBc6hParams {
    /// How many of the 2-subset difference-endpoint modes to fit before falling
    /// back to the absolute-endpoint mode (0 skips them entirely). The value
    /// picks how far down the largest-base-bits-first mode order to walk.
    pub num_diff_endpoint_modes_to_try: u32,
    /// Number of 2-subset partition patterns to fit (0 disables the 2-subset
    /// search; `BC6H_NUM_PATS` tries all 32, otherwise the N closest to the
    /// block's own principal-axis split).
    pub max_2subset_pats_to_try: u32,
    /// Run a second least-squares refinement pass on the 1-subset endpoints.
    pub hq_ls: bool,
    /// Assign 4-bit weights by testing all 16 ramp entries per texel instead of
    /// the projection-and-bisect shortcut.
    pub brute_force_weight4_assignment: bool,
}

impl Default for FastBc6hParams {
    /// The encoder settings the 6x6 transcode path starts from before it
    /// overrides `max_2subset_pats_to_try` per the HIGH_QUALITY flag.
    fn default() -> Self {
        Self {
            hq_ls: true,
            // consider two of the difference-endpoint modes
            num_diff_endpoint_modes_to_try: 2,
            max_2subset_pats_to_try: 1,
            brute_force_weight4_assignment: false,
        }
    }
}

/// The BC6H 4-bit interpolation weight ramp (identical to the BC7 4-bit ramp).
const BC6H_WEIGHTS4: [u32; 16] = BC7_WEIGHTS4;

// 2^-14 = 0.00006103515625, the smallest positive normal half (exact in f32).
const MIN_HALF_FLOAT: f32 = 1.0 / 16384.0;
const MAX_HALF_FLOAT: f32 = 65504.0;
const MAX_HALF_FLOAT_AS_INT_BITS: u32 = 0x7BFF;
const MAX_BC6H_HALF_FLOAT_AS_UINT: u32 = 0x7BFF;
const SMALL_FLOAT_VAL: f32 = 0.000_012_5;
const REALLY_SMALL_FLOAT_VAL: f32 = 0.000_000_125;
const BIG_FLOAT_VAL: f32 = 1e30;

const FAST_BC6H_STD_DEV_THRESH: i64 = 256;
const FAST_BC6H_COMPLEX_STD_DEV_THRESH: i64 = 512;
const FAST_BC6H_VERY_COMPLEX_STD_DEV_THRESH: i64 = 2048;
const BC6H_NUM_PATS: usize = 32;
const BC6H_2SUBSET_ABS_ENDPOINT_MODE: usize = 9;
const BC6H_FIRST_1SUBSET_MODE_INDEX: usize = 10;

/// Round-half-away-from-zero via truncation.
#[inline]
fn fast_roundf_int(x: f32) -> i32 {
    if x >= 0.0 {
        (x + 0.5) as i32
    } else {
        (x - 0.5) as i32
    }
}

/// Convert a non-negative, finite `f32` to a half-float bit pattern by an
/// exponent-rebias multiply then a truncate-with-nudge (mantissa > 4096 rounds
/// up). Does not clamp negatives, NaN, or infinities.
#[inline]
fn fast_float_to_half(f: f32) -> u16 {
    // 0x0780_0000 as an f32 is 2^-112, the exponent-rebias factor.
    let g_f_to_h = f32::from_bits(0x0780_0000);
    let fu = (f * g_f_to_h).to_bits();
    let mut h = (fu >> (23 - 10)) & 0x7FFF;
    let mant = fu & 8191;
    h += u32::from(mant > 4096);
    if h > MAX_HALF_FLOAT_AS_INT_BITS {
        h = MAX_HALF_FLOAT_AS_INT_BITS;
    }
    h as u16
}

/// Round an `f32` to the nearest half-float and back to `f32`, preserving sign.
#[inline]
fn ftoh(f: f32) -> f32 {
    let res = fast_float_to_half(fabsf(f)) as f32;
    if f < 0.0 {
        -res
    } else {
        res
    }
}

/// Convert a half-float bit pattern (non-negative, finite) to `f32` by a shift
/// and an exponent-rebias multiply.
#[inline]
fn half_to_float(h: u16) -> f32 {
    // K = bits 0x7780_0000 as float: 2^112.
    let k = f32::from_bits(0x7780_0000);
    f32::from_bits((h as u32) << 13) * k
}

/// Fast reciprocal square root: the classic bit-trick seed plus one Newton
/// refinement step.
#[inline]
fn inv_sqrt(v: f32) -> f32 {
    let u = 0x5F1F_FFF9u32.wrapping_sub(v.to_bits() >> 1);
    let flt = f32::from_bits(u);
    0.703_952_25 * flt * (2.389_244_6 - v * (flt * flt))
}

/// Dequantize a `bits`-wide BC6H endpoint code to a 16-bit value.
#[inline]
fn bc6h_dequantize(val: i32, bits: u32) -> i32 {
    if bits >= 15 {
        val
    } else if val == 0 {
        0
    } else if val == (1 << bits) - 1 {
        0xFFFF
    } else {
        ((val << 16) + 0x8000) >> bits
    }
}

/// Scale a dequantized BC6H value into the half-float domain (multiply by
/// 31/64).
#[inline]
fn bc6h_convert_to_half(val: i32) -> u32 {
    ((val * 31) >> 6) as u32
}

/// Round-trip each endpoint through the `bits`-wide blog quantizer and back to
/// the half-float domain (every call site passes `bits` = 10).
fn bc6h_quant_dequant_endpoints(e: &mut [u32; 6], bits: u32) {
    for v in e.iter_mut() {
        *v = bc6h_convert_to_half(bc6h_dequantize(half_to_blog(*v as u16, bits) as i32, bits));
    }
}

/// Quantize six half-float endpoints to `bits`-wide blog codes.
fn bc6h_quant_endpoints(h: &[u32; 6], b: &mut [u32; 6], bits: u32) {
    for i in 0..6 {
        b[i] = half_to_blog(h[i] as u16, bits);
    }
}

/// Dequantize six blog endpoint codes back to half-float values.
fn bc6h_dequant_endpoints(b: &[u32; 6], h: &mut [u32; 6], bits: u32) {
    for i in 0..6 {
        h[i] = bc6h_convert_to_half(bc6h_dequantize(b[i] as i32, bits));
    }
}

/// One float texel, RGB.
type Vec3F = [f32; 3];

/// The least-squares weight tabs and the 32 BC6H 2-subset partition patterns,
/// built once on first use.
struct Tables {
    ls3: [[f32; 4]; 8],
    ls4: [[f32; 4]; 16],
    pats2: [u32; BC6H_NUM_PATS],
}

/// The [`Tables`], built on first call and shared for the process lifetime.
fn tables() -> &'static Tables {
    static TABLES: OnceBox<Tables> = OnceBox::new();
    TABLES.get_or_init(|| {
        let mut t = Box::new(Tables {
            ls3: [[0.0; 4]; 8],
            ls4: [[0.0; 4]; 16],
            pats2: [0; BC6H_NUM_PATS],
        });
        for i in 0..8 {
            let w = BC7_WEIGHTS3[i] as f32 * (1.0 / 64.0);
            t.ls3[i] = [w * w, (1.0 - w) * w, (1.0 - w) * (1.0 - w), w];
        }
        for i in 0..16 {
            let w = BC6H_WEIGHTS4[i] as f32 * (1.0 / 64.0);
            t.ls4[i] = [w * w, (1.0 - w) * w, (1.0 - w) * (1.0 - w), w];
        }
        for pat_index in 0..BC6H_NUM_PATS {
            let mut pat_bits = 0u32;
            for j in 0..16 {
                pat_bits |= (G_BC7_PARTITION2[pat_index * 16 + j] as u32) << j;
            }
            t.pats2[pat_index] = pat_bits;
        }
        t
    })
}

/// Pick 4-bit weights against the interpolated half ramp, returning the
/// accumulated MRSSE (f32 error terms, f64 accumulator).
fn assign_weights_4(
    float_pixels: &[Vec3F; 16],
    pixel_scales: &[f32; 16],
    weights: &mut [u8; 16],
    min_r: i32,
    min_g: i32,
    min_b: i32,
    max_r: i32,
    max_g: i32,
    max_b: i32,
    block_max_var: i64,
    try_2subsets_flag: bool,
    params: &FastBc6hParams,
) -> f64 {
    let mut cr = [0f32; 16];
    let mut cg = [0f32; 16];
    let mut cb = [0f32; 16];
    for i in 0..16 {
        let w = BC6H_WEIGHTS4[i] as i32;
        cr[i] = half_to_float((((min_r * (64 - w) + max_r * w + 32) >> 6) & 0xFFFF) as u16);
        cg[i] = half_to_float((((min_g * (64 - w) + max_g * w + 32) >> 6) & 0xFFFF) as u16);
        cb[i] = half_to_float((((min_b * (64 - w) + max_b * w + 32) >> 6) & 0xFFFF) as u16);
    }

    let mut total_err = 0f64;
    if params.brute_force_weight4_assignment {
        for i in 0..16 {
            let (qr, qg, qb) = (float_pixels[i][0], float_pixels[i][1], float_pixels[i][2]);
            let mut best_err = (cr[0] - qr) * (cr[0] - qr)
                + (cg[0] - qg) * (cg[0] - qg)
                + (cb[0] - qb) * (cb[0] - qb);
            let mut best_idx = 0usize;
            for j in 1..16 {
                let rd = cr[j] - qr;
                let gd = cg[j] - qg;
                let bd = cb[j] - qb;
                let e = rd * rd + gd * gd + bd * bd;
                if e < best_err {
                    best_err = e;
                    best_idx = j;
                }
            }
            weights[i] = best_idx as u8;
            total_err += (best_err * pixel_scales[i]) as f64;
        }
        return total_err;
    }

    let dir_r = cr[15] - cr[0];
    let dir_g = cg[15] - cg[0];
    let dir_b = cb[15] - cb[0];
    let mut dots = [0f32; 16];
    for i in 0..16 {
        dots[i] = cr[i] * dir_r + cg[i] * dir_g + cb[i] * dir_b;
    }
    let mut mid_dots = [0f32; 15];
    let mut monotonically_increasing = true;
    for i in 0..15 {
        mid_dots[i] = (dots[i] + dots[i + 1]) * 0.5;
        if dots[i] > dots[i + 1] {
            monotonically_increasing = false;
        }
    }
    let check_more_colors = block_max_var
        > FAST_BC6H_VERY_COMPLEX_STD_DEV_THRESH * FAST_BC6H_VERY_COMPLEX_STD_DEV_THRESH * 16;

    if !monotonically_increasing {
        for i in 0..16 {
            let (qr, qg, qb) = (float_pixels[i][0], float_pixels[i][1], float_pixels[i][2]);
            let d = qr * dir_r + qg * dir_g + qb * dir_b;
            let mut best_e = fabsf(d - dots[0]);
            let mut best_idx = 0usize;
            for j in 1..16 {
                let e = fabsf(d - dots[j]);
                if e < best_e {
                    best_e = e;
                    best_idx = j;
                }
            }
            weights[i] = best_idx as u8;
            let err = (qr - cr[best_idx]) * (qr - cr[best_idx])
                + (qg - cg[best_idx]) * (qg - cg[best_idx])
                + (qb - cb[best_idx]) * (qb - cb[best_idx]);
            total_err += (err * pixel_scales[i]) as f64;
        }
    } else if !try_2subsets_flag || !check_more_colors {
        for i in 0..16 {
            let (qr, qg, qb) = (float_pixels[i][0], float_pixels[i][1], float_pixels[i][2]);
            let d = qr * dir_r + qg * dir_g + qb * dir_b;
            let mut low = 0usize;
            let mut mid = low + 7;
            if d >= mid_dots[mid] {
                low = mid + 1;
            }
            mid = low + 3;
            if d >= mid_dots[mid] {
                low = mid + 1;
            }
            mid = low + 1;
            if d >= mid_dots[mid] {
                low = mid + 1;
            }
            mid = low;
            if d >= mid_dots[mid] {
                low = mid + 1;
            }
            let best_idx = low;
            weights[i] = best_idx as u8;
            let err = (qr - cr[best_idx]) * (qr - cr[best_idx])
                + (qg - cg[best_idx]) * (qg - cg[best_idx])
                + (qb - cb[best_idx]) * (qb - cb[best_idx]);
            total_err += (err * pixel_scales[i]) as f64;
        }
    } else {
        for i in 0..16 {
            let (qr, qg, qb) = (float_pixels[i][0], float_pixels[i][1], float_pixels[i][2]);
            let d = qr * dir_r + qg * dir_g + qb * dir_b;
            let mut low = 0usize;
            let mut mid = low + 7;
            if d >= mid_dots[mid] {
                low = mid + 1;
            }
            mid = low + 3;
            if d >= mid_dots[mid] {
                low = mid + 1;
            }
            mid = low + 1;
            if d >= mid_dots[mid] {
                low = mid + 1;
            }
            mid = low;
            if d >= mid_dots[mid] {
                low = mid + 1;
            }
            let mut best_idx = low as i32;
            let mut err = (qr - cr[best_idx as usize]) * (qr - cr[best_idx as usize])
                + (qg - cg[best_idx as usize]) * (qg - cg[best_idx as usize])
                + (qb - cb[best_idx as usize]) * (qb - cb[best_idx as usize]);
            {
                let mut alt_idx = best_idx + 1;
                if alt_idx > 15 {
                    alt_idx = 13;
                }
                let a = alt_idx as usize;
                let alt_err = (qr - cr[a]) * (qr - cr[a])
                    + (qg - cg[a]) * (qg - cg[a])
                    + (qb - cb[a]) * (qb - cb[a]);
                if alt_err < err {
                    err = alt_err;
                    best_idx = alt_idx;
                }
            }
            {
                let mut alt_idx2 = best_idx - 1;
                if alt_idx2 < 0 {
                    alt_idx2 = 2;
                }
                let a = alt_idx2 as usize;
                let alt_err2 = (qr - cr[a]) * (qr - cr[a])
                    + (qg - cg[a]) * (qg - cg[a])
                    + (qb - cb[a]) * (qb - cb[a]);
                if alt_err2 < err {
                    err = alt_err2;
                    best_idx = alt_idx2;
                }
            }
            weights[i] = best_idx as u8;
            total_err += (err * pixel_scales[i]) as f64;
        }
    }
    total_err
}

/// Half-domain projection weight assignment for simple blocks, with a float
/// fallback when the axis dots overflow the half range.
fn assign_weights_simple_4(
    pixels: &[u16; 48],
    weights: &mut [u8; 16],
    min_r: i32,
    min_g: i32,
    min_b: i32,
    max_r: i32,
    max_g: i32,
    max_b: i32,
    block_max_var: i64,
    params: &FastBc6hParams,
) {
    let fmin_r = half_to_float(min_r as u16);
    let fmin_g = half_to_float(min_g as u16);
    let fmin_b = half_to_float(min_b as u16);
    let fmax_r = half_to_float(max_r as u16);
    let fmax_g = half_to_float(max_g as u16);
    let fmax_b = half_to_float(max_b as u16);

    let mut fdir_r = fmax_r - fmin_r;
    let mut fdir_g = fmax_g - fmin_g;
    let mut fdir_b = fmax_b - fmin_b;
    let l = inv_sqrt(fdir_r * fdir_r + fdir_g * fdir_g + fdir_b * fdir_b);
    if l != 0.0 {
        fdir_r *= l;
        fdir_g *= l;
        fdir_b *= l;
    }
    let lf = fmin_r * fdir_r + fmin_g * fdir_g + fmin_b * fdir_b;
    let hf = fmax_r * fdir_r + fmax_g * fdir_g + fmax_b * fdir_b;

    if lf >= MAX_HALF_FLOAT || hf >= MAX_HALF_FLOAT {
        // Fallback: the half-domain tricks below would overflow.
        let mut float_pixels = [[0f32; 3]; 16];
        let mut pixel_scales = [0f32; 16];
        for i in 0..16 {
            float_pixels[i][0] = half_to_float(pixels[i * 3]);
            float_pixels[i][1] = half_to_float(pixels[i * 3 + 1]);
            float_pixels[i][2] = half_to_float(pixels[i * 3 + 2]);
            pixel_scales[i] = 1.0
                / (float_pixels[i][0] * float_pixels[i][0]
                    + float_pixels[i][1] * float_pixels[i][1]
                    + float_pixels[i][2] * float_pixels[i][2]
                    + MIN_HALF_FLOAT);
        }
        assign_weights_4(
            &float_pixels,
            &pixel_scales,
            weights,
            min_r,
            min_g,
            min_b,
            max_r,
            max_g,
            max_b,
            block_max_var,
            false,
            params,
        );
        return;
    }

    let lr = ftoh(lf);
    let hr = ftoh(hf);
    let frr = if hr == lr { 0.0 } else { 14.93333 / (hr - lr) };
    let lr = (-lr * frr) + 0.53333;
    for i in 0..16 {
        let r = half_to_float(pixels[i * 3]);
        let g = half_to_float(pixels[i * 3 + 1]);
        let b = half_to_float(pixels[i * 3 + 2]);
        let w = ftoh((r * fdir_r + g * fdir_g + b * fdir_b).min(MAX_HALF_FLOAT));
        weights[i] = ((w * frr + lr) as i32).clamp(0, 15) as u8;
    }
}

/// Pick the nearest interpolated color per texel as a 3-bit weight, over two
/// subsets. Returns the accumulated MRSSE when `pixel_scales` is supplied.
fn assign_weights3(
    trial_weights: &mut [u8; 16],
    best_pat_bits: u32,
    subset_min: &[[u32; 3]; 2],
    subset_max: &[[u32; 3]; 2],
    float_pixels: &[Vec3F; 16],
    pixel_scales: Option<&[f32; 16]>,
) -> f64 {
    let mut subset_c = [[[0f32; 3]; 8]; 2];
    for subset in 0..2 {
        for j in 0..8 {
            let w = BC7_WEIGHTS3[j];
            for c in 0..3 {
                subset_c[subset][j][c] = half_to_float(
                    ((subset_min[subset][c] * (64 - w) + subset_max[subset][c] * w + 32) >> 6)
                        as u16,
                );
            }
        }
    }
    let mut trial_error = 0f64;
    for i in 0..16 {
        let subset = ((best_pat_bits >> i) & 1) as usize;
        let (qr, qg, qb) = (float_pixels[i][0], float_pixels[i][1], float_pixels[i][2]);
        let sc = &subset_c[subset];
        let mut best_error = (sc[0][0] - qr) * (sc[0][0] - qr)
            + (sc[0][1] - qg) * (sc[0][1] - qg)
            + (sc[0][2] - qb) * (sc[0][2] - qb);
        let mut best_idx = 0usize;
        for j in 1..8 {
            let e = (sc[j][0] - qr) * (sc[j][0] - qr)
                + (sc[j][1] - qg) * (sc[j][1] - qg)
                + (sc[j][2] - qb) * (sc[j][2] - qb);
            if e < best_error {
                best_error = e;
                best_idx = j;
            }
        }
        trial_weights[i] = best_idx as u8;
        if let Some(scales) = pixel_scales {
            trial_error += (best_error * scales[i]) as f64;
        }
    }
    trial_error
}

/// Fit one 2-subset partition pattern (per-subset PCA axis, extreme-dot
/// endpoints, unquantized least squares, BC6H mode fit, weight/error trial) and
/// keep it when it beats `cur_error`.
fn fast_encode_bc6h_2subsets_pattern(
    best_pat_index: u32,
    best_pat_bits: u32,
    pixels: &[u16; 48],
    float_pixels: &[Vec3F; 16],
    pixel_scales: &[f32; 16],
    cur_error: &mut f64,
    log_blk: &mut LogicalBlock,
    mean_r: i32,
    mean_g: i32,
    mean_b: i32,
    params: &FastBc6hParams,
) {
    let t = tables();

    let mut subset_icov = [[0i64; 6]; 2];
    for i in 0..16 {
        let subset = ((best_pat_bits >> i) & 1) as usize;
        let r = pixels[i * 3] as i32 - mean_r;
        let g = pixels[i * 3 + 1] as i32 - mean_g;
        let b = pixels[i * 3 + 2] as i32 - mean_b;
        subset_icov[subset][0] += (r * r) as i64;
        subset_icov[subset][1] += (r * g) as i64;
        subset_icov[subset][2] += (r * b) as i64;
        subset_icov[subset][3] += (g * g) as i64;
        subset_icov[subset][4] += (g * b) as i64;
        subset_icov[subset][5] += (b * b) as i64;
    }

    let mut subset_axis = [[0f32; 3]; 2];
    for subset in 0..2 {
        let mut cov = [0f32; 6];
        for i in 0..6 {
            cov[i] = subset_icov[subset][i] as f32;
        }
        let sc = 1.0f32 / (cov[0].max(cov[3]).max(cov[5]) + REALLY_SMALL_FLOAT_VAL);
        let wx = sc * cov[0];
        let wy = sc * cov[3];
        let wz = sc * cov[5];
        let alt_xr = cov[0] * wx + cov[1] * wy + cov[2] * wz;
        let alt_xg = cov[1] * wx + cov[3] * wy + cov[4] * wz;
        let alt_xb = cov[2] * wx + cov[4] * wy + cov[5] * wz;
        let l = alt_xr * alt_xr + alt_xg * alt_xg + alt_xb * alt_xb;
        let mut axis = [0.577_350_26f32; 3];
        if fabsf(l) >= SMALL_FLOAT_VAL {
            let inv_l = inv_sqrt(l);
            axis = [alt_xr * inv_l, alt_xg * inv_l, alt_xb * inv_l];
        }
        subset_axis[subset] = axis;
    }

    let mut subset_min_dot = [BIG_FLOAT_VAL; 2];
    let mut subset_max_dot = [-BIG_FLOAT_VAL; 2];
    let mut subset_min_idx = [0usize; 2];
    let mut subset_max_idx = [0usize; 2];
    for i in 0..16 {
        let subset = ((best_pat_bits >> i) & 1) as usize;
        let r = pixels[i * 3] as f32;
        let g = pixels[i * 3 + 1] as f32;
        let b = pixels[i * 3 + 2] as f32;
        let dot =
            r * subset_axis[subset][0] + g * subset_axis[subset][1] + b * subset_axis[subset][2];
        if dot < subset_min_dot[subset] {
            subset_min_dot[subset] = dot;
            subset_min_idx[subset] = i;
        }
        if dot > subset_max_dot[subset] {
            subset_max_dot[subset] = dot;
            subset_max_idx[subset] = i;
        }
    }

    let mut subset_min = [[0u32; 3]; 2];
    let mut subset_max = [[0u32; 3]; 2];
    for subset in 0..2 {
        for c in 0..3 {
            subset_min[subset][c] = pixels[subset_min_idx[subset] * 3 + c] as u32;
            subset_max[subset][c] = pixels[subset_max_idx[subset] * 3 + c] as u32;
        }
    }

    // Least squares with unquantized endpoints.
    {
        let mut trial_weights = [0u8; 16];
        assign_weights3(
            &mut trial_weights,
            best_pat_bits,
            &subset_min,
            &subset_max,
            float_pixels,
            None,
        );

        let mut z00 = [0f32; 2];
        let mut z10 = [0f32; 2];
        let mut z11 = [0f32; 2];
        let mut q00 = [[0f32; 3]; 2];
        let mut tt = [[0f32; 3]; 2];
        for i in 0..16 {
            let subset = ((best_pat_bits >> i) & 1) as usize;
            let px = [
                pixels[i * 3] as f32,
                pixels[i * 3 + 1] as f32,
                pixels[i * 3 + 2] as f32,
            ];
            let sel = trial_weights[i] as usize;
            z00[subset] += t.ls3[sel][0];
            z10[subset] += t.ls3[sel][1];
            z11[subset] += t.ls3[sel][2];
            let w = t.ls3[sel][3];
            for c in 0..3 {
                q00[subset][c] += w * px[c];
                tt[subset][c] += px[c];
            }
        }
        for subset in 0..2 {
            let q10 = [
                tt[subset][0] - q00[subset][0],
                tt[subset][1] - q00[subset][1],
                tt[subset][2] - q00[subset][2],
            ];
            let z01 = z10[subset];
            let mut det = z00[subset] * z11[subset] - z01 * z10[subset];
            if fabsf(det) >= SMALL_FLOAT_VAL {
                det = 1.0 / det;
                let iz00 = z11[subset] * det;
                let iz01 = -z01 * det;
                let iz10 = -z10[subset] * det;
                let iz11 = z00[subset] * det;
                for c in 0..3 {
                    subset_max[subset][c] = fast_roundf_int(iz00 * q00[subset][c] + iz01 * q10[c])
                        .clamp(0, MAX_BC6H_HALF_FLOAT_AS_UINT as i32)
                        as u32;
                    subset_min[subset][c] = fast_roundf_int(iz10 * q00[subset][c] + iz11 * q10[c])
                        .clamp(0, MAX_BC6H_HALF_FLOAT_AS_UINT as i32)
                        as u32;
                }
            }
        }
    }

    let mut bc6h_mode_index = BC6H_2SUBSET_ABS_ENDPOINT_MODE;
    let mut num_endpoint_bits = 6u32;
    // [comp][subset*2 + low/high]
    let mut abs_blog_endpoints = [[0u32; 4]; 3];

    if params.num_diff_endpoint_modes_to_try > 0 {
        // Ordered from largest base bits to least.
        const ORDER2: [usize; 2] = [5, 1];
        const ORDER4: [usize; 4] = [0, 5, 7, 1];
        const ORDER9: [usize; 9] = [2, 3, 4, 0, 5, 6, 7, 8, 1];

        let order: &[usize] = if params.num_diff_endpoint_modes_to_try >= 9 {
            &ORDER9
        } else if params.num_diff_endpoint_modes_to_try >= 4 {
            &ORDER4
        } else {
            &ORDER2
        };

        for &mode in order {
            let num_base_bits = BC6H_MODE_SIG_BITS[mode][0] as u32;
            let num_delta_bits = [
                BC6H_MODE_SIG_BITS[mode][1] as u32,
                BC6H_MODE_SIG_BITS[mode][2] as u32,
                BC6H_MODE_SIG_BITS[mode][3] as u32,
            ];

            for subset in 0..2 {
                let h = [
                    subset_min[subset][0],
                    subset_min[subset][1],
                    subset_min[subset][2],
                    subset_max[subset][0],
                    subset_max[subset][1],
                    subset_max[subset][2],
                ];
                let mut b = [0u32; 6];
                bc6h_quant_endpoints(&h, &mut b, num_base_bits);
                for c in 0..3 {
                    abs_blog_endpoints[c][subset * 2] = b[c];
                    abs_blog_endpoints[c][subset * 2 + 1] = b[c + 3];
                }
            }

            let mut ok = true;
            for c in 0..3 {
                // Conservative symmetric delta limit (endpoints may swap).
                let max_delta = (1i32 << (num_delta_bits[c] - 1)) - 1;
                let min_delta = -max_delta;
                let e = &abs_blog_endpoints[c];
                let deltas = [
                    e[1] as i32 - e[0] as i32,
                    e[2] as i32 - e[0] as i32,
                    e[3] as i32 - e[0] as i32,
                    e[2] as i32 - e[1] as i32,
                    e[3] as i32 - e[1] as i32,
                ];
                if deltas.iter().any(|&d| d < min_delta || d > max_delta) {
                    ok = false;
                    break;
                }
            }
            if ok {
                bc6h_mode_index = mode;
                num_endpoint_bits = num_base_bits;
                break;
            }
        }
    }

    if bc6h_mode_index == BC6H_2SUBSET_ABS_ENDPOINT_MODE {
        for subset in 0..2 {
            let h = [
                subset_min[subset][0],
                subset_min[subset][1],
                subset_min[subset][2],
                subset_max[subset][0],
                subset_max[subset][1],
                subset_max[subset][2],
            ];
            let mut b = [0u32; 6];
            bc6h_quant_endpoints(&h, &mut b, num_endpoint_bits);
            for c in 0..3 {
                abs_blog_endpoints[c][subset * 2] = b[c];
                abs_blog_endpoints[c][subset * 2 + 1] = b[c + 3];
            }
        }
    }

    for subset in 0..2 {
        let b = [
            abs_blog_endpoints[0][subset * 2],
            abs_blog_endpoints[1][subset * 2],
            abs_blog_endpoints[2][subset * 2],
            abs_blog_endpoints[0][subset * 2 + 1],
            abs_blog_endpoints[1][subset * 2 + 1],
            abs_blog_endpoints[2][subset * 2 + 1],
        ];
        let mut h = [0u32; 6];
        bc6h_dequant_endpoints(&b, &mut h, num_endpoint_bits);
        subset_min[subset].copy_from_slice(&h[..3]);
        subset_max[subset].copy_from_slice(&h[3..]);
    }

    let mut trial_weights = [0u8; 16];
    let trial_error = assign_weights3(
        &mut trial_weights,
        best_pat_bits,
        &subset_min,
        &subset_max,
        float_pixels,
        Some(pixel_scales),
    );

    if trial_error < *cur_error {
        let mut trial = LogicalBlock {
            mode: bc6h_mode_index,
            partition_pattern: best_pat_index as usize,
            endpoints: abs_blog_endpoints,
            weights: trial_weights,
        };
        if trial.weights[0] & 4 != 0 {
            for c in 0..3 {
                trial.endpoints[c].swap(0, 1);
            }
            for i in 0..16 {
                if (best_pat_bits >> i) & 1 == 0 {
                    trial.weights[i] = 7 - trial.weights[i];
                }
            }
        }
        let subset2_anchor_index = G_BC7_ANCHOR_SECOND[best_pat_index as usize] as usize;
        if trial.weights[subset2_anchor_index] & 4 != 0 {
            for c in 0..3 {
                trial.endpoints[c].swap(2, 3);
            }
            for i in 0..16 {
                if (best_pat_bits >> i) & 1 == 1 {
                    trial.weights[i] = 7 - trial.weights[i];
                }
            }
        }
        if bc6h_mode_index != BC6H_2SUBSET_ABS_ENDPOINT_MODE {
            for c in 0..3 {
                let delta_bitmask = (1u32 << BC6H_MODE_SIG_BITS[bc6h_mode_index][c + 1]) - 1;
                let delta0 = trial.endpoints[c][1] as i32 - trial.endpoints[c][0] as i32;
                let delta1 = trial.endpoints[c][2] as i32 - trial.endpoints[c][0] as i32;
                let delta2 = trial.endpoints[c][3] as i32 - trial.endpoints[c][0] as i32;
                trial.endpoints[c][1] = delta0 as u32 & delta_bitmask;
                trial.endpoints[c][2] = delta1 as u32 & delta_bitmask;
                trial.endpoints[c][3] = delta2 as u32 & delta_bitmask;
            }
        }
        *cur_error = trial_error;
        *log_blk = trial;
    }
}

/// Choose which 2-subset partition patterns to try: the single best match, the
/// N best, or all 32.
fn fast_encode_bc6h_2subsets(
    pixels: &[u16; 48],
    float_pixels: &[Vec3F; 16],
    pixel_scales: &[f32; 16],
    cur_error: &mut f64,
    log_blk: &mut LogicalBlock,
    mean_r: i32,
    mean_g: i32,
    mean_b: i32,
    block_axis: [f32; 3],
    params: &FastBc6hParams,
) {
    let t = tables();

    if params.max_2subset_pats_to_try as usize == BC6H_NUM_PATS {
        for i in 0..BC6H_NUM_PATS {
            fast_encode_bc6h_2subsets_pattern(
                i as u32,
                t.pats2[i],
                pixels,
                float_pixels,
                pixel_scales,
                cur_error,
                log_blk,
                mean_r,
                mean_g,
                mean_b,
                params,
            );
        }
        return;
    }

    let mut desired_pat_bits = 0u32;
    for i in 0..16 {
        let f = (pixels[i * 3] as i32 - mean_r) as f32 * block_axis[0]
            + (pixels[i * 3 + 1] as i32 - mean_g) as f32 * block_axis[1]
            + (pixels[i * 3 + 2] as i32 - mean_b) as f32 * block_axis[2];
        desired_pat_bits |= u32::from(f >= 0.0) << i;
    }

    if params.max_2subset_pats_to_try == 1 {
        let mut best_diff = u32::MAX;
        for p in 0..BC6H_NUM_PATS {
            let pat_bits = t.pats2[p];
            let diff = (pat_bits ^ desired_pat_bits).count_ones() as i32;
            let diff_inv = 16 - diff;
            let min_diff = ((diff.min(diff_inv) as u32) << 8) | p as u32;
            if min_diff < best_diff {
                best_diff = min_diff;
            }
        }
        let best_pat_index = best_diff & 0xFF;
        fast_encode_bc6h_2subsets_pattern(
            best_pat_index,
            t.pats2[best_pat_index as usize],
            pixels,
            float_pixels,
            pixel_scales,
            cur_error,
            log_blk,
            mean_r,
            mean_g,
            mean_b,
            params,
        );
    } else {
        let mut pat_diffs = [0u32; BC6H_NUM_PATS];
        for p in 0..BC6H_NUM_PATS {
            let pat_bits = t.pats2[p];
            let diff = (pat_bits ^ desired_pat_bits).count_ones() as i32;
            let diff_inv = 16 - diff;
            pat_diffs[p] = ((diff.min(diff_inv) as u32) << 8) | p as u32;
        }
        pat_diffs.sort_unstable();
        for pat_iter in 0..params.max_2subset_pats_to_try as usize {
            let best_pat_index = pat_diffs[pat_iter] & 0xFF;
            fast_encode_bc6h_2subsets_pattern(
                best_pat_index,
                t.pats2[best_pat_index as usize],
                pixels,
                float_pixels,
                pixel_scales,
                cur_error,
                log_blk,
                mean_r,
                mean_g,
                mean_b,
                params,
            );
        }
    }
}

/// Encode 16 RGB half-float texels (48 halves, raster order) as one unsigned
/// BC6H block.
pub fn fast_encode_bc6h(pixels: &[u16; 48], block: &mut [u8; 16], params: &FastBc6hParams) {
    let mut log_blk = LogicalBlock {
        mode: BC6H_FIRST_1SUBSET_MODE_INDEX,
        partition_pattern: 0,
        endpoints: [[0; 4]; 3],
        weights: [0; 16],
    };

    let mut omin = [u32::MAX; 3];
    let mut omax = [0u32; 3];
    let mut totals = [0u32; 3];
    for i in 0..16 {
        for c in 0..3 {
            let v = pixels[i * 3 + c] as u32;
            totals[c] += v;
            omin[c] = omin[c].min(v);
            omax[c] = omax[c].max(v);
        }
    }

    if omin == omax {
        // Solid block.
        for c in 0..3 {
            log_blk.endpoints[c][0] = half_to_blog(omin[c] as u16, 16);
            log_blk.endpoints[c][1] = 0;
        }
        log_blk.mode = 13;
        *block = pack_block(&log_blk);
        return;
    }

    let mean_r = ((totals[0] + 8) / 16) as i32;
    let mean_g = ((totals[1] + 8) / 16) as i32;
    let mean_b = ((totals[2] + 8) / 16) as i32;

    let mut icov = [0i64; 6];
    for i in 0..16 {
        let r = pixels[i * 3] as i32 - mean_r;
        let g = pixels[i * 3 + 1] as i32 - mean_g;
        let b = pixels[i * 3 + 2] as i32 - mean_b;
        icov[0] += (r * r) as i64;
        icov[1] += (r * g) as i64;
        icov[2] += (r * b) as i64;
        icov[3] += (g * g) as i64;
        icov[4] += (g * b) as i64;
        icov[5] += (b * b) as i64;
    }
    let block_max_var = icov[0].max(icov[3]).max(icov[5]);

    if block_max_var < FAST_BC6H_STD_DEV_THRESH * FAST_BC6H_STD_DEV_THRESH * 16 {
        // Simple block: pull the endpoints slightly toward the mean.
        let mut e = [
            (omax[0] - omin[0]) / 32 + omin[0],
            (omax[1] - omin[1]) / 32 + omin[1],
            (omax[2] - omin[2]) / 32 + omin[2],
            ((omax[0] - omin[0]) * 31) / 32 + omin[0],
            ((omax[1] - omin[1]) * 31) / 32 + omin[1],
            ((omax[2] - omin[2]) * 31) / 32 + omin[2],
        ];
        bc6h_quant_dequant_endpoints(&mut e, 10);
        assign_weights_simple_4(
            pixels,
            &mut log_blk.weights,
            e[0] as i32,
            e[1] as i32,
            e[2] as i32,
            e[3] as i32,
            e[4] as i32,
            e[5] as i32,
            block_max_var,
            params,
        );
        for c in 0..3 {
            log_blk.endpoints[c][0] = half_to_blog(e[c] as u16, 10);
            log_blk.endpoints[c][1] = half_to_blog(e[c + 3] as u16, 10);
        }
        if log_blk.weights[0] & 8 != 0 {
            for i in 0..16 {
                log_blk.weights[i] = 15 - log_blk.weights[i];
            }
            for c in 0..3 {
                log_blk.endpoints[c].swap(0, 1);
            }
        }
        *block = pack_block(&log_blk);
        return;
    }

    // Complex block (edges / strong gradients).
    let mut cov = [0f32; 6];
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
    let l = alt_xr * alt_xr + alt_xg * alt_xg + alt_xb * alt_xb;
    let mut axis_r = 0.577_350_26f32;
    let mut axis_g = 0.577_350_26f32;
    let mut axis_b = 0.577_350_26f32;
    if fabsf(l) >= SMALL_FLOAT_VAL {
        let inv_l = inv_sqrt(l);
        axis_r = alt_xr * inv_l;
        axis_g = alt_xg * inv_l;
        axis_b = alt_xb * inv_l;
    }
    let tr = axis_r * cov[0] + axis_g * cov[1] + axis_b * cov[2];
    let tg = axis_r * cov[1] + axis_g * cov[3] + axis_b * cov[4];
    let tb = axis_r * cov[2] + axis_g * cov[4] + axis_b * cov[5];
    let principle_axis_var = tr * axis_r + tg * axis_g + tb * axis_b;
    let inv_principle_axis_var = 1.0f32 / (principle_axis_var + REALLY_SMALL_FLOAT_VAL);
    axis_r = tr * inv_principle_axis_var;
    axis_g = tg * inv_principle_axis_var;
    axis_b = tb * inv_principle_axis_var;
    let total_var = cov[0] + cov[3] + cov[5];
    const COMPLEX_BLOCK_PRINCIPLE_AXIS_FRACT_THRESH: f32 = 0.995;
    let try_2subsets = principle_axis_var < total_var * COMPLEX_BLOCK_PRINCIPLE_AXIS_FRACT_THRESH;

    let mut float_pixels = [[0f32; 3]; 16];
    let mut pixel_scales = [0f32; 16];
    let mut min_idx = 0usize;
    let mut max_idx = 0usize;
    let mut min_dot = BIG_FLOAT_VAL;
    let mut max_dot = -BIG_FLOAT_VAL;
    for i in 0..16 {
        let r = pixels[i * 3] as f32;
        let g = pixels[i * 3 + 1] as f32;
        let b = pixels[i * 3 + 2] as f32;
        float_pixels[i][0] = half_to_float(pixels[i * 3]);
        float_pixels[i][1] = half_to_float(pixels[i * 3 + 1]);
        float_pixels[i][2] = half_to_float(pixels[i * 3 + 2]);
        pixel_scales[i] = 1.0
            / (float_pixels[i][0] * float_pixels[i][0]
                + float_pixels[i][1] * float_pixels[i][1]
                + float_pixels[i][2] * float_pixels[i][2]
                + MIN_HALF_FLOAT);
        let dot = r * axis_r + g * axis_g + b * axis_b;
        if dot < min_dot {
            min_dot = dot;
            min_idx = i;
        }
        if dot > max_dot {
            max_dot = dot;
            max_idx = i;
        }
    }

    let mut e = [
        pixels[min_idx * 3] as u32,
        pixels[min_idx * 3 + 1] as u32,
        pixels[min_idx * 3 + 2] as u32,
        pixels[max_idx * 3] as u32,
        pixels[max_idx * 3 + 1] as u32,
        pixels[max_idx * 3 + 2] as u32,
    ];
    bc6h_quant_dequant_endpoints(&mut e, 10);
    let mut cur_err = assign_weights_4(
        &float_pixels,
        &pixel_scales,
        &mut log_blk.weights,
        e[0] as i32,
        e[1] as i32,
        e[2] as i32,
        e[3] as i32,
        e[4] as i32,
        e[5] as i32,
        block_max_var,
        try_2subsets,
        params,
    );

    let t = tables();
    let max_ls_passes = if params.hq_ls { 2 } else { 1 };
    for _pass in 0..max_ls_passes {
        let mut z00 = 0f32;
        let mut z10 = 0f32;
        let mut z11 = 0f32;
        let mut q00 = [0f32; 3];
        let mut tt = [0f32; 3];
        for i in 0..16 {
            let px = [
                pixels[i * 3] as f32,
                pixels[i * 3 + 1] as f32,
                pixels[i * 3 + 2] as f32,
            ];
            let sel = log_blk.weights[i] as usize;
            z00 += t.ls4[sel][0];
            z10 += t.ls4[sel][1];
            z11 += t.ls4[sel][2];
            let w = t.ls4[sel][3];
            for c in 0..3 {
                q00[c] += w * px[c];
                tt[c] += px[c];
            }
        }
        let q10 = [tt[0] - q00[0], tt[1] - q00[1], tt[2] - q00[2]];
        let z01 = z10;
        let mut det = z00 * z11 - z01 * z10;
        if fabsf(det) < SMALL_FLOAT_VAL {
            break;
        }
        det = 1.0 / det;
        let iz00 = z11 * det;
        let iz01 = -z01 * det;
        let iz10 = -z10 * det;
        let iz11 = z00 * det;

        let mut trial = [0u32; 6];
        for c in 0..3 {
            trial[c + 3] = roundf(iz00 * q00[c] + iz01 * q10[c])
                .clamp(0.0, MAX_BC6H_HALF_FLOAT_AS_UINT as f32) as i32
                as u32;
            trial[c] = roundf(iz10 * q00[c] + iz11 * q10[c])
                .clamp(0.0, MAX_BC6H_HALF_FLOAT_AS_UINT as f32) as i32
                as u32;
        }
        bc6h_quant_dequant_endpoints(&mut trial, 10);
        let mut trial_weights = [0u8; 16];
        let trial_err = assign_weights_4(
            &float_pixels,
            &pixel_scales,
            &mut trial_weights,
            trial[0] as i32,
            trial[1] as i32,
            trial[2] as i32,
            trial[3] as i32,
            trial[4] as i32,
            trial[5] as i32,
            block_max_var,
            try_2subsets,
            params,
        );
        if trial_err < cur_err {
            cur_err = trial_err;
            e = trial;
            log_blk.weights = trial_weights;
        } else {
            break;
        }
    }

    for c in 0..3 {
        log_blk.endpoints[c][0] = half_to_blog(e[c] as u16, 10);
        log_blk.endpoints[c][1] = half_to_blog(e[c + 3] as u16, 10);
    }
    if log_blk.weights[0] & 8 != 0 {
        for i in 0..16 {
            log_blk.weights[i] = 15 - log_blk.weights[i];
        }
        for c in 0..3 {
            log_blk.endpoints[c].swap(0, 1);
        }
    }

    if params.max_2subset_pats_to_try > 0
        && try_2subsets
        && block_max_var > FAST_BC6H_COMPLEX_STD_DEV_THRESH * FAST_BC6H_COMPLEX_STD_DEV_THRESH * 16
    {
        fast_encode_bc6h_2subsets(
            pixels,
            &float_pixels,
            &pixel_scales,
            &mut cur_err,
            &mut log_blk,
            mean_r,
            mean_g,
            mean_b,
            [axis_r, axis_g, axis_b],
            params,
        );
    }

    *block = pack_block(&log_blk);
}
