use std::ops::{Deref, Range};

use crate::glyph_id::GlyphId;

// The top bit of a glyph's id: whether a line break before its cluster
// changes the shapes on either side of it. No font format numbers glyphs
// past 24 bits.
const UNSAFE_TO_BREAK: u32 = 1 << 31;

/// A glyph shaping placed: its id, the text offset of its cluster and how
/// far it moves the pen. Where it draws from the pen stands apart, in
/// [`ShapedGlyphs`], since most glyphs draw at the pen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShapedGlyph {
    id: u32,
    /// The byte offset in the text of the cluster the glyph belongs to.
    pub cluster: u32,
    /// How far the glyph moves the pen.
    pub advance: i32,
}

/// How far right of the pen and up from the baseline a glyph draws, in
/// design units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlyphOffset {
    /// How far right of the pen.
    pub x: i32,
    /// How far up from the baseline.
    pub y: i32,
}

// Whether a line break before a glyph's cluster keeps the shapes on either
// side of it.
#[derive(Clone, Copy)]
pub(crate) enum BreakBefore {
    Safe,
    Unsafe,
}

/// Glyphs in the order shaping placed them, with where each draws from the
/// pen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedGlyphs {
    glyphs: Vec<ShapedGlyph>,
    // Where the first glyphs draw from the pen, up to the last one that
    // draws off it; the glyphs past them draw at the pen, as in most text.
    offsets: Vec<GlyphOffset>,
}

impl ShapedGlyph {
    pub(crate) fn new(id: GlyphId, cluster: u32, advance: i32, before: BreakBefore) -> Self {
        let flag = match before {
            BreakBefore::Safe => 0,
            BreakBefore::Unsafe => UNSAFE_TO_BREAK,
        };
        Self {
            id: id.0 | flag,
            cluster,
            advance,
        }
    }

    // The glyph in the cluster at `cluster`.
    pub(crate) fn in_cluster(self, cluster: u32) -> Self {
        Self { cluster, ..self }
    }

    /// The glyph.
    pub fn id(&self) -> GlyphId {
        GlyphId(self.id & !UNSAFE_TO_BREAK)
    }

    /// Whether a line break before this glyph's cluster changes the shapes
    /// on either side of it.
    pub fn unsafe_to_break(&self) -> bool {
        self.id & UNSAFE_TO_BREAK != 0
    }

    /// The byte offset in the text of the cluster the glyph belongs to.
    pub fn text_offset(&self) -> usize {
        usize::try_from(self.cluster).unwrap_or(usize::MAX)
    }
}

impl ShapedGlyphs {
    /// Where glyph `index` draws from the pen.
    pub fn offset(&self, index: usize) -> GlyphOffset {
        self.offsets.get(index).copied().unwrap_or_default()
    }

    // Where the first glyphs draw from the pen; the glyphs past them draw at
    // the pen.
    pub(crate) fn offsets(&self) -> &[GlyphOffset] {
        &self.offsets
    }

    pub(crate) fn glyphs_mut(&mut self) -> &mut [ShapedGlyph] {
        &mut self.glyphs
    }

    // Adds `glyph`, drawn at `offset` from the pen.
    pub(crate) fn push(&mut self, glyph: ShapedGlyph, offset: GlyphOffset) {
        if offset != GlyphOffset::default() {
            self.offsets
                .resize(self.glyphs.len(), GlyphOffset::default());
            self.offsets.push(offset);
        }
        self.glyphs.push(glyph);
    }

    // Adds `glyphs`, the first drawn at `offsets` from the pen and the rest
    // at the pen.
    pub(crate) fn extend_from_slice(&mut self, glyphs: &[ShapedGlyph], offsets: &[GlyphOffset]) {
        self.add_offsets(offsets);
        self.glyphs.extend_from_slice(glyphs);
    }

    // Adds `glyphs`, the first drawn at `offsets` from the pen and the rest
    // at the pen.
    pub(crate) fn extend(
        &mut self,
        glyphs: impl Iterator<Item = ShapedGlyph>,
        offsets: &[GlyphOffset],
    ) {
        self.add_offsets(offsets);
        self.glyphs.extend(glyphs);
    }

    // Adds the `glyphs` of a word that starts at `start`, whose clusters
    // count from the word's start, drawn at `offsets` as `extend_from_slice`
    // draws them.
    pub(crate) fn extend_from_word(
        &mut self,
        glyphs: &[ShapedGlyph],
        offsets: &[GlyphOffset],
        start: u32,
    ) {
        self.add_offsets(offsets);
        self.glyphs.extend(
            glyphs
                .iter()
                .map(|glyph| glyph.in_cluster(glyph.cluster + start)),
        );
    }

    // Adds the offsets of glyphs about to be added.
    fn add_offsets(&mut self, offsets: &[GlyphOffset]) {
        if !offsets.is_empty() {
            self.offsets
                .resize(self.glyphs.len(), GlyphOffset::default());
            self.offsets.extend_from_slice(offsets);
        }
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.glyphs.truncate(len);
        self.offsets.truncate(len);
    }

    pub(crate) fn clear(&mut self) {
        self.glyphs.clear();
        self.offsets.clear();
    }

    pub(crate) fn reserve(&mut self, additional: usize) {
        self.glyphs.reserve(additional);
    }

    pub(crate) fn capacity(&self) -> usize {
        self.glyphs.capacity()
    }

    // Reverses the order of the glyphs of `range`.
    pub(crate) fn reverse(&mut self, range: Range<usize>) {
        if self.offsets.len() > range.start {
            let len = self.offsets.len().max(range.end);
            self.offsets.resize(len, GlyphOffset::default());
            self.offsets[range.clone()].reverse();
        }
        self.glyphs[range].reverse();
    }
}

impl Deref for ShapedGlyphs {
    type Target = [ShapedGlyph];

    fn deref(&self) -> &[ShapedGlyph] {
        &self.glyphs
    }
}
