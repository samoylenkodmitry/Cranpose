//! The XUASTC LDR stream decompressor: all three stream syntaxes decoded block
//! by block into logical ASTC blocks and handed to a caller callback in raster
//! order. The init callback validates the header (aborting before any
//! dimension-driven allocation), and the block callback then consumes each
//! logical block.
// Block state is filled in field by field after a `default()`, which trips
// clippy::field_reassign_with_default; the explicit assignments read more
// clearly here than one large struct-update expression would.
#![allow(clippy::field_reassign_with_default, clippy::too_many_arguments)]
// let-else keeps each failure site explicit at its own check, and the range
// match and index loops follow the level-threshold chains directly.
#![allow(
    clippy::question_mark,
    clippy::match_overlapping_arm,
    clippy::needless_range_loop
)]

use super::arith::{ArithDec, BitModel, DataModel, GammaContexts, SimplifiedDecoder};
use super::dct::{
    decode_block_weights, num_weight_dc_levels, DctCoeff, DctSyms, DCT_MEAN_LEVELS1,
    DCT_RUN_LEN_EOB_SYM_INDEX,
};
use super::endpoints::{
    cem_supports_bc, cem_to_ldrcem_index, convert_endpoints_across_cems, decode_endpoints,
    num_cem_values, used_blue_contraction, CEM_LDR_RGBA_BASE_PLUS_OFFSET, CEM_LDR_RGBA_DIRECT,
    CEM_LDR_RGB_BASE_PLUS_OFFSET, CEM_LDR_RGB_DIRECT,
};
use super::modes::{
    block_size_modes, total_unique_patterns, unique_pat_index_to_part_seed, ASTC_BLOCK_SIZES,
};
use crate::astc::dequant::quant_tables;
use crate::astc::unpack::{ise_levels, LogAstcBlock};
use crate::basislz::decoder::BitwiseDecoder;
use crate::uastc_hdr_6x6::{decode_values, tables::REUSE_XY_DELTAS};
use alloc::vec;
use alloc::vec::Vec;

const ARITH_HEADER_MARKER: u32 = 0x01;
const ARITH_HEADER_MARKER_BITS: u32 = 5;
const FULL_ZSTD_HEADER_MARKER: u32 = 0x01;
const FULL_ZSTD_HEADER_MARKER_BITS: u32 = 5;
const FINAL_SYNC_MARKER: u32 = 0xAF;
const FINAL_SYNC_MARKER_BITS: u32 = 8;
const MAX_CONFIG_REUSE_NEIGHBORS: u32 = 3;
const PART_HASH_SIZE: usize = 64;
const TM_HASH_SIZE: usize = 128;

// Mode-byte flag bits (zstd/hybrid syntaxes).
const MODE_BYTE_IS_BASE_OFS: u32 = 1 << 3;
const MODE_BYTE_PART_HASH_HIT: u32 = 1 << 4;
const MODE_BYTE_DPCM_ENDPOINTS: u32 = 1 << 5;
const MODE_BYTE_TM_HASH_HIT: u32 = 1 << 6;
const MODE_BYTE_USE_DCT: u32 = 1 << 7;

// Mode symbols for the full-arith syntax.
const MODE_SOLID: u32 = 0;
const MODE_RAW: u32 = 1;
const MODE_REUSE_LEFT: u32 = 2;
const MODE_REUSE_DIAG: u32 = 4;
const MODE_RUN: u32 = 5;
const MODE_TOTAL: u32 = 6;

// OTM group counts.
const OTM_NUM_CEMS: usize = 14;
const OTM_NUM_SUBSETS: usize = 3;
const OTM_NUM_CCS: usize = 5;
const OTM_NUM_GRID_SIZES: usize = 2;
const OTM_NUM_GRID_ANISOS: usize = 3;

/// Fibonacci hash of `x` into the partition hash table.
#[inline]
fn part_hash_index(x: u32) -> usize {
    (x.wrapping_mul(2654435769) & (PART_HASH_SIZE as u32 - 1)) as usize
}
/// Fibonacci hash of `x` into the trial-mode hash table.
#[inline]
fn tm_hash_index(x: u32) -> usize {
    (x.wrapping_mul(2654435769) & (TM_HASH_SIZE as u32 - 1)) as usize
}

/// The stream header fields the container needs to validate.
pub struct XuastcInfo {
    pub block_width: u32,
    pub block_height: u32,
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,
    pub srgb: bool,
}

/// Decompress one whole zstd frame. The frame must declare its content size
/// (unknown-size frames are rejected), which is capped at i32::MAX. Empty
/// input decodes to empty output.
#[cfg(feature = "zstd")]
fn zstd_channel(comp: &[u8]) -> Option<Vec<u8>> {
    use ruzstd::io::Read;
    if comp.is_empty() {
        return Some(Vec::new());
    }
    // Frame header: magic, then the descriptor byte giving the
    // Frame_Content_Size field width. When that field is absent, the size is
    // known only if the single-segment flag implies a 1-byte size.
    if comp.len() < 5 || comp[..4] != [0x28, 0xB5, 0x2F, 0xFD] {
        return None;
    }
    let desc = comp[4];
    // The reserved descriptor bit must be zero; a set bit is a malformed frame.
    if desc & 0x08 != 0 {
        return None;
    }
    let fcs_flag = desc >> 6;
    let single_segment = desc & 0x20 != 0;
    let did_size = [0usize, 1, 2, 4][(desc & 3) as usize];
    let fcs_size = match fcs_flag {
        0 => {
            if single_segment {
                1
            } else {
                return None; // content size not present
            }
        }
        1 => 2,
        2 => 4,
        _ => 8,
    };
    let window_size = usize::from(!single_segment);
    let fcs_ofs = 5 + window_size + did_size;
    if comp.len() < fcs_ofs + fcs_size {
        return None;
    }
    let mut want = 0u64;
    for i in 0..fcs_size {
        want |= (comp[fcs_ofs + i] as u64) << (8 * i);
    }
    if fcs_size == 2 {
        want += 256;
    }
    if want > i32::MAX as u64 {
        return None;
    }
    let want = want as usize;

    // The frame may legitimately decode to fewer bytes than its declared
    // content size, so the output is truncated to what actually decoded;
    // decoding more than the declared size is an error.
    let mut dec = ruzstd::StreamingDecoder::new(comp).ok()?;
    let mut out = vec![0u8; want];
    let mut total = 0usize;
    loop {
        if total == want {
            let mut extra = [0u8; 1];
            if dec.read(&mut extra).ok()? != 0 {
                return None; // dstSize_tooSmall
            }
            break;
        }
        match dec.read(&mut out[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(_) => return None,
        }
    }
    out.truncate(total);
    Some(out)
}

/// Stub used when the `zstd` feature is off: no channel can be decoded.
#[cfg(not(feature = "zstd"))]
fn zstd_channel(_comp: &[u8]) -> Option<Vec<u8>> {
    None
}

/// Read a little-endian u32 from `d` at byte offset `o`.
#[inline]
fn rd32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

/// The 8-row ring of logical blocks that neighbor prediction reads back,
/// indexed by `by & 7`.
struct LogRing {
    rows: Vec<LogAstcBlock>,
    width: usize,
}

impl LogRing {
    /// Allocate the ring for `width` blocks per row, eight rows deep.
    fn new(width: usize) -> Self {
        Self {
            rows: vec![LogAstcBlock::default(); width * 8],
            width,
        }
    }
    /// Borrow the block at `(bx, by)`, wrapping the row into the eight-row ring.
    #[inline]
    fn get(&self, bx: u32, by: u32) -> &LogAstcBlock {
        &self.rows[(by & 7) as usize * self.width + bx as usize]
    }
    /// Store `blk` at `(bx, by)`, wrapping the row into the eight-row ring.
    #[inline]
    fn set(&mut self, bx: u32, by: u32, blk: &LogAstcBlock) {
        self.rows[(by & 7) as usize * self.width + bx as usize] = *blk;
    }
}

/// The solid-mode predictor (shared by both syntaxes): the previous
/// block's midpoint, or its solid color.
fn solid_predictor(prev: Option<&LogAstcBlock>) -> [u32; 4] {
    let Some(p) = prev else { return [0; 4] };
    if p.solid_color_flag_ldr {
        [
            (p.solid_color[0] >> 8) as u32,
            (p.solid_color[1] >> 8) as u32,
            (p.solid_color[2] >> 8) as u32,
            (p.solid_color[3] >> 8) as u32,
        ]
    } else {
        let (l, h) = decode_endpoints(
            p.color_endpoint_modes[0] as u32,
            &p.endpoints,
            p.endpoint_ise_range,
        );
        core::array::from_fn(|c| (l[c] as u32 + h[c] as u32 + 1) >> 1)
    }
}

/// Build a solid-color logical block from 8-bit RGBA, each channel replicated
/// into the 16-bit solid color.
#[inline]
fn make_solid(r: u32, g: u32, b: u32, a: u32) -> LogAstcBlock {
    let mut log = LogAstcBlock::default();
    log.solid_color_flag_ldr = true;
    log.solid_color = [
        (r | (r << 8)) as u16,
        (g | (g << 8)) as u16,
        (b | (b << 8)) as u16,
        (a | (a << 8)) as u16,
    ];
    log
}

/// Fill a logical block's configuration from a trial mode + actual CEM.
fn fill_config_from_tm(log: &mut LogAstcBlock, tm: &super::modes::TrialMode, actual_cem: u32) {
    for p in 0..tm.num_parts as usize {
        log.color_endpoint_modes[p] = actual_cem as u8;
    }
    log.num_partitions = tm.num_parts;
    log.dual_plane = tm.ccs_index >= 0;
    if log.dual_plane {
        log.color_component_selector = tm.ccs_index as u32;
    }
    log.weight_ise_range = tm.weight_ise_range;
    log.endpoint_ise_range = tm.endpoint_ise_range;
    log.grid_width = tm.grid_width;
    log.grid_height = tm.grid_height;
}

/// Copy full config + partition + endpoints from a neighbor's logical
/// block (the REUSE_CFG_ENDPOINTS modes and the run copies rely on the
/// blocks being plain `Copy`).
fn copy_full_config(dst: &mut LogAstcBlock, src: &LogAstcBlock) {
    let actual_cem = src.color_endpoint_modes[0] as u32;
    for i in 0..src.num_partitions as usize {
        dst.color_endpoint_modes[i] = actual_cem as u8;
    }
    dst.dual_plane = src.dual_plane;
    dst.color_component_selector = src.color_component_selector;
    dst.num_partitions = src.num_partitions;
    dst.partition_id = src.partition_id;
    dst.endpoint_ise_range = src.endpoint_ise_range;
    dst.weight_ise_range = src.weight_ise_range;
    dst.grid_width = src.grid_width;
    dst.grid_height = src.grid_height;
    let total = num_cem_values(actual_cem) * src.num_partitions as usize;
    dst.endpoints[..total].copy_from_slice(&src.endpoints[..total]);
}

/// The endpoint-DPCM prediction shared by both syntaxes: locate the reuse
/// block, check it, and predict per-partition endpoints across CEMs.
/// Returns the predicted per-partition endpoint symbols.
fn predict_endpoints(
    log: &LogAstcBlock,
    ring: &LogRing,
    bx: u32,
    by: u32,
    num_blocks_x: u32,
    num_blocks_y: u32,
    reuse_delta_index: u32,
    endpoints_use_bc: &[bool; 4],
) -> Option<[[u8; 8]; 4]> {
    if reuse_delta_index >= 32 {
        return None;
    }
    let (dx, dy) = REUSE_XY_DELTAS[reuse_delta_index as usize];
    let rbx = bx as i32 + dx as i32;
    let rby = by as i32 + dy as i32;
    if rbx < 0 || rby < 0 || rbx >= num_blocks_x as i32 || rby >= num_blocks_y as i32 {
        return None;
    }
    let pred = ring.get(rbx as u32, rby as u32);
    if pred.solid_color_flag_ldr {
        return None;
    }

    let mut out = [[0u8; 8]; 4];
    for (part, out_part) in out.iter_mut().enumerate().take(log.num_partitions as usize) {
        let mut bc_clamped = false;
        let mut ofs_clamped = false;
        if !convert_endpoints_across_cems(
            pred.color_endpoint_modes[0] as u32,
            pred.endpoint_ise_range,
            &pred.endpoints,
            log.color_endpoint_modes[0] as u32,
            log.endpoint_ise_range,
            out_part,
            false,
            endpoints_use_bc[part],
            false,
            &mut bc_clamped,
            &mut ofs_clamped,
        ) {
            return None;
        }
    }
    Some(out)
}

/// Decode the full-zstd syntax: each channel is its own zstd stream, with the
/// block layout driven by the raw bit stream.
fn decompress_full_zstd(
    comp: &[u8],
    init_cb: &mut dyn FnMut(&XuastcInfo) -> bool,
    block_cb: &mut dyn FnMut(u32, u32, &LogAstcBlock) -> bool,
) -> Option<XuastcInfo> {
    const HDR_SIZE: usize = 1 + 21 * 4; // the syntax-selector byte (matched by the caller) plus 21 packed u32 slots; f(0)..f(19) are channel byte-lengths, the last slot is unread
    if comp.len() < HDR_SIZE {
        return None;
    }
    let f = |i: usize| rd32(comp, 1 + i * 4) as usize;
    let (raw_bits_len, mode_bytes_len, solid_dpcm_len) = (f(0), f(1), f(2));
    let (ep_reuse_len, use_bc_len) = (f(3), f(4));
    let ep_dpcm_len = [f(5), f(6), f(7), f(8), f(9), f(10)];
    let (mean0_len, mean1_len, run_len, coeff_len, sign_len) = (f(11), f(12), f(13), f(14), f(15));
    let (w2_len, w3_len, w4_len, w8_len) = (f(16), f(17), f(18), f(19));

    if raw_bits_len == 0 || mode_bytes_len == 0 {
        return None;
    }
    let total: u64 = [
        raw_bits_len,
        mode_bytes_len,
        solid_dpcm_len,
        ep_reuse_len,
        use_bc_len,
        ep_dpcm_len[0],
        ep_dpcm_len[1],
        ep_dpcm_len[2],
        ep_dpcm_len[3],
        ep_dpcm_len[4],
        ep_dpcm_len[5],
        mean0_len,
        mean1_len,
        run_len,
        coeff_len,
        sign_len,
        w2_len,
        w3_len,
        w4_len,
        w8_len,
    ]
    .iter()
    .map(|&v| v as u64)
    .sum();
    if (comp.len() as u64) < HDR_SIZE as u64 + total {
        return None;
    }

    let mut cur = HDR_SIZE;
    macro_rules! take {
        ($len:expr) => {{
            let start = cur;
            #[allow(unused_assignments)]
            {
                cur += $len;
            }
            &comp[start..start + $len]
        }};
    }

    let raw_slice = take!(raw_bits_len);
    let mut raw_bits = BitwiseDecoder::new(raw_slice);

    // The zstd side channels, in payload order; sign bits are stored raw.
    let uncomp_mode = zstd_channel(take!(mode_bytes_len))?;
    let uncomp_solid = zstd_channel(take!(solid_dpcm_len))?;
    let uncomp_ep_reuse = zstd_channel(take!(ep_reuse_len))?;
    let uncomp_use_bc = zstd_channel(take!(use_bc_len))?;
    let mut uncomp_ep_dpcm: [Vec<u8>; 6] = Default::default();
    for (i, l) in ep_dpcm_len.iter().enumerate() {
        uncomp_ep_dpcm[i] = zstd_channel(take!(*l))?;
    }
    let uncomp_mean0 = zstd_channel(take!(mean0_len))?;
    let uncomp_mean1 = zstd_channel(take!(mean1_len))?;
    let uncomp_run = zstd_channel(take!(run_len))?;
    let uncomp_coeff = zstd_channel(take!(coeff_len))?;
    let sign_slice = take!(sign_len);
    let uncomp_w2 = zstd_channel(take!(w2_len))?;
    let uncomp_w3 = zstd_channel(take!(w3_len))?;
    let uncomp_w4 = zstd_channel(take!(w4_len))?;
    let uncomp_w8 = zstd_channel(take!(w8_len))?;

    let mut mode_dec = SimplifiedDecoder::new(&uncomp_mode);
    let mut solid_dec = SimplifiedDecoder::new(&uncomp_solid);
    let mut ep_reuse_dec = SimplifiedDecoder::new(&uncomp_ep_reuse);
    let mut use_bc_dec = SimplifiedDecoder::new(&uncomp_use_bc);
    let mut ep_dpcm_dec: [SimplifiedDecoder; 6] =
        core::array::from_fn(|i| SimplifiedDecoder::new(&uncomp_ep_dpcm[i]));
    let mut mean0_dec = SimplifiedDecoder::new(&uncomp_mean0);
    let mut mean1_dec = SimplifiedDecoder::new(&uncomp_mean1);
    let mut run_dec = SimplifiedDecoder::new(&uncomp_run);
    let mut coeff_dec = SimplifiedDecoder::new(&uncomp_coeff);
    let mut sign_dec = SimplifiedDecoder::new(sign_slice);
    let mut w2_dec = SimplifiedDecoder::new(&uncomp_w2);
    let mut w3_dec = SimplifiedDecoder::new(&uncomp_w3);
    let mut w4_dec = SimplifiedDecoder::new(&uncomp_w4);
    let mut w8_dec = SimplifiedDecoder::new(&uncomp_w8);

    // Header fields from the raw bit stream.
    if raw_bits.get_bits(FULL_ZSTD_HEADER_MARKER_BITS) != FULL_ZSTD_HEADER_MARKER {
        return None;
    }
    let bsi = raw_bits.get_bits(4) as usize;
    if bsi >= ASTC_BLOCK_SIZES.len() {
        return None;
    }
    let (block_width, block_height) = ASTC_BLOCK_SIZES[bsi];
    let srgb = raw_bits.get_bits(1) != 0;
    let width = raw_bits.get_bits(16);
    let height = raw_bits.get_bits(16);
    let has_alpha = raw_bits.get_bits(1) != 0;
    let use_dct = raw_bits.get_bits(1) != 0;
    let mut int_q = 0;
    if use_dct {
        int_q = raw_bits.get_bits(8);
    }
    let dct_q = int_q as f32 / 2.0;
    if use_dct && (dct_q <= 0.0 || dct_q > 100.0) {
        return None;
    }
    // Dimensions are not otherwise range-checked here, so reject zero width or
    // height before the block loop starts to rely on them.
    if width == 0 || height == 0 {
        return None;
    }

    let info = XuastcInfo {
        block_width,
        block_height,
        width,
        height,
        has_alpha,
        srgb,
    };
    if !init_cb(&info) {
        return None;
    }

    let num_blocks_x = width.div_ceil(block_width);
    let num_blocks_y = height.div_ceil(block_height);
    let bsm = block_size_modes(bsi);

    let mut ring = LogRing::new(num_blocks_x as usize);
    // Trial-mode index per block, kept for two rows so neighbor prediction can
    // read the left, upper, and diagonal blocks.
    let mut tm_states = vec![0i32; num_blocks_x as usize * 2];
    let mut cur_run_len = 0u32;
    let mut part2_hash = [-1i32; PART_HASH_SIZE];
    let mut part3_hash = [-1i32; PART_HASH_SIZE];
    let mut tm_hash = [-1i32; TM_HASH_SIZE];

    let mut syms = DctSyms {
        dc_sym: 0,
        coeffs: Vec::with_capacity(65),
    };

    for by in 0..num_blocks_y {
        for bx in 0..num_blocks_x {
            let state_idx = (by & 1) as usize * num_blocks_x as usize + bx as usize;
            let left_tm = if bx != 0 {
                Some(tm_states[(by & 1) as usize * num_blocks_x as usize + bx as usize - 1])
            } else {
                None
            };
            let upper_tm = if by != 0 {
                Some(tm_states[((by - 1) & 1) as usize * num_blocks_x as usize + bx as usize])
            } else {
                None
            };
            let diag_tm = if bx != 0 && by != 0 {
                Some(tm_states[((by - 1) & 1) as usize * num_blocks_x as usize + bx as usize - 1])
            } else {
                None
            };

            if cur_run_len != 0 {
                let prev = *if bx != 0 {
                    ring.get(bx - 1, by)
                } else {
                    ring.get(bx, by - 1)
                };
                ring.set(bx, by, &prev);
                if !block_cb(bx, by, &prev) {
                    return None;
                }
                tm_states[state_idx] = left_tm.or(upper_tm).unwrap_or(0);
                cur_run_len -= 1;
                continue;
            }

            let mode_byte = mode_dec.get_bits8();

            if mode_byte & 3 == 0b01 {
                // run mode
                if bx == 0 && by == 0 {
                    return None;
                }
                cur_run_len = 1 + (mode_byte >> 2);
                if cur_run_len > num_blocks_x - bx {
                    return None;
                }
                let prev = *if bx != 0 {
                    ring.get(bx - 1, by)
                } else {
                    ring.get(bx, by - 1)
                };
                ring.set(bx, by, &prev);
                if !block_cb(bx, by, &prev) {
                    return None;
                }
                tm_states[state_idx] = left_tm.or(upper_tm).unwrap_or(0);
                cur_run_len -= 1;
                continue;
            } else if mode_byte & 15 == 0b0011 {
                // solid mode
                let prev_blk = if bx != 0 {
                    Some(ring.get(bx - 1, by))
                } else if by != 0 {
                    Some(ring.get(bx, by - 1))
                } else {
                    None
                };
                let pred = solid_predictor(prev_blk);
                let dr = solid_dec.get_bits8();
                let dg = solid_dec.get_bits8();
                let db = solid_dec.get_bits8();
                let da = if has_alpha { solid_dec.get_bits8() } else { 0 };
                let r = (pred[0] + dr) & 0xFF;
                let g = (pred[1] + dg) & 0xFF;
                let b = (pred[2] + db) & 0xFF;
                let a = if has_alpha {
                    (pred[3] + da) & 0xFF
                } else {
                    255
                };
                let log = make_solid(r, g, b, a);
                ring.set(bx, by, &log);
                if !block_cb(bx, by, &log) {
                    return None;
                }
                tm_states[state_idx] = -1;
                continue;
            }

            let mut log = LogAstcBlock::default();
            let tm_index: u32;
            let actual_cem: u32;

            if mode_byte & 1 == 0 {
                // RAW mode.
                let config_reuse_index = (mode_byte >> 1) & 3;
                if config_reuse_index < MAX_CONFIG_REUSE_NEIGHBORS {
                    let (cfg_dx, cfg_dy, cfg_tm) = match config_reuse_index {
                        0 => (-1i32, 0i32, left_tm),
                        1 => (0, -1, upper_tm),
                        _ => (-1, -1, diag_tm),
                    };
                    let Some(cfg_tm) = cfg_tm else { return None };
                    if cfg_tm < 0 {
                        return None;
                    }
                    let cfg_blk =
                        ring.get((bx as i32 + cfg_dx) as u32, (by as i32 + cfg_dy) as u32);
                    tm_index = cfg_tm as u32;
                    log.partition_id = cfg_blk.partition_id;
                    actual_cem = cfg_blk.color_endpoint_modes[0] as u32;
                    tm_states[state_idx] = tm_index as i32;
                } else {
                    if mode_byte & MODE_BYTE_TM_HASH_HIT != 0 {
                        tm_index = tm_hash[raw_bits.get_bits(7) as usize] as u32;
                    } else {
                        tm_index = raw_bits.decode_truncated_binary(bsm.modes.len() as u32);
                        tm_hash[tm_hash_index(tm_index)] = tm_index as i32;
                    }
                    if tm_index as usize >= bsm.modes.len() {
                        return None;
                    }
                    tm_states[state_idx] = tm_index as i32;
                    let tm = &bsm.modes[tm_index as usize];

                    let mut cem = tm.cem;
                    if (cem == CEM_LDR_RGB_DIRECT || cem == CEM_LDR_RGBA_DIRECT)
                        && mode_byte & MODE_BYTE_IS_BASE_OFS != 0
                    {
                        cem = if cem == CEM_LDR_RGB_DIRECT {
                            CEM_LDR_RGB_BASE_PLUS_OFFSET
                        } else {
                            CEM_LDR_RGBA_BASE_PLUS_OFFSET
                        };
                    }
                    actual_cem = cem;

                    if tm.num_parts > 1 {
                        let total_unique = total_unique_patterns(bsi, tm.num_parts);
                        let hash: &mut [i32; PART_HASH_SIZE] = if tm.num_parts == 2 {
                            &mut part2_hash
                        } else {
                            &mut part3_hash
                        };
                        let unique_pat_index = if mode_byte & MODE_BYTE_PART_HASH_HIT != 0 {
                            hash[raw_bits.get_bits(6) as usize] as u32
                        } else {
                            let idx = raw_bits.decode_truncated_binary(total_unique);
                            hash[part_hash_index(idx)] = idx as i32;
                            idx
                        };
                        if unique_pat_index >= total_unique {
                            return None;
                        }
                        log.partition_id =
                            unique_pat_index_to_part_seed(bsi, tm.num_parts, unique_pat_index)
                                as u32;
                    }
                }

                if tm_index as usize >= bsm.modes.len() {
                    return None;
                }
                let tm = &bsm.modes[tm_index as usize];
                let bc_supported = cem_supports_bc(actual_cem);
                let total_endpoint_vals = num_cem_values(actual_cem);
                fill_config_from_tm(&mut log, tm, actual_cem);

                if mode_byte & MODE_BYTE_DPCM_ENDPOINTS != 0 {
                    let num_levels = ise_levels(log.endpoint_ise_range) as i32;
                    let r = (log.endpoint_ise_range - 4) as usize;
                    let qt = quant_tables();

                    let reuse_delta_index = ep_reuse_dec.get_bits8();
                    let mut endpoints_use_bc = [false; 4];
                    if bc_supported {
                        for e in endpoints_use_bc
                            .iter_mut()
                            .take(log.num_partitions as usize)
                        {
                            *e = use_bc_dec.get_bits1() != 0;
                        }
                    }
                    let predicted = predict_endpoints(
                        &log,
                        &ring,
                        bx,
                        by,
                        num_blocks_x,
                        num_blocks_y,
                        reuse_delta_index,
                        &endpoints_use_bc,
                    )?;

                    // The delta channel is chosen by the endpoint level
                    // count; note the 3-bit channel reads 4-bit codes.
                    let chan = match num_levels {
                        ..=8 => 0usize,
                        ..=16 => 1,
                        ..=32 => 2,
                        ..=64 => 3,
                        ..=128 => 4,
                        _ => 5,
                    };
                    for part in 0..tm.num_parts as usize {
                        for val in 0..total_endpoint_vals {
                            let delta = if chan <= 1 {
                                ep_dpcm_dec[chan].get_bits4() as i32
                            } else {
                                ep_dpcm_dec[chan].get_bits8() as i32
                            };
                            let e_rank = (delta
                                + qt.endpoint_ise_to_rank[r][predicted[part][val] as usize] as i32)
                                % num_levels;
                            log.endpoints[part * total_endpoint_vals + val] =
                                qt.endpoint_rank_to_ise[r][e_rank as usize];
                        }
                    }
                } else {
                    // decode_values cannot fail; it reads a fixed number of
                    // symbols.
                    decode_values(
                        &mut raw_bits,
                        (tm.num_parts as usize * total_endpoint_vals) as u32,
                        log.endpoint_ise_range,
                        &mut log.endpoints,
                    );
                }
            } else if mode_byte & 15 >= 0b0111 {
                // reuse-config-endpoints modes (left, up, diag)
                let reuse_index = ((mode_byte >> 2) & 3).wrapping_sub(1);
                let (cfg_dx, cfg_dy, cfg_tm) = match reuse_index {
                    0 => (-1i32, 0i32, left_tm),
                    1 => (0, -1, upper_tm),
                    2 => (-1, -1, diag_tm),
                    _ => return None,
                };
                let Some(cfg_tm) = cfg_tm else { return None };
                if cfg_tm < 0 {
                    return None;
                }
                let cfg_blk = *ring.get((bx as i32 + cfg_dx) as u32, (by as i32 + cfg_dy) as u32);
                tm_index = cfg_tm as u32;
                actual_cem = cfg_blk.color_endpoint_modes[0] as u32;
                copy_full_config(&mut log, &cfg_blk);
                tm_states[state_idx] = tm_index as i32;
                let _ = actual_cem;
            } else {
                return None;
            }

            // Decode weights.
            if tm_index as usize >= bsm.modes.len() {
                return None;
            }
            let tm = &bsm.modes[tm_index as usize];
            let total_planes = if tm.ccs_index >= 0 { 2u32 } else { 1 };
            let total_weights = tm.grid_width * tm.grid_height;

            let block_used_dct = use_dct && (mode_byte & MODE_BYTE_USE_DCT != 0);

            if block_used_dct {
                let num_dc_levels = num_weight_dc_levels(log.weight_ise_range);
                for plane in 0..total_planes {
                    syms.coeffs.clear();
                    syms.dc_sym = if num_dc_levels == DCT_MEAN_LEVELS1 {
                        mean1_dec.get_bits8()
                    } else {
                        mean0_dec.get_bits4()
                    };
                    let mut cur_zig_ofs = 1u32;
                    while cur_zig_ofs < total_weights {
                        let run = run_dec.get_bits8();
                        if run == DCT_RUN_LEN_EOB_SYM_INDEX {
                            break;
                        }
                        cur_zig_ofs += run;
                        if cur_zig_ofs >= total_weights {
                            return None;
                        }
                        let sign = sign_dec.get_bits1();
                        let mut coeff = coeff_dec.get_bits8() as i32 + 1;
                        if sign != 0 {
                            coeff = -coeff;
                        }
                        syms.coeffs.push(DctCoeff {
                            num_zeros: run as u16,
                            coeff: coeff as i16,
                        });
                        cur_zig_ofs += 1;
                    }
                    decode_block_weights(dct_q, plane, block_width, block_height, &mut log, &syms)?;
                }
            } else {
                let num_weight_levels = ise_levels(log.weight_ise_range);
                let wr = &quant_tables().weight_rank_to_ise[log.weight_ise_range as usize];
                for plane in 0..total_planes as usize {
                    let mut prev_w = num_weight_levels / 2;
                    for wi in 0..total_weights as usize {
                        // The masked power-of-two cases reduce identically to
                        // the modulo ones; both forms are kept so each
                        // weight-level count uses the cheapest reduction and
                        // read width.
                        let w = if num_weight_levels < 4 {
                            (prev_w + w2_dec.get_bits2()) % num_weight_levels
                        } else if num_weight_levels == 4 {
                            (prev_w + w2_dec.get_bits2()) & 3
                        } else if num_weight_levels < 8 {
                            (prev_w + w3_dec.get_bits4()) % num_weight_levels
                        } else if num_weight_levels == 8 {
                            (prev_w + w3_dec.get_bits4()) & 7
                        } else if num_weight_levels < 16 {
                            (prev_w + w4_dec.get_bits4()) % num_weight_levels
                        } else if num_weight_levels == 16 {
                            (prev_w + w4_dec.get_bits4()) & 15
                        } else {
                            (prev_w + w8_dec.get_bits8()) % num_weight_levels
                        };
                        prev_w = w;
                        log.weights[plane + wi * total_planes as usize] = wr[w as usize];
                    }
                }
            }

            ring.set(bx, by, &log);
            if !block_cb(bx, by, &log) {
                return None;
            }
        }
    }

    if raw_bits.get_bits(FINAL_SYNC_MARKER_BITS) != FINAL_SYNC_MARKER {
        return None;
    }
    if !mode_dec.fully_consumed() {
        return None;
    }

    Some(info)
}

/// Per-block predictor state carried between blocks in the arith syntaxes. Its
/// `Default` zeroes every field, `tm_index` included.
#[derive(Clone, Copy, Default)]
struct PrevState {
    was_solid_color: bool,
    used_weight_dct: bool,
    first_endpoint_uses_bc: bool,
    reused_full_cfg: bool,
    used_part_hash: bool,
    tm_index: i32,
    base_cem_index: u32,
    subset_index: u32,
    ccs_index: u32,
    grid_size: u32,
    grid_aniso: u32,
}

/// The hybrid syntax's zstd side channels: the DCT and weight streams decoded
/// separately from the arith stream. The full-arith path passes `None` here and
/// pulls everything from the arith stream instead.
struct SideChannels<'a> {
    mean0: SimplifiedDecoder<'a>,
    mean1: SimplifiedDecoder<'a>,
    run: SimplifiedDecoder<'a>,
    coeff: SimplifiedDecoder<'a>,
    sign: SimplifiedDecoder<'a>,
    w2: SimplifiedDecoder<'a>,
    w3: SimplifiedDecoder<'a>,
    w4: SimplifiedDecoder<'a>,
    w8: SimplifiedDecoder<'a>,
}

/// Decode the full-arith and hybrid syntaxes. `side` supplies the hybrid
/// syntax's zstd side channels; `None` pulls every symbol from the arith
/// stream.
fn decompress_arith(
    arith_buf: &[u8],
    mut side: Option<SideChannels<'_>>,
    init_cb: &mut dyn FnMut(&XuastcInfo) -> bool,
    block_cb: &mut dyn FnMut(u32, u32, &LogAstcBlock) -> bool,
) -> Option<XuastcInfo> {
    let mut dec = ArithDec::new(arith_buf)?;

    if dec.get_bits(ARITH_HEADER_MARKER_BITS) != ARITH_HEADER_MARKER {
        return None;
    }
    let bsi = dec.get_bits(4) as usize;
    if bsi >= ASTC_BLOCK_SIZES.len() {
        return None;
    }
    let (block_width, block_height) = ASTC_BLOCK_SIZES[bsi];
    let srgb = dec.get_bit() != 0;
    let width = dec.get_bits(16);
    let height = dec.get_bits(16);
    if width < 1 || height < 1 {
        return None;
    }
    let has_alpha = dec.get_bit() != 0;
    let use_dct = dec.get_bits(1) != 0;
    let mut int_q = 0;
    if use_dct {
        int_q = dec.get_bits(8);
    }
    let dct_q = int_q as f32 / 2.0;
    if use_dct && (dct_q <= 0.0 || dct_q > 100.0) {
        return None;
    }

    let info = XuastcInfo {
        block_width,
        block_height,
        width,
        height,
        has_alpha,
        srgb,
    };
    if !init_cb(&info) {
        return None;
    }

    let num_blocks_x = width.div_ceil(block_width);
    let num_blocks_y = height.div_ceil(block_height);
    let bsm = block_size_modes(bsi);

    // Adaptive models.
    let mut mode_model = DataModel::new(MODE_TOTAL, false);
    let mut solid_color_dpcm_model: [DataModel; 4] =
        core::array::from_fn(|_| DataModel::new(256, true));
    let mut raw_endpoint_models: [DataModel; 17] =
        core::array::from_fn(|i| DataModel::new(ise_levels(4 + i as u32), false));
    let mut dpcm_endpoint_models: [DataModel; 17] =
        core::array::from_fn(|i| DataModel::new(ise_levels(4 + i as u32), false));
    let mut is_base_ofs_model = BitModel::default();
    let mut use_dct_model: [BitModel; 4] = Default::default();
    let mut use_dpcm_endpoints_model = BitModel::default();
    let mut cem_index_model: [DataModel; 8] =
        core::array::from_fn(|_| DataModel::new(OTM_NUM_CEMS as u32, false));
    let mut subset_index_model: [DataModel; OTM_NUM_SUBSETS] =
        core::array::from_fn(|_| DataModel::new(OTM_NUM_SUBSETS as u32, false));
    let mut ccs_index_model: [DataModel; OTM_NUM_CCS] =
        core::array::from_fn(|_| DataModel::new(OTM_NUM_CCS as u32, false));
    let mut grid_size_model: [DataModel; OTM_NUM_GRID_SIZES] =
        core::array::from_fn(|_| DataModel::new(OTM_NUM_GRID_SIZES as u32, false));
    let mut grid_aniso_model: [DataModel; OTM_NUM_GRID_ANISOS] =
        core::array::from_fn(|_| DataModel::new(OTM_NUM_GRID_ANISOS as u32, false));
    let use_fast_decoding = side.is_some();
    let mut dct_run_len_model = DataModel::default();
    let mut dct_coeff_mag = DataModel::default();
    let mut weight_mean_models: [DataModel; 2] = Default::default();
    let mut raw_weight_models: [DataModel; 12] = Default::default();
    if !use_fast_decoding {
        dct_run_len_model.init(DCT_RUN_LEN_EOB_SYM_INDEX + 1, false);
        dct_coeff_mag.init(255, false);
        weight_mean_models[0].init(9, false);
        weight_mean_models[1].init(DCT_MEAN_LEVELS1, false);
        for (i, m) in raw_weight_models.iter_mut().enumerate() {
            m.init(ise_levels(i as u32), false);
        }
    }
    // The 5-D submode models, lazily initialized on first use.
    let mut submode_models: Vec<DataModel> =
        vec![
            DataModel::default();
            OTM_NUM_CEMS * OTM_NUM_SUBSETS * OTM_NUM_CCS * OTM_NUM_GRID_SIZES * OTM_NUM_GRID_ANISOS
        ];
    let mut endpoints_use_bc_models: [BitModel; 4] = Default::default();
    let mut endpoint_reuse_delta_model = DataModel::new(32, false);
    let mut config_reuse_model: [DataModel; 4] = core::array::from_fn(|_| DataModel::new(4, false));
    let mut run_len_contexts = GammaContexts::default();
    let mut use_part_hash_model: [BitModel; 4] = Default::default();
    let mut part2_hash_index_model = DataModel::new(PART_HASH_SIZE as u32, true);
    let mut part3_hash_index_model = DataModel::new(PART_HASH_SIZE as u32, true);

    let mut ring = LogRing::new(num_blocks_x as usize);
    let mut states = vec![PrevState::default(); num_blocks_x as usize * 2];
    let mut cur_run_len = 0u32;
    let mut part2_hash = [-1i32; PART_HASH_SIZE];
    let mut part3_hash = [-1i32; PART_HASH_SIZE];

    let mut syms = DctSyms {
        dc_sym: 0,
        coeffs: Vec::with_capacity(65),
    };

    for by in 0..num_blocks_y {
        for bx in 0..num_blocks_x {
            let row = (by & 1) as usize * num_blocks_x as usize;
            let prev_row = ((by.wrapping_sub(1)) & 1) as usize * num_blocks_x as usize;
            let state_idx = row + bx as usize;
            let left = if bx != 0 {
                Some(states[row + bx as usize - 1])
            } else {
                None
            };
            let upper = if by != 0 {
                Some(states[prev_row + bx as usize])
            } else {
                None
            };
            let diag = if bx != 0 && by != 0 {
                Some(states[prev_row + bx as usize - 1])
            } else {
                None
            };
            let pred_state = left.or(upper);

            let mut new_state = PrevState::default();

            if cur_run_len != 0 {
                let prev_blk = *if bx != 0 {
                    ring.get(bx - 1, by)
                } else {
                    ring.get(bx, by - 1)
                };
                ring.set(bx, by, &prev_blk);
                if !block_cb(bx, by, &prev_blk) {
                    return None;
                }
                let p = left.or(upper).unwrap_or_default();
                new_state = p;
                new_state.reused_full_cfg = true;
                states[state_idx] = new_state;
                cur_run_len -= 1;
                continue;
            }

            let mode_index = dec.decode_sym(&mut mode_model);

            match mode_index {
                MODE_SOLID => {
                    let prev_blk = if bx != 0 {
                        Some(ring.get(bx - 1, by))
                    } else if by != 0 {
                        Some(ring.get(bx, by - 1))
                    } else {
                        None
                    };
                    let pred = solid_predictor(prev_blk);
                    let r = (pred[0] + dec.decode_sym(&mut solid_color_dpcm_model[0])) & 0xFF;
                    let g = (pred[1] + dec.decode_sym(&mut solid_color_dpcm_model[1])) & 0xFF;
                    let b = (pred[2] + dec.decode_sym(&mut solid_color_dpcm_model[2])) & 0xFF;
                    let a = if has_alpha {
                        (pred[3] + dec.decode_sym(&mut solid_color_dpcm_model[3])) & 0xFF
                    } else {
                        255
                    };
                    let log = make_solid(r, g, b, a);
                    ring.set(bx, by, &log);
                    if !block_cb(bx, by, &log) {
                        return None;
                    }
                    // Bias the neighbor-prediction state for the following
                    // blocks.
                    if use_dct {
                        new_state.used_weight_dct = true;
                    }
                    new_state.first_endpoint_uses_bc = true;
                    new_state.was_solid_color = true;
                    new_state.tm_index = -1;
                    new_state.base_cem_index = CEM_LDR_RGB_DIRECT;
                    new_state.used_part_hash = true;
                    states[state_idx] = new_state;
                    continue;
                }
                MODE_RUN => {
                    if bx == 0 && by == 0 {
                        return None;
                    }
                    cur_run_len = dec.decode_gamma(&mut run_len_contexts);
                    if cur_run_len == 0 || cur_run_len > num_blocks_x - bx {
                        return None;
                    }
                    let prev_blk = *if bx != 0 {
                        ring.get(bx - 1, by)
                    } else {
                        ring.get(bx, by - 1)
                    };
                    ring.set(bx, by, &prev_blk);
                    if !block_cb(bx, by, &prev_blk) {
                        return None;
                    }
                    let p = left.or(upper).unwrap_or_default();
                    new_state = p;
                    new_state.reused_full_cfg = true;
                    states[state_idx] = new_state;
                    cur_run_len -= 1;
                    continue;
                }
                MODE_RAW | MODE_REUSE_LEFT..=MODE_REUSE_DIAG => {}
                _ => return None,
            }

            // RAW or REUSE_CFG_ENDPOINTS_*.
            let mut log = LogAstcBlock::default();
            let tm_index: u32;
            let mut actual_cem: u32;

            if mode_index != MODE_RAW {
                // Full config + partition + endpoint reuse.
                let (cfg_dx, cfg_dy, cfg_state) = match mode_index {
                    MODE_REUSE_LEFT => (-1i32, 0i32, left),
                    3 => (0, -1, upper), // reuse up
                    _ => (-1, -1, diag),
                };
                let Some(cfg_state) = cfg_state else {
                    return None;
                };
                if cfg_state.tm_index < 0 {
                    return None;
                }
                let cfg_blk = *ring.get((bx as i32 + cfg_dx) as u32, (by as i32 + cfg_dy) as u32);
                tm_index = cfg_state.tm_index as u32;
                actual_cem = cfg_blk.color_endpoint_modes[0] as u32;
                copy_full_config(&mut log, &cfg_blk);

                new_state.tm_index = cfg_state.tm_index;
                new_state.base_cem_index = cfg_state.base_cem_index;
                new_state.subset_index = cfg_state.subset_index;
                new_state.ccs_index = cfg_state.ccs_index;
                new_state.grid_size = cfg_state.grid_size;
                new_state.grid_aniso = cfg_state.grid_aniso;
                new_state.used_part_hash = cfg_state.used_part_hash;
                new_state.reused_full_cfg = true;

                if cem_supports_bc(actual_cem) {
                    new_state.first_endpoint_uses_bc =
                        used_blue_contraction(actual_cem, &log.endpoints, log.endpoint_ise_range);
                }
            } else {
                let reuse_ctx = (left.map_or(1, |s| s.reused_full_cfg as u32))
                    | (upper.map_or(2, |s| (s.reused_full_cfg as u32) << 1));
                let config_reuse_index =
                    dec.decode_sym(&mut config_reuse_model[reuse_ctx as usize]);

                if config_reuse_index < MAX_CONFIG_REUSE_NEIGHBORS {
                    let (cfg_dx, cfg_dy, cfg_state) = match config_reuse_index {
                        0 => (-1i32, 0i32, left),
                        1 => (0, -1, upper),
                        _ => (-1, -1, diag),
                    };
                    let Some(cfg_state) = cfg_state else {
                        return None;
                    };
                    if cfg_state.tm_index < 0 {
                        return None;
                    }
                    let cfg_blk =
                        ring.get((bx as i32 + cfg_dx) as u32, (by as i32 + cfg_dy) as u32);
                    tm_index = cfg_state.tm_index as u32;
                    log.partition_id = cfg_blk.partition_id;
                    actual_cem = cfg_blk.color_endpoint_modes[0] as u32;

                    new_state.tm_index = cfg_state.tm_index;
                    new_state.base_cem_index = cfg_state.base_cem_index;
                    new_state.subset_index = cfg_state.subset_index;
                    new_state.ccs_index = cfg_state.ccs_index;
                    new_state.grid_size = cfg_state.grid_size;
                    new_state.grid_aniso = cfg_state.grid_aniso;
                    new_state.used_part_hash = cfg_state.used_part_hash;
                    new_state.reused_full_cfg = true;
                } else {
                    // Full ASTC config decode.
                    let (prev_cem_index, prev_subset, prev_ccs, prev_gs, prev_ga) = pred_state
                        .map_or((CEM_LDR_RGB_DIRECT, 0, 0, 0, 0), |s| {
                            (
                                s.base_cem_index,
                                s.subset_index,
                                s.ccs_index,
                                s.grid_size,
                                s.grid_aniso,
                            )
                        });
                    let ldrcem_index = cem_to_ldrcem_index(prev_cem_index);
                    let cem_index = dec.decode_sym(&mut cem_index_model[ldrcem_index]);
                    let subset_index =
                        dec.decode_sym(&mut subset_index_model[prev_subset as usize]);
                    let ccs_index = dec.decode_sym(&mut ccs_index_model[prev_ccs as usize]);
                    let grid_size_index = dec.decode_sym(&mut grid_size_model[prev_gs as usize]);
                    let grid_aniso_index = dec.decode_sym(&mut grid_aniso_model[prev_ga as usize]);

                    let candidates = bsm.tm_candidates(
                        cem_index,
                        subset_index,
                        ccs_index,
                        grid_size_index,
                        grid_aniso_index,
                    );
                    let mut submode_index = 0u32;
                    if candidates.len() > 1 {
                        let flat = (((cem_index as usize * OTM_NUM_SUBSETS
                            + subset_index as usize)
                            * OTM_NUM_CCS
                            + ccs_index as usize)
                            * OTM_NUM_GRID_SIZES
                            + grid_size_index as usize)
                            * OTM_NUM_GRID_ANISOS
                            + grid_aniso_index as usize;
                        let submode_model = &mut submode_models[flat];
                        if !submode_model.is_initialized() {
                            submode_model.init(candidates.len() as u32, true);
                        }
                        submode_index = dec.decode_sym(submode_model);
                    }
                    if submode_index as usize >= candidates.len() {
                        return None;
                    }
                    tm_index = candidates[submode_index as usize];

                    new_state.tm_index = tm_index as i32;
                    new_state.base_cem_index = cem_index;
                    new_state.subset_index = subset_index;
                    new_state.ccs_index = ccs_index;
                    new_state.grid_size = grid_size_index;
                    new_state.grid_aniso = grid_aniso_index;
                    new_state.reused_full_cfg = false;

                    if tm_index as usize >= bsm.modes.len() {
                        return None;
                    }
                    let tm = &bsm.modes[tm_index as usize];
                    actual_cem = tm.cem;
                    if (tm.cem == CEM_LDR_RGB_DIRECT || tm.cem == CEM_LDR_RGBA_DIRECT)
                        && dec.decode_bit(&mut is_base_ofs_model) != 0
                    {
                        actual_cem = if actual_cem == CEM_LDR_RGB_DIRECT {
                            CEM_LDR_RGB_BASE_PLUS_OFFSET
                        } else {
                            CEM_LDR_RGBA_BASE_PLUS_OFFSET
                        };
                    }

                    if tm.num_parts > 1 {
                        let total_unique = total_unique_patterns(bsi, tm.num_parts);
                        let part_ctx = (left.map_or(1, |s| s.used_part_hash as u32))
                            | (upper.map_or(2, |s| (s.used_part_hash as u32) << 1));
                        let hash: &mut [i32; PART_HASH_SIZE] = if tm.num_parts == 2 {
                            &mut part2_hash
                        } else {
                            &mut part3_hash
                        };
                        let use_hash =
                            dec.decode_bit(&mut use_part_hash_model[part_ctx as usize]) != 0;
                        let unique_pat_index = if !use_hash {
                            let idx = dec.decode_truncated_binary(total_unique);
                            hash[part_hash_index(idx)] = idx as i32;
                            new_state.used_part_hash = false;
                            idx
                        } else {
                            let slot = dec.decode_sym(if tm.num_parts == 2 {
                                &mut part2_hash_index_model
                            } else {
                                &mut part3_hash_index_model
                            });
                            let v = hash[slot as usize];
                            if v < 0 {
                                return None;
                            }
                            new_state.used_part_hash = true;
                            v as u32
                        };
                        if unique_pat_index >= total_unique {
                            return None;
                        }
                        log.partition_id =
                            unique_pat_index_to_part_seed(bsi, tm.num_parts, unique_pat_index)
                                as u32;
                    } else {
                        new_state.used_part_hash = true; // bias to true
                    }
                }

                if tm_index as usize >= bsm.modes.len() {
                    return None;
                }
                let tm = &bsm.modes[tm_index as usize];
                let bc_supported = cem_supports_bc(actual_cem);
                let total_endpoint_vals = num_cem_values(actual_cem);
                fill_config_from_tm(&mut log, tm, actual_cem);

                // Decode endpoints.
                let used_dpcm = dec.decode_bit(&mut use_dpcm_endpoints_model) != 0;
                if !used_dpcm {
                    let raw_model = &mut raw_endpoint_models[(log.endpoint_ise_range - 4) as usize];
                    for part in 0..tm.num_parts as usize {
                        for val in 0..total_endpoint_vals {
                            log.endpoints[part * total_endpoint_vals + val] =
                                dec.decode_sym(raw_model) as u8;
                        }
                    }
                } else {
                    let num_levels = ise_levels(log.endpoint_ise_range) as i32;
                    let r = (log.endpoint_ise_range - 4) as usize;
                    let qt = quant_tables();

                    let reuse_delta_index = dec.decode_sym(&mut endpoint_reuse_delta_model);

                    let bc_ctx = (left.map_or(1, |s| s.first_endpoint_uses_bc as u32))
                        | (upper.map_or(2, |s| (s.first_endpoint_uses_bc as u32) << 1));
                    let mut endpoints_use_bc = [false; 4];
                    if bc_supported {
                        for e in endpoints_use_bc
                            .iter_mut()
                            .take(log.num_partitions as usize)
                        {
                            *e = dec.decode_bit(&mut endpoints_use_bc_models[bc_ctx as usize]) != 0;
                        }
                    }
                    let predicted = predict_endpoints(
                        &log,
                        &ring,
                        bx,
                        by,
                        num_blocks_x,
                        num_blocks_y,
                        reuse_delta_index,
                        &endpoints_use_bc,
                    )?;

                    let dpcm_model =
                        &mut dpcm_endpoint_models[(log.endpoint_ise_range - 4) as usize];
                    for part in 0..tm.num_parts as usize {
                        for val in 0..total_endpoint_vals {
                            let delta = dec.decode_sym(dpcm_model) as u8 as i32;
                            let e_rank = (delta
                                + qt.endpoint_ise_to_rank[r][predicted[part][val] as usize] as i32)
                                % num_levels;
                            log.endpoints[part * total_endpoint_vals + val] =
                                qt.endpoint_rank_to_ise[r][e_rank as usize];
                        }
                    }
                }

                if bc_supported {
                    new_state.first_endpoint_uses_bc =
                        used_blue_contraction(actual_cem, &log.endpoints, log.endpoint_ise_range);
                }
            }

            // Decode weights.
            if tm_index as usize >= bsm.modes.len() {
                return None;
            }
            let tm = &bsm.modes[tm_index as usize];
            let total_planes = if tm.ccs_index >= 0 { 2u32 } else { 1 };
            let total_weights = tm.grid_width * tm.grid_height;

            let mut block_used_dct = false;
            if use_dct {
                let dct_ctx = (left.map_or(1, |s| s.used_weight_dct as u32))
                    | (upper.map_or(2, |s| (s.used_weight_dct as u32) << 1));
                block_used_dct = dec.decode_bit(&mut use_dct_model[dct_ctx as usize]) != 0;
            }

            if block_used_dct {
                new_state.used_weight_dct = true;
                let num_dc_levels = num_weight_dc_levels(log.weight_ise_range);
                for plane in 0..total_planes {
                    syms.coeffs.clear();
                    if let Some(sc) = side.as_mut() {
                        syms.dc_sym = if num_dc_levels == DCT_MEAN_LEVELS1 {
                            sc.mean1.get_bits8()
                        } else {
                            sc.mean0.get_bits4()
                        };
                    } else {
                        syms.dc_sym = dec.decode_sym(
                            &mut weight_mean_models[usize::from(num_dc_levels == DCT_MEAN_LEVELS1)],
                        );
                    }
                    let mut cur_zig_ofs = 1u32;
                    while cur_zig_ofs < total_weights {
                        let run = if let Some(sc) = side.as_mut() {
                            sc.run.get_bits8()
                        } else {
                            dec.decode_sym(&mut dct_run_len_model)
                        };
                        if run == DCT_RUN_LEN_EOB_SYM_INDEX {
                            break;
                        }
                        cur_zig_ofs += run;
                        if cur_zig_ofs >= total_weights {
                            return None;
                        }
                        let (sign, mut coeff) = if let Some(sc) = side.as_mut() {
                            (sc.sign.get_bits1(), sc.coeff.get_bits8() as i32 + 1)
                        } else {
                            (dec.get_bit(), dec.decode_sym(&mut dct_coeff_mag) as i32 + 1)
                        };
                        if sign != 0 {
                            coeff = -coeff;
                        }
                        syms.coeffs.push(DctCoeff {
                            num_zeros: run as u16,
                            coeff: coeff as i16,
                        });
                        cur_zig_ofs += 1;
                    }
                    decode_block_weights(dct_q, plane, block_width, block_height, &mut log, &syms)?;
                }
            } else {
                let num_weight_levels = ise_levels(log.weight_ise_range);
                let wr = &quant_tables().weight_rank_to_ise[log.weight_ise_range as usize];
                for plane in 0..total_planes as usize {
                    let mut prev_w = num_weight_levels / 2;
                    for wi in 0..total_weights as usize {
                        let r = if let Some(sc) = side.as_mut() {
                            if num_weight_levels <= 4 {
                                sc.w2.get_bits2()
                            } else if num_weight_levels <= 8 {
                                sc.w3.get_bits4()
                            } else if num_weight_levels <= 16 {
                                sc.w4.get_bits4()
                            } else {
                                sc.w8.get_bits8()
                            }
                        } else {
                            dec.decode_sym(&mut raw_weight_models[log.weight_ise_range as usize])
                        };
                        let w = (prev_w + r) % num_weight_levels;
                        prev_w = w;
                        log.weights[plane + wi * total_planes as usize] = wr[w as usize];
                    }
                }
            }

            ring.set(bx, by, &log);
            if !block_cb(bx, by, &log) {
                return None;
            }
            states[state_idx] = new_state;
        }
    }

    if dec.get_bits(FINAL_SYNC_MARKER_BITS) != FINAL_SYNC_MARKER {
        return None;
    }

    Some(info)
}

/// Syntax dispatch by the leading byte. The init callback sees the validated
/// header (dims, block size, alpha, sRGB) and may abort; the block callback
/// receives every logical block in raster order.
pub fn decompress_image(
    comp: &[u8],
    init_cb: &mut dyn FnMut(&XuastcInfo) -> bool,
    block_cb: &mut dyn FnMut(u32, u32, &LogAstcBlock) -> bool,
) -> Option<XuastcInfo> {
    if comp.is_empty() {
        return None;
    }
    match comp[0] {
        2 => decompress_full_zstd(comp, init_cb, block_cb), // full-zstd syntax
        0 => {
            // full-arith syntax: everything after the syntax byte is arith data.
            if comp.len() < 1 + super::arith::ARITH_MIN_EXPECTED_DATA_BUF_SIZE {
                return None;
            }
            decompress_arith(&comp[1..], None, init_cb, block_cb)
        }
        1 => {
            // hybrid syntax: a 45-byte header, the arith buffer, then the DCT
            // and weight side channels (sign bits raw), each zstd-compressed.
            const HDR_SIZE: usize = 45;
            if comp.len() < HDR_SIZE {
                return None;
            }
            let f = |i: usize| rd32(comp, 1 + i * 4) as usize;
            let arith_len = f(0);
            let (mean0_len, mean1_len, run_len, coeff_len, sign_len) =
                (f(1), f(2), f(3), f(4), f(5));
            let (w2_len, w3_len, w4_len, w8_len) = (f(6), f(7), f(8), f(9));
            if arith_len < super::arith::ARITH_MIN_EXPECTED_DATA_BUF_SIZE {
                return None;
            }
            let total: u64 = [
                arith_len, mean0_len, mean1_len, run_len, coeff_len, sign_len, w2_len, w3_len,
                w4_len, w8_len,
            ]
            .iter()
            .map(|&v| v as u64)
            .sum();
            if (HDR_SIZE as u64 + total) > comp.len() as u64 {
                return None;
            }
            let mut cur = HDR_SIZE;
            macro_rules! take {
                ($len:expr) => {{
                    let start = cur;
                    #[allow(unused_assignments)]
                    {
                        cur += $len;
                    }
                    &comp[start..start + $len]
                }};
            }
            let arith_buf = take!(arith_len);
            let mean0 = zstd_channel(take!(mean0_len))?;
            let mean1 = zstd_channel(take!(mean1_len))?;
            let run = zstd_channel(take!(run_len))?;
            let coeff = zstd_channel(take!(coeff_len))?;
            let sign_slice = take!(sign_len);
            let w2 = zstd_channel(take!(w2_len))?;
            let w3 = zstd_channel(take!(w3_len))?;
            let w4 = zstd_channel(take!(w4_len))?;
            let w8 = zstd_channel(take!(w8_len))?;
            let side = SideChannels {
                mean0: SimplifiedDecoder::new(&mean0),
                mean1: SimplifiedDecoder::new(&mean1),
                run: SimplifiedDecoder::new(&run),
                coeff: SimplifiedDecoder::new(&coeff),
                sign: SimplifiedDecoder::new(sign_slice),
                w2: SimplifiedDecoder::new(&w2),
                w3: SimplifiedDecoder::new(&w3),
                w4: SimplifiedDecoder::new(&w4),
                w8: SimplifiedDecoder::new(&w8),
            };
            decompress_arith(arith_buf, Some(side), init_cb, block_cb)
        }
        _ => None,
    }
}
