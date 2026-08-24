#[cfg(feature = "text-hyphenation")]
use std::collections::HashMap;
#[cfg(feature = "text-hyphenation")]
use std::path::Path;
#[cfg(feature = "text-hyphenation")]
use std::sync::RwLock;

use cranpose_ui::text::TextStyle;
#[cfg(feature = "text-hyphenation")]
use hyphenation::{Hyphenator, Language, Load, Standard};

#[cfg(feature = "text-hyphenation")]
const MIN_SEGMENT_CHARS: usize = 2;

#[cfg(feature = "text-hyphenation")]
#[derive(thiserror::Error, Debug)]
pub enum HyphenationDictionaryError {
    #[error("Unsupported hyphenation locale: {0}")]
    UnsupportedLocale(String),
    #[error("Failed to load hyphenation dictionary for {locale}: {message}")]
    LoadFailed { locale: String, message: String },
    #[error("Hyphenation dictionary cache is unavailable")]
    CacheUnavailable,
}

#[cfg(feature = "text-hyphenation")]
pub struct HyphenationDictionaryStore {
    dictionaries: RwLock<HashMap<Language, Standard>>,
}

#[cfg(feature = "text-hyphenation")]
impl Default for HyphenationDictionaryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "text-hyphenation")]
impl HyphenationDictionaryStore {
    pub fn new() -> Self {
        Self {
            dictionaries: RwLock::new(HashMap::new()),
        }
    }

    pub fn register_dictionary_path(
        &self,
        locale: &str,
        path: impl AsRef<Path>,
    ) -> Result<(), HyphenationDictionaryError> {
        let language = resolve_language_tag(locale)
            .ok_or_else(|| HyphenationDictionaryError::UnsupportedLocale(locale.to_string()))?;
        let dictionary = Standard::from_path(language, path).map_err(|err| {
            HyphenationDictionaryError::LoadFailed {
                locale: locale.to_string(),
                message: err.to_string(),
            }
        })?;
        self.store_dictionary(language, dictionary)
    }

    pub fn register_dictionary_reader(
        &self,
        locale: &str,
        reader: &mut impl std::io::Read,
    ) -> Result<(), HyphenationDictionaryError> {
        let language = resolve_language_tag(locale)
            .ok_or_else(|| HyphenationDictionaryError::UnsupportedLocale(locale.to_string()))?;
        let dictionary = Standard::from_reader(language, reader).map_err(|err| {
            HyphenationDictionaryError::LoadFailed {
                locale: locale.to_string(),
                message: err.to_string(),
            }
        })?;
        self.store_dictionary(language, dictionary)
    }

    fn store_dictionary(
        &self,
        language: Language,
        dictionary: Standard,
    ) -> Result<(), HyphenationDictionaryError> {
        let mut write_guard = self
            .dictionaries
            .write()
            .map_err(|_| HyphenationDictionaryError::CacheUnavailable)?;
        write_guard.insert(language, dictionary);
        Ok(())
    }

    fn get_dictionary(&self, language: Language) -> Option<Standard> {
        if let Ok(read_guard) = self.dictionaries.read() {
            if let Some(dict) = read_guard.get(&language) {
                return Some(dict.clone());
            }
        }

        #[cfg(feature = "text-hyphenation-embedded")]
        {
            if let Ok(dict) = Standard::from_embedded(language) {
                let _ = self.store_dictionary(language, dict.clone());
                return Some(dict);
            }
        }

        None
    }

    pub fn choose_auto_hyphen_break(
        &self,
        line: &str,
        style: &TextStyle,
        segment_start_char: usize,
        measured_break_char: usize,
    ) -> Option<usize> {
        if line.is_empty() || measured_break_char <= segment_start_char {
            return None;
        }

        let language = resolve_hyphenation_language(style)?;

        let dictionary = self.get_dictionary(language)?;
        let boundaries = char_boundaries(line);
        let char_count = boundaries.len().saturating_sub(1);

        if measured_break_char == 0 || measured_break_char >= char_count {
            return None;
        }
        if !is_break_inside_word(line, &boundaries, measured_break_char) {
            return None;
        }

        let (word_start, word_end) = word_bounds(line, &boundaries, measured_break_char);
        let word = &line[boundaries[word_start]..boundaries[word_end]];
        if word.is_empty() {
            return None;
        }

        let max_local_break = measured_break_char.saturating_sub(word_start);
        let min_local_break = segment_start_char
            .saturating_sub(word_start)
            .saturating_add(MIN_SEGMENT_CHARS);

        if min_local_break > max_local_break {
            return None;
        }

        let hyphenated = dictionary.hyphenate(word);
        for break_byte in hyphenated.breaks.into_iter().rev() {
            if !word.is_char_boundary(break_byte) {
                continue;
            }
            let local_break_chars = word[..break_byte].chars().count();
            if local_break_chars < min_local_break || local_break_chars > max_local_break {
                continue;
            }
            return Some(word_start + local_break_chars);
        }

        None
    }
}

#[cfg(not(feature = "text-hyphenation"))]
#[derive(Default)]
pub struct HyphenationDictionaryStore;

#[cfg(not(feature = "text-hyphenation"))]
impl HyphenationDictionaryStore {
    pub fn new() -> Self {
        Self
    }

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

#[cfg(feature = "text-hyphenation")]
fn resolve_hyphenation_language(style: &TextStyle) -> Option<Language> {
    let Some(locale_list) = style.span_style.locale_list.as_ref() else {
        return Some(Language::EnglishUS);
    };
    if locale_list.is_empty() {
        return Some(Language::EnglishUS);
    }

    let primary_locale = locale_list.locales().first()?;
    resolve_language_tag(primary_locale)
}

#[cfg(feature = "text-hyphenation")]
fn resolve_language_tag(locale: &str) -> Option<Language> {
    if locale.trim().is_empty() {
        return Some(Language::EnglishUS);
    }

    let normalized = locale.trim().replace('_', "-").to_ascii_lowercase();

    if normalized.starts_with("en-gb") {
        return Some(Language::EnglishGB);
    }
    if normalized.starts_with("en") || normalized == "und" {
        return Some(Language::EnglishUS);
    }
    if normalized.starts_with("fr") {
        return Some(Language::French);
    }
    if normalized.starts_with("de") {
        return Some(Language::German1996);
    }
    if normalized.starts_with("es") {
        return Some(Language::Spanish);
    }
    if normalized.starts_with("it") {
        return Some(Language::Italian);
    }
    if normalized.starts_with("ru") {
        return Some(Language::Russian);
    }
    if normalized.starts_with("pt") {
        return Some(Language::Portuguese);
    }
    if normalized.starts_with("nl") {
        return Some(Language::Dutch);
    }
    if normalized.starts_with("pl") {
        return Some(Language::Polish);
    }
    if normalized.starts_with("sv") {
        return Some(Language::Swedish);
    }
    if normalized.starts_with("da") {
        return Some(Language::Danish);
    }
    if normalized.starts_with("cs") {
        return Some(Language::Czech);
    }
    if normalized.starts_with("sk") {
        return Some(Language::Slovak);
    }
    if normalized.starts_with("uk") {
        return Some(Language::Ukrainian);
    }

    None
}

#[cfg(feature = "text-hyphenation")]
fn char_boundaries(text: &str) -> Vec<usize> {
    let mut out = Vec::with_capacity(text.chars().count() + 1);
    out.push(0);
    for (idx, _) in text.char_indices() {
        if idx != 0 {
            out.push(idx);
        }
    }
    out.push(text.len());
    out
}

#[cfg(feature = "text-hyphenation")]
fn is_break_inside_word(line: &str, boundaries: &[usize], break_idx: usize) -> bool {
    if break_idx == 0 || break_idx >= boundaries.len() - 1 {
        return false;
    }
    let prev = &line[boundaries[break_idx - 1]..boundaries[break_idx]];
    let next = &line[boundaries[break_idx]..boundaries[break_idx + 1]];
    !prev.chars().all(char::is_whitespace) && !next.chars().all(char::is_whitespace)
}

#[cfg(feature = "text-hyphenation")]
fn word_bounds(line: &str, boundaries: &[usize], anchor: usize) -> (usize, usize) {
    let mut start = anchor;
    while start > 0 {
        let prev = &line[boundaries[start - 1]..boundaries[start]];
        if prev.chars().all(char::is_whitespace) {
            break;
        }
        start -= 1;
    }

    let mut end = anchor;
    while end < boundaries.len() - 1 {
        let current = &line[boundaries[end]..boundaries[end + 1]];
        if current.chars().all(char::is_whitespace) {
            break;
        }
        end += 1;
    }
    (start, end)
}

#[cfg(all(test, not(feature = "text-hyphenation")))]
mod disabled_tests {
    use super::*;

    #[test]
    fn auto_hyphenation_without_dictionary_feature_returns_none() {
        let break_idx = choose_auto_hyphen_break("Transformation", &TextStyle::default(), 8, 12);
        assert_eq!(break_idx, None);
    }
}

#[cfg(all(test, feature = "text-hyphenation-embedded"))]
mod tests {
    use cranpose_ui::text::{LocaleList, SpanStyle, TextStyle};

    use super::*;

    fn style_with_locale(tags: &str) -> TextStyle {
        TextStyle {
            span_style: SpanStyle {
                locale_list: Some(LocaleList::from_language_tags(tags)),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn dictionary_breaks_transformation_like_compose_contract() {
        let break_idx = choose_auto_hyphen_break("Transformation", &TextStyle::default(), 8, 12);
        assert_eq!(break_idx, Some(10));
    }

    #[test]
    fn locale_gate_uses_french_dictionary() {
        let break_idx = choose_auto_hyphen_break("éléphant", &style_with_locale("fr-FR"), 0, 7);
        assert_eq!(break_idx, Some(3));
    }

    #[test]
    fn locale_gate_uses_german_dictionary() {
        let break_idx = choose_auto_hyphen_break(
            "Geschwindigkeitsbegrenzung",
            &style_with_locale("de-DE"),
            10,
            20,
        );
        assert!(break_idx.is_some());
    }

    #[test]
    fn unknown_locale_disables_hyphenation() {
        let break_idx =
            choose_auto_hyphen_break("Transformation", &style_with_locale("ja-JP"), 8, 12);
        assert_eq!(break_idx, None);
    }

    #[test]
    fn dictionary_uses_english_locale_alias() {
        let break_idx =
            choose_auto_hyphen_break("Transformation", &style_with_locale("en_GB"), 8, 12);
        assert_eq!(break_idx, Some(10));
    }

    #[test]
    fn ignores_breaks_outside_words() {
        let break_idx = choose_auto_hyphen_break("ab cd", &TextStyle::default(), 0, 2);
        assert_eq!(break_idx, None);
    }
}
