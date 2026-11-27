//! The UASTC HDR 6x6 intermediate codec: a bitwise-coded stream of 6x6 ASTC
//! HDR blocks with run, solid, reuse, and block encodings, decompressed into
//! standard physical ASTC 6x6 blocks. The decompressed blocks then feed the
//! same slice paths as the raw ASTC HDR 6x6 source (`astc::hdr6x6`).
//!
//! The stream codes *logical* blocks against 75 block-mode descriptors whose
//! coding ISE ranges may differ from the output ("transcode") ranges; the
//! decoder requantizes endpoints and weights into the transcode ranges and
//! packs a physical block per source block. Endpoint requantization uses
//! MSB-preserving quantization tables so the qlog sign/exponent bits survive.

// Each logical block is default-constructed and then filled field by field in
// the order the stream codes them, so the struct-literal form this lint
// prefers would not follow the decode sequence.
#![allow(clippy::field_reassign_with_default)]

use crate::astc::dequant::{dequant_tables, quant_tables};
use crate::astc::pack::pack_astc_block;
use crate::astc::unpack::{ise_levels, LogAstcBlock};
use crate::basislz::decoder::BitwiseDecoder;
use crate::once::OnceBox;
use crate::uastc::tables::ASTC_BISE_RANGE_TABLE;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

pub mod tables;
use tables::{
    BlockModeDesc, BLOCK_MODE_DESCS, PART2_UNIQUE_INDEX_TO_SEED, PART3_UNIQUE_INDEX_TO_SEED,
    REUSE_XY_DELTAS,
};

const UASTC_6X6_HDR_SIG0: u32 = 0xABCD; // original release (v1.6)
const UASTC_6X6_HDR_SIG1: u32 = 0xABCE; // fixed 2x2->4x4 weight upsampling
const MAX_ASTC_HDR_6X6_DIM: u32 = 32768;
const REUSE_MAX_BUFFER_ROWS: usize = 5;
const REUSE_XY_DELTA_BITS: u32 = 5;
const NUM_ENDPOINT_DELTA_BITS: u32 = 5;
const NUM_UNIQUE_PARTITIONS2: u32 = 521;
const NUM_UNIQUE_PARTITIONS3: u32 = 333;
const TOTAL_BLOCK_MODE_DECS: u32 = 75;
const BLOCK_W: u32 = 6;
const BLOCK_H: u32 = 6;

/// Number of endpoint values for a CEM (only CEM 7 or 11 occur in this codec).
#[inline]
fn num_endpoint_vals(cem: u32) -> usize {
    if cem == 11 {
        6
    } else {
        4
    }
}

/// The MSB-preserving endpoint quantization tables: for each endpoint range
/// 4..=19, the nearest ISE symbol whose dequantization keeps the value's upper
/// 2 (or 3) bits, ties to the lowest symbol.
pub(crate) struct PreserveTables {
    pub(crate) preserve2: [[u8; 256]; 16],
    pub(crate) preserve3: [[u8; 256]; 16],
}

/// The MSB-preserving quantization tables, built on first use and cached.
pub(crate) fn preserve_tables() -> &'static PreserveTables {
    static TABLES: OnceBox<PreserveTables> = OnceBox::new();
    TABLES.get_or_init(|| {
        let dq = dequant_tables();
        let mut t = Box::new(PreserveTables {
            preserve2: [[0; 256]; 16],
            preserve3: [[0; 256]; 16],
        });
        for range in 4..=19u32 {
            let num_levels = ise_levels(range);
            let ise_to_val = &dq.endpoints[(range - 4) as usize];
            for desired in 0..256u32 {
                let mut best_err = u32::MAX;
                let mut best_ise = 0;
                for ise in 0..num_levels {
                    let qv = ise_to_val[ise as usize] as u32;
                    if (qv & 0b1100_0000) != (desired & 0b1100_0000) {
                        continue;
                    }
                    let d = qv as i32 - desired as i32;
                    let err = (d * d) as u32;
                    if err < best_err {
                        best_err = err;
                        best_ise = ise;
                    }
                }
                t.preserve2[(range - 4) as usize][desired as usize] = best_ise as u8;

                if range >= 5 {
                    // ranges of 8 levels or more carry a third MSB to preserve
                    let mut best_err = u32::MAX;
                    let mut best_ise = 0;
                    for ise in 0..num_levels {
                        let qv = ise_to_val[ise as usize] as u32;
                        if (qv & 0b1110_0000) != (desired & 0b1110_0000) {
                            continue;
                        }
                        let d = qv as i32 - desired as i32;
                        let err = (d * d) as u32;
                        if err < best_err {
                            best_err = err;
                            best_ise = ise;
                        }
                    }
                    t.preserve3[(range - 4) as usize][desired as usize] = best_ise as u8;
                }
            }
        }
        t
    })
}

/// `requantize_astc_weights`: weight ISE symbols from one range to another
/// through the dequantized [0,64] domain.
fn requantize_astc_weights(n: usize, src: &[u8], from_range: u32, dst: &mut [u8], to_range: u32) {
    if from_range == to_range {
        dst[..n].copy_from_slice(&src[..n]);
        return;
    }
    let dq = &dequant_tables().weights[from_range as usize];
    let q = &quant_tables().weight_val_to_ise[to_range as usize];
    for i in 0..n {
        dst[i] = q[dq[src[i] as usize] as usize];
    }
}

/// `requantize_ise_endpoints`: endpoint ISE symbols from one range to
/// another, using the MSB-preserving quantizers on the qlog components so
/// the encoding's meaning survives (except CEM 11 direct, where the plain
/// quantizer already preserves the MSB).
fn requantize_ise_endpoints(cem: u32, src_range: u32, src: &[u8], dst_range: u32, dst: &mut [u8]) {
    let n = num_endpoint_vals(cem);
    if src_range == dst_range {
        dst[..n].copy_from_slice(&src[..n]);
        return;
    }

    let mut temp = [0u8; 6];
    let vals: &[u8] = if src_range != 20 {
        let dq = &dequant_tables().endpoints[(src_range - 4) as usize];
        for i in 0..n {
            temp[i] = dq[src[i] as usize];
        }
        &temp[..n]
    } else {
        &src[..n]
    };

    if dst_range == 20 {
        dst[..n].copy_from_slice(vals);
        return;
    }

    let q = &quant_tables().endpoint_val_to_ise[(dst_range - 4) as usize];
    let pt = preserve_tables();
    let p2 = &pt.preserve2[(dst_range - 4) as usize];
    let p3 = &pt.preserve3[(dst_range - 4) as usize];

    if cem == 11 {
        let maj_comp = ((vals[4] >> 7) & 1) | (((vals[5] >> 7) & 1) << 1);
        if maj_comp == 3 {
            // Direct: the plain quantizer preserves the MSB already.
            for i in 0..6 {
                dst[i] = q[vals[i] as usize];
            }
        } else {
            dst[0] = q[vals[0] as usize];
            dst[1] = p2[vals[1] as usize];
            dst[2] = p2[vals[2] as usize];
            dst[3] = p2[vals[3] as usize];
            dst[4] = p3[vals[4] as usize];
            dst[5] = p3[vals[5] as usize];
        }
    } else {
        dst[0] = p2[vals[0] as usize];
        dst[1] = p3[vals[1] as usize];
        dst[2] = p3[vals[2] as usize];
        dst[3] = p3[vals[3] as usize];
    }
}

/// `copy_weight_grid`: move transcode-range weights into the output logical
/// block. The 2x2 non-dual-plane grids (not valid ASTC) upsample to 4x4
/// through the fixed-point bilinear kernel; `orig_behavior` reproduces the
/// original release's mis-indexing (a bool used as the source index), which
/// SIG0 streams were encoded against.
fn copy_weight_grid(
    dual_plane: bool,
    grid_x: u32,
    grid_y: u32,
    transcode_weights: &[u8],
    decomp: &mut LogAstcBlock,
    orig_behavior: bool,
) {
    if !dual_plane && grid_x == 2 && grid_y == 2 {
        decomp.grid_width = 4;
        decomp.grid_height = 4;

        let dq = &dequant_tables().weights[decomp.weight_ise_range as usize];
        let q = &quant_tables().weight_val_to_ise[decomp.weight_ise_range as usize];

        // Fixed-point bilinear upsample weights, 2x2 grid to 4x4, fused.
        let scale = (1024 + 4 / 2) / (4 - 1);
        for dy in 0..4u32 {
            let gy = (scale * dy + 32) >> 6;
            let (sy, fy) = (gy >> 4, gy & 0xF);
            for dx in 0..4u32 {
                let gx = (scale * dx + 32) >> 6;
                let (sx, fx) = (gx >> 4, gx & 0xF);
                let w11 = (fx * fy + 8) >> 4;
                let w = [[16 - fx - fy + w11, fx - w11], [fy - w11, w11]];

                let mut total_weight = 8u32;
                for yo in 0..2u32 {
                    for xo in 0..2u32 {
                        if w[yo as usize][xo as usize] == 0 {
                            continue;
                        }
                        let idx = if orig_behavior {
                            // The original, incorrect, but ultimately harmless
                            // behavior: `is_in_bounds(...)` (a bool) used as
                            // the index, so weight 0 or 1 is sampled.
                            usize::from((dx + xo) + (dy + yo) * grid_x < grid_x * grid_y)
                        } else {
                            ((sx + xo) + (sy + yo) * grid_x) as usize
                        };
                        total_weight += dq[transcode_weights[idx] as usize] as u32
                            * w[yo as usize][xo as usize];
                    }
                }
                total_weight >>= 4;
                decomp.weights[(dx + dy * 4) as usize] = q[total_weight as usize];
            }
        }
    } else {
        let num_planes = if dual_plane { 2 } else { 1 };
        decomp.grid_width = grid_x;
        decomp.grid_height = grid_y;
        let n = (grid_x * grid_y * num_planes) as usize;
        decomp.weights[..n].copy_from_slice(&transcode_weights[..n]);
    }
}

/// `calc_row_index`: map a source row (`prev_y`, within 4 rows above the
/// current row `cur_y`) to its slot in the 5-row logical-block ring buffer.
#[inline]
fn calc_row_index(cur_y: u32, prev_y: u32, cur_row_index: usize) -> usize {
    let delta_y = prev_y as i32 - cur_y as i32;
    let mut idx = cur_row_index as i32 + delta_y;
    if idx < 0 {
        idx += REUSE_MAX_BUFFER_ROWS as i32;
    }
    idx as usize
}

/// `decode_values`: read `total_values` ISE symbols of `ise_range`. The
/// trit/quint groups are coded up front (8 or 7 bits per bundle, fewer for
/// the final partial bundle), the plain bits inline per value.
pub(crate) fn decode_values(
    decoder: &mut BitwiseDecoder,
    total_values: u32,
    ise_range: u32,
    out: &mut [u8],
) {
    let t = &ASTC_BISE_RANGE_TABLE[ise_range as usize];
    let ep_bits = t[0] as u32;
    let ep_trits = t[1] != 0;
    let ep_quints = t[2] != 0;

    let mut total_tqs = 0u32;
    let mut bundle_size = 0u32;
    let mut mul = 0u32;
    if ep_trits {
        total_tqs = total_values.div_ceil(5);
        bundle_size = 5;
        mul = 3;
    } else if ep_quints {
        total_tqs = total_values.div_ceil(3);
        bundle_size = 3;
        mul = 5;
    }

    let mut tq_values = [0u32; 32];
    for i in 0..total_tqs {
        let mut num_bits = if ep_trits { 8 } else { 7 };
        if i == total_tqs - 1 {
            let num_remaining = total_values - (total_tqs - 1) * bundle_size;
            if ep_trits {
                match num_remaining {
                    1 => num_bits = 2,
                    2 => num_bits = 4,
                    3 => num_bits = 5,
                    4 => num_bits = 7,
                    _ => {}
                }
            } else if ep_quints {
                match num_remaining {
                    1 => num_bits = 3,
                    2 => num_bits = 5,
                    _ => {}
                }
            }
        }
        tq_values[i as usize] = decoder.get_bits(num_bits);
    }

    let mut accum = 0u32;
    let mut accum_remaining = 0u32;
    let mut next_tq_index = 0usize;
    for i in 0..total_values {
        let mut value = decoder.get_bits(ep_bits);
        if total_tqs != 0 {
            if accum_remaining == 0 {
                accum = tq_values[next_tq_index];
                next_tq_index += 1;
                accum_remaining = bundle_size;
            }
            let v = accum % mul;
            accum /= mul;
            accum_remaining -= 1;
            value |= v << ep_bits;
        }
        out[i as usize] = value as u8;
    }
}

/// One slot of the 5-row logical-block ring: the coded logical block plus
/// its block-mode index (`user_mode`; 255 marks a solid block).
#[derive(Clone, Copy)]
struct CodedBlock {
    log: LogAstcBlock,
    user_mode: u8,
}

/// Build the output ("decomp") logical block for a coded block: transcode
/// ISE ranges from the mode descriptor, requantized endpoints and weights,
/// and the weight grid moved/upsampled into place.
fn build_decomp_block(
    bmd: &BlockModeDesc,
    log: &LogAstcBlock,
    log_weight_range: u32,
    orig_behavior: bool,
) -> Option<[u8; 16]> {
    let nvals = num_endpoint_vals(bmd.cem);
    let total_grid_weights = bmd.grid_x * bmd.grid_y * if bmd.dp { 2 } else { 1 };

    let mut decomp = LogAstcBlock::default();
    decomp.dual_plane = bmd.dp;
    decomp.color_component_selector = bmd.dp_channel;
    decomp.partition_id = log.partition_id;
    decomp.num_partitions = bmd.num_partitions;
    for p in 0..bmd.num_partitions as usize {
        decomp.color_endpoint_modes[p] = bmd.cem as u8;
    }
    decomp.endpoint_ise_range = bmd.transcode_endpoint_ise_range;
    decomp.weight_ise_range = bmd.transcode_weight_ise_range;

    for p in 0..bmd.num_partitions as usize {
        let mut requant = [0u8; 6];
        requantize_ise_endpoints(
            bmd.cem,
            log.endpoint_ise_range,
            &log.endpoints[nvals * p..],
            bmd.transcode_endpoint_ise_range,
            &mut requant,
        );
        decomp.endpoints[nvals * p..nvals * (p + 1)].copy_from_slice(&requant[..nvals]);
    }

    let mut transcode_weights = [0u8; (BLOCK_W * BLOCK_H * 2) as usize];
    requantize_astc_weights(
        total_grid_weights as usize,
        &log.weights,
        log_weight_range,
        &mut transcode_weights,
        bmd.transcode_weight_ise_range,
    );

    copy_weight_grid(
        bmd.dp,
        bmd.grid_x,
        bmd.grid_y,
        &transcode_weights,
        &mut decomp,
        orig_behavior,
    );

    pack_astc_block(&decomp)
}

/// Decompress an intermediate stream into physical ASTC 6x6 blocks. Returns
/// the block bytes (`ceil(w/6) * ceil(h/6)` 16-byte blocks, row-major) and the
/// coded pixel dimensions; `None` on a bad signature or dimensions, a
/// malformed stream, or a missing 0xA742 end marker.
pub fn decode_6x6_hdr(comp: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    if comp.len() <= 2 * 3 + 1 {
        return None;
    }

    let mut decoder = BitwiseDecoder::new(comp);

    let hdr_sig = decoder.get_bits(16);
    let orig_behavior = hdr_sig == UASTC_6X6_HDR_SIG0;
    if !orig_behavior && hdr_sig != UASTC_6X6_HDR_SIG1 {
        return None;
    }

    let width = decoder.get_bits(16);
    let height = decoder.get_bits(16);
    if width == 0 || height == 0 || width > MAX_ASTC_HDR_6X6_DIM || height > MAX_ASTC_HDR_6X6_DIM {
        return None;
    }

    let num_blocks_x = width.div_ceil(BLOCK_W);
    let num_blocks_y = height.div_ceil(BLOCK_H);
    let total_blocks = num_blocks_x * num_blocks_y;

    let mut decoded_blocks = vec![0u8; (total_blocks as usize) * 16];

    // The decoded *logical* blocks, a ring of the last 5 rows.
    let empty = CodedBlock {
        log: LogAstcBlock::default(),
        user_mode: 0,
    };
    let mut log_rows = vec![empty; num_blocks_x as usize * REUSE_MAX_BUFFER_ROWS];

    let mut cur_bx = 0u32;
    let mut cur_by = 0u32;
    let mut cur_row_index = 0usize;

    // Advance one block in raster order, wrapping the row ring.
    macro_rules! advance {
        () => {
            cur_bx += 1;
            if cur_bx == num_blocks_x {
                cur_bx = 0;
                cur_by += 1;
                cur_row_index = (cur_row_index + 1) % REUSE_MAX_BUFFER_ROWS;
            }
        };
    }
    // Slot index of (bx, row_slot) in the log ring.
    let slot = |bx: u32, row: usize| row * num_blocks_x as usize + bx as usize;

    while cur_by < num_blocks_y {
        if decoder.bits_remaining() < 1 {
            return None;
        }

        // Encoding type: 1 -> block, 01 -> reuse, 001 -> solid, 000 -> run.
        enum Et {
            Run,
            Solid,
            Reuse,
            Block,
        }
        let et = if decoder.get_bits(1) != 0 {
            Et::Block
        } else if decoder.get_bits(1) != 0 {
            Et::Reuse
        } else if decoder.get_bits(1) != 0 {
            Et::Solid
        } else {
            Et::Run
        };

        match et {
            Et::Run => {
                if cur_bx == 0 && cur_by == 0 {
                    return None;
                }
                let run_len = decoder.decode_vlc(5) + 1;
                let num_blocks_remaining = total_blocks - (cur_bx + cur_by * num_blocks_x);
                if run_len > num_blocks_remaining {
                    return None;
                }

                let (prev_bx, prev_by) = if cur_bx != 0 {
                    (cur_bx - 1, cur_by)
                } else {
                    (num_blocks_x - 1, cur_by - 1)
                };

                // Snapshot the source block before writing: a long run can wrap
                // the ring back onto the source slot, so read it once up front;
                // every write in the run stores this same value.
                let prev_coded =
                    log_rows[slot(prev_bx, calc_row_index(cur_by, prev_by, cur_row_index))];
                let mut prev_phys = [0u8; 16];
                let po = ((prev_bx + prev_by * num_blocks_x) as usize) * 16;
                prev_phys.copy_from_slice(&decoded_blocks[po..po + 16]);

                for _ in 0..run_len {
                    log_rows[slot(cur_bx, cur_row_index)] = prev_coded;
                    let o = ((cur_bx + cur_by * num_blocks_x) as usize) * 16;
                    decoded_blocks[o..o + 16].copy_from_slice(&prev_phys);
                    advance!();
                }
            }
            Et::Solid => {
                let rh = decoder.get_bits(15) as u16;
                let gh = decoder.get_bits(15) as u16;
                let bh = decoder.get_bits(15) as u16;

                let mut log = LogAstcBlock::default();
                log.solid_color_flag_hdr = true;
                log.solid_color = [rh, gh, bh, 0x3C00]; // alpha = 1.0

                let phys = pack_astc_block(&log)?;
                log_rows[slot(cur_bx, cur_row_index)] = CodedBlock {
                    log,
                    user_mode: 255,
                };
                let o = ((cur_bx + cur_by * num_blocks_x) as usize) * 16;
                decoded_blocks[o..o + 16].copy_from_slice(&phys);
                advance!();
            }
            Et::Reuse => {
                if cur_bx == 0 && cur_by == 0 {
                    return None;
                }
                let reuse_delta_index = decoder.get_bits(REUSE_XY_DELTA_BITS) as usize;
                let (dx, dy) = REUSE_XY_DELTAS[reuse_delta_index];
                let prev_bx = cur_bx as i32 + dx as i32;
                let prev_by = cur_by as i32 + dy as i32;
                if prev_bx < 0 || prev_bx >= num_blocks_x as i32 || prev_by < 0 {
                    return None;
                }

                let prev = log_rows[slot(
                    prev_bx as u32,
                    calc_row_index(cur_by, prev_by as u32, cur_row_index),
                )];
                if prev.log.solid_color_flag_hdr {
                    return None;
                }

                let mut log = prev.log;
                let total_grid_weights =
                    log.grid_width * log.grid_height * if log.dual_plane { 2 } else { 1 };
                decode_values(
                    &mut decoder,
                    total_grid_weights,
                    log.weight_ise_range,
                    &mut log.weights,
                );

                let bmd = &BLOCK_MODE_DESCS[prev.user_mode as usize];
                let phys = build_decomp_block(bmd, &log, log.weight_ise_range, orig_behavior)?;

                log_rows[slot(cur_bx, cur_row_index)] = CodedBlock {
                    log,
                    user_mode: prev.user_mode,
                };
                let o = ((cur_bx + cur_by * num_blocks_x) as usize) * 16;
                decoded_blocks[o..o + 16].copy_from_slice(&phys);
                advance!();
            }
            Et::Block => {
                let bm = decoder.decode_truncated_binary(TOTAL_BLOCK_MODE_DECS) as usize;
                let em = decoder.decode_truncated_binary(5); // 5 endpoint modes

                // endpoint_mode: 0 raw, 1 use-left, 2 use-upper,
                // 3 use-left-delta, 4 use-upper-delta.
                match em {
                    1 | 2 => {
                        let (neighbor_bx, neighbor_by) = if em == 1 {
                            (cur_bx as i32 - 1, cur_by as i32)
                        } else {
                            (cur_bx as i32, cur_by as i32 - 1)
                        };
                        if neighbor_bx < 0 || neighbor_by < 0 {
                            return None;
                        }
                        let neighbor = log_rows[slot(
                            neighbor_bx as u32,
                            calc_row_index(cur_by, neighbor_by as u32, cur_row_index),
                        )];
                        if neighbor.log.color_endpoint_modes[0] == 0 {
                            return None;
                        }

                        let bmd = &BLOCK_MODE_DESCS[bm];
                        let nvals = num_endpoint_vals(bmd.cem);
                        if bmd.cem != neighbor.log.color_endpoint_modes[0] as u32 {
                            return None;
                        }

                        let mut log = LogAstcBlock::default();
                        log.num_partitions = 1;
                        log.color_endpoint_modes[0] = bmd.cem as u8;
                        // Copy the neighbor's endpoint ISE range, not the
                        // mode's, to avoid another round of quantization.
                        log.endpoint_ise_range = neighbor.log.endpoint_ise_range;
                        log.weight_ise_range = bmd.weight_ise_range;
                        log.grid_width = bmd.grid_x;
                        log.grid_height = bmd.grid_y;
                        log.dual_plane = bmd.dp;
                        log.color_component_selector = bmd.dp_channel;
                        log.endpoints[..nvals].copy_from_slice(&neighbor.log.endpoints[..nvals]);

                        let total_grid_weights =
                            bmd.grid_x * bmd.grid_y * if bmd.dp { 2 } else { 1 };
                        decode_values(
                            &mut decoder,
                            total_grid_weights,
                            bmd.weight_ise_range,
                            &mut log.weights,
                        );

                        let phys =
                            build_decomp_block(bmd, &log, bmd.weight_ise_range, orig_behavior)?;

                        log_rows[slot(cur_bx, cur_row_index)] = CodedBlock {
                            log,
                            user_mode: bm as u8,
                        };
                        let o = ((cur_bx + cur_by * num_blocks_x) as usize) * 16;
                        decoded_blocks[o..o + 16].copy_from_slice(&phys);
                        advance!();
                    }
                    3 | 4 => {
                        let (neighbor_bx, neighbor_by) = if em == 3 {
                            (cur_bx as i32 - 1, cur_by as i32)
                        } else {
                            (cur_bx as i32, cur_by as i32 - 1)
                        };
                        if neighbor_bx < 0 || neighbor_by < 0 {
                            return None;
                        }
                        let neighbor = log_rows[slot(
                            neighbor_bx as u32,
                            calc_row_index(cur_by, neighbor_by as u32, cur_row_index),
                        )];
                        if neighbor.log.color_endpoint_modes[0] == 0 {
                            return None;
                        }

                        let bmd = &BLOCK_MODE_DESCS[bm];
                        let nvals = num_endpoint_vals(bmd.cem);
                        if bmd.cem != neighbor.log.color_endpoint_modes[0] as u32 {
                            return None;
                        }

                        let mut log = LogAstcBlock::default();
                        log.num_partitions = 1;
                        log.color_endpoint_modes[0] = bmd.cem as u8;
                        log.dual_plane = bmd.dp;
                        log.color_component_selector = bmd.dp_channel;
                        log.endpoint_ise_range = bmd.endpoint_ise_range;

                        let mut requant = [0u8; 6];
                        requantize_ise_endpoints(
                            bmd.cem,
                            neighbor.log.endpoint_ise_range,
                            &neighbor.log.endpoints,
                            bmd.endpoint_ise_range,
                            &mut requant,
                        );
                        log.endpoints[..nvals].copy_from_slice(&requant[..nvals]);

                        // Apply the coded per-value rank deltas.
                        let total_endpoint_delta_vals = 1i32 << NUM_ENDPOINT_DELTA_BITS;
                        let low_delta_limit = -(total_endpoint_delta_vals / 2);
                        let qt = quant_tables();
                        let r = (log.endpoint_ise_range - 4) as usize;
                        let total_endpoint_levels = ise_levels(log.endpoint_ise_range) as i32;
                        for i in 0..nvals {
                            let cur_rank =
                                qt.endpoint_ise_to_rank[r][log.endpoints[i] as usize] as i32;
                            let delta =
                                decoder.get_bits(NUM_ENDPOINT_DELTA_BITS) as i32 + low_delta_limit;
                            let new_rank = cur_rank + delta;
                            if new_rank < 0 || new_rank >= total_endpoint_levels {
                                return None;
                            }
                            log.endpoints[i] = qt.endpoint_rank_to_ise[r][new_rank as usize];
                        }

                        log.weight_ise_range = bmd.weight_ise_range;
                        log.grid_width = bmd.grid_x;
                        log.grid_height = bmd.grid_y;

                        let total_grid_weights =
                            bmd.grid_x * bmd.grid_y * if bmd.dp { 2 } else { 1 };
                        decode_values(
                            &mut decoder,
                            total_grid_weights,
                            bmd.weight_ise_range,
                            &mut log.weights,
                        );

                        let phys =
                            build_decomp_block(bmd, &log, bmd.weight_ise_range, orig_behavior)?;

                        log_rows[slot(cur_bx, cur_row_index)] = CodedBlock {
                            log,
                            user_mode: bm as u8,
                        };
                        let o = ((cur_bx + cur_by * num_blocks_x) as usize) * 16;
                        decoded_blocks[o..o + 16].copy_from_slice(&phys);
                        advance!();
                    }
                    0 => {
                        let bmd = &BLOCK_MODE_DESCS[bm];
                        let nvals = num_endpoint_vals(bmd.cem);

                        let mut log = LogAstcBlock::default();
                        log.num_partitions = bmd.num_partitions;
                        for p in 0..bmd.num_partitions as usize {
                            log.color_endpoint_modes[p] = bmd.cem as u8;
                        }
                        log.endpoint_ise_range = bmd.endpoint_ise_range;
                        log.weight_ise_range = bmd.weight_ise_range;
                        log.grid_width = bmd.grid_x;
                        log.grid_height = bmd.grid_y;
                        log.dual_plane = bmd.dp;
                        log.color_component_selector = bmd.dp_channel;

                        if bmd.num_partitions == 2 {
                            let idx = decoder.decode_truncated_binary(NUM_UNIQUE_PARTITIONS2);
                            log.partition_id = PART2_UNIQUE_INDEX_TO_SEED[idx as usize] as u32;
                        } else if bmd.num_partitions == 3 {
                            let idx = decoder.decode_truncated_binary(NUM_UNIQUE_PARTITIONS3);
                            log.partition_id = PART3_UNIQUE_INDEX_TO_SEED[idx as usize] as u32;
                        }

                        decode_values(
                            &mut decoder,
                            (nvals as u32) * bmd.num_partitions,
                            bmd.endpoint_ise_range,
                            &mut log.endpoints,
                        );

                        let total_grid_weights =
                            bmd.grid_x * bmd.grid_y * if bmd.dp { 2 } else { 1 };
                        decode_values(
                            &mut decoder,
                            total_grid_weights,
                            bmd.weight_ise_range,
                            &mut log.weights,
                        );

                        let phys =
                            build_decomp_block(bmd, &log, bmd.weight_ise_range, orig_behavior)?;

                        log_rows[slot(cur_bx, cur_row_index)] = CodedBlock {
                            log,
                            user_mode: bm as u8,
                        };
                        let o = ((cur_bx + cur_by * num_blocks_x) as usize) * 16;
                        decoded_blocks[o..o + 16].copy_from_slice(&phys);
                        advance!();
                    }
                    _ => return None,
                }
            }
        }
    }

    if decoder.get_bits(16) != 0xA742 {
        return None;
    }

    Some((decoded_blocks, width, height))
}
