use cranpose_ui::text::TextStyle;
use hyphenation::{Hyphenator, Language, Load, Standard};
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

const MIN_SEGMENT_CHARS: usize = 2;

fn get_dictionary(language: Language) -> Option<Standard> {
    static DICTIONARIES: OnceLock<RwLock<HashMap<Language, Standard>>> = OnceLock::new();
    let cache = DICTIONARIES.get_or_init(|| RwLock::new(HashMap::new()));

    if let Ok(read_guard) = cache.read() {
        if let Some(dict) = read_guard.get(&language) {
            return Some(dict.clone());
        }
    }

    // Load if not in cache
    match Standard::from_embedded(language) {
        Ok(dict) => {
            if let Ok(mut write_guard) = cache.write() {
                write_guard.insert(language, dict.clone());
            }
            Some(dict)
        }
        Err(_) => None,
    }
}

pub fn choose_auto_hyphen_break(
    line: &str,
    style: &TextStyle,
    segment_start_char: usize,
    measured_break_char: usize,
) -> Option<usize> {
    if line.is_empty() || measured_break_char <= segment_start_char {
        return None;
    }

    let language = resolve_hyphenation_language(style)?;

    let dictionary = get_dictionary(language)?;
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

fn resolve_hyphenation_language(style: &TextStyle) -> Option<Language> {
    let Some(locale_list) = style.span_style.locale_list.as_ref() else {
        return Some(Language::EnglishUS);
    };
    if locale_list.is_empty() {
        return Some(Language::EnglishUS);
    }

    // Check first matching locale
    let primary_locale = locale_list.locales().first()?;
    let normalized = primary_locale.trim().replace('_', "-").to_ascii_lowercase();

    // Handle specific regions or fallbacks
    // The hyphenation crate uses specific enums for languages
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
        // hyphenation crate distinguishes old/new orthography
        // Defaulting to new (1996) orthography for modern texts
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

    // We only embed standard dictionary languages right now.
    // If not found, hyphenation is disabled.
    None
}

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

fn is_break_inside_word(line: &str, boundaries: &[usize], break_idx: usize) -> bool {
    if break_idx == 0 || break_idx >= boundaries.len() - 1 {
        return false;
    }
    let prev = &line[boundaries[break_idx - 1]..boundaries[break_idx]];
    let next = &line[boundaries[break_idx]..boundaries[break_idx + 1]];
    !prev.chars().all(char::is_whitespace) && !next.chars().all(char::is_whitespace)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui::text::{LocaleList, SpanStyle, TextStyle};

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
        // "éléphant" -> él-éphant (break at byte 5, which is char 3)
        let break_idx = choose_auto_hyphen_break("éléphant", &style_with_locale("fr-FR"), 0, 7);
        assert_eq!(break_idx, Some(3));
    }

    #[test]
    fn locale_gate_uses_german_dictionary() {
        // "Geschwindigkeitsbegrenzung" -> Ge-schwin-dig-keits-be-gren-zung
        let break_idx = choose_auto_hyphen_break(
            "Geschwindigkeitsbegrenzung",
            &style_with_locale("de-DE"),
            10,
            20,
        );
        assert!(break_idx.is_some());
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
