//! ETC1S -> EAC R11 / RG11 transcode: map a decoded ETC1S endpoint+selector
//! pair to an 8-byte EAC R11 block (R only). RG11 output composes two R11
//! blocks per 4x4: R in bytes 0..8, and G in bytes 8..16 (transcoded from the
//! alpha slice, or a constant block when there is none).

use super::etc1s::{Endpoint, Etc1sTranscoder, Selector};
use crate::etc::block::DecoderEtcBlock;
use crate::etc::tables::ETC2_EAC_A8_SEL4;
use crate::tables::etc2_eac_r11::S_ETC1_G_TO_ETC2_R11;
use alloc::vec::Vec;

/// The `(low, high)` ETC1S selector ranges the R11 conversion table is split
/// by; the block's actual selector extremes pick which entry of a table row
/// applies.
static EAC_SELECTOR_RANGES: [[u32; 2]; 4] = [[0, 3], [1, 3], [0, 2], [1, 2]];

/// A constant EAC R11 block: base 255, modifier table 13, multiplier 1, all-4
/// selectors. Serves as the G plane when there is no alpha slice.
const OPAQUE_EAC_R11: [u8; 8] = [255, 29, 0x92, 0x49, 0x24, 0x92, 0x49, 0x24];

/// Convert one ETC1S endpoint/selector pair to an EAC R11 block, treating the
/// endpoint's red channel as the single-channel value.
pub fn convert_etc1s_to_etc2_eac_r11(ep: &Endpoint, sel: &Selector) -> [u8; 8] {
    let low_selector = sel.lo_selector as u32;
    let high_selector = sel.hi_selector as u32;
    let inten_table = ep.inten5 as u32;
    let mut out = [0u8; 8];

    if low_selector == high_selector {
        // flat block: table 13's selector-4 modifier is 0, so with base = the
        // block's decoded red value every texel reproduces base exactly.
        let r = DecoderEtcBlock::get_block_color5_r(ep.color5, inten_table, low_selector as usize);
        out[0] = r as u8;
        out[1] = 13 | (1 << 4); // table 13, multiplier 1
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

    let e = &S_ETC1_G_TO_ETC2_R11[ep.color5.r() as usize + inten_table as usize * 32][srt];
    out[0] = e.m_base;
    // the table entry packs table<<4 | multiplier, but EAC byte 1 wants
    // multiplier<<4 | table, so swap the nibbles.
    out[1] = (e.m_table_mul >> 4) | ((e.m_table_mul & 15) << 4);

    // m_trans maps each 2-bit ETC1S selector to a 3-bit EAC selector; EAC
    // stores them column-major, MSB first, in the 48-bit tail.
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
    /// Transcode one ETC1S slice to EAC R11: one 8-byte block per 4x4,
    /// treating the slice's red channel as the R plane, written into `out`
    /// (exactly `num_blocks_x*num_blocks_y*8` bytes). `None` on a corrupt
    /// stream.
    pub fn transcode_slice_eac_r11(
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
            let block = convert_etc1s_to_etc2_eac_r11(
                &self.endpoints[ei as usize],
                &self.selectors[si as usize],
            );
            out[b * 8..b * 8 + 8].copy_from_slice(&block);
        }
        Some(())
    }

    /// ETC1S -> EAC RG11 when there is no alpha slice: R11 of the RGB slice's
    /// red channel in bytes 0..8, and the constant `OPAQUE_EAC_R11` block as
    /// the G plane in bytes 8..16, written into `out` (exactly
    /// `num_blocks_x*num_blocks_y*16` bytes).
    pub fn transcode_slice_eac_rg11_opaque(
        &self,
        rgb_slice: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let rgb = self.decode_slice_indices(rgb_slice, num_blocks_x, num_blocks_y, video)?;
        if out.len() != rgb.len() * 16 {
            return None;
        }
        out.fill(0);
        for (b, &(ei, si)) in rgb.iter().enumerate() {
            let r = convert_etc1s_to_etc2_eac_r11(
                &self.endpoints[ei as usize],
                &self.selectors[si as usize],
            );
            out[b * 16..b * 16 + 8].copy_from_slice(&r);
            out[b * 16 + 8..b * 16 + 16].copy_from_slice(&OPAQUE_EAC_R11);
        }
        Some(())
    }

    /// Combined ETC1S -> EAC RG11: 16-byte blocks of R11(R) (from the RGB slice,
    /// bytes 0..8) + R11(alpha) as the G plane (from the alpha slice, bytes
    /// 8..16), written into `out` (exactly `num_blocks_x*num_blocks_y*16`
    /// bytes). `video`, if present, is the previous frame's (color, alpha)
    /// index slots for ETC1S video.
    pub fn transcode_image_eac_rg11(
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
            let r = convert_etc1s_to_etc2_eac_r11(
                &self.endpoints[rei as usize],
                &self.selectors[rsi as usize],
            );
            let g = convert_etc1s_to_etc2_eac_r11(
                &self.endpoints[aei as usize],
                &self.selectors[asi as usize],
            );
            out[b * 16..b * 16 + 8].copy_from_slice(&r);
            out[b * 16 + 8..b * 16 + 16].copy_from_slice(&g);
        }
        Some(())
    }
}
