//! PVRTC1 4bpp transcode (RGB and RGBA), the image-global path.
//!
//! Unlike the per-block targets, PVRTC1 endpoints are not independent per block:
//! each block's low/high endpoints come from the block's bounding box, then a
//! whole-image apply/modulation pass resolves every 8-byte output block from its
//! neighbors' endpoints (with wrap). The prep stage records per-block endpoints
//! and per-texel luma; the apply stage bilinearly blends neighboring endpoint
//! lumas at each texel to pick its 2-bit modulation level.
//!
//! Both source codecs (ETC1S and UASTC) and both color modes (RGB and RGBA)
//! share the endpoint addressing, swizzle, and modulation logic. They differ
//! only in how each block's per-texel luma is recovered: ETC1S decodes the ETC
//! block, UASTC reads the unpacked pixels, and the RGBA paths fold alpha into
//! both the endpoints and the per-texel luma.

use alloc::vec::Vec;
mod tables;

use crate::basislz::etc1s::{Endpoint, Selector};
use crate::color::Color32;
use crate::etc::tables::{ETC_5_TO_8, INTEN_TABLES};
use crate::uastc::unpack::unpack_uastc;
use tables::{
    ETC1_X_SELECTOR_UNPACK, PVRTC_3, PVRTC_3_FLOOR, PVRTC_4, PVRTC_4_CEIL, PVRTC_4_FLOOR, PVRTC_5,
    PVRTC_5_CEIL, PVRTC_5_FLOOR, PVRTC_ALPHA, PVRTC_ALPHA_CEIL, PVRTC_ALPHA_FLOOR,
    PVRTC_SWIZZLE_TABLE,
};

/// Number of bits needed to represent `v` (0 for `v == 0`).
#[inline]
fn total_bits(mut v: u32) -> u32 {
    let mut l = 0;
    while v > 0 {
        v >>= 1;
        l += 1;
    }
    l
}

/// Pack endpoint 0 as opaque 554 by floor-quantizing `c`. Returns the 16-bit
/// low half of the endpoints word.
#[inline]
fn pack_opaque_endpoint0_floor(c: Color32) -> u32 {
    let r = PVRTC_5_FLOOR[c.r() as usize] as u32;
    let g = PVRTC_5_FLOOR[c.g() as usize] as u32;
    let b = (PVRTC_4_FLOOR[c.b() as usize] as u32) << 1;
    // 1RRRRRGGGGGBBBBM, with M (the block's modulation-mode flag) forced to 0,
    // selecting standard 4-level modulation.
    (0x8000 | (r << 10) | (g << 5) | b) & !1
}

/// Pack endpoint 1 as opaque 555 by ceil-quantizing `c`. Returns the 16-bit
/// high half of the endpoints word.
#[inline]
fn pack_opaque_endpoint1_ceil(c: Color32) -> u32 {
    let r = PVRTC_5_CEIL[c.r() as usize] as u32;
    let g = PVRTC_5_CEIL[c.g() as usize] as u32;
    let b = PVRTC_5_CEIL[c.b() as usize] as u32;
    // 1RRRRRGGGGGBBBBB
    0x8000 | (r << 10) | (g << 5) | b
}

/// Endpoints word from a block's RGB bounding box `[low, high]`: endpoint 0
/// floor with a cleared modulation-mode bit, endpoint 1 ceil in the high half.
#[inline]
fn pack_endpoints(low: Color32, high: Color32) -> u32 {
    pack_opaque_endpoint0_floor(low) | (pack_opaque_endpoint1_ceil(high) << 16)
}

/// Pack one opaque or translucent endpoint into its 16-bit half. `m` is the
/// modulation-mode bit, which only exists in endpoint 0's layout; endpoint 1
/// uses that bit for an extra blue bit instead.
#[inline]
fn pack_endpoint_raw(
    endpoint_index: u32,
    r: u32,
    g: u32,
    b: u32,
    a: u32,
    opaque: bool,
    m: u32,
) -> u32 {
    if opaque {
        if endpoint_index == 0 {
            // 554: 1RRRRRGGGGGBBBBM
            (0x8000 | (r << 10) | (g << 5) | (b << 1) | m) & 0xFFFF
        } else {
            // 555: 1RRRRRGGGGGBBBBB
            (0x8000 | (r << 10) | (g << 5) | b) & 0xFFFF
        }
    } else if endpoint_index == 0 {
        // 3443: 0AAA RRRR GGGG BBBM
        ((a << 12) | (r << 8) | (g << 4) | (b << 1) | m) & 0xFFFF
    } else {
        // 3444: 0AAA RRRR GGGG BBBB
        ((a << 12) | (r << 8) | (g << 4) | b) & 0xFFFF
    }
}

/// Alpha-aware floor pack of endpoint 0 (opaque 554 if the floored alpha is 8,
/// else translucent 3443). The modulation-mode bit stays 0: per-texel
/// modulation goes in the separate 32-bit word emitted by the apply pass.
/// Returns the 16-bit low half.
#[inline]
fn pack_endpoint0_floor_rgba(c: Color32) -> u32 {
    let a = PVRTC_ALPHA_FLOOR[c.a() as usize] as u32;
    if a == 8 {
        let r = PVRTC_5_FLOOR[c.r() as usize] as u32;
        let g = PVRTC_5_FLOOR[c.g() as usize] as u32;
        let b = PVRTC_4_FLOOR[c.b() as usize] as u32;
        pack_endpoint_raw(0, r, g, b, a, true, 0)
    } else {
        let r = PVRTC_4_FLOOR[c.r() as usize] as u32;
        let g = PVRTC_4_FLOOR[c.g() as usize] as u32;
        let b = PVRTC_3_FLOOR[c.b() as usize] as u32;
        pack_endpoint_raw(0, r, g, b, a, false, 0)
    }
}

/// Alpha-aware ceil pack of endpoint 1 (opaque 555 if the ceiled alpha is 8,
/// else translucent 3444). Returns the 16-bit high half.
#[inline]
fn pack_endpoint1_ceil_rgba(c: Color32) -> u32 {
    let a = PVRTC_ALPHA_CEIL[c.a() as usize] as u32;
    if a == 8 {
        let r = PVRTC_5_CEIL[c.r() as usize] as u32;
        let g = PVRTC_5_CEIL[c.g() as usize] as u32;
        let b = PVRTC_5_CEIL[c.b() as usize] as u32;
        pack_endpoint_raw(1, r, g, b, a, true, 0)
    } else {
        let r = PVRTC_4_CEIL[c.r() as usize] as u32;
        let g = PVRTC_4_CEIL[c.g() as usize] as u32;
        let b = PVRTC_4_CEIL[c.b() as usize] as u32;
        pack_endpoint_raw(1, r, g, b, a, false, 0)
    }
}

/// Endpoints word from a block's RGBA bounding box `[low, high]` (endpoint 0
/// alpha-aware floor; endpoint 1 alpha-aware ceil).
#[inline]
fn pack_endpoints_rgba(low: Color32, high: Color32) -> u32 {
    pack_endpoint0_floor_rgba(low) | (pack_endpoint1_ceil_rgba(high) << 16)
}

/// Dequantize one packed endpoint to RGBA8 and sum `r + g + b + a`: the
/// sliding-window luma used by the RGBA apply pass.
#[inline]
fn endpoint_l8(endpoints: u32, endpoint_index: u32) -> i32 {
    let mask = if endpoint_index == 0 {
        0xFFFEu32
    } else {
        0xFFFFu32
    };
    let packed = (endpoints >> (if endpoint_index != 0 { 16 } else { 0 })) & mask;
    let (r, g, b, a);
    if packed & 0x8000 != 0 {
        // opaque 554 / 555
        r = PVRTC_5[((packed >> 10) & 31) as usize] as i32;
        g = PVRTC_5[((packed >> 5) & 31) as usize] as i32;
        b = if endpoint_index == 0 {
            PVRTC_4[((packed & 31) >> 1) as usize] as i32
        } else {
            PVRTC_5[(packed & 31) as usize] as i32
        };
        a = 255;
    } else {
        // translucent 3443 / 3444
        r = PVRTC_4[((packed >> 8) & 0xF) as usize] as i32;
        g = PVRTC_4[((packed >> 4) & 0xF) as usize] as i32;
        b = if endpoint_index == 0 {
            PVRTC_3[((packed & 0xF) >> 1) as usize] as i32
        } else {
            PVRTC_4[(packed & 0xF) as usize] as i32
        };
        a = PVRTC_ALPHA[((packed >> 12) & 7) as usize] as i32;
    }
    r + g + b + a
}

/// Luma of the low (554) endpoint as the sum of its three 5-bit components;
/// the 4-bit blue is widened to 5 bits first.
#[inline]
fn opaque_endpoint_l0(endpoints: u32) -> u32 {
    let packed = endpoints;
    let r = (packed >> 10) & 31;
    let g = (packed >> 5) & 31;
    let mut b = packed & 30;
    b |= b >> 4;
    r + g + b
}

/// Luma of the high (555) endpoint, in the same 5-bit-sum units.
#[inline]
fn opaque_endpoint_l1(endpoints: u32) -> u32 {
    let packed = endpoints >> 16;
    let r = (packed >> 10) & 31;
    let g = (packed >> 5) & 31;
    let b = packed & 31;
    r + g + b
}

/// The RGB bounding box of an ETC1S endpoint, evaluated at the block's actual
/// low/high used selectors (`l`, `h`) rather than the extreme selectors 0/3.
/// Callers pass the block's `lo_selector` and `hi_selector`.
#[inline]
fn block_colors5_bounds(
    base_color5: Color32,
    inten_table: u32,
    l: u32,
    h: u32,
) -> (Color32, Color32) {
    let br = ((base_color5.r() as i32) << 3) | ((base_color5.r() as i32) >> 2);
    let bg = ((base_color5.g() as i32) << 3) | ((base_color5.g() as i32) >> 2);
    let bb = ((base_color5.b() as i32) << 3) | ((base_color5.b() as i32) >> 2);
    let it = &INTEN_TABLES[inten_table as usize];
    let lo = Color32::new_clamped(
        br + it[l as usize],
        bg + it[l as usize],
        bb + it[l as usize],
        255,
    );
    let hi = Color32::new_clamped(
        br + it[h as usize],
        bg + it[h as usize],
        bb + it[h as usize],
        255,
    );
    (lo, hi)
}

/// The 16 per-texel bilinear weight tuples `(lx, ly, w0, w1, w2, w3)`, listed
/// sub-quadrant by sub-quadrant (ex, ey): all four texels of a 2x2 sub-quadrant
/// interpolate from the same 2x2 cell of the endpoint-luma window.
const DO_PIX: [(u32, u32, i32, i32, i32, i32); 16] = [
    // ex=0, ey=0
    (0, 0, 4, 4, 4, 4),
    (1, 0, 2, 6, 2, 6),
    (0, 1, 2, 2, 6, 6),
    (1, 1, 1, 3, 3, 9),
    // ex=1, ey=0
    (2, 0, 8, 0, 8, 0),
    (3, 0, 6, 2, 6, 2),
    (2, 1, 4, 0, 12, 0),
    (3, 1, 3, 1, 9, 3),
    // ex=0, ey=1
    (0, 2, 8, 8, 0, 0),
    (1, 2, 4, 12, 0, 0),
    (0, 3, 6, 6, 2, 2),
    (1, 3, 3, 9, 1, 3),
    // ex=1, ey=1
    (2, 2, 16, 0, 0, 0),
    (3, 2, 12, 4, 0, 0),
    (2, 3, 12, 0, 4, 0),
    (3, 3, 9, 3, 3, 1),
];

/// Map a `DO_PIX` index to its `(ex, ey)` sub-quadrant (groups of 4).
#[inline]
fn do_pix_sub_quadrant(i: usize) -> (usize, usize) {
    match i / 4 {
        0 => (0, 0),
        1 => (1, 0),
        2 => (0, 1),
        _ => (1, 1),
    }
}

/// Whether `v` is a nonzero power of two.
#[inline]
fn is_pow2(v: u32) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// Image-global PVRTC1 builder: a prep stage (per-block endpoints + per-block luma
/// source) then a whole-image `apply` emitting the final `num_blocks * 8` buffer.
pub struct Pvrtc1Image {
    /// Image width in 4x4 blocks.
    num_blocks_x: u32,
    /// Image height in 4x4 blocks.
    num_blocks_y: u32,
    /// Per-block packed PVRTC endpoints word (block order `x + y*nbx`).
    endpoints: Vec<u32>,
    /// Per-block per-texel luma source: `luma16[block][ly*4+lx]` is the texel's
    /// `(r+g+b) * 16` value (RGB path) or `(r+g+b+a) * 16` (RGBA path). For
    /// ETC1S RGBA it is the clamped color luma plus the clamped alpha luma
    /// (see [`Self::prep_etc1s_rgba`]).
    luma16: Vec<[i32; 16]>,
    /// Whether this is the RGBA (alpha-aware) path. The apply pass then reads
    /// the sliding-window endpoint luma via the 4-component [`endpoint_l8`]
    /// instead of the opaque RGB form.
    rgba: bool,
}

impl Pvrtc1Image {
    /// PVRTC1 requires power-of-two pixel dimensions; returns `None` otherwise
    /// (and for a zero block count).
    fn new(num_blocks_x: u32, num_blocks_y: u32, rgba: bool) -> Option<Self> {
        if num_blocks_x == 0 || num_blocks_y == 0 {
            return None;
        }
        if !is_pow2(num_blocks_x * 4) || !is_pow2(num_blocks_y * 4) {
            return None;
        }
        let n = (num_blocks_x * num_blocks_y) as usize;
        Some(Self {
            num_blocks_x,
            num_blocks_y,
            endpoints: vec![0u32; n],
            luma16: vec![[0i32; 16]; n],
            rgba,
        })
    }

    /// Prep one ETC1S block: stash its bounding-box endpoints and the per-texel
    /// luma recovered from the block's ETC1-encoded selector bytes.
    fn prep_etc1s(&mut self, block_index: usize, ep: &Endpoint, sel: &Selector) {
        let (low, high) = block_colors5_bounds(
            ep.color5,
            ep.inten5 as u32,
            sel.lo_selector as u32,
            sel.hi_selector as u32,
        );
        self.endpoints[block_index] = pack_endpoints(low, high);

        // The four candidate luma values, one per raw ETC1 selector code:
        // by = (r+g+b)*16 from the 8-bit base color, plus the intensity
        // modifier scaled by 48 (3 channels times the *16 luma scale). The
        // {2,3,1,0} permutation converts a raw ETC1 selector code (produced by
        // the unpack table below) to its intensity-modifier index.
        let base_r = ETC_5_TO_8[ep.color5.r() as usize] as i32;
        let base_g = ETC_5_TO_8[ep.color5.g() as usize] as i32;
        let base_b = ETC_5_TO_8[ep.color5.b() as usize] as i32;
        let it = &INTEN_TABLES[ep.inten5 as usize];
        let by = (base_r + base_g + base_b) * 16;
        let block_colors_y_x16 = [
            by + it[2] * 48,
            by + it[3] * 48,
            by + it[1] * 48,
            by + it[0] * 48,
        ];

        // Each texel's raw ETC1 selector is split across an LSB bit plane and
        // an MSB bit plane; `lookup_x[lx] = (lsb&0xF) | ((msb&0xF)<<4)` gathers
        // one column's two 4-bit nibbles for the `ETC1_X_SELECTOR_UNPACK`
        // table. The `byte_ofs` values index a full 8-byte ETC1 block, whose
        // selector words occupy bytes 4..8, while `Selector::bytes` holds only
        // those four bytes, hence the extra -4 (and -4 on the MSB's -2).
        let bytes = &sel.bytes;
        let mut lookup_x = [0u32; 4];
        for (lx, lk) in lookup_x.iter_mut().enumerate() {
            let byte_ofs = 7 - ((lx * 4) >> 3);
            let lsb_bits = (bytes[byte_ofs - 4] as u32) >> ((lx & 1) * 4);
            let msb_bits = (bytes[byte_ofs - 6] as u32) >> ((lx & 1) * 4);
            *lk = (lsb_bits & 0xF) | ((msb_bits & 0xF) << 4);
        }

        let luma = &mut self.luma16[block_index];
        for ly in 0..4usize {
            for lx in 0..4usize {
                let idx = ETC1_X_SELECTOR_UNPACK[ly][lookup_x[lx] as usize] as usize;
                luma[ly * 4 + lx] = block_colors_y_x16[idx];
            }
        }
    }

    /// Prep one UASTC block from its 16 unpacked RGBA pixels: bounding-box
    /// endpoints + per-texel `(r+g+b)*16` luma.
    fn prep_uastc(&mut self, block_index: usize, pixels: &[Color32; 16]) {
        let mut low = Color32::new(255, 255, 255, 255);
        let mut high = Color32::new(0, 0, 0, 0);
        for &p in pixels.iter() {
            low = Color32::comp_min(low, p);
            high = Color32::comp_max(high, p);
        }
        self.endpoints[block_index] = pack_endpoints(low, high);

        let luma = &mut self.luma16[block_index];
        for (i, &p) in pixels.iter().enumerate() {
            luma[i] = (p.r() as i32 + p.g() as i32 + p.b() as i32) * 16;
        }
    }

    /// Prep one ETC1S block for the RGBA path: alpha-aware bounding-box endpoints
    /// (RGB box from the color codebook + alpha box from the alpha codebook's
    /// green channel), then the per-texel combined luma
    /// `clamp(by + inten48[s], 0, 48*255) + clamp(alpha_base_g + inten16[as], 0, 16*255)`,
    /// indexed by the raw color/alpha selector bits `selectors[ly] >> (lx*2) & 3`.
    fn prep_etc1s_rgba(
        &mut self,
        block_index: usize,
        ep: &Endpoint,
        sel: &Selector,
        alpha_ep: &Endpoint,
        alpha_sel: &Selector,
    ) {
        // RGB bounding box (used selectors), then alpha box from green channel.
        let (mut low, mut high) = block_colors5_bounds(
            ep.color5,
            ep.inten5 as u32,
            sel.lo_selector as u32,
            sel.hi_selector as u32,
        );
        let ag = ETC_5_TO_8[alpha_ep.color5.g() as usize] as i32;
        let ait = &INTEN_TABLES[alpha_ep.inten5 as usize];
        let alo = (ag + ait[alpha_sel.lo_selector as usize]).clamp(0, 255);
        let ahi = (ag + ait[alpha_sel.hi_selector as usize]).clamp(0, 255);
        low = Color32::new(low.r() as u32, low.g() as u32, low.b() as u32, alo as u32);
        high = Color32::new(
            high.r() as u32,
            high.g() as u32,
            high.b() as u32,
            ahi as u32,
        );
        self.endpoints[block_index] = pack_endpoints_rgba(low, high);

        // Per-texel combined luma (color + alpha), {0,1,2,3} selector order.
        let base_r = ETC_5_TO_8[ep.color5.r() as usize] as i32;
        let base_g = ETC_5_TO_8[ep.color5.g() as usize] as i32;
        let base_b = ETC_5_TO_8[ep.color5.b() as usize] as i32;
        let it = &INTEN_TABLES[ep.inten5 as usize];
        let by = (base_r + base_g + base_b) * 16;
        let color_y = [
            (by + it[0] * 48).clamp(0, 48 * 255),
            (by + it[1] * 48).clamp(0, 48 * 255),
            (by + it[2] * 48).clamp(0, 48 * 255),
            (by + it[3] * 48).clamp(0, 48 * 255),
        ];
        let alpha_base_g = ETC_5_TO_8[alpha_ep.color5.g() as usize] as i32 * 16;
        let alpha_y = [
            (alpha_base_g + ait[0] * 16).clamp(0, 16 * 255),
            (alpha_base_g + ait[1] * 16).clamp(0, 16 * 255),
            (alpha_base_g + ait[2] * 16).clamp(0, 16 * 255),
            (alpha_base_g + ait[3] * 16).clamp(0, 16 * 255),
        ];

        let luma = &mut self.luma16[block_index];
        for ly in 0..4usize {
            let crow = sel.selectors[ly] as u32;
            let arow = alpha_sel.selectors[ly] as u32;
            for lx in 0..4usize {
                let cs = ((crow >> (lx * 2)) & 3) as usize;
                let as_ = ((arow >> (lx * 2)) & 3) as usize;
                luma[ly * 4 + lx] = color_y[cs] + alpha_y[as_];
            }
        }
    }

    /// Prep one UASTC block for the RGBA path: bounding-box RGBA endpoints
    /// (alpha-aware floor/ceil) + per-texel `(r+g+b+a)*16` luma.
    fn prep_uastc_rgba(&mut self, block_index: usize, pixels: &[Color32; 16]) {
        let mut low = Color32::new(255, 255, 255, 255);
        let mut high = Color32::new(0, 0, 0, 0);
        for &p in pixels.iter() {
            low = Color32::comp_min(low, p);
            high = Color32::comp_max(high, p);
        }
        self.endpoints[block_index] = pack_endpoints_rgba(low, high);

        let luma = &mut self.luma16[block_index];
        for (i, &p) in pixels.iter().enumerate() {
            luma[i] = (p.r() as i32 + p.g() as i32 + p.b() as i32 + p.a() as i32) * 16;
        }
    }

    /// The whole-image apply/modulation pass: for each block, bilinearly blend
    /// the 3x3 neighborhood's endpoint lumas at every texel and pick its 2-bit
    /// modulation level. Writes the final `num_blocks * 8` swizzled buffer
    /// into `out`.
    fn apply(&self, out: &mut [u8]) -> Option<()> {
        let nbx = self.num_blocks_x;
        let nby = self.num_blocks_y;
        let x_mask = nbx - 1;
        let y_mask = nby - 1;
        let x_bits = total_bits(x_mask);
        let y_bits = total_bits(y_mask);
        let min_bits = x_bits.min(y_bits);
        let swizzle_mask = (1u32 << (min_bits * 2)).wrapping_sub(1);

        // Each output block is 8 bytes little-endian (the 32-bit modulation
        // word, then the 32-bit endpoints word), stored at its swizzled index.
        let n = (nbx * nby) as usize;
        if out.len() != n * 8 {
            return None;
        }
        out.fill(0);

        // Sliding 3x3 endpoint-luma windows (e0 for the low endpoints, e1 for
        // the high), rebuilt at the start of each row. RGB scales the opaque
        // 5-bit-sum luma by 255/31; RGBA uses the 4-component `endpoint_l8`.
        let rgba = self.rgba;
        let win_l0 = |e: u32| -> i32 {
            if rgba {
                endpoint_l8(e, 0)
            } else {
                (opaque_endpoint_l0(e) * 255 / 31) as i32
            }
        };
        let win_l1 = |e: u32| -> i32 {
            if rgba {
                endpoint_l8(e, 1)
            } else {
                (opaque_endpoint_l1(e) * 255 / 31) as i32
            }
        };

        let mut e0 = [[0i32; 4]; 4];
        let mut e1 = [[0i32; 4]; 4];

        let mut block_index = 0usize;
        for y in 0..nby as i32 {
            // Seed the window at the row start; the trailing column (ex=2) is
            // refreshed per block below, so only ex=0,1 survive to the loop.
            let mut pe_rows: [usize; 3] = [0; 3];
            for ey in 0..3usize {
                let by = y + ey as i32 - 1;
                let row_base = ((by as u32 & y_mask) * nbx) as usize;
                pe_rows[ey] = row_base;
                for ex in 0..3usize {
                    let bx = ex as i32 - 1;
                    let e = self.endpoints[row_base + (bx as u32 & x_mask) as usize];
                    e0[ex][ey] = win_l0(e);
                    e1[ex][ey] = win_l1(e);
                }
            }

            let y_swizzle = ((PVRTC_SWIZZLE_TABLE[(y >> 8) as usize] as u32) << 16)
                | (PVRTC_SWIZZLE_TABLE[(y & 0xFF) as usize] as u32);

            for x in 0..nbx as i32 {
                let x_swizzle = ((PVRTC_SWIZZLE_TABLE[(x >> 8) as usize] as u32) << 17)
                    | ((PVRTC_SWIZZLE_TABLE[(x & 0xFF) as usize] as u32) << 1);

                let mut swizzled = x_swizzle | y_swizzle;
                if nbx != nby {
                    swizzled &= swizzle_mask;
                    if nbx > nby {
                        swizzled |= ((x as u32) >> min_bits) << (min_bits * 2);
                    } else {
                        swizzled |= ((y as u32) >> min_bits) << (min_bits * 2);
                    }
                }
                let dst = (swizzled as usize) * 8;

                let endpoints_word = self.endpoints[block_index];

                // Fill the trailing column (ex=2) of the sliding window.
                {
                    let bx = ((x + 1) as u32 & x_mask) as usize;
                    for ey in 0..3usize {
                        let e = self.endpoints[pe_rows[ey] + bx];
                        e0[2][ey] = win_l0(e);
                        e1[2][ey] = win_l1(e);
                    }
                }

                let luma = &self.luma16[block_index];
                let mut modulation = 0u32;
                for (i, &(lx, ly, w0, w1, w2, w3)) in DO_PIX.iter().enumerate() {
                    let (ex, ey) = do_pix_sub_quadrant(i);
                    let a0 = e0[ex][ey];
                    let a1 = e0[ex + 1][ey];
                    let a2 = e0[ex][ey + 1];
                    let a3 = e0[ex + 1][ey + 1];
                    let b0 = e1[ex][ey];
                    let b1 = e1[ex + 1][ey];
                    let b2 = e1[ex][ey + 1];
                    let b3 = e1[ex + 1][ey + 1];

                    let ca_l = a0 * w0 + a1 * w1 + a2 * w2 + a3 * w3;
                    let cb_l = b0 * w0 + b1 * w1 + b2 * w2 + b3 * w3;
                    let cl = luma[(ly * 4 + lx) as usize];
                    let mut dl = cb_l - ca_l;
                    let vl = cl - ca_l;
                    let mut p = vl * 16;
                    if ca_l > cb_l {
                        p = -p;
                        dl = -dl;
                    }
                    let shift = ly * 8 + lx * 2;
                    let mut m = 0u32;
                    if p > 3 * dl {
                        m = 1 << shift;
                    }
                    if p > 8 * dl {
                        m = 2 << shift;
                    }
                    if p > 13 * dl {
                        m = 3 << shift;
                    }
                    modulation |= m;
                }

                out[dst..dst + 4].copy_from_slice(&modulation.to_le_bytes());
                out[dst + 4..dst + 8].copy_from_slice(&endpoints_word.to_le_bytes());

                // Slide the window left by one column.
                for ey in 0..3usize {
                    e0[0][ey] = e0[1][ey];
                    e0[1][ey] = e0[2][ey];
                    e1[0][ey] = e1[1][ey];
                    e1[1][ey] = e1[2][ey];
                }

                block_index += 1;
            }
        }
        Some(())
    }
}

/// Whole-image PVRTC1 encode for the raw-ASTC path: per-4x4-block bounding-box
/// endpoints over the decoded (padded, possibly deblocked) image, then the
/// shared modulation/swizzle
/// apply pass. `extract` yields the 16 edge-clamped pixels of a destination
/// block. The RGB target with `from_alpha` broadcasts each pixel's alpha to
/// RGB first (endpoints and modulation luma both see the broadcast). Unlike
/// the UASTC path there is no RGBA-to-RGB downgrade for opaque files; the
/// caller passes the target as-is.
pub(crate) fn encode_pvrtc1_raw(
    num_blocks_x: u32,
    num_blocks_y: u32,
    rgba: bool,
    from_alpha: bool,
    mut extract: impl FnMut(u32, u32) -> [Color32; 16],
    out: &mut [u8],
) -> Option<()> {
    let mut img = Pvrtc1Image::new(num_blocks_x, num_blocks_y, rgba)?;
    for by in 0..num_blocks_y {
        for bx in 0..num_blocks_x {
            let mut px = extract(bx, by);
            if !rgba && from_alpha {
                for p in px.iter_mut() {
                    let a = p.a() as u32;
                    *p = Color32::new(a, a, a, 255);
                }
            }
            let idx = (bx + by * num_blocks_x) as usize;
            if rgba {
                img.prep_uastc_rgba(idx, &px);
            } else {
                img.prep_uastc(idx, &px);
            }
        }
    }
    img.apply(out)
}

/// Build the whole PVRTC1_4_RGB image from ETC1S per-block `(endpoint, selector)`
/// indices, written into `out`. `None` for non-pow2 dimensions.
#[allow(clippy::too_many_arguments)]
pub fn transcode_etc1s_pvrtc1_4_rgb(
    indices: &[(u16, u16)],
    endpoints: &[Endpoint],
    selectors: &[Selector],
    num_blocks_x: u32,
    num_blocks_y: u32,
    out: &mut [u8],
) -> Option<()> {
    let mut img = Pvrtc1Image::new(num_blocks_x, num_blocks_y, false)?;
    for (b, &(ei, si)) in indices.iter().enumerate() {
        img.prep_etc1s(b, &endpoints[ei as usize], &selectors[si as usize]);
    }
    img.apply(out)
}

/// Build the whole PVRTC1_4_RGBA image from ETC1S color + alpha per-block
/// `(endpoint, selector)` indices. The color indices give the RGB box; the alpha
/// indices (decoded from the alpha slice) give the alpha box + per-texel alpha.
/// The image is written into `out`. `None` for non-pow2 dimensions.
#[allow(clippy::too_many_arguments)]
pub fn transcode_etc1s_pvrtc1_4_rgba(
    color_indices: &[(u16, u16)],
    alpha_indices: &[(u16, u16)],
    endpoints: &[Endpoint],
    selectors: &[Selector],
    num_blocks_x: u32,
    num_blocks_y: u32,
    out: &mut [u8],
) -> Option<()> {
    let mut img = Pvrtc1Image::new(num_blocks_x, num_blocks_y, true)?;
    if alpha_indices.len() != color_indices.len() {
        return None;
    }
    for (b, (&(ei, si), &(aei, asi))) in color_indices.iter().zip(alpha_indices.iter()).enumerate()
    {
        img.prep_etc1s_rgba(
            b,
            &endpoints[ei as usize],
            &selectors[si as usize],
            &endpoints[aei as usize],
            &selectors[asi as usize],
        );
    }
    img.apply(out)
}

/// Build the whole PVRTC1_4_RGB image from raw UASTC blocks (`num_blocks * 16`
/// bytes), written into `out`. `None` for non-pow2 dimensions or a corrupt
/// block.
pub fn transcode_uastc_pvrtc1_4_rgb(
    uastc_blocks: &[u8],
    num_blocks_x: u32,
    num_blocks_y: u32,
    out: &mut [u8],
) -> Option<()> {
    let mut img = Pvrtc1Image::new(num_blocks_x, num_blocks_y, false)?;
    let n = (num_blocks_x * num_blocks_y) as usize;
    let data = uastc_blocks.get(..n * 16)?;
    for (b, blk) in data.chunks_exact(16).enumerate() {
        let src: [u8; 16] = blk.try_into().ok()?;
        let pixels = unpack_uastc(&src, false)?;
        img.prep_uastc(b, &pixels);
    }
    img.apply(out)
}

/// Build the whole PVRTC1_4_RGBA image from raw UASTC blocks, written into
/// `out`. `None` for non-pow2 dimensions or a corrupt block.
pub fn transcode_uastc_pvrtc1_4_rgba(
    uastc_blocks: &[u8],
    num_blocks_x: u32,
    num_blocks_y: u32,
    out: &mut [u8],
) -> Option<()> {
    let mut img = Pvrtc1Image::new(num_blocks_x, num_blocks_y, true)?;
    let n = (num_blocks_x * num_blocks_y) as usize;
    let data = uastc_blocks.get(..n * 16)?;
    for (b, blk) in data.chunks_exact(16).enumerate() {
        let src: [u8; 16] = blk.try_into().ok()?;
        let pixels = unpack_uastc(&src, false)?;
        img.prep_uastc_rgba(b, &pixels);
    }
    img.apply(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `total_bits` returns the position of the highest set bit plus one, and
    /// zero for zero.
    #[test]
    fn total_bits_matches_reference() {
        assert_eq!(total_bits(0), 0);
        assert_eq!(total_bits(1), 1);
        assert_eq!(total_bits(0b1111), 4);
        assert_eq!(total_bits(0b1000_0000), 8);
    }

    /// The constructor accepts only power-of-two pixel dimensions.
    #[test]
    fn pow2_guard_rejects_non_pow2() {
        // 3 blocks => 12 px wide, not pow2.
        assert!(Pvrtc1Image::new(3, 2, false).is_none());
        // 2 blocks => 8 px, pow2.
        assert!(Pvrtc1Image::new(2, 2, false).is_some());
    }
}
