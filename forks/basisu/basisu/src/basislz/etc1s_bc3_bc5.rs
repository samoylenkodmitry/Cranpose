//! ETC1S to BC3 (DXT5) and BC5 (3Dc) at the image-level: each combines two
//! per-slice converts already used elsewhere in the ETC1S path. Both targets are
//! 16 bytes per block.
//!
//! - BC3 is the BC4 alpha block (from the alpha slice) in bytes 0..8 followed by
//!   the BC1 color block (from the rgb slice, with 3-color blocks forbidden) in
//!   bytes 8..16. When there is no alpha slice the first half is a fully opaque
//!   BC4 block.
//! - BC5 is the BC4 block from the rgb slice in bytes 0..8 and the BC4 block from
//!   the alpha slice in bytes 8..16. When there is no alpha slice the second half
//!   is a fully opaque BC4 block.

use super::etc1s::Etc1sTranscoder;
use super::etc1s_bc1::convert_etc1s_to_dxt1;
use super::etc1s_bc4::convert_etc1s_to_dxt5a;
use alloc::vec::Vec;

/// A fully opaque BC4 block: both endpoints 255 and all selectors 0, so every
/// texel decodes to 255. Used as the alpha half of BC3 or the second half of BC5
/// when the source has no alpha slice.
const BC4_OPAQUE_BLOCK: [u8; 8] = [255, 255, 0, 0, 0, 0, 0, 0];

impl Etc1sTranscoder {
    /// ETC1S -> BC3 (DXT5 RGBA), opaque (no alpha slice): BC4 opaque alpha in
    /// 0..8, BC1 color (3-color forbidden) in 8..16, written into `out`
    /// (exactly `num_blocks_x*num_blocks_y*16` bytes).
    pub fn transcode_slice_bc3_opaque(
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
        for (b, &(rei, rsi)) in rgb.iter().enumerate() {
            out[b * 16..b * 16 + 8].copy_from_slice(&BC4_OPAQUE_BLOCK);
            // BC3 color: forbid 3-color blocks => use_threecolor_blocks = false.
            let color = convert_etc1s_to_dxt1(
                &self.endpoints[rei as usize],
                &self.selectors[rsi as usize],
                false,
            );
            out[b * 16 + 8..b * 16 + 16].copy_from_slice(&color);
        }
        Some(())
    }

    /// ETC1S -> BC3 (DXT5 RGBA), with alpha: BC4(alpha slice) in 0..8, BC1
    /// color (3-color forbidden) in 8..16, written into `out` (exactly
    /// `num_blocks_x*num_blocks_y*16` bytes). `video`, if present, is the
    /// previous frame's (color, alpha) index slots for ETC1S video.
    pub fn transcode_image_bc3_rgba(
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
            let a = convert_etc1s_to_dxt5a(
                &self.endpoints[aei as usize],
                &self.selectors[asi as usize],
            );
            out[b * 16..b * 16 + 8].copy_from_slice(&a);
            let color = convert_etc1s_to_dxt1(
                &self.endpoints[rei as usize],
                &self.selectors[rsi as usize],
                false,
            );
            out[b * 16 + 8..b * 16 + 16].copy_from_slice(&color);
        }
        Some(())
    }

    /// ETC1S -> BC5 (RG), opaque (no alpha slice): BC4(rgb slice) in 0..8, BC4
    /// opaque block in 8..16, written into `out` (exactly
    /// `num_blocks_x*num_blocks_y*16` bytes).
    pub fn transcode_slice_bc5_opaque(
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
        for (b, &(rei, rsi)) in rgb.iter().enumerate() {
            let r = convert_etc1s_to_dxt5a(
                &self.endpoints[rei as usize],
                &self.selectors[rsi as usize],
            );
            out[b * 16..b * 16 + 8].copy_from_slice(&r);
            out[b * 16 + 8..b * 16 + 16].copy_from_slice(&BC4_OPAQUE_BLOCK);
        }
        Some(())
    }

    /// ETC1S -> BC5 (RG), with alpha: BC4(rgb slice) in 0..8, BC4(alpha slice)
    /// in 8..16, written into `out` (exactly `num_blocks_x*num_blocks_y*16`
    /// bytes). `video`, if present, is the previous frame's (color, alpha)
    /// index slots for ETC1S video.
    pub fn transcode_image_bc5_rg(
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
            let r = convert_etc1s_to_dxt5a(
                &self.endpoints[rei as usize],
                &self.selectors[rsi as usize],
            );
            out[b * 16..b * 16 + 8].copy_from_slice(&r);
            let g = convert_etc1s_to_dxt5a(
                &self.endpoints[aei as usize],
                &self.selectors[asi as usize],
            );
            out[b * 16 + 8..b * 16 + 16].copy_from_slice(&g);
        }
        Some(())
    }
}
