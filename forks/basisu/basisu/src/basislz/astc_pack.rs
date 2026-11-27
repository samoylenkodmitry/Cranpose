//! Fixed-layout ASTC block packers for the ETC1S to ASTC path. Each supported
//! CEM mode packs an `AstcBlockParams` into a 16-byte ASTC block, reusing the
//! shared ASTC bit/trit writers from the UASTC ASTC packer. The headers are
//! precomputed byte patterns; only the endpoint and weight payloads vary per
//! block.

use crate::uastc::astc_pack::{encode_trits, set_bits, REV2};

/// Endpoint and weight staging for the packers.
#[derive(Clone, Copy, Default)]
pub struct AstcBlockParams {
    /// endpoint values as low/high pairs per component, at most 8 slots used.
    /// Sized 10 so the trit packer can consume two whole 5-value bundles; the
    /// unused trailing slots stay zero.
    pub endpoints: [u8; 10],
    /// up to 32 weights: color/alpha interleaved per texel for the dual-plane
    /// CEMs, 16 single-plane values otherwise.
    pub weights: [u8; 32],
}

/// CEM 12 (LDR RGBA direct), endpoint range 13 (trits, 0..=47), 2-bit weights.
pub fn pack_cem_12_weight_range2(blk: &AstcBlockParams) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0] = 0x42;
    out[1] = 0x84;
    out[2] = 0x01;
    out[3] = 0x00;
    out[4] = 0x00;
    out[5] = 0x00;
    out[6] = 0x00;
    out[7] = 0xc0;
    // endpoint data starts at bit 17: 11 bits of block mode, 2 bits partition
    // count, 4 bits CEM (the same offset applies in the packers below).
    let mut bit_pos = 17i32;
    let e0: [u8; 5] = blk.endpoints[0..5].try_into().unwrap();
    let e1: [u8; 5] = blk.endpoints[5..10].try_into().unwrap();
    encode_trits(&mut out, &e0, &mut bit_pos, 4);
    encode_trits(&mut out, &e1, &mut bit_pos, 4);
    // ASTC weight bits fill the block from the top down in reversed bit
    // order; REV2 reverses each 2-bit weight while ofs walks down from bit 126.
    for i in 0..32usize {
        let ofs = (126 - i * 2) as u32;
        out[(ofs >> 3) as usize] |= REV2[blk.weights[i] as usize] << (ofs & 7);
    }
    out
}

/// CEM 12, 8-bit endpoints, 1-bit weights (block truncation coding).
pub fn pack_cem_12_weight_range0(blk: &AstcBlockParams) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0] = 0x41;
    out[1] = 0x84;
    out[2] = 0x01;
    out[11] = 0xc0;
    let mut bit_pos = 17i32;
    for i in 0..8usize {
        set_bits(&mut out, &mut bit_pos, blk.endpoints[i] as u32, 8);
    }
    // 1-bit weights are their own bit reversal, so no REV table here.
    for i in 0..32usize {
        let ofs = (127 - i) as u32;
        out[(ofs >> 3) as usize] |= blk.weights[i] << (ofs & 7);
    }
    out
}

/// CEM 4 (LDR Luminance+Alpha direct), 8-bit endpoints, 2-bit weights.
pub fn pack_cem_4_weight_range2(blk: &AstcBlockParams) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0] = 0x42;
    out[1] = 0x84;
    out[2] = 0x00;
    out[3] = 0x00;
    out[4] = 0x00;
    out[5] = 0x00;
    out[6] = 0x00;
    out[7] = 0xc0;
    let mut bit_pos = 17i32;
    for i in 0..4usize {
        set_bits(&mut out, &mut bit_pos, blk.endpoints[i] as u32, 8);
    }
    for i in 0..32usize {
        let ofs = (126 - i * 2) as u32;
        out[(ofs >> 3) as usize] |= REV2[blk.weights[i] as usize] << (ofs & 7);
    }
    out
}

/// CEM 8 (LDR RGB direct), 8-bit endpoints, 2-bit weights (16 weights).
pub fn pack_cem_8_weight_range2(blk: &AstcBlockParams) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0] = 0x42;
    out[1] = 0x00;
    out[2] = 0x01;
    let mut bit_pos = 17i32;
    for i in 0..6usize {
        set_bits(&mut out, &mut bit_pos, blk.endpoints[i] as u32, 8);
    }
    for i in 0..16usize {
        let ofs = (126 - i * 2) as u32;
        out[(ofs >> 3) as usize] |= REV2[blk.weights[i] as usize] << (ofs & 7);
    }
    out
}
