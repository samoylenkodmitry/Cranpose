use read_fonts::{
    tables::gpos::{PairPos, ValueFormat},
    types::GlyphId,
};

// The pair adjustments plain glyphs take from the glyph after them: the
// subtables of a plan's pair adjustment lookups that move only the first
// glyph's advance, as HarfBuzz applies them. A pair reads the few words of
// the font it needs where the plan found each part: on a small device the
// table readers' checks of a whole subtable for each pair cost more than
// the reads.
#[derive(Default, PartialEq)]
pub(crate) struct PlainKerning {
    subtables: Box<[KernSubtable]>,
}

// A pair adjustment subtable that moves only the first glyph's advance, by
// where its parts start in the font's bytes.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct KernSubtable {
    // The coverage of the first glyphs.
    coverage: u32,
    pairs: Pairs,
    // The index of its lookup in the font's GPOS lookups.
    lookup: u16,
}

// Where a subtable keeps its adjustments, each the one word that moves the
// first glyph's advance.
#[derive(Clone, Copy, PartialEq)]
enum Pairs {
    Glyphs(PairSets),
    Classes(ClassMatrix),
}

// Format 1: a set of the glyphs after it for each covered first glyph, at
// the `offsets` from the subtable's start, by coverage index.
#[derive(Clone, Copy, PartialEq)]
struct PairSets {
    subtable: u32,
    offsets: u32,
    count: u16,
}

// Format 2: the classes of the first and second glyphs, their counts, and
// the matrix of the adjustment of each pair of classes.
#[derive(Clone, Copy, PartialEq)]
struct ClassMatrix {
    first: u32,
    second: u32,
    counts: [u16; 2],
    cells: u32,
}

// The bytes of a record of a pair set: the second glyph, then its
// adjustment.
const PAIR_RECORD: usize = 4;
// The bytes of a range record of a coverage or a class definition: its
// first and last glyph, then the coverage index of the first or the class.
const RANGE_RECORD: usize = 6;

impl KernSubtable {
    // `pair`, a subtable of the lookup at `lookup` applied to plain glyphs,
    // when it moves only the first glyph's advance; `font` holds it.
    pub(crate) fn of(pair: &PairPos<'_>, lookup: u16, font: &[u8]) -> Option<Self> {
        let (formats, data, coverage) = match pair {
            PairPos::Format1(format) => (
                [format.value_format1(), format.value_format2()],
                format.offset_data(),
                format.coverage_offset(),
            ),
            PairPos::Format2(format) => (
                [format.value_format1(), format.value_format2()],
                format.offset_data(),
                format.coverage_offset(),
            ),
            _ => return None,
        };
        if formats != [ValueFormat::X_ADVANCE, ValueFormat::empty()] {
            return None;
        }
        let start = data
            .as_bytes()
            .as_ptr()
            .addr()
            .checked_sub(font.as_ptr().addr())?;
        let at = |offset: usize| u32::try_from(start.checked_add(offset)?).ok();
        let pairs = match pair {
            PairPos::Format1(format) => Pairs::Glyphs(PairSets {
                subtable: at(0)?,
                offsets: at(format.pair_set_offsets_byte_range().start)?,
                count: format.pair_set_count(),
            }),
            PairPos::Format2(format) => Pairs::Classes(ClassMatrix {
                first: at(index(format.class_def1_offset().to_u32())?)?,
                second: at(index(format.class_def2_offset().to_u32())?)?,
                counts: [format.class1_count(), format.class2_count()],
                cells: at(format.class1_records_byte_range().start)?,
            }),
            _ => return None,
        };
        Some(Self {
            coverage: at(index(coverage.to_u32())?)?,
            pairs,
            lookup,
        })
    }

    // Whether the subtable may move `first`'s advance before some glyph.
    fn starts(self, font: &[u8], first: u16) -> bool {
        index(self.coverage).is_some_and(|at| covered(font, at, first).is_some())
    }

    // How far the subtable moves `first`'s advance before `second`; `None`
    // when it does not apply to the pair, so the lookup's next one is tried.
    fn adjustment(self, font: &[u8], first: u16, second: u16) -> Option<i16> {
        let covering = covered(font, index(self.coverage)?, first)?;
        let cell = match self.pairs {
            Pairs::Glyphs(sets) => sets.cell(font, covering, second)?,
            Pairs::Classes(matrix) => matrix.cell(font, first, second)?,
        };
        Some(word(font, cell)?.cast_signed())
    }
}

impl PairSets {
    // Where the adjustment of the first glyph at `covering` before `second`
    // sits: format 1 applies only to the pairs it lists.
    fn cell(self, font: &[u8], covering: u16, second: u16) -> Option<usize> {
        if covering >= self.count {
            return None;
        }
        let set = word(font, index(self.offsets)? + 2 * usize::from(covering))?;
        let set = index(self.subtable)? + usize::from(set);
        let record = last_at_most(font, set + 2, word(font, set)?, PAIR_RECORD, second)?;
        (word(font, record)? == second).then_some(record + 2)
    }
}

impl ClassMatrix {
    // Where the adjustment of `first` before `second` sits: format 2 applies
    // to every pair its coverage starts, unless a class falls outside its
    // matrix.
    fn cell(self, font: &[u8], first: u16, second: u16) -> Option<usize> {
        let [count1, count2] = self.counts;
        let class1 = class(font, index(self.first)?, first)?;
        let class2 = class(font, index(self.second)?, second)?;
        if class1 >= count1 || class2 >= count2 {
            return None;
        }
        let cell = usize::from(class1) * usize::from(count2) + usize::from(class2);
        index(self.cells)?.checked_add(cell.checked_mul(2)?)
    }
}

// The coverage index of `glyph` in the coverage table at `at`.
fn covered(font: &[u8], at: usize, glyph: u16) -> Option<u16> {
    let count = word(font, at + 2)?;
    match word(font, at)? {
        1 => {
            let record = last_at_most(font, at + 4, count, 2, glyph)?;
            if word(font, record)? != glyph {
                return None;
            }
            u16::try_from((record - at - 4) / 2).ok()
        }
        2 => {
            let record = last_at_most(font, at + 4, count, RANGE_RECORD, glyph)?;
            if glyph > word(font, record + 2)? {
                return None;
            }
            word(font, record + 4)?.checked_add(glyph - word(font, record)?)
        }
        _ => None,
    }
}

// The class of `glyph` in the class definition at `at`; class 0 holds the
// glyphs it puts in no class. `None` for a format it does not have.
fn class(font: &[u8], at: usize, glyph: u16) -> Option<u16> {
    let ranged = match word(font, at)? {
        1 => {
            let start = word(font, at + 2)?;
            let count = word(font, at + 4)?;
            glyph
                .checked_sub(start)
                .filter(|&index| index < count)
                .map(|index| at + 6 + 2 * usize::from(index))
        }
        2 => last_at_most(font, at + 4, word(font, at + 2)?, RANGE_RECORD, glyph)
            .filter(|&record| word(font, record + 2).is_some_and(|last| glyph <= last))
            .map(|record| record + 4),
        _ => return None,
    };
    Some(ranged.and_then(|at| word(font, at)).unwrap_or(0))
}

// Where the last of the `count` records of `size` bytes from `at` starts
// that starts at `glyph` or before it; the records are sorted by the glyph
// each starts with.
fn last_at_most(font: &[u8], at: usize, count: u16, size: usize, glyph: u16) -> Option<usize> {
    let (mut low, mut high) = (0, usize::from(count));
    while low < high {
        let middle = low + (high - low) / 2;
        if word(font, at + middle * size)? <= glyph {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    Some(at + low.checked_sub(1)? * size)
}

// The big-endian word at `at` in `font`.
fn word(font: &[u8], at: usize) -> Option<u16> {
    let bytes = font.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes(bytes.try_into().ok()?))
}

fn index(offset: u32) -> Option<usize> {
    usize::try_from(offset).ok()
}

impl PlainKerning {
    // Kerning by `subtables`, in the order HarfBuzz tries them.
    pub(crate) fn new(subtables: Vec<KernSubtable>) -> Self {
        Self {
            subtables: subtables.into_boxed_slice(),
        }
    }

    // Whether the font may move `first`'s advance before some glyph. Out of
    // line, as is `between`: a text's characters and pairs mostly come from
    // the plain caches, and their reads would grow every caller. A font
    // names at most 65,536 glyphs, so a larger id kerns with nothing.
    #[inline(never)]
    pub(crate) fn starts(&self, font: &[u8], first: GlyphId) -> bool {
        u16::try_from(first.to_u32()).is_ok_and(|first| {
            self.subtables
                .iter()
                .any(|subtable| subtable.starts(font, first))
        })
    }

    // How far the font moves `first`'s advance before `second`, in font
    // units: each lookup adds the adjustment of its first subtable that
    // applies to the pair.
    #[inline(never)]
    pub(crate) fn between(&self, font: &[u8], first: GlyphId, second: GlyphId) -> i32 {
        let (Ok(first), Ok(second)) = (
            u16::try_from(first.to_u32()),
            u16::try_from(second.to_u32()),
        ) else {
            return 0;
        };
        let mut total = 0;
        // The lookup whose subtable applied: its later ones are not tried.
        let mut applied = None;
        for subtable in &self.subtables {
            if applied == Some(subtable.lookup) {
                continue;
            }
            if let Some(advance) = subtable.adjustment(font, first, second) {
                total += i32::from(advance);
                applied = Some(subtable.lookup);
            }
        }
        total
    }
}
