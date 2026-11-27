//! XUASTC weight-grid DCT decode: zigzag-ordered deadzone coefficients
//! dequantized against a bilinearly-sampled JPEG luma matrix,
//! inverse-transformed by the baked-literal IDCT, then rounded onto the
//! [0,64] weight scale and quantized to ISE symbols. Floats are safe here
//! because they only affect this block's decoded weights and never feed back
//! into future decode state.

use super::endpoints::{decode_endpoints, num_cem_values};
use super::idct::idct_2d;
use super::tables::{BASELINE_JPEG_Y, SCALE_QUANT_STEPS};
use crate::astc::dequant::quant_tables;
use crate::astc::unpack::{ise_levels, LogAstcBlock};
use crate::mathf::sqrtf;

/// One decoded (run, coefficient) pair of a plane's AC scan.
#[derive(Clone, Copy)]
pub struct DctCoeff {
    pub num_zeros: u16,
    pub coeff: i16,
}

/// The per-plane DCT symbols the stream carries for one weight grid.
pub struct DctSyms {
    pub dc_sym: u32,
    pub coeffs: alloc::vec::Vec<DctCoeff>,
}

/// Run-length symbol that ends a plane's AC scan; runs 0..=63 count skipped
/// zero coefficients, so the model spans 65 symbols.
pub const DCT_RUN_LEN_EOB_SYM_INDEX: u32 = 64;
/// DC (mean) symbol level count for weight ISE ranges up to 5.
pub const DCT_MEAN_LEVELS0: u32 = 9;
/// DC (mean) symbol level count for weight ISE ranges above 5.
pub const DCT_MEAN_LEVELS1: u32 = 33;
const DEADZONE_ALPHA: f32 = 0.5;
const SCALED_WEIGHT_BASE_CODING_SCALE: f32 = 0.5;

/// Round to the nearest integer, with halves going away from zero.
#[inline]
fn fast_roundf_int(x: f32) -> i32 {
    if x >= 0.0 {
        (x + 0.5) as i32
    } else {
        (x - 0.5) as i32
    }
}

/// Number of levels the DC (mean) weight symbol can take: 9 when the weight
/// ISE range is at most 5 (up to 8 levels), otherwise 33.
pub fn num_weight_dc_levels(weight_ise_range: u32) -> u32 {
    let scaled = if weight_ise_range <= 5 {
        1.0 / 8.0
    } else {
        SCALED_WEIGHT_BASE_CODING_SCALE
    };
    (64.0 * scaled) as u32 + 1
}

/// Fill `out` with a JPEG-style anti-diagonal scan over a `width x height`
/// grid, alternating direction on each diagonal.
pub fn zigzag_order(width: usize, height: usize, out: &mut [u16]) {
    let mut idx = 0;
    let mut diag = [0u16; 12];
    for s in 0..(width + height - 1) {
        let x_start = if s < height { 0 } else { s - height + 1 };
        let x_end = if s < width { s } else { width - 1 };
        let diag_size = x_end - x_start + 1;
        for (j, x) in (x_start..=x_end).enumerate() {
            let y = s - x;
            diag[j] = (x + y * width) as u16;
        }
        if s & 1 == 1 {
            for k in (0..diag_size).rev() {
                out[idx] = diag[k];
                idx += 1;
            }
        } else {
            for &d in &diag[..diag_size] {
                out[idx] = d;
                idx += 1;
            }
        }
    }
}

/// The endpoint span that drives the adaptive quantization. For a dual-plane
/// block it covers the channels this plane carries; otherwise it is the
/// largest per-subset endpoint distance.
fn max_span_len(log: &LogAstcBlock, plane_index: u32) -> f32 {
    if log.dual_plane {
        let (l, h) = decode_endpoints(
            log.color_endpoint_modes[0] as u32,
            &log.endpoints,
            log.endpoint_ise_range,
        );
        let mut span = 0f32;
        for c in 0..4u32 {
            let selected = if plane_index == 1 {
                c == log.color_component_selector
            } else {
                c != log.color_component_selector
            };
            if selected {
                let d = h[c as usize] as f32 - l[c as usize] as f32;
                span += d * d;
            }
        }
        sqrtf(span)
    } else {
        let cem = log.color_endpoint_modes[0] as u32;
        let stride = num_cem_values(cem);
        let mut span = 0f32;
        for i in 0..log.num_partitions as usize {
            let (l, h) =
                decode_endpoints(cem, &log.endpoints[stride * i..], log.endpoint_ise_range);
            let mut part = 0f32;
            for c in 0..4 {
                let d = h[c] as f32 - l[c] as f32;
                part += d * d;
            }
            span = sqrtf(part).max(span);
        }
        span
    }
}

/// JPEG quality-factor scaling combined with the span-adaptive factor and the
/// per-weight-range step scale.
fn compute_level_scale(q: f32, span_len: f32, weight_ise_range: u32) -> f32 {
    let q = q.clamp(1.0, 100.0);
    let mut level_scale = if q < 50.0 {
        5000.0 / q
    } else {
        200.0 - 2.0 * q
    };
    level_scale *= 1.0 / 100.0;

    let span_floor = 14.0f32;
    let mut adaptive_factor = 64.0 / span_len.max(span_floor);
    adaptive_factor *= SCALE_QUANT_STEPS[weight_ise_range as usize];
    level_scale * adaptive_factor
}

/// Bilinear sample of the baseline JPEG luma matrix at the grid frequency,
/// scaled by `level_scale` and floored to 1.
fn sample_quant_table(q: f32, sx: f32, sy: f32, level_scale: f32, x: u32, y: u32) -> i32 {
    if q >= 100.0 {
        return 1;
    }
    let rx = x as f32 * sx;
    let ry = y as f32 * sy;

    let i = rx.min(7.0);
    let j = ry.min(7.0);
    let i0 = i as i32;
    let j0 = j as i32;
    let i1 = (i0 + 1).min(7);
    let j1 = (j0 + 1).min(7);
    let ti = i - i0 as f32;
    let tj = j - j0 as f32;
    let a = (1.0 - ti) * BASELINE_JPEG_Y[j0 as usize][i0 as usize] as f32
        + ti * BASELINE_JPEG_Y[j0 as usize][i1 as usize] as f32;
    let b = (1.0 - ti) * BASELINE_JPEG_Y[j1 as usize][i0 as usize] as f32
        + ti * BASELINE_JPEG_Y[j1 as usize][i1 as usize] as f32;
    let base = (1.0 - tj) * a + tj * b;

    let quant_scale = (base * level_scale + 0.5) as i32;
    quant_scale.max(1)
}

/// Dequantize one coefficient: the two first-order AC terms decode linearly,
/// everything else lands at the center of its deadzone bin.
fn dequant_deadzone(q: i32, l: i32, x: u32, y: u32) -> f32 {
    if (x == 1 && y == 0) || (x == 0 && y == 1) {
        return q as f32 * l as f32;
    }
    if q == 0 || l <= 0 {
        return 0.0;
    }
    let tau = DEADZONE_ALPHA * l as f32;
    let mag = tau + (q.unsigned_abs() as f32) * l as f32;
    if q < 0 {
        -mag
    } else {
        mag
    }
}

/// Decode one plane's weight grid from `syms` into `log.weights`. `block_w`
/// and `block_h` are the ASTC block dims and set the quant-matrix scale; the
/// weight grid dims come from `log`. Returns `None` when a run overruns the
/// grid.
pub fn decode_block_weights(
    q: f32,
    plane_index: u32,
    block_w: u32,
    block_h: u32,
    log: &mut LogAstcBlock,
    syms: &DctSyms,
) -> Option<()> {
    let grid_width = log.grid_width as usize;
    let grid_height = log.grid_height as usize;
    let total_grid_samples = grid_width * grid_height;
    let num_planes = if log.dual_plane { 2 } else { 1 };

    // Grid validity: only grids from 2 up to the block dims, with at most 64
    // samples, are decodable; anything else is a malformed stream.
    if total_grid_samples > 64
        || grid_width < 2
        || grid_height < 2
        || grid_width > block_w as usize
        || grid_height > block_h as usize
    {
        return None;
    }

    let quant_tab = &quant_tables().weight_val_to_ise[log.weight_ise_range as usize];

    let span_len = max_span_len(log, plane_index);
    let level_scale = compute_level_scale(q, span_len, log.weight_ise_range);

    let scaled_weight_coding_scale = if log.weight_ise_range <= 5 {
        1.0f32 / 8.0
    } else {
        SCALED_WEIGHT_BASE_CODING_SCALE
    };
    let mean_weight = syms.dc_sym as f32 / scaled_weight_coding_scale;

    let mut zigzag = [0u16; 144];
    zigzag_order(grid_width, grid_height, &mut zigzag);

    let mut dct_weights = [0f32; 144];
    let sx = 8.0f32 / block_w as f32;
    let sy = 8.0f32 / block_h as f32;

    let mut zig_idx = 1usize;
    for c in &syms.coeffs {
        let run_len = c.num_zeros as usize;
        if run_len + zig_idx > total_grid_samples {
            return None;
        }
        zig_idx += run_len;
        if zig_idx >= total_grid_samples {
            break;
        }
        let dct_idx = zigzag[zig_idx] as usize;
        let y = (dct_idx / grid_width) as u32;
        let x = (dct_idx % grid_width) as u32;
        let quant = sample_quant_table(q, sx, sy, level_scale, x, y);
        dct_weights[dct_idx] = dequant_deadzone(c.coeff as i32, quant, x, y);
        zig_idx += 1;
    }

    let mut idct_weights = [0f32; 144];
    idct_2d(&dct_weights, &mut idct_weights, grid_height, grid_width);

    for y in 0..grid_height {
        for x in 0..grid_width {
            let w = fast_roundf_int(mean_weight + idct_weights[x + y * grid_width]).clamp(0, 64);
            log.weights[(x + y * grid_width) * num_planes + plane_index as usize] =
                quant_tab[w as usize];
        }
    }

    // Weight symbols must stay inside the range's level count for the
    // physical pack later; quant_tab already guarantees this.
    debug_assert!(log.weights[..total_grid_samples * num_planes]
        .iter()
        .all(|&w| (w as u32) < ise_levels(log.weight_ise_range)));

    Some(())
}
