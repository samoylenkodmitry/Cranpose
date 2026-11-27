//! ETC1S to FXT1_RGB slice transcode.
//!
//! FXT1 is the only target with an 8x4 block (16 bytes). Blocks are emitted in
//! the CC_MIXED encoding, which is close to DXT1: the transcode first produces
//! a DXT1 block (`convert_etc1s_to_dxt1`), then re-packs it into one half
//! (subblock) of the FXT1 block. Two ETC1S 4x4 blocks (left and right) feed
//! one FXT1 8x4 block, so `fxt1_block_x = etc_block_x >> 1` and
//! `fxt1_subblock = etc_block_x & 1`.
//!
//! The 16-byte block layout:
//! - bytes 0..8: 2-bit-per-texel selectors, one byte per 4-texel row of a 4x4
//!   half; bytes 0..4 hold the left half's rows, bytes 4..8 the right half's.
//! - bytes 8..16: a little-endian 64-bit word of LSB-first bitfields:
//!   `b0:5 g0:5 r0:5  b1:5 g1:5 r1:5  b2:5 g2:5 r2:5  b3:5 g3:5 r3:5
//!    alpha:1 glsb:2 mode:1`. Colors 0/1 are the left half's endpoint pair,
//!   colors 2/3 the right half's.

use super::etc1s::{Endpoint, Etc1sTranscoder, Selector};
use super::etc1s_bc1::convert_etc1s_to_dxt1;
use alloc::vec::Vec;

/// Re-map a packed DXT1 selector byte (four 2-bit selectors) to FXT1 CC_MIXED
/// selector values. Per texel the map is 0->0, 1->3, 2->1, 3->2, turning
/// DXT1's 0,2,3,1 wire ordering into FXT1's linear color0-to-color1 ramp; the
/// table handles two texels (one nibble) per lookup, low nibble then high.
#[inline]
fn conv_dxt1_to_fxt1_sels(sels: u32) -> u8 {
    const TBL: [u8; 16] = [0, 3, 1, 2, 12, 15, 13, 14, 4, 7, 5, 6, 8, 11, 9, 10];
    (TBL[(sels & 15) as usize] as u32 | ((TBL[(sels >> 4) as usize] as u32) << 4)) as u8
}

/// One FXT1 block under construction. The two subblocks write disjoint fields
/// (down to individual `glsb` bits), so this carries the running state from
/// subblock 0 into subblock 1.
#[derive(Clone, Copy, Default)]
pub struct Fxt1Block {
    /// the eight selector bytes (block bytes 0..8).
    sels: [u8; 8],
    /// 12 5-bit endpoint channels in field order b0,g0,r0, b1,g1,r1, ... b3,g3,r3.
    rgb: [u8; 12],
    /// alpha flag (1 bit); always 0 for CC_MIXED RGB output.
    alpha: u8,
    /// green low bits (2 bits): each half stores only color1's green LSB
    /// (bit 0 left half, bit 1 right); color0's LSB is implied, see
    /// `convert_etc1s_to_fxt1`.
    glsb: u8,
    /// block mode (1 bit); 1 selects the CC_MIXED encoding.
    mode: u8,
}

impl Fxt1Block {
    /// Serialize to 16 little-endian bytes: the selector bytes, then the
    /// packed high word.
    pub fn to_bytes(self) -> [u8; 16] {
        let mut hi: u64 = 0;
        let mut shift = 0u32;
        for &c in &self.rgb {
            hi |= ((c & 0x1F) as u64) << shift;
            shift += 5;
        }
        hi |= ((self.alpha & 1) as u64) << 60;
        hi |= ((self.glsb & 3) as u64) << 61;
        hi |= ((self.mode & 1) as u64) << 63;

        let mut out = [0u8; 16];
        out[0..8].copy_from_slice(&self.sels);
        out[8..16].copy_from_slice(&hi.to_le_bytes());
        out
    }

    /// Store one 5:5:5 color into endpoint slot `idx` (0..=3); the bitfield
    /// order within each color is b, g, r.
    #[inline]
    fn set_rgb(&mut self, idx: usize, r: u8, g: u8, b: u8) {
        self.rgb[idx * 3] = b;
        self.rgb[idx * 3 + 1] = g;
        self.rgb[idx * 3 + 2] = r;
    }
}

/// Write one subblock (0 = left half, 1 = right half) of `block` from an
/// ETC1S endpoint/selector pair. Subblock 1 preserves the `glsb` bit left by
/// subblock 0, so subblock 0 must be encoded first.
pub fn convert_etc1s_to_fxt1(
    block: &mut Fxt1Block,
    ep: &Endpoint,
    sel: &Selector,
    fxt1_subblock: u32,
) {
    // CC_MIXED is close enough to DXT1 that going through a DXT1 block first
    // is nearly lossless (FXT1 only rounds its color lerps differently).
    let blk = convert_etc1s_to_dxt1(ep, sel, false);
    let l = (blk[0] as u32) | ((blk[1] as u32) << 8); // low 565 endpoint word
    let h = (blk[2] as u32) | ((blk[3] as u32) << 8); // high 565 endpoint word

    // color0 from low, color1 from high (565 unpack; alpha unused).
    let mut c0 = [(l >> 11) & 31, (l >> 5) & 63, l & 31]; // r,g,b
    let mut c1 = [(h >> 11) & 31, (h >> 5) & 63, h & 31];

    let mut g0 = c0[1] & 1;
    let mut g1 = c1[1] & 1;
    c0[1] >>= 1;
    c1[1] >>= 1;

    let mut s = [
        conv_dxt1_to_fxt1_sels(blk[4] as u32),
        conv_dxt1_to_fxt1_sels(blk[5] as u32),
        conv_dxt1_to_fxt1_sels(blk[6] as u32),
        conv_dxt1_to_fxt1_sels(blk[7] as u32),
    ];

    // only color1's green LSB is stored (in glsb); the decoder reconstructs
    // color0's as glsb ^ (texel (0,0)'s selector MSB), so that MSB must equal
    // g0 ^ g1. When it does not, swap the endpoints and invert every selector
    // byte: with FXT1's linear ramp, ^0xFF is an exact per-texel ramp
    // reversal, so decoded colors are unchanged.
    if ((s[0] >> 1) & 1) as u32 != (g0 ^ g1) {
        core::mem::swap(&mut c0, &mut c1);
        core::mem::swap(&mut g0, &mut g1);
        for b in s.iter_mut() {
            *b ^= 0xFF;
        }
    }

    let (r0, g0c, b0) = (c0[0] as u8, c0[1] as u8, c0[2] as u8);
    let (r1, g1c, b1) = (c1[0] as u8, c1[1] as u8, c1[2] as u8);

    if fxt1_subblock == 0 {
        block.mode = 1;
        block.alpha = 0;
        block.glsb = (g1 | (g1 << 1)) as u8;
        block.set_rgb(0, r0, g0c, b0);
        block.set_rgb(1, r1, g1c, b1);
        block.set_rgb(2, r0, g0c, b0);
        block.set_rgb(3, r1, g1c, b1);
        block.sels[0] = s[0];
        block.sels[1] = s[1];
        block.sels[2] = s[2];
        block.sels[3] = s[3];

        // Default the right half to an edge clamp: colors 2/3 duplicate 0/1
        // (set_rgb above), and each right-half row repeats the rightmost
        // selector of the matching left row (bits 6..7 of s[i]). Subblock 1
        // overwrites all of this when a right-hand ETC1S block exists; it only
        // stands for images too narrow to have one. Each table entry fills a
        // byte with one repeated 2-bit value (85 = 0b01010101, etc.).
        const BORDER_DUP: [u8; 4] = [0, 85, 170, 255];
        block.sels[4] = BORDER_DUP[(s[0] >> 6) as usize];
        block.sels[5] = BORDER_DUP[(s[1] >> 6) as usize];
        block.sels[6] = BORDER_DUP[(s[2] >> 6) as usize];
        block.sels[7] = BORDER_DUP[(s[3] >> 6) as usize];
    } else {
        block.glsb = ((block.glsb & 1) as u32 | (g1 << 1)) as u8;
        block.set_rgb(2, r0, g0c, b0);
        block.set_rgb(3, r1, g1c, b1);
        block.sels[4] = s[0];
        block.sels[5] = s[1];
        block.sels[6] = s[2];
        block.sels[7] = s[3];
    }
}

impl Etc1sTranscoder {
    /// Transcode one ETC1S slice to FXT1_RGB, written into `out` (exactly
    /// `num_blocks_x*num_blocks_y*16` bytes). FXT1 has an 8x4 block, so the
    /// output grid is `ceil(width/8) x ceil(height/4)` (16 bytes per block).
    /// `num_blocks_x` and `num_blocks_y` are the ETC1S 4x4 block counts; each
    /// even/odd column pair of 4x4 blocks maps to one FXT1 block subblock.
    /// `video`, if present, is the previous frame's per-block indices for this
    /// slice (ETC1S video).
    #[allow(clippy::too_many_arguments)]
    pub fn transcode_slice_fxt1(
        &self,
        slice_data: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        orig_width: u32,
        orig_height: u32,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let indices = self.decode_slice_indices(slice_data, num_blocks_x, num_blocks_y, video)?;

        let fxt1_blocks_x = orig_width.div_ceil(8);
        let fxt1_blocks_y = orig_height.div_ceil(4);
        let mut blocks = vec![Fxt1Block::default(); (fxt1_blocks_x * fxt1_blocks_y) as usize];

        for block_y in 0..num_blocks_y {
            for block_x in 0..num_blocks_x {
                let (ei, si) = indices[(block_x + block_y * num_blocks_x) as usize];
                let fxt1_block_x = block_x >> 1;
                let fxt1_subblock = block_x & 1;
                let dst = (fxt1_block_x + block_y * fxt1_blocks_x) as usize;
                convert_etc1s_to_fxt1(
                    &mut blocks[dst],
                    &self.endpoints[ei as usize],
                    &self.selectors[si as usize],
                    fxt1_subblock,
                );
            }
        }

        // The output buffer is sized by the 4x4 block count, not the FXT1 8x4
        // count. Each FXT1 block lands at its ceil(w/8)-pitched
        // slot, so the FXT1 data is contiguous from offset 0 and the trailing
        // slots stay zero. The length and padding are part of the expected output.
        let total_4x4 = (num_blocks_x * num_blocks_y) as usize;
        if out.len() != total_4x4 * 16 {
            return None;
        }
        out.fill(0);
        for (b, blk) in blocks.iter().enumerate() {
            out[b * 16..b * 16 + 16].copy_from_slice(&blk.to_bytes());
        }
        Some(())
    }
}
