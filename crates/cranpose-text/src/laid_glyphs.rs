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

// The glyphs of a room that any shorter text takes, 128 bytes: a text that
// changes length keeps one room, and so little slack costs less than a new
// room.
const SMALL_ROOM: usize = 16;

// Whether glyphs `count` long take a room `room` long: one at least as
// long, and at most twice as long or a small room, so a short text keeps no
// long text's room.
fn takes_room(room: usize, count: usize) -> bool {
    count <= room && (room <= count.saturating_mul(2) || room <= SMALL_ROOM)
}

// The room a kept layout takes when its text outgrew the room it had: a
// small room, or half as long again as its glyphs, so a text that grows a
// little at a time takes a new room once in a while, not each frame.
fn grown_room(count: usize) -> usize {
    count.saturating_add(count / 2).max(SMALL_ROOM)
}

// `shaped` packed into a room of `room` glyphs, at least as many as it has,
// collected from iterators of known length in one allocation.
fn packed_room(shaped: &ShapedGlyphs, room: usize) -> Arc<[PackedGlyph]> {
    let packed = shaped.iter().map(PackedGlyph::pack);
    match room.saturating_sub(shaped.len()) {
        0 => packed.collect(),
        spare => packed
            .chain(std::iter::repeat_n(PackedGlyph::EMPTY, spare))
            .collect(),
    }
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
    // What a room holds past its glyphs.
    const EMPTY: Self = Self {
        id: 0,
        advance: 0,
        cluster: 0,
    };

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
        Self::in_room(shaped, shaped.len())
    }

    // Keeps `shaped` as `new` does, packed into a room of `room` glyphs, at
    // least as many as it has.
    fn in_room(shaped: &ShapedGlyphs, room: usize) -> Self {
        let offsets = shaped.offsets();
        let glyphs = match u32::try_from(shaped.len()) {
            Ok(len) if shaped.iter().all(PackedGlyph::fits) => Glyphs::Packed {
                room: packed_room(shaped, room),
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
    // as a counter's text keeps its room from one number to the next. A
    // text that outgrew the room takes a new one with room to grow.
    pub(crate) fn new_in(shaped: &ShapedGlyphs, spare: Self) -> Self {
        let Glyphs::Packed { mut room, .. } = spare.glyphs else {
            return Self::new(shaped);
        };
        if takes_room(room.len(), shaped.len())
            && let Ok(len) = u32::try_from(shaped.len())
            && shaped
                .offsets()
                .iter()
                .all(|offset| *offset == GlyphOffset::default())
            && let Some(slots) = Arc::get_mut(&mut room)
        {
            // One pass packs the glyphs and checks them: glyphs that do not
            // all fit leave the room, which costs no allocation, for new
            // storage of their own.
            let mut fits = true;
            for (slot, glyph) in slots[..shaped.len()].iter_mut().zip(shaped.iter()) {
                fits &= PackedGlyph::fits(glyph);
                *slot = PackedGlyph::pack(glyph);
            }
            if fits {
                return Self {
                    glyphs: Glyphs::Packed { room, len },
                    offsets: None,
                };
            }
        }
        if room.is_empty() || shaped.len() <= room.len() {
            return Self::new(shaped);
        }
        Self::in_room(shaped, grown_room(shaped.len()))
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
