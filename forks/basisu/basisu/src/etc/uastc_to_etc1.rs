//! UASTC to ETC1 transcode: unpack a UASTC block to pixels, average them into
//! two ETC1 subblock colors (with the per-mode bias hint applied), then pick
//! per-pixel selectors by luma.

use super::block::DecoderEtcBlock;
use super::tables::{
    ETC1_COLOR_DELTA_MAX, ETC1_COLOR_DELTA_MIN, ETC1_PIXEL_COORDS, ETC1_SOLID_SELECTORS,
};
use crate::color::Color32;
use crate::uastc::tables::HAS_ETC1_BIAS;
use crate::uastc::unpack::{unpack_pixels, unpack_to_block, UnpackedUastcBlock};
use crate::uastc::UASTC_MODE_INDEX_SOLID_COLOR;

/// Nudge a subblock base color by a small per-channel delta selected by the
/// UASTC `bias` hint. `limit` is the channel's max value (15 or 31), and
/// `subblock` selects which of the two ETC1 subblocks is being biased.
fn apply_etc1_bias(block_color: Color32, bias: u32, limit: u32, subblock: u32) -> Color32 {
    // DIVS spreads the bias value across the three channels in the default arm:
    // channel c reads digit c of the base-3 expansion of `bias`.
    const DIVS: [i32; 3] = [1, 3, 9];
    let mut result = Color32::default();
    let sub = subblock != 0;
    for c in 0..3usize {
        // Most bias values map to a small per-channel delta through the general
        // base-3 formula in the default arm; the rest are explicit overrides
        // that target a single channel or both subblocks.
        let delta: i32 = match bias {
            2 => {
                if sub {
                    0
                } else if c == 0 {
                    -1
                } else {
                    0
                }
            }
            5 => {
                if sub {
                    0
                } else if c == 1 {
                    -1
                } else {
                    0
                }
            }
            6 => {
                if sub {
                    0
                } else if c == 2 {
                    -1
                } else {
                    0
                }
            }
            7 => {
                if sub {
                    0
                } else if c == 0 {
                    1
                } else {
                    0
                }
            }
            11 => {
                if sub {
                    0
                } else if c == 1 {
                    1
                } else {
                    0
                }
            }
            15 => {
                if sub {
                    0
                } else if c == 2 {
                    1
                } else {
                    0
                }
            }
            18 => {
                if sub {
                    if c == 0 {
                        -1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            19 => {
                if sub {
                    if c == 1 {
                        -1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            20 => {
                if sub {
                    if c == 2 {
                        -1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            21 => {
                if sub {
                    if c == 0 {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            24 => {
                if sub {
                    if c == 1 {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            8 => {
                if sub {
                    if c == 2 {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            10 => -2,
            27 => {
                if sub {
                    0
                } else {
                    -1
                }
            }
            28 => {
                if sub {
                    -1
                } else {
                    1
                }
            }
            29 => {
                if sub {
                    1
                } else {
                    0
                }
            }
            30 => {
                if sub {
                    -1
                } else {
                    0
                }
            }
            31 => {
                if sub {
                    0
                } else {
                    1
                }
            }
            _ => ((bias as i32 / DIVS[c]) % 3) - 1,
        };

        // Apply the delta but keep the result in range. At the endpoints 0 and
        // limit the bias is steered inward so it never clips; in the interior an
        // out-of-range result is reflected by applying the delta in reverse.
        let lim = limit as i32;
        let mut v = block_color[c] as i32;
        if v == 0 {
            if delta == -2 {
                v += 3;
            } else {
                v += delta + 1;
            }
        } else if v == lim {
            v += delta - 1;
        } else {
            v += delta;
            if v < 0 || v > lim {
                v = (v - delta) - delta;
            }
        }
        result[c] = v as u8;
    }
    result
}

/// Pick each pixel's ETC1 selector by comparing the pixel's luma to the
/// subblock's four candidate colors and writing the chosen 2-bit selector into
/// the block's low and high selector bitplanes.
fn etc1_determine_selectors(
    blk: &mut DecoderEtcBlock,
    pixels: &[Color32; 16],
    first_subblock: u32,
    last_subblock: u32,
) {
    // The four candidate colors come out darkest to brightest, but ETC1's
    // selector codes for that order are 3, 2, 0, 1. S_TRAN is indexed by a
    // brightness rank (0 = brightest) and returns the matching selector code.
    const S_TRAN: [u8; 4] = [1, 0, 2, 3];
    let mut l_bitmask = 0u16;
    let mut h_bitmask = 0u16;
    let flip = blk.get_flip_bit();

    for subblock in first_subblock..last_subblock {
        // Integer luma of each candidate color with fixed-point weights. The
        // per-pixel luma below uses exactly twice these weights so a pixel can
        // be compared against the midpoints between adjacent candidate lumas.
        let bc = blk.get_block_colors(subblock);
        let block_y: [u32; 4] = core::array::from_fn(|i| {
            bc[i][0] as u32 * 54 + bc[i][1] as u32 * 183 + bc[i][2] as u32 * 19
        });
        let block_y01 = block_y[0] + block_y[1];
        let block_y12 = block_y[1] + block_y[2];
        let block_y23 = block_y[2] + block_y[3];

        // The two branches are mirror images: flip splits the block into top and
        // bottom halves, no-flip into left and right halves. Each only changes
        // the pixel scan order and how `ofs` (the selector bit index) advances.
        // The number of pairwise-luma midpoints the pixel falls below is its
        // sorted rank, which S_TRAN turns into the ETC1 selector code.
        if flip {
            let mut ofs = subblock * 2;
            for y in 0..2u32 {
                for x in 0..4u32 {
                    let c = pixels[(x + (subblock * 2 + y) * 4) as usize];
                    let l = c[0] as u32 * 108 + c[1] as u32 * 366 + c[2] as u32 * 38;
                    let t = S_TRAN[(l < block_y01) as usize
                        + (l < block_y12) as usize
                        + (l < block_y23) as usize] as u16;
                    l_bitmask |= (t & 1) << ofs;
                    h_bitmask |= (t >> 1) << ofs;
                    ofs += 4;
                }
                ofs = ofs + 1 - 4 * 4;
            }
        } else {
            let mut ofs = subblock * 2 * 4;
            for x in 0..2u32 {
                for y in 0..4u32 {
                    let c = pixels[(subblock * 2 + x + y * 4) as usize];
                    let l = c[0] as u32 * 108 + c[1] as u32 * 366 + c[2] as u32 * 38;
                    let t = S_TRAN[(l < block_y01) as usize
                        + (l < block_y12) as usize
                        + (l < block_y23) as usize] as u16;
                    l_bitmask |= (t & 1) << ofs;
                    h_bitmask |= (t >> 1) << ofs;
                    ofs += 1;
                }
            }
        }
    }

    blk.m_bytes[7] = l_bitmask as u8;
    blk.m_bytes[6] = (l_bitmask >> 8) as u8;
    blk.m_bytes[5] = h_bitmask as u8;
    blk.m_bytes[4] = (h_bitmask >> 8) as u8;
}

/// Fill the ETC1 block `blk` from an unpacked UASTC block and its decoded
/// pixels. The solid-color mode takes precomputed hint fields directly; every
/// other mode averages the pixels into two subblock colors, then derives the
/// selectors.
pub(crate) fn transcode_to_etc1(
    u: &UnpackedUastcBlock,
    pixels: &[Color32; 16],
    blk: &mut DecoderEtcBlock,
) {
    if u.mode == UASTC_MODE_INDEX_SOLID_COLOR {
        // Byte 3 holds the control bits: diff at bit 1, and the same intensity
        // in both the high (bits 5-7) and low (bits 2-4) subblock fields, since
        // a solid block uses one color for both halves.
        blk.m_bytes[3] =
            ((u.etc1_diff as u32) << 1 | (u.etc1_inten0 << 5) | (u.etc1_inten0 << 2)) as u8;
        if u.etc1_diff {
            // Differential mode: 5-bit base color in the top bits, zero delta.
            blk.m_bytes[0] = (u.etc1_r << 3) as u8;
            blk.m_bytes[1] = (u.etc1_g << 3) as u8;
            blk.m_bytes[2] = (u.etc1_b << 3) as u8;
        } else {
            // Individual mode: 4-bit base replicated into both nibbles.
            blk.m_bytes[0] = (u.etc1_r | (u.etc1_r << 4)) as u8;
            blk.m_bytes[1] = (u.etc1_g | (u.etc1_g << 4)) as u8;
            blk.m_bytes[2] = (u.etc1_b | (u.etc1_b << 4)) as u8;
        }
        blk.m_bytes[4..8].copy_from_slice(&ETC1_SOLID_SELECTORS[u.etc1_selector as usize]);
        return;
    }

    let flip = u.etc1_flip;
    let diff = u.etc1_diff;
    blk.m_bytes[3] =
        (flip as u32 | (diff as u32) << 1 | (u.etc1_inten0 << 5) | (u.etc1_inten1 << 2)) as u8;

    let limit = if diff { 31 } else { 15 };
    let mut block_colors = [Color32::default(); 2];

    for subset in 0..2usize {
        // Sum the 8 pixels of this subblock per channel, then scale the 0..2040
        // sum down to the 0..limit quantized range with rounding (the +1020 is
        // half of the 8*255 divisor).
        let mut avg = [0u32; 3];
        for c in &ETC1_PIXEL_COORDS[flip as usize][subset] {
            let (x, y) = (c[0] as usize, c[1] as usize);
            let p = pixels[x + y * 4];
            avg[0] += p.r() as u32;
            avg[1] += p.g() as u32;
            avg[2] += p.b() as u32;
        }
        block_colors[subset] = Color32::new(
            (avg[0] * limit + 1020) / (8 * 255),
            (avg[1] * limit + 1020) / (8 * 255),
            (avg[2] * limit + 1020) / (8 * 255),
            0,
        );
        if HAS_ETC1_BIAS[u.mode as usize] != 0 {
            block_colors[subset] =
                apply_etc1_bias(block_colors[subset], u.etc1_bias, limit, subset as u32);
        }
    }

    if diff {
        // Differential mode stores subblock 0 as a 5-bit base and subblock 1 as
        // a signed 3-bit delta. Clamp the delta to the encodable [-4, 3] range,
        // then fold negatives into their two's-complement 3-bit form.
        let mut dr = block_colors[1].r() as i32 - block_colors[0].r() as i32;
        let mut dg = block_colors[1].g() as i32 - block_colors[0].g() as i32;
        let mut db = block_colors[1].b() as i32 - block_colors[0].b() as i32;
        dr = dr.clamp(ETC1_COLOR_DELTA_MIN, ETC1_COLOR_DELTA_MAX);
        dg = dg.clamp(ETC1_COLOR_DELTA_MIN, ETC1_COLOR_DELTA_MAX);
        db = db.clamp(ETC1_COLOR_DELTA_MIN, ETC1_COLOR_DELTA_MAX);
        if dr < 0 {
            dr += 8;
        }
        if dg < 0 {
            dg += 8;
        }
        if db < 0 {
            db += 8;
        }
        blk.m_bytes[0] = ((block_colors[0].r() as i32) << 3 | dr) as u8;
        blk.m_bytes[1] = ((block_colors[0].g() as i32) << 3 | dg) as u8;
        blk.m_bytes[2] = ((block_colors[0].b() as i32) << 3 | db) as u8;
    } else {
        // Individual mode stores both 4-bit subblock colors, subblock 0 in the
        // high nibble and subblock 1 in the low nibble.
        blk.m_bytes[0] = block_colors[1].r() | (block_colors[0].r() << 4);
        blk.m_bytes[1] = block_colors[1].g() | (block_colors[0].g() << 4);
        blk.m_bytes[2] = block_colors[1].b() | (block_colors[0].b() << 4);
    }

    etc1_determine_selectors(blk, pixels, 0, 2);
}

/// Transcode one 16-byte UASTC block to an 8-byte ETC1 block. `None` if the
/// source block has an invalid mode.
pub fn transcode_uastc_to_etc1(src: &[u8; 16]) -> Option<[u8; 8]> {
    // read_hints=true: the ETC1 path reads the etc1_flip, intensity, and bias
    // hint fields carried by the UASTC block.
    let u = unpack_to_block(src, false, true)?;
    let pixels = if u.mode == UASTC_MODE_INDEX_SOLID_COLOR {
        [Color32::default(); 16]
    } else {
        unpack_pixels(u.mode, u.common_pattern, u.solid_color, &u.astc, false)
    };
    let mut blk = DecoderEtcBlock::new();
    transcode_to_etc1(&u, &pixels, &mut blk);
    Some(blk.m_bytes)
}
