//! Thai puts no space between words: a line of it wraps between the words a
//! dictionary finds, and Latin text beside it still wraps at its spaces. A
//! word longer than the line splits, but never before a vowel or tone mark.

use cranpose_ui::{
    AppContext, TextLayoutOptions, TextOverflow, TextStyle, prepare_text_layout, set_text_measurer,
    text::AnnotatedString,
};

use crate::text_contract_measurer::{CHAR_WIDTH, ContractMeasurer};

/// "Thai is written without spaces between words", word by word.
const WORDS: [&str; 9] = [
    "ภาษา",
    "ไทย",
    "เขียน",
    "โดย",
    "ไม่",
    "เว้น",
    "วรรค",
    "ระหว่าง",
    "คำ",
];

fn wrapped_lines(text: &str, max_chars: usize) -> Vec<String> {
    let options = TextLayoutOptions {
        overflow: TextOverflow::Clip,
        soft_wrap: true,
        max_lines: usize::MAX,
        min_lines: 1,
    };
    let prepared = AppContext::new().enter(|| {
        set_text_measurer(ContractMeasurer);
        prepare_text_layout(
            &AnnotatedString::from(text),
            &TextStyle::default(),
            options,
            Some(max_chars as f32 * CHAR_WIDTH),
        )
    });
    prepared.text.text.lines().map(str::to_owned).collect()
}

#[test]
fn a_thai_line_wraps_between_words() {
    let text = WORDS.concat();
    // The longest word, ระหว่าง, has seven characters.
    for max_chars in [7, 9, 12, 16] {
        let lines = wrapped_lines(&text, max_chars);
        assert!(lines.len() > 1, "{max_chars} characters: {lines:?}");
        assert_eq!(lines.concat(), text);
        let mut words = WORDS.iter();
        for line in &lines {
            let mut rest = line.as_str();
            while !rest.is_empty() {
                let word = words.next().expect("a line holds only whole words");
                rest = rest
                    .strip_prefix(word)
                    .unwrap_or_else(|| panic!("{max_chars} characters: {line:?} splits a word"));
            }
        }
    }
}

#[test]
fn latin_beside_thai_still_wraps_at_its_spaces() {
    let lines = wrapped_lines("Hello world ภาษาไทย", 8);
    assert_eq!(lines, ["Hello", "world", "ภาษาไทย"]);
}

/// Thai's combining vowel and tone marks.
const THAI_MARKS: [std::ops::RangeInclusive<char>; 3] = [
    '\u{0E31}'..='\u{0E31}',
    '\u{0E34}'..='\u{0E3A}',
    '\u{0E47}'..='\u{0E4E}',
];

#[test]
fn a_word_longer_than_the_line_keeps_its_marks_on_their_letters() {
    // ร ะ ห ว ่ า ง: a break after four characters would start a line with
    // the tone mark ่ that sits on ว.
    let lines = wrapped_lines("ระหว่าง", 4);
    assert_eq!(lines.concat(), "ระหว่าง");
    for line in &lines {
        let first = line.chars().next().expect("no empty line");
        assert!(
            !THAI_MARKS.iter().any(|marks| marks.contains(&first)),
            "{lines:?}: a line starts with the mark {first:?}"
        );
    }
}
