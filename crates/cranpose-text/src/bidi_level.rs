use icu_properties::{CodePointMapData, props::BidiClass};
use unicode_bidi::{Level, ParagraphBidiInfo};

/// The embedding level the Unicode bidirectional algorithm gives text: even
/// levels run left to right, odd levels right to left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BidiLevel(u8);

impl BidiLevel {
    /// The level of a left-to-right paragraph.
    pub const LTR: Self = Self(0);
    /// The level of a right-to-left paragraph.
    pub const RTL: Self = Self(1);

    /// The level as a number, at most 126.
    pub fn value(self) -> u8 {
        self.0
    }

    /// Whether text at this level runs right to left.
    pub fn is_rtl(self) -> bool {
        self.0 & 1 == 1
    }
}

// Pushes the level runs of `paragraph`, whose base level is `base`, onto
// `runs`, each as the end of its byte range offset by `offset` and its level.
// A left-to-right paragraph without a right-to-left, Arabic-number or explicit
// directional character is one run at level 0, as the algorithm would find.
pub(crate) fn level_runs(
    paragraph: &str,
    base: BidiLevel,
    offset: usize,
    runs: &mut Vec<(usize, BidiLevel)>,
) {
    let classes = CodePointMapData::<BidiClass>::new();
    if base == BidiLevel::LTR
        && (paragraph.is_ascii() || !paragraph.chars().any(|ch| turns(classes.get(ch))))
    {
        runs.push((offset + paragraph.len(), base));
        return;
    }
    let base = if base.is_rtl() {
        Level::rtl()
    } else {
        Level::ltr()
    };
    let info = ParagraphBidiInfo::new_with_data_source(&classes, paragraph, Some(base));
    let mut current: Option<Level> = None;
    for (at, &level) in info.levels.iter().enumerate() {
        if let Some(previous) = current
            && previous != level
        {
            runs.push((offset + at, BidiLevel(previous.number())));
        }
        current = Some(level);
    }
    if let Some(level) = current {
        runs.push((offset + paragraph.len(), BidiLevel(level.number())));
    }
}

// Puts `runs` of one line, in text order, in the order they are drawn from
// left to right, by rule L2 of UAX #9: from the highest level down to the
// lowest odd one, every sequence of runs at that level or higher reverses.
pub(crate) fn reorder_visually<T>(runs: &mut [T], level: impl Fn(&T) -> BidiLevel) {
    let Some(highest) = runs.iter().map(|run| level(run).0).max() else {
        return;
    };
    let lowest = runs.iter().map(|run| level(run).0).min().unwrap_or(0) | 1;
    for current in (lowest..=highest).rev() {
        let mut start = 0;
        while start < runs.len() {
            if level(&runs[start]).0 < current {
                start += 1;
                continue;
            }
            let mut end = start;
            while end < runs.len() && level(&runs[end]).0 >= current {
                end += 1;
            }
            runs[start..end].reverse();
            start = end;
        }
    }
}

// Whether a character of `class` can give text in a left-to-right paragraph a
// level other than 0.
fn turns(class: BidiClass) -> bool {
    matches!(
        class,
        BidiClass::RightToLeft
            | BidiClass::ArabicLetter
            | BidiClass::ArabicNumber
            | BidiClass::RightToLeftEmbedding
            | BidiClass::RightToLeftOverride
            | BidiClass::RightToLeftIsolate
            | BidiClass::FirstStrongIsolate
            | BidiClass::LeftToRightEmbedding
            | BidiClass::LeftToRightOverride
            | BidiClass::LeftToRightIsolate
            | BidiClass::PopDirectionalFormat
            | BidiClass::PopDirectionalIsolate
    )
}
