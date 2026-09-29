//! Printable ASCII glyph facts of one face, read from the font once.

use std::sync::{
    OnceLock,
    atomic::{AtomicU32, AtomicU64, Ordering},
};

use ab_glyph::{Font, GlyphId};

/// The first character the table holds: space.
const FIRST: u32 = 0x20;
/// Space through tilde.
pub(crate) const ASCII_COUNT: usize = 95;
/// Marks a filled glyph slot, above the glyph id's 16 bits and the
/// advance's 32.
const KNOWN_GLYPH: u64 = 1 << 48;
/// An unfilled kerning slot: a NaN, which no kerning value is.
const UNKNOWN_KERN: u32 = u32::MAX;

/// The glyph id, advance and pair kerning of each printable ASCII character
/// of a face. Measuring and drawing text read them a character at a time,
/// and the font answers each with a cmap search, a metrics read and a GPOS
/// pair search; the table answers with one load, filled on first use by
/// whichever thread asks first.
pub(crate) struct AsciiGlyphs {
    /// Per character: [`KNOWN_GLYPH`], the glyph id in bits 32..48 and the
    /// advance's bits below, or 0 while unread.
    glyphs: [AtomicU64; ASCII_COUNT],
    /// Per pair, previous character major: the kerning's bits, or
    /// [`UNKNOWN_KERN`] while unread. Allocated on the first kerned pair.
    kerns: OnceLock<Box<[AtomicU32]>>,
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
            kerns: OnceLock::new(),
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
        let index = ascii_slot(previous)? * ASCII_COUNT + ascii_slot(ch)?;
        let kerns = self.kerns.get_or_init(|| {
            (0..ASCII_COUNT * ASCII_COUNT)
                .map(|_| AtomicU32::new(UNKNOWN_KERN))
                .collect()
        });
        let cell = &kerns[index];
        let bits = cell.load(Ordering::Relaxed);
        if bits != UNKNOWN_KERN {
            return Some(f32::from_bits(bits));
        }
        let kern = font.kern_unscaled(previous_id, id);
        cell.store(kern.to_bits(), Ordering::Relaxed);
        Some(kern)
    }
}

#[cfg(test)]
#[path = "tests/ascii_glyphs_tests.rs"]
mod tests;
