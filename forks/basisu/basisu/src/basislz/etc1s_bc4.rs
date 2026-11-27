//! `convert_etc1s_to_dxt5a` and the ETC1S to BC4 (DXT5A) slice emit. Maps a
//! decoded ETC1S endpoint and selector to an 8-byte DXT5A block: two 8-bit
//! endpoints (low and high alpha) followed by sixteen 3-bit selectors. For
//! BC4-R the alpha channel carries the ETC1S endpoint red (the source is
//! grayscale). The same 8-byte block is also the alpha half of a BC3 block.

use super::etc1s::{Endpoint, Etc1sTranscoder, Selector};
use crate::etc::block::DecoderEtcBlock;
use crate::tables::dxt5a::G_ETC1_G_TO_DXT5A;
use alloc::vec::Vec;

/// The (low, high) selector ranges, in the column order of
/// `G_ETC1_G_TO_DXT5A`.
const DXT5A_SELECTOR_RANGES: [(u8, u8); 4] = [(0, 3), (1, 3), (0, 2), (1, 2)];

/// Write the sixteen 3-bit selectors (raster order, LSB-first) into the six
/// selector bytes of a `dxt5a_block` (`dst[2..8]`).
fn write_selectors(dst: &mut [u8; 8], sels: &[u8; 16]) {
    let mut v: u64 = 0;
    for (i, &s) in sels.iter().enumerate() {
        v |= (s as u64 & 7) << (i * 3);
    }
    for (k, b) in dst[2..8].iter_mut().enumerate() {
        *b = (v >> (k * 8)) as u8;
    }
}

/// Convert a decoded ETC1S endpoint+selector to an 8-byte BC4 block.
pub fn convert_etc1s_to_dxt5a(ep: &Endpoint, sel: &Selector) -> [u8; 8] {
    let low_selector = sel.lo_selector as u32;
    let high_selector = sel.hi_selector as u32;
    let base_color = ep.color5;
    let inten_table = ep.inten5 as u32;

    let mut out = [0u8; 8];

    if low_selector == high_selector {
        let r = DecoderEtcBlock::get_block_color5_r(base_color, inten_table, low_selector as usize);
        out[0] = r as u8;
        out[1] = r as u8;
        // selectors already all zero.
        return out;
    }

    if sel.num_unique_selectors == 2 {
        let block_colors = DecoderEtcBlock::get_block_colors5(base_color, inten_table);
        let r0 = block_colors[low_selector as usize].r() as u32;
        let r1 = block_colors[high_selector as usize].r() as u32;
        out[0] = r0 as u8; // low alpha
        out[1] = r1 as u8; // high alpha
        let mut sels = [0u8; 16];
        for y in 0..4u32 {
            for x in 0..4u32 {
                let s = sel.get_selector(x, y);
                sels[(y * 4 + x) as usize] = if s == high_selector { 1 } else { 0 };
            }
        }
        write_selectors(&mut out, &sels);
        return out;
    }

    // General case: table-driven endpoints + per-selector translation.
    let mut srt = 0usize;
    for (i, &(lo, hi)) in DXT5A_SELECTOR_RANGES.iter().enumerate() {
        if low_selector == lo as u32 && high_selector == hi as u32 {
            srt = i;
            break;
        }
    }
    let row = base_color.r() as usize + inten_table as usize * 32;
    let entry = &G_ETC1_G_TO_DXT5A[row * DXT5A_SELECTOR_RANGES.len() + srt];
    out[0] = entry.m_lo;
    out[1] = entry.m_hi;
    let trans = entry.m_trans as u32;
    let mut sels = [0u8; 16];
    for y in 0..4u32 {
        for x in 0..4u32 {
            let s = sel.get_selector(x, y);
            let ds = (trans >> (s * 3)) & 7;
            sels[(y * 4 + x) as usize] = ds as u8;
        }
    }
    write_selectors(&mut out, &sels);
    out
}

impl Etc1sTranscoder {
    /// Transcode a (grayscale) ETC1S color slice to 8-byte BC4 blocks, written
    /// into `out` (exactly `num_blocks_x*num_blocks_y*8` bytes). `None`
    /// on a corrupt stream.
    pub fn transcode_slice_bc4(
        &self,
        slice_data: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let indices = self.decode_slice_indices(slice_data, num_blocks_x, num_blocks_y, video)?;
        if out.len() != indices.len() * 8 {
            return None;
        }
        out.fill(0);
        for (b, &(ei, si)) in indices.iter().enumerate() {
            let block =
                convert_etc1s_to_dxt5a(&self.endpoints[ei as usize], &self.selectors[si as usize]);
            out[b * 8..b * 8 + 8].copy_from_slice(&block);
        }
        Some(())
    }
}
