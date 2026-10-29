use icu_segmenter::{
    LineSegmenter,
    options::{LineBreakOptions, LineBreakStrictness},
};

use super::ascii_breaks;
use crate::line_breaker::{BreakKind, BreakOpportunity};

// A character of each class ASCII characters take, and a second one of
// the classes with several.
const CLASSES: &[u8] = b"aZ0 \t\n\r\x0b\x01!\"($%)},-./|?':[+";

// The opportunities the segmenter finds in `text` at `strictness`.
fn segmented(text: &[u8], strictness: LineBreakStrictness) -> Vec<BreakOpportunity> {
    let mut options = LineBreakOptions::default();
    options.strictness = Some(strictness);
    let text = std::str::from_utf8(text).expect("ASCII");
    LineSegmenter::new_for_non_complex_scripts(options)
        .segment_str(text)
        .filter(|&offset| offset > 0)
        .map(|offset| BreakOpportunity {
            offset,
            kind: if matches!(text.as_bytes()[offset - 1], b'\n' | b'\r' | 0x0B | 0x0C) {
                BreakKind::Hard
            } else {
                BreakKind::Soft
            },
        })
        .collect()
}

fn assert_breaks_as_segmented(text: &[u8], breaks: &mut Vec<BreakOpportunity>) {
    ascii_breaks(text, breaks);
    for strictness in [
        LineBreakStrictness::Loose,
        LineBreakStrictness::Normal,
        LineBreakStrictness::Strict,
    ] {
        assert_eq!(
            *breaks,
            segmented(text, strictness),
            "{:?} at {strictness:?}",
            String::from_utf8_lossy(text)
        );
    }
}

// Every pair of ASCII characters, and every text of up to four characters
// of each class, breaks where the segmenter breaks it.
#[test]
fn short_ascii_text_breaks_where_the_segmenter_breaks_it() {
    let mut breaks = Vec::new();
    for first in 0..=0x7F_u8 {
        for second in 0..=0x7F_u8 {
            assert_breaks_as_segmented(&[first, second], &mut breaks);
        }
    }
    let mut text = Vec::new();
    for length in 1..=4 {
        let count = CLASSES.len().pow(length);
        for mut index in 0..count {
            text.clear();
            for _ in 0..length {
                text.push(CLASSES[index % CLASSES.len()]);
                index /= CLASSES.len();
            }
            assert_breaks_as_segmented(&text, &mut breaks);
        }
    }
}

// Longer texts break where the segmenter breaks them: the rules that read
// past the character before.
#[test]
fn long_ascii_text_breaks_where_the_segmenter_breaks_it() {
    let alphabet = CLASSES;
    let mut state = 0x2545_F491_u32;
    let mut text = Vec::new();
    let mut breaks = Vec::new();
    for _ in 0..100_000 {
        text.clear();
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let length = 5 + state % 20;
        for _ in 0..length {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            text.push(alphabet[state as usize % alphabet.len()]);
        }
        assert_breaks_as_segmented(&text, &mut breaks);
    }
}
