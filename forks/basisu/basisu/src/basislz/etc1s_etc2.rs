//! `convert_etc1s_to_etc2_eac_a8` and the ETC1S to ETC2 EAC-A8 slice emit. Maps
//! a decoded ETC1S endpoint and selector to an 8-byte EAC alpha block. The
//! color half of an ETC2_RGBA block is the plain ETC1 block produced by the
//! ETC1S to ETC1 path.

use super::etc1s::{convert_etc1s_to_etc1, Endpoint, Etc1sTranscoder, Selector};
use crate::etc::block::DecoderEtcBlock;
use crate::etc::tables::ETC2_EAC_A8_SEL4;
use crate::tables::etc2_eac_a8::S_ETC1_G_TO_ETC2_A8;
use alloc::vec::Vec;

/// The `{low, high}` selector ranges (4 entries).
static EAC_SELECTOR_RANGES: [[u32; 2]; 4] = [[0, 3], [1, 3], [0, 2], [1, 2]];

/// Convert an ETC1S grayscale (red) block to an 8-byte EAC alpha block.
pub fn convert_etc1s_to_etc2_eac_a8(ep: &Endpoint, sel: &Selector) -> [u8; 8] {
    let low_selector = sel.lo_selector as u32;
    let high_selector = sel.hi_selector as u32;
    let inten_table = ep.inten5 as u32;
    let mut out = [0u8; 8];

    if low_selector == high_selector {
        // Constant alpha block: table 13, multiplier 1, base = red, all-4 selectors.
        let r = DecoderEtcBlock::get_block_color5_r(ep.color5, inten_table, low_selector as usize);
        out[0] = r as u8;
        out[1] = 13 | (1 << 4); // m_table = 13, m_multiplier = 1
        out[2..8].copy_from_slice(&ETC2_EAC_A8_SEL4);
        return out;
    }

    let mut srt = 0usize;
    for (i, r) in EAC_SELECTOR_RANGES.iter().enumerate() {
        if low_selector == r[0] && high_selector == r[1] {
            srt = i;
            break;
        }
    }

    let e = &S_ETC1_G_TO_ETC2_A8[ep.color5.r() as usize + inten_table as usize * 32][srt];
    out[0] = e.m_base;
    // byte1 = m_table | (m_multiplier << 4); m_table = m_table_mul>>4, m_mul = m_table_mul&15.
    out[1] = (e.m_table_mul >> 4) | ((e.m_table_mul & 15) << 4);

    let mut selector_bits: u64 = 0;
    for y in 0..4u32 {
        for x in 0..4u32 {
            let s = sel.get_selector(x, y);
            let ds = ((e.m_trans >> (s * 3)) & 7) as u64;
            let dst_ofs = 45 - (y + x * 4) * 3;
            selector_bits |= ds << dst_ofs;
        }
    }
    out[2] = (selector_bits >> 40) as u8;
    out[3] = (selector_bits >> 32) as u8;
    out[4] = (selector_bits >> 24) as u8;
    out[5] = (selector_bits >> 16) as u8;
    out[6] = (selector_bits >> 8) as u8;
    out[7] = selector_bits as u8;
    out
}

impl Etc1sTranscoder {
    /// Transcode a slice to ETC2 EAC-A8: 8-byte EAC alpha
    /// blocks (treating the slice's red channel as alpha), written into `out`
    /// (exactly `num_blocks_x*num_blocks_y*8` bytes). `None` on a corrupt
    /// stream.
    pub fn transcode_slice_eac_a8(
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
            let block = convert_etc1s_to_etc2_eac_a8(
                &self.endpoints[ei as usize],
                &self.selectors[si as usize],
            );
            out[b * 8..b * 8 + 8].copy_from_slice(&block);
        }
        Some(())
    }

    /// ETC1S -> ETC2_RGBA with no alpha slice: a constant opaque EAC-A8 block
    /// (base 255, table 13, multiplier 1, all-4 selectors) + the ETC1 color
    /// block, written into `out` (exactly `num_blocks_x*num_blocks_y*16`
    /// bytes).
    pub fn transcode_slice_etc2_rgba_opaque(
        &self,
        rgb_slice: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        const OPAQUE_EAC_A8: [u8; 8] = [255, 29, 0x92, 0x49, 0x24, 0x92, 0x49, 0x24];
        let rgb = self.decode_slice_indices(rgb_slice, num_blocks_x, num_blocks_y, video)?;
        if out.len() != rgb.len() * 16 {
            return None;
        }
        out.fill(0);
        for (b, &(ei, si)) in rgb.iter().enumerate() {
            out[b * 16..b * 16 + 8].copy_from_slice(&OPAQUE_EAC_A8);
            let color =
                convert_etc1s_to_etc1(&self.endpoints[ei as usize], &self.selectors[si as usize]);
            out[b * 16 + 8..b * 16 + 16].copy_from_slice(&color);
        }
        Some(())
    }

    /// Combined ETC1S -> ETC2_RGBA: 16-byte blocks of EAC-A8 alpha (from the
    /// alpha slice, bytes 0-7) + ETC1 color (from the RGB slice, bytes 8-15),
    /// written into `out` (exactly `num_blocks_x*num_blocks_y*16` bytes).
    /// `video`, if present, is the previous frame's (color, alpha) index slots
    /// for ETC1S video.
    pub fn transcode_image_etc2_rgba(
        &self,
        rgb_slice: &[u8],
        alpha_slice: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        video: Option<(&mut Vec<u32>, &mut Vec<u32>)>,
        out: &mut [u8],
    ) -> Option<()> {
        let (vc, va) = match video {
            Some((c, a)) => (Some(c), Some(a)),
            None => (None, None),
        };
        let rgb = self.decode_slice_indices(rgb_slice, num_blocks_x, num_blocks_y, vc)?;
        let alpha = self.decode_slice_indices(alpha_slice, num_blocks_x, num_blocks_y, va)?;
        if out.len() != rgb.len() * 16 {
            return None;
        }
        out.fill(0);
        for (b, (&(rei, rsi), &(aei, asi))) in rgb.iter().zip(alpha.iter()).enumerate() {
            let eac = convert_etc1s_to_etc2_eac_a8(
                &self.endpoints[aei as usize],
                &self.selectors[asi as usize],
            );
            let color =
                convert_etc1s_to_etc1(&self.endpoints[rei as usize], &self.selectors[rsi as usize]);
            out[b * 16..b * 16 + 8].copy_from_slice(&eac);
            out[b * 16 + 8..b * 16 + 16].copy_from_slice(&color);
        }
        Some(())
    }
}
