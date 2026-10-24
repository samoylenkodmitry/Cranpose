//! Printable ASCII glyph facts of one face, read from the font once.

use std::sync::{
    OnceLock,
    atomic::{AtomicI16, Ordering},
};

use ab_glyph::{Font, GlyphId};
use portable_atomic::AtomicU64;

/// The first character the table holds: space.
const FIRST: u32 = 0x20;
/// Space through tilde.
pub(crate) const ASCII_COUNT: usize = 95;
/// Marks a filled glyph slot, above the glyph id's 16 bits and the
/// advance's 32.
const KNOWN_GLYPH: u64 = 1 << 48;
/// An unfilled kerning slot, which holds as well a kerning that is no whole
/// number of font units, as a font variation may give.
const UNKNOWN_KERN: i16 = i16::MIN;

/// The kerning in font units after one character, per next character.
type KernRow = [AtomicI16; ASCII_COUNT];

/// The glyph id, advance and pair kerning of each printable ASCII character
/// of a face. Measuring and drawing text read them a character at a time,
/// and the font answers each with a cmap search, a metrics read and a GPOS
/// pair search; the table answers with one load, filled on first use by
/// whichever thread asks first.
pub(crate) struct AsciiGlyphs {
    /// Per character: [`KNOWN_GLYPH`], the glyph id in bits 32..48 and the
    /// advance's bits below, or 0 while unread.
    glyphs: [AtomicU64; ASCII_COUNT],
    /// Per previous character, its row of kernings, or [`UNKNOWN_KERN`]
    /// while unread. A row is allocated when its character first precedes
    /// another: a screen's text uses a few dozen of the 95, and the whole
    /// table took 36 KB, a seventh of what a classic ESP32 has free.
    kerns: [OnceLock<Box<KernRow>>; ASCII_COUNT],
}

/// The index of a printable ASCII character, `None` for any other.
pub(crate) fn ascii_slot(ch: char) -> Option<usize> {
    let index = usize::try_from(u32::from(ch).checked_sub(FIRST)?).ok()?;
    (index < ASCII_COUNT).then_some(index)
}

impl AsciiGlyphs {
    pub(crate) fn new() -> Self {
        Self {
            glyphs: std::array::from_fn(|_| AtomicU64::new(0)),
            kerns: std::array::from_fn(|_| OnceLock::new()),
        }
    }

    /// The glyph of `ch` in `font` and its advance in font units; `None`
    /// outside printable ASCII.
    pub(crate) fn glyph(&self, font: &impl Font, ch: char) -> Option<(GlyphId, f32)> {
        let cell = &self.glyphs[ascii_slot(ch)?];
        let packed = cell.load(Ordering::Relaxed);
        if packed & KNOWN_GLYPH != 0 {
            return Some((
                GlyphId((packed >> 32) as u16),
                f32::from_bits(packed as u32),
            ));
        }
        let id = font.glyph_id(ch);
        let advance = font.h_advance_unscaled(id);
        cell.store(
            KNOWN_GLYPH | (u64::from(id.0) << 32) | u64::from(advance.to_bits()),
            Ordering::Relaxed,
        );
        Some((id, advance))
    }

    /// The kerning in font units between the glyphs of two consecutive
    /// characters; `None` unless both are printable ASCII.
    pub(crate) fn kern(
        &self,
        font: &impl Font,
        (previous, previous_id): (char, GlyphId),
        (ch, id): (char, GlyphId),
    ) -> Option<f32> {
        let (row, next) = (ascii_slot(previous)?, ascii_slot(ch)?);
        let row = self.kerns[row]
            .get_or_init(|| Box::new(std::array::from_fn(|_| AtomicI16::new(UNKNOWN_KERN))));
        let cell = &row[next];
        let units = cell.load(Ordering::Relaxed);
        if units != UNKNOWN_KERN {
            return Some(f32::from(units));
        }
        let kern = font.kern_unscaled(previous_id, id);
        let whole = kern as i16;
        if f32::from(whole) == kern && whole != UNKNOWN_KERN {
            cell.store(whole, Ordering::Relaxed);
        }
        Some(kern)
    }
}

#[cfg(test)]
#[path = "tests/ascii_glyphs_tests.rs"]
mod tests;
