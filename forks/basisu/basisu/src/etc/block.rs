//! `decoder_etc_block`: the ETC1 block bit layout plus base-color and
//! block-color decode helpers. The 8-byte block is addressed big-endian
//! (`byte_ofs = 7 - (ofs >> 3)`).

use super::tables::INTEN_TABLES;
use crate::color::Color32;

/// Clamp a signed intermediate to the 0..=255 byte range.
#[inline]
fn clamp255(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// `unpack_delta3`: signed 3-bit deltas from a packed 9-bit value.
fn unpack_delta3(packed: u16) -> (i32, i32, i32) {
    let mut r = ((packed >> 6) & 7) as i32;
    let mut g = ((packed >> 3) & 7) as i32;
    let mut b = (packed & 7) as i32;
    if r >= 4 {
        r -= 8;
    }
    if g >= 4 {
        g -= 8;
    }
    if b >= 4 {
        b -= 8;
    }
    (r, g, b)
}

/// Unpack a packed 5:5:5 color (`b | g<<5 | r<<10`) into a [`Color32`] with the
/// given alpha. With `scaled` set, each 5-bit channel is expanded to 8 bits.
fn unpack_color5(packed: u16, scaled: bool, alpha: u32) -> Color32 {
    let mut b = (packed & 31) as u32;
    let mut g = ((packed >> 5) & 31) as u32;
    let mut r = ((packed >> 10) & 31) as u32;
    if scaled {
        b = (b << 3) | (b >> 2);
        g = (g << 3) | (g >> 2);
        r = (r << 3) | (r >> 2);
    }
    Color32::new(r, g, b, alpha)
}

/// Unpack a 5-bit base color plus a signed 3-bit per-channel delta into a
/// [`Color32`]. An out-of-range channel is clamped back into 0..=31, then each
/// channel is optionally expanded to 8 bits when `scaled` is set.
fn unpack_color5_delta(packed5: u16, delta3: u16, scaled: bool, alpha: u32) -> Color32 {
    let (dr, dg, db) = unpack_delta3(delta3);
    let mut r = ((packed5 >> 10) & 31) as i32 + dr;
    let mut g = ((packed5 >> 5) & 31) as i32 + dg;
    let mut b = (packed5 & 31) as i32 + db;
    // one compare catches both directions: a negative component wraps to a
    // huge u32, so or-ing and testing > 31 flags any out-of-range channel
    if ((r | g | b) as u32) > 31 {
        r = r.clamp(0, 31);
        g = g.clamp(0, 31);
        b = b.clamp(0, 31);
    }
    if scaled {
        b = (b << 3) | (b >> 2);
        g = (g << 3) | (g >> 2);
        r = (r << 3) | (r >> 2);
    }
    Color32::new(r as u32, g as u32, b as u32, alpha.min(255))
}

/// Unpack a packed 4:4:4 color (`b | g<<4 | r<<8`) into a [`Color32`] with the
/// given alpha. With `scaled` set, each 4-bit channel is expanded to 8 bits by
/// nibble replication.
fn unpack_color4(packed: u16, scaled: bool, alpha: u32) -> Color32 {
    let mut b = (packed & 15) as u32;
    let mut g = ((packed >> 4) & 15) as u32;
    let mut r = ((packed >> 8) & 15) as u32;
    if scaled {
        b = (b << 4) | b;
        g = (g << 4) | g;
        r = (r << 4) | r;
    }
    Color32::new(r, g, b, alpha.min(255))
}

/// An ETC1 block: 8 raw bytes in the ETC1/ETC2 bit layout.
#[derive(Clone, Copy, Default)]
pub struct DecoderEtcBlock {
    /// The 8 raw block bytes in the big-endian ETC1/ETC2 bit layout.
    pub m_bytes: [u8; 8],
}

impl DecoderEtcBlock {
    /// An all-zero block.
    pub fn new() -> Self {
        Self { m_bytes: [0; 8] }
    }

    /// Write `num` bits at bit offset `ofs`. The layout is big-endian within the
    /// 8-byte block (`byte_ofs = 7 - (ofs >> 3)`), so growing bit offsets walk
    /// the bytes from the high end down.
    fn set_byte_bits(&mut self, ofs: u32, num: u32, bits: u32) {
        let byte_ofs = (7 - (ofs >> 3)) as usize;
        let byte_bit_ofs = ofs & 7;
        let mask = (1u32 << num) - 1;
        self.m_bytes[byte_ofs] &= !((mask << byte_bit_ofs) as u8);
        self.m_bytes[byte_ofs] |= (bits << byte_bit_ofs) as u8;
    }

    /// Flip bit (byte 3, bit 0): selects horizontal vs vertical subblock split.
    pub fn set_flip_bit(&mut self, flip: bool) {
        self.m_bytes[3] &= !1;
        self.m_bytes[3] |= flip as u8;
    }

    /// Diff bit (byte 3, bit 1): differential vs individual base-color mode.
    pub fn set_diff_bit(&mut self, diff: bool) {
        self.m_bytes[3] &= !2;
        self.m_bytes[3] |= (diff as u8) << 1;
    }

    /// Write the 3-bit intensity-table index for one subblock. Subblock 1 lives
    /// at byte 3 bit 2, subblock 0 at byte 3 bit 5.
    pub fn set_inten_table(&mut self, subblock: u32, t: u32) {
        let ofs = if subblock != 0 { 2 } else { 5 };
        self.m_bytes[3] &= !((7 << ofs) as u8);
        self.m_bytes[3] |= (t << ofs) as u8;
    }

    /// `set_base5_color`: write a packed 5-bit base color (`b | g<<5 | r<<10`).
    pub fn set_base5_color(&mut self, c: u16) {
        let c = c as u32;
        self.set_byte_bits(59, 5, (c >> 10) & 31);
        self.set_byte_bits(51, 5, (c >> 5) & 31);
        self.set_byte_bits(43, 5, c & 31);
    }

    /// Pack a 5-bit-per-channel color into the 15-bit `b | g<<5 | r<<10` field,
    /// taking the low 5 bits of each channel without rescaling.
    pub fn pack_color5_unscaled(color5: Color32) -> u16 {
        (color5.b() as u16) | ((color5.g() as u16) << 5) | ((color5.r() as u16) << 10)
    }

    /// `get_block_color5`: the single (r, g, b) at `index` for a base5 + inten.
    pub fn get_block_color5(
        base_color5: Color32,
        inten_table: u32,
        index: usize,
    ) -> (u32, u32, u32) {
        let br = ((base_color5.r() as u32) << 3) | ((base_color5.r() as u32) >> 2);
        let bg = ((base_color5.g() as u32) << 3) | ((base_color5.g() as u32) >> 2);
        let bb = ((base_color5.b() as u32) << 3) | ((base_color5.b() as u32) >> 2);
        let inten = &INTEN_TABLES[inten_table as usize];
        (
            clamp255(br as i32 + inten[index]) as u32,
            clamp255(bg as i32 + inten[index]) as u32,
            clamp255(bb as i32 + inten[index]) as u32,
        )
    }

    /// `get_block_color5_r`: the single red-channel value at `index`.
    pub fn get_block_color5_r(base_color5: Color32, inten_table: u32, index: usize) -> u32 {
        let br = ((base_color5.r() as u32) << 3) | ((base_color5.r() as u32) >> 2);
        clamp255(br as i32 + INTEN_TABLES[inten_table as usize][index]) as u32
    }

    /// `get_block_colors5_g`: the 4 green-channel candidate values.
    pub fn get_block_colors5_g(base_color5: Color32, inten_table: u32) -> [i32; 4] {
        let g = ((base_color5.g() as i32) << 3) | ((base_color5.g() as i32) >> 2);
        let inten = &INTEN_TABLES[inten_table as usize];
        core::array::from_fn(|i| clamp255(g + inten[i]) as i32)
    }

    /// `get_block_colors5`: the 4 candidate colors for a 5-bit base color +
    /// intensity table (scales the 5-bit color and applies the modifiers).
    pub fn get_block_colors5(base_color5: Color32, inten_table: u32) -> [Color32; 4] {
        let r = ((base_color5.r() as u32) << 3) | ((base_color5.r() as u32) >> 2);
        let g = ((base_color5.g() as u32) << 3) | ((base_color5.g() as u32) >> 2);
        let b = ((base_color5.b() as u32) << 3) | ((base_color5.b() as u32) >> 2);
        let inten = &INTEN_TABLES[inten_table as usize];
        let mut out = [Color32::default(); 4];
        for (i, c) in out.iter_mut().enumerate() {
            *c = Color32::new(
                clamp255(r as i32 + inten[i]) as u32,
                clamp255(g as i32 + inten[i]) as u32,
                clamp255(b as i32 + inten[i]) as u32,
                255,
            );
        }
        out
    }

    /// Read `num` bits at bit offset `ofs`, using the same big-endian byte
    /// layout as [`Self::set_byte_bits`].
    fn get_byte_bits(&self, ofs: u32, num: u32) -> u32 {
        let byte_ofs = (7 - (ofs >> 3)) as usize;
        let byte_bit_ofs = ofs & 7;
        ((self.m_bytes[byte_ofs] as u32) >> byte_bit_ofs) & ((1 << num) - 1)
    }

    /// Packed 5-bit base color of the differential-mode subblock 0 (`b | g<<5 |
    /// r<<10`).
    fn get_base5_color(&self) -> u16 {
        let r = self.get_byte_bits(59, 5);
        let g = self.get_byte_bits(51, 5);
        let b = self.get_byte_bits(43, 5);
        (b | (g << 5) | (r << 10)) as u16
    }

    /// Packed 4-bit individual-mode base color for subblock `idx` (`b | g<<4 |
    /// r<<8`). The two subblocks occupy different bit fields of the block.
    fn get_base4_color(&self, idx: u32) -> u16 {
        let (r, g, b) = if idx != 0 {
            (
                self.get_byte_bits(56, 4),
                self.get_byte_bits(48, 4),
                self.get_byte_bits(40, 4),
            )
        } else {
            (
                self.get_byte_bits(60, 4),
                self.get_byte_bits(52, 4),
                self.get_byte_bits(44, 4),
            )
        };
        (b | (g << 4) | (r << 8)) as u16
    }

    /// Packed signed 3-bit deltas applied to the base5 color in differential
    /// mode (`b | g<<3 | r<<6`).
    fn get_delta3_color(&self) -> u16 {
        let r = self.get_byte_bits(56, 3);
        let g = self.get_byte_bits(48, 3);
        let b = self.get_byte_bits(40, 3);
        (b | (g << 3) | (r << 6)) as u16
    }

    /// Flip bit (byte 3, bit 0).
    pub fn get_flip_bit(&self) -> bool {
        self.m_bytes[3] & 1 != 0
    }

    /// Diff bit (byte 3, bit 1).
    pub fn get_diff_bit(&self) -> bool {
        self.m_bytes[3] & 2 != 0
    }

    /// 3-bit intensity-table index for one subblock (see
    /// [`Self::set_inten_table`] for the bit positions).
    fn get_inten_table(&self, subblock: u32) -> u32 {
        let ofs = if subblock != 0 { 2 } else { 5 };
        ((self.m_bytes[3] as u32) >> ofs) & 7
    }

    /// `get_block_colors`: the 4 candidate colors for a subblock.
    pub fn get_block_colors(&self, subblock: u32) -> [Color32; 4] {
        let base = if self.get_diff_bit() {
            if subblock != 0 {
                unpack_color5_delta(self.get_base5_color(), self.get_delta3_color(), true, 255)
            } else {
                unpack_color5(self.get_base5_color(), true, 255)
            }
        } else {
            unpack_color4(self.get_base4_color(subblock), true, 255)
        };

        let inten = &INTEN_TABLES[self.get_inten_table(subblock) as usize];
        let mut out = [Color32::default(); 4];
        for (i, c) in out.iter_mut().enumerate() {
            *c = Color32::new(
                clamp255(base.r() as i32 + inten[i]) as u32,
                clamp255(base.g() as i32 + inten[i]) as u32,
                clamp255(base.b() as i32 + inten[i]) as u32,
                255,
            );
        }
        out
    }
}
