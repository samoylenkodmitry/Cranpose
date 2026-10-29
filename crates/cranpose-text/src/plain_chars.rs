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

// A plain character's glyph and advance, and its slot when it has one.
#[derive(Clone, Copy)]
pub(crate) struct PlainChar {
    pub(crate) glyph: u32,
    pub(crate) advance: i32,
    // `None` for a glyph or an advance a slot cannot hold.
    slot: Option<usize>,
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
// A filled slot; with `PLAIN` its character is plain.
const FILLED: u64 = 1 << 63;
const PLAIN: u64 = 1 << 62;
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
    // or from `read` the first time.
    pub(crate) fn char(
        &mut self,
        ch: char,
        read: impl FnOnce() -> Option<(u32, i32)>,
    ) -> Option<PlainChar> {
        let code = u64::from(u32::from(ch));
        let slot = usize::try_from(u32::from(ch) & (SLOTS as u32 - 1)).unwrap_or(0);
        let packed = self.slots[slot];
        if packed & FILLED != 0 && (packed >> 32) & 0x1F_FFFF == code {
            return (packed & PLAIN != 0).then(|| unpack(slot, packed));
        }
        let found = read();
        let packed = match found {
            None => FILLED | code << 32,
            Some((glyph, advance)) => {
                let (Ok(id), Ok(units)) = (u16::try_from(glyph), i16::try_from(advance)) else {
                    // Too wide for a slot: read again each time.
                    return Some(PlainChar {
                        glyph,
                        advance,
                        slot: None,
                    });
                };
                FILLED | PLAIN | code << 32 | u64::from(id) << 16 | u64::from(units as u16)
            }
        };
        if self.slots[slot] & FILLED != 0 {
            self.forget(slot);
        }
        self.slots[slot] = packed;
        found.map(|(glyph, advance)| PlainChar {
            glyph,
            advance,
            slot: Some(slot),
        })
    }

    // What stands between the glyphs of `first` and `second`, from their
    // cell, or from `read` the first time.
    pub(crate) fn pair(
        &mut self,
        first: PlainChar,
        second: PlainChar,
        read: impl FnOnce() -> PairFit,
    ) -> PairFit {
        let (Some(first), Some(second)) = (first.slot, second.slot) else {
            return read();
        };
        let row = self.rows[first].get_or_insert_with(|| Box::new([UNREAD; SLOTS]));
        match row[second] {
            JOINS => return PairFit::Joins,
            UNREAD => {}
            kern => return PairFit::Kern(i32::from(kern)),
        }
        let fit = read();
        row[second] = match fit {
            PairFit::Joins => JOINS,
            PairFit::Kern(kern) => match i16::try_from(kern) {
                Ok(kern) if kern != UNREAD && kern != JOINS => kern,
                _ => return fit,
            },
        };
        fit
    }

    // Drops what pairs knew of the character leaving `slot`.
    fn forget(&mut self, slot: usize) {
        self.rows[slot] = None;
        for row in self.rows.iter_mut().flatten() {
            row[slot] = UNREAD;
        }
    }
}

fn unpack(slot: usize, packed: u64) -> PlainChar {
    PlainChar {
        glyph: u32::from((packed >> 16) as u16),
        advance: i32::from(packed as u16 as i16),
        slot: Some(slot),
    }
}
