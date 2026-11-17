use std::{ops::Range, sync::Arc};

use crate::{
    glyph_id::GlyphId,
    index_range::widen,
    shaped_glyphs::{BreakBefore, GlyphOffset, ShapedGlyph, ShapedGlyphs},
};

// The glyphs of a layout, with where the first draw from the pen when any
// draws off it, as `ShapedGlyphs` holds them; the copies of a layout and a
// relayout that adds no glyphs share them.
#[derive(Clone, Debug, Default)]
pub(crate) struct LaidGlyphs {
    glyphs: Glyphs,
    offsets: Option<Arc<[GlyphOffset]>>,
}

impl PartialEq for LaidGlyphs {
    fn eq(&self, other: &Self) -> bool {
        self.all() == other.all() && self.offsets == other.offsets
    }
}

// A layout's glyphs: in eight bytes each when every one fits them, as the
// glyphs of all but the widest fonts do.
#[derive(Clone, Debug)]
enum Glyphs {
    // The first `len` glyphs of `room`, which holds more when the layout
    // took the room of more glyphs, as a counter's number one digit shorter.
    Packed { room: Arc<[PackedGlyph]>, len: u32 },
    Full(Arc<[ShapedGlyph]>),
}

impl Default for Glyphs {
    fn default() -> Self {
        Self::Packed {
            room: Arc::default(),
            len: 0,
        }
    }
}

// Whether glyphs `count` long take a room `room` long: one as long, or one
// at most twice as long, so a short text keeps no long text's room.
fn takes_room(room: usize, count: usize) -> bool {
    count <= room && room <= count.saturating_mul(2)
}

// A glyph whose id fits 16 bits and whose advance fits 15.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedGlyph {
    id: u16,
    // The advance above the lowest bit, which is set when a line break
    // before the glyph's cluster is unsafe.
    advance: i16,
    cluster: u32,
}

impl PackedGlyph {
    pub(crate) fn id(self) -> GlyphId {
        GlyphId(u32::from(self.id))
    }

    fn text_offset(self) -> usize {
        usize::try_from(self.cluster).unwrap_or(usize::MAX)
    }

    // Whether `glyph`'s id fits 16 bits and its advance 15. No branch, so a
    // layout's glyphs are checked in one pass.
    fn fits(glyph: &ShapedGlyph) -> bool {
        let advance = glyph.advance.cast_unsigned();
        (glyph.id().0 >> 16 == 0) & (advance.wrapping_add(1 << 14) >> 15 == 0)
    }

    // `glyph` in eight bytes, its id and advance cut to their low bits.
    fn pack(glyph: &ShapedGlyph) -> Self {
        let advance = glyph.advance.cast_unsigned();
        Self {
            id: (glyph.id().0 & 0xFFFF) as u16,
            advance: (((advance & 0x7FFF) as u16) << 1).cast_signed()
                | i16::from(glyph.unsafe_to_break()),
            cluster: glyph.cluster,
        }
    }

    fn unpack(self) -> ShapedGlyph {
        let before = if self.advance & 1 == 0 {
            BreakBefore::Safe
        } else {
            BreakBefore::Unsafe
        };
        ShapedGlyph::new(
            GlyphId(u32::from(self.id)),
            self.cluster,
            i32::from(self.advance >> 1),
            before,
        )
    }
}

// Some of a layout's glyphs, as it keeps them.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum RunGlyphs<'a> {
    Packed(&'a [PackedGlyph]),
    Full(&'a [ShapedGlyph]),
}

impl<'a> RunGlyphs<'a> {
    pub(crate) fn len(self) -> usize {
        match self {
            Self::Packed(glyphs) => glyphs.len(),
            Self::Full(glyphs) => glyphs.len(),
        }
    }

    pub(crate) fn get(self, index: usize) -> Option<ShapedGlyph> {
        match self {
            Self::Packed(glyphs) => glyphs.get(index).map(|glyph| glyph.unpack()),
            Self::Full(glyphs) => glyphs.get(index).copied(),
        }
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = ShapedGlyph> + 'a {
        (0..self.len()).filter_map(move |index| self.get(index))
    }

    fn slice(self, range: Range<usize>) -> Self {
        match self {
            Self::Packed(glyphs) => Self::Packed(&glyphs[range]),
            Self::Full(glyphs) => Self::Full(&glyphs[range]),
        }
    }
}

impl LaidGlyphs {
    // Keeps `shaped`, in eight bytes a glyph when every one fits them.
    pub(crate) fn new(shaped: &ShapedGlyphs) -> Self {
        let offsets = shaped.offsets();
        let glyphs = match u32::try_from(shaped.len()) {
            // Collected from an iterator of known length, in one allocation.
            Ok(len) if shaped.iter().all(PackedGlyph::fits) => Glyphs::Packed {
                room: shaped.iter().map(PackedGlyph::pack).collect(),
                len,
            },
            _ => Glyphs::Full(Arc::from(&shaped[..])),
        };
        Self {
            glyphs,
            offsets: offsets
                .iter()
                .any(|offset| *offset != GlyphOffset::default())
                .then(|| Arc::from(offsets)),
        }
    }

    // Keeps `shaped` in the room of `spare` when nothing else holds that,
    // the room takes the glyphs and every glyph packs and draws at its pen,
    // as a counter's text keeps its room from one number to the next.
    pub(crate) fn new_in(shaped: &ShapedGlyphs, spare: Self) -> Self {
        if let Glyphs::Packed { mut room, .. } = spare.glyphs
            && takes_room(room.len(), shaped.len())
            && let Ok(len) = u32::try_from(shaped.len())
            && shaped
                .offsets()
                .iter()
                .all(|offset| *offset == GlyphOffset::default())
            && shaped.iter().all(PackedGlyph::fits)
            && let Some(slots) = Arc::get_mut(&mut room)
        {
            for (slot, glyph) in slots.iter_mut().zip(shaped.iter()) {
                *slot = PackedGlyph::pack(glyph);
            }
            return Self {
                glyphs: Glyphs::Packed { room, len },
                offsets: None,
            };
        }
        Self::new(shaped)
    }

    pub(crate) fn len(&self) -> usize {
        self.all().len()
    }

    // The glyphs of `range`.
    pub(crate) fn run(&self, range: Range<usize>) -> RunGlyphs<'_> {
        self.all().slice(range)
    }

    fn all(&self) -> RunGlyphs<'_> {
        match &self.glyphs {
            Glyphs::Packed { room, len } => RunGlyphs::Packed(&room[..widen(*len)]),
            Glyphs::Full(glyphs) => RunGlyphs::Full(glyphs),
        }
    }

    // Whether a glyph draws off its pen.
    pub(crate) fn has_offsets(&self) -> bool {
        self.offsets.is_some()
    }

    // Where the first glyphs of `range` draw from the pen, when any glyph
    // draws off it; the glyphs past them draw at the pen.
    pub(crate) fn offsets(&self, range: Range<usize>) -> Option<&[GlyphOffset]> {
        self.offsets.as_deref().map(|offsets| {
            let len = offsets.len();
            &offsets[range.start.min(len)..range.end.min(len)]
        })
    }

    // Appends to `out` those of the glyphs of `range`, from its start,
    // whose clusters fall in `text`, and returns where the rest start.
    pub(crate) fn take(
        &self,
        range: Range<usize>,
        text: &Range<usize>,
        out: &mut ShapedGlyphs,
    ) -> usize {
        let start = range.start;
        // Read in place, so each glyph is unpacked only to be appended.
        let taken = match self.run(range) {
            RunGlyphs::Packed(glyphs) => glyphs
                .iter()
                .take_while(|glyph| text.contains(&glyph.text_offset()))
                .count(),
            RunGlyphs::Full(glyphs) => glyphs
                .iter()
                .take_while(|glyph| text.contains(&glyph.text_offset()))
                .count(),
        };
        let end = start + taken;
        let offsets = self.offsets(start..end).unwrap_or_default();
        match self.run(start..end) {
            RunGlyphs::Packed(glyphs) => {
                out.extend(glyphs.iter().map(|glyph| glyph.unpack()), offsets);
            }
            RunGlyphs::Full(glyphs) => out.extend_from_slice(glyphs, offsets),
        }
        end
    }
}
