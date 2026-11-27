//! XUASTC LDR per-target transcode: decompress the stream once, then route by
//! target. The ASTC pass-through emits the decompressed logical blocks
//! directly; the generic targets re-pack those blocks and run them through
//! the same raw-ASTC LDR slice machinery (the shared `transcode_4x4_block`,
//! row reblocker, deblock filter, and PVRTC1 encoder), so a decompressed
//! block transcodes identically whether it arrived here or through the raw
//! ASTC path. BC7 at default flags takes the dedicated fast tile arms for the
//! 4x4, 8x6, and 6x6 block sizes.

use super::decode::{decompress_image, XuastcInfo};
use crate::api::AstcBlock;
use crate::astc::pack::pack_astc_block;
use crate::astc::slice::{transcode_ldr, Target4x4};
use alloc::vec;
use alloc::vec::Vec;

// `basisd_decode_flags` bits this layer routes on.
const FLAG_HIGH_QUALITY: u32 = 32;
const FLAG_NO_DEBLOCK: u32 = 128;
const FLAG_FORCE_DEBLOCK: u32 = 512;
/// Disables the fast BC7 tile arms, forcing the generic per-pixel encoder.
pub const FLAG_DISABLE_FAST_BC7: u32 = 1024;

/// Decompress a stream into packed physical blocks + header info, validating
/// the container's expectations (block size and block counts).
fn decompress_to_blocks(
    stream: &[u8],
    block: AstcBlock,
    bx: u32,
    by: u32,
) -> Option<(Vec<u8>, bool, bool)> {
    let (sbw, sbh) = block.dims();
    use core::cell::{Cell, RefCell};
    let blocks: RefCell<Vec<u8>> = RefCell::new(Vec::new());
    let ok_dims = Cell::new(false);
    let srgb = Cell::new(false);
    let has_alpha = Cell::new(false);
    let mut init_cb = |info: &XuastcInfo| -> bool {
        if info.block_width != sbw || info.block_height != sbh {
            return false;
        }
        let nbx = info.width.div_ceil(info.block_width);
        let nby = info.height.div_ceil(info.block_height);
        if nbx != bx || nby != by {
            return false;
        }
        ok_dims.set(true);
        srgb.set(info.srgb);
        has_alpha.set(info.has_alpha);
        *blocks.borrow_mut() = vec![0u8; (nbx * nby) as usize * 16];
        true
    };
    let mut block_cb = |cbx: u32, cby: u32, log: &crate::astc::unpack::LogAstcBlock| -> bool {
        match pack_astc_block(log) {
            Some(phys) => {
                let o = ((cby * bx + cbx) as usize) * 16;
                blocks.borrow_mut()[o..o + 16].copy_from_slice(&phys);
                true
            }
            None => false,
        }
    };
    decompress_image(stream, &mut init_cb, &mut block_cb)?;
    if !ok_dims.get() {
        return None;
    }
    Some((blocks.into_inner(), srgb.get(), has_alpha.get()))
}

#[allow(clippy::too_many_arguments)]
/// Transcode one XUASTC LDR image to `fmt` (a `transcoder_texture_format`
/// id, as in `dispatch::target`), writing the output into `out` (which must
/// be exactly the output length). `None` for unsupported targets or an
/// invalid stream.
pub fn transcode_xuastc(
    stream: &[u8],
    block: AstcBlock,
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    fmt: i32,
    flags: u32,
    out: &mut [u8],
) -> Option<()> {
    use crate::dispatch::target;

    let (sbw, sbh) = block.dims();
    let (blocks, srgb, has_alpha) = decompress_to_blocks(stream, block, bx, by)?;

    // ASTC pass-through: the decompressed physical blocks are the output.
    if fmt == block.passthrough_target().as_i32() {
        if out.len() != blocks.len() {
            return None;
        }
        out.copy_from_slice(&blocks);
        return Some(());
    }

    let high_quality = flags & FLAG_HIGH_QUALITY != 0;
    let enable_fast_bc7 = flags & FLAG_DISABLE_FAST_BC7 == 0;
    let disable_deblocking = flags & FLAG_NO_DEBLOCK != 0;
    let force_deblocking = flags & FLAG_FORCE_DEBLOCK != 0;
    let deblock_filtering = !disable_deblocking && (force_deblocking || sbw > 8 || sbh > 6);

    if fmt == target::BC7_RGBA
        && enable_fast_bc7
        && !high_quality
        && !deblock_filtering
        && matches!((sbw, sbh), (4, 4) | (8, 6) | (6, 6))
    {
        // The dedicated BC7 fast tile arms.
        return super::bc7_fast::transcode_bc7_fast(
            &blocks, sbw, sbh, bx, by, lw, lh, srgb, has_alpha, out,
        );
    }

    // Generic path: identical to the raw-ASTC slice machinery, with the
    // stream header's sRGB/alpha bits standing in for the container's. The
    // only divergence from the raw path is BC7's explicit no-alpha entry.
    let t4 = match fmt {
        target::RGBA32 => Target4x4::Rgba32,
        target::RGB565 => Target4x4::Rgb565,
        target::BGR565 => Target4x4::Bgr565,
        target::RGBA4444 => Target4x4::Rgba4444,
        target::BC1_RGB => Target4x4::Bc1,
        target::BC3_RGBA => Target4x4::Bc3,
        target::BC4_R => Target4x4::Bc4,
        target::BC5_RG => Target4x4::Bc5,
        target::EAC_R11 => Target4x4::EacR11,
        target::EAC_RG11 => Target4x4::EacRg11,
        target::ETC1_RGB => Target4x4::Etc1,
        target::ETC2_RGBA => Target4x4::Etc2Rgba,
        target::PVRTC1_4_RGB => Target4x4::Pvrtc1Rgb,
        target::PVRTC1_4_RGBA => Target4x4::Pvrtc1Rgba,
        target::BC7_RGBA => {
            if has_alpha {
                Target4x4::Bc7
            } else {
                Target4x4::Bc7Opaque
            }
        }
        _ => return None,
    };
    transcode_ldr(
        &blocks, sbw, sbh, bx, by, lw, lh, t4, flags, srgb, has_alpha, out,
    )
}
