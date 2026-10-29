use read_fonts::{
    FontData, FontRead, ReadError,
    tables::gpos::{PairPos, PairPosFormat1, PairPosFormat2, ValueFormat},
    types::GlyphId,
};

// The pair adjustments plain glyphs take from the glyph after them: the
// subtables of a plan's pair adjustment lookups that move only the first
// glyph's advance, read from the font's bytes, as HarfBuzz applies them.
#[derive(Default, PartialEq)]
pub(crate) struct PlainKerning {
    subtables: Box<[KernSubtable]>,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct KernSubtable {
    // Where the subtable starts in the font's bytes.
    offset: u32,
    // The index of its lookup in the font's GPOS lookups.
    lookup: u16,
}

impl KernSubtable {
    // `pair`, a subtable of the lookup at `lookup` applied to plain glyphs,
    // when it moves only the first glyph's advance; `font` holds it.
    pub(crate) fn of(pair: &PairPos<'_>, lookup: u16, font: &[u8]) -> Option<Self> {
        let (formats, data) = match pair {
            PairPos::Format1(format) => (
                [format.value_format1(), format.value_format2()],
                format.offset_data(),
            ),
            PairPos::Format2(format) => (
                [format.value_format1(), format.value_format2()],
                format.offset_data(),
            ),
            _ => return None,
        };
        if formats != [ValueFormat::X_ADVANCE, ValueFormat::empty()] {
            return None;
        }
        let offset = data
            .as_bytes()
            .as_ptr()
            .addr()
            .checked_sub(font.as_ptr().addr())?;
        Some(Self {
            offset: u32::try_from(offset).ok()?,
            lookup,
        })
    }

    // Whether the subtable may move `first`'s advance before some glyph.
    fn starts(self, font: &[u8], first: GlyphId) -> bool {
        let Some(data) = usize::try_from(self.offset)
            .ok()
            .and_then(|offset| font.get(offset..))
        else {
            return false;
        };
        let coverage = match PairPos::read(FontData::new(data)) {
            Ok(PairPos::Format1(format)) => format.coverage(),
            Ok(PairPos::Format2(format)) => format.coverage(),
            _ => return false,
        };
        coverage.is_ok_and(|coverage| coverage.get(first).is_some())
    }

    // How far the subtable moves `first`'s advance before `second`; `None`
    // when it does not apply to the pair, so the lookup's next one is tried.
    fn adjustment(self, font: &[u8], first: GlyphId, second: GlyphId) -> Option<i32> {
        let data = FontData::new(font.get(usize::try_from(self.offset).ok()?..)?);
        match PairPos::read(data).ok()? {
            PairPos::Format1(format) => glyph_pair(&format, first, second).ok()?,
            PairPos::Format2(format) => class_pair(&format, first, second).ok()?,
            _ => None,
        }
    }
}

// A pair set's record for `second`: format 1 applies only to the pairs it
// lists.
fn glyph_pair(
    format: &PairPosFormat1<'_>,
    first: GlyphId,
    second: GlyphId,
) -> Result<Option<i32>, ReadError> {
    let Some(index) = format.coverage()?.get(first) else {
        return Ok(None);
    };
    let set = format
        .pair_sets()
        .get(usize::try_from(index).unwrap_or(usize::MAX))?;
    let records = set.pair_value_records();
    let (mut low, mut high) = (0, records.len());
    while low < high {
        let middle = low + (high - low) / 2;
        let record = records.get(middle)?;
        match GlyphId::from(record.second_glyph()).cmp(&second) {
            std::cmp::Ordering::Less => low = middle + 1,
            std::cmp::Ordering::Greater => high = middle,
            std::cmp::Ordering::Equal => {
                return Ok(Some(i32::from(
                    record.value_record1().x_advance().unwrap_or(0),
                )));
            }
        }
    }
    Ok(None)
}

// The cell of the two glyphs' classes: format 2 applies to every pair its
// coverage starts, unless a class falls outside its matrix.
fn class_pair(
    format: &PairPosFormat2<'_>,
    first: GlyphId,
    second: GlyphId,
) -> Result<Option<i32>, ReadError> {
    if format.coverage()?.get(first).is_none() {
        return Ok(None);
    }
    let class1 = format.class_def1()?.get(first);
    let class2 = format.class_def2()?.get(second);
    if class1 >= u32::from(format.class1_count()) || class2 >= u32::from(format.class2_count()) {
        return Ok(None);
    }
    let row = format
        .class1_records()
        .get(usize::try_from(class1).unwrap_or(usize::MAX))?;
    let cell = row
        .class2_records()
        .get(usize::try_from(class2).unwrap_or(usize::MAX))?;
    Ok(Some(i32::from(
        cell.value_record1().x_advance().unwrap_or(0),
    )))
}

impl PlainKerning {
    // Kerning by `subtables`, in the order HarfBuzz tries them.
    pub(crate) fn new(subtables: Vec<KernSubtable>) -> Self {
        Self {
            subtables: subtables.into_boxed_slice(),
        }
    }

    // Whether the font may move `first`'s advance before some glyph.
    pub(crate) fn starts(&self, font: &[u8], first: GlyphId) -> bool {
        self.subtables
            .iter()
            .any(|subtable| subtable.starts(font, first))
    }

    // How far the font moves `first`'s advance before `second`, in font
    // units: each lookup adds the adjustment of its first subtable that
    // applies to the pair.
    pub(crate) fn between(&self, font: &[u8], first: GlyphId, second: GlyphId) -> i32 {
        let mut total = 0;
        // The lookup whose subtable applied: its later ones are not tried.
        let mut applied = None;
        for subtable in &self.subtables {
            if applied == Some(subtable.lookup) {
                continue;
            }
            if let Some(advance) = subtable.adjustment(font, first, second) {
                total += advance;
                applied = Some(subtable.lookup);
            }
        }
        total
    }
}
