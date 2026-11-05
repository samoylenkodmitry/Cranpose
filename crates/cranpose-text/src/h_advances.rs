// The advances of a font at its default location, read straight from the
// bytes of its `hmtx` table. A whole pixel layout reads the advance of
// every glyph in it. The general reader costs more per glyph than the read
// itself.

use std::ops::Range;

use read_fonts::{ReadError, TableProvider, model::Font};

use crate::simple_glyf::{default_location, range_in};

pub(crate) struct HAdvances {
    // The advance and side bearing pairs of `hmtx`, four bytes each.
    metrics: Range<usize>,
    num_glyphs: u32,
}

impl HAdvances {
    // The advances of `font`; `None` for a font at a variation location, one
    // whose advances come from an `HMTX` table, or one without `hmtx`, which
    // the general reader measures.
    pub(crate) fn of(font: &Font) -> Option<Self> {
        let (bytes, tables) = default_location(font)?;
        if !matches!(tables.hmtx_extended(), Err(ReadError::TableIsMissing(_))) {
            return None;
        }
        let metrics = range_in(bytes, tables.hmtx().ok()?.h_metrics())?;
        (!metrics.is_empty()).then(|| Self {
            metrics,
            num_glyphs: font.metrics().num_glyphs,
        })
    }

    // The table's advance and side bearing pairs in `bytes`, the font's.
    pub(crate) fn metrics<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        &bytes[self.metrics.clone()]
    }

    // The advance of `glyph` in font units, from `metrics`: the last pair's
    // past the pairs, as `hmtx` repeats it, and none past the font's glyphs,
    // as the general reader gives.
    #[inline(always)]
    pub(crate) fn advance(&self, metrics: &[u8], glyph: u32) -> u16 {
        if glyph >= self.num_glyphs {
            return 0;
        }
        let last = metrics.len() / 4 - 1;
        let at = usize::try_from(glyph).map_or(last, |glyph| glyph.min(last)) * 4;
        metrics
            .get(at..at + 2)
            .map_or(0, |pair| u16::from_be_bytes([pair[0], pair[1]]))
    }
}
