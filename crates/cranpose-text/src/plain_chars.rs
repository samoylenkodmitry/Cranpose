// What a plan found for the characters it met, so plain text reads each
// character with one load and each pair of characters with another, as
// main's ASCII table did, for any script: per character its glyph, its
// advance and whether it is plain, direct mapped by the character's low
// bits, so a script block of up to `SLOTS` characters fills without
// collisions; per pair of slots the kerning between their glyphs, or that a
// ligature joins them, in rows made for the characters that precede others.
pub(crate) struct PlainChars {
    slots: Box<[u64; SLOTS]>,
    rows: Box<[Option<Box<[i16; SLOTS]>>; SLOTS]>,
}

// A plain character's glyph, its advance, whether its glyph may kern with
// or join the glyph after it, and its slot.
#[derive(Clone, Copy)]
pub(crate) struct PlainChar {
    pub(crate) glyph: u16,
    pub(crate) advance: i16,
    pub(crate) pairs: bool,
    slot: u8,
}

// What a font reads for a plain character: its glyph, its advance and
// whether its glyph may kern with or join the glyph after it.
pub(crate) struct CharGlyph {
    pub(crate) glyph: u32,
    pub(crate) advance: i32,
    pub(crate) pairs: bool,
}

// What stands between the glyphs of two characters side by side.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PairFit {
    // The kerning the first takes, in font units.
    Kern(i32),
    // A ligature joins them: the pair is not plain.
    Joins,
}

const SLOT_BITS: u32 = 7;
const SLOTS: usize = 1 << SLOT_BITS;
// A slot holds its character in bits 32 to 52, the character's glyph in
// bits 16 to 31 and its advance in the low 16. `FILLED` marks a slot that
// holds a character, `PLAIN` one whose character is plain and `PAIRS` one
// whose glyph may kern with or join the next.
const FILLED: u64 = 1 << 63;
const PLAIN: u64 = 1 << 62;
const PAIRS: u64 = 1 << 61;
// A pair cell not read yet, and one a ligature joins.
const UNREAD: i16 = i16::MIN;
const JOINS: i16 = i16::MIN + 1;

impl Default for PlainChars {
    fn default() -> Self {
        Self {
            slots: Box::new([0; SLOTS]),
            rows: Box::new(std::array::from_fn(|_| None)),
        }
    }
}

impl PlainChars {
    // The glyph of `ch` and its advance when `ch` is plain, from its slot,
    // or from `read` the first time. A glyph or an advance a slot cannot
    // hold is not plain.
    pub(crate) fn char(
        &mut self,
        ch: char,
        read: impl FnOnce() -> Option<CharGlyph>,
    ) -> Option<PlainChar> {
        let code = u32::from(ch);
        let slot = (code & (SLOTS as u32 - 1)) as u8;
        let packed = self.slots[usize::from(slot)];
        let known = (FILLED >> 32) | u64::from(code);
        if (packed >> 32) & !(PAIRS >> 32) == known | (PLAIN >> 32) {
            return Some(PlainChar {
                glyph: (packed >> 16) as u16,
                advance: packed as u16 as i16,
                pairs: packed & PAIRS != 0,
                slot,
            });
        }
        if packed >> 32 == known {
            return None;
        }
        let plain = read().and_then(|read| {
            Some(PlainChar {
                glyph: u16::try_from(read.glyph).ok()?,
                advance: i16::try_from(read.advance).ok()?,
                pairs: read.pairs,
                slot,
            })
        });
        if packed & FILLED != 0 {
            self.forget(slot);
        }
        self.slots[usize::from(slot)] = known << 32
            | plain.map_or(0, |plain| {
                let pairs = if plain.pairs { PAIRS } else { 0 };
                PLAIN | pairs | u64::from(plain.glyph) << 16 | u64::from(plain.advance as u16)
            });
        plain
    }

    // What stands between the glyphs of `first` and `second`, from their
    // cell, or from `read` the first time.
    pub(crate) fn pair(
        &mut self,
        first: PlainChar,
        second: PlainChar,
        read: impl FnOnce() -> PairFit,
    ) -> PairFit {
        let row =
            self.rows[usize::from(first.slot)].get_or_insert_with(|| Box::new([UNREAD; SLOTS]));
        let cell = &mut row[usize::from(second.slot)];
        match *cell {
            JOINS => return PairFit::Joins,
            UNREAD => {}
            kern => return PairFit::Kern(i32::from(kern)),
        }
        let fit = read();
        // A cell holds what stands between the glyphs its slots hold now:
        // `second` took the slot of `first` from it when they share one.
        if first.slot == second.slot && first.glyph != second.glyph {
            return fit;
        }
        *cell = match fit {
            PairFit::Joins => JOINS,
            PairFit::Kern(kern) => match i16::try_from(kern) {
                Ok(kern) if kern != UNREAD && kern != JOINS => kern,
                _ => return fit,
            },
        };
        fit
    }

    // Drops what pairs knew of the character leaving `slot`.
    fn forget(&mut self, slot: u8) {
        self.rows[usize::from(slot)] = None;
        for row in self.rows.iter_mut().flatten() {
            row[usize::from(slot)] = UNREAD;
        }
    }
}
