#[cfg(feature = "text-hyphenation")]
use std::{
    path::Path,
    sync::{Mutex, PoisonError},
};

#[cfg(feature = "text-hyphenation")]
use cranpose_text::{HyphenationError, Hyphenator};
use cranpose_ui::text::TextStyle;

#[cfg(feature = "text-hyphenation")]
const MIN_SEGMENT_CHARS: usize = 2;

/// Hyphenation dictionaries by language. Dictionaries that the app registers are used first, and
/// a dictionary embedded in the build is the fallback when `text-hyphenation-embedded` is on.
#[cfg(feature = "text-hyphenation")]
#[derive(Default)]
pub struct HyphenationDictionaryStore {
    hyphenator: Mutex<Hyphenator>,
}

#[cfg(feature = "text-hyphenation")]
impl HyphenationDictionaryStore {
    /// Creates a store with no registered dictionaries.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the dictionary for `locale` read from the file at `path`, replacing an earlier
    /// one.
    pub fn register_dictionary_path(
        &self,
        locale: &str,
        path: impl AsRef<Path>,
    ) -> Result<(), HyphenationError> {
        self.hyphenator
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .register_dictionary_path(locale, path)
    }

    /// Registers the dictionary for `locale` read from `reader`, replacing an earlier one.
    pub fn register_dictionary_reader(
        &self,
        locale: &str,
        reader: &mut impl std::io::Read,
    ) -> Result<(), HyphenationError> {
        self.hyphenator
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .register_dictionary(locale, reader)
    }

    /// Chooses where to hyphenate the word that contains the break at character
    /// `measured_break_char` of `line`. `segment_start_char` is the character index where the
    /// current line segment starts, and the language comes from the first locale of `style`,
    /// English when it has none. Returns the character index of the break, at least two
    /// characters after the segment start and not after `measured_break_char`, or `None` when
    /// the break is not inside a word or no dictionary is available.
    pub fn choose_auto_hyphen_break(
        &self,
        line: &str,
        style: &TextStyle,
        segment_start_char: usize,
        measured_break_char: usize,
    ) -> Option<usize> {
        let boundaries: Vec<usize> = line
            .char_indices()
            .map(|(at, _)| at)
            .chain([line.len()])
            .collect();
        if measured_break_char <= segment_start_char
            || measured_break_char + 1 >= boundaries.len()
            || !is_break_inside_word(line, &boundaries, measured_break_char)
        {
            return None;
        }
        let (word_start, word_end) = word_bounds(line, &boundaries, measured_break_char);
        let word = line.get(boundaries[word_start]..boundaries[word_end])?;
        let locale = style
            .span_style
            .locale_list
            .as_ref()
            .and_then(|locales| locales.locales().first())
            .map(String::as_str);
        let mut points = Vec::new();
        self.hyphenator
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .break_points(locale, word, &mut points);
        let max_local_break = measured_break_char - word_start;
        let min_local_break = segment_start_char
            .saturating_sub(word_start)
            .saturating_add(MIN_SEGMENT_CHARS);
        points.iter().rev().find_map(|&point| {
            let local_break = word.get(..point)?.chars().count();
            (min_local_break..=max_local_break)
                .contains(&local_break)
                .then_some(word_start + local_break)
        })
    }
}

/// Stands in for the dictionary store when the `text-hyphenation` feature is off. It never finds
/// a break.
#[cfg(not(feature = "text-hyphenation"))]
#[derive(Default)]
pub struct HyphenationDictionaryStore;

#[cfg(not(feature = "text-hyphenation"))]
impl HyphenationDictionaryStore {
    /// Creates the store.
    pub fn new() -> Self {
        Self
    }

    /// Always returns `None`, because hyphenation is not built in.
    pub fn choose_auto_hyphen_break(
        &self,
        line: &str,
        _style: &TextStyle,
        segment_start_char: usize,
        measured_break_char: usize,
    ) -> Option<usize> {
        let _ = (self, line, segment_start_char, measured_break_char);
        None
    }
}

/// Chooses a hyphenation break with a store that holds no registered dictionaries, so only a
/// dictionary embedded in the build can be used. See
/// `HyphenationDictionaryStore::choose_auto_hyphen_break`.
pub fn choose_auto_hyphen_break(
    line: &str,
    style: &TextStyle,
    segment_start_char: usize,
    measured_break_char: usize,
) -> Option<usize> {
    HyphenationDictionaryStore::new().choose_auto_hyphen_break(
        line,
        style,
        segment_start_char,
        measured_break_char,
    )
}

// Whether the characters on both sides of character `break_idx` are not
// whitespace.
#[cfg(feature = "text-hyphenation")]
fn is_break_inside_word(line: &str, boundaries: &[usize], break_idx: usize) -> bool {
    !is_space(line, boundaries, break_idx - 1) && !is_space(line, boundaries, break_idx)
}

// The characters around character `anchor` up to the whitespace on each side.
#[cfg(feature = "text-hyphenation")]
fn word_bounds(line: &str, boundaries: &[usize], anchor: usize) -> (usize, usize) {
    let mut start = anchor;
    while start > 0 && !is_space(line, boundaries, start - 1) {
        start -= 1;
    }
    let mut end = anchor;
    while end + 1 < boundaries.len() && !is_space(line, boundaries, end) {
        end += 1;
    }
    (start, end)
}

#[cfg(feature = "text-hyphenation")]
fn is_space(line: &str, boundaries: &[usize], index: usize) -> bool {
    line.get(boundaries[index]..boundaries[index + 1])
        .is_some_and(|part| part.chars().all(char::is_whitespace))
}

#[cfg(all(test, not(feature = "text-hyphenation")))]
#[path = "tests/text_hyphenation_disabled_tests.rs"]
mod disabled_tests;

#[cfg(all(test, feature = "text-hyphenation-embedded"))]
#[path = "tests/text_hyphenation_tests.rs"]
mod tests;
