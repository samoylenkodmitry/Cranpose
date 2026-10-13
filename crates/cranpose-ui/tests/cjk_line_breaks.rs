//! Chinese and Japanese put no space between words: a line of either wraps
//! between characters, but never starts with closing punctuation or a small
//! kana, nor ends with opening punctuation. Korean puts spaces between words
//! and wraps at them.

use crate::text_contract_measurer::wrapped_lines;

#[test]
fn a_latin_word_before_japanese_shares_its_line() {
    let lines = wrapped_lines("Cranslate は相手の話を書き取り、翻訳します。", 14);
    assert_eq!(lines, ["Cranslate は相手の", "話を書き取り、翻訳します。"]);
    assert!(
        lines.iter().all(|line| line.chars().count() <= 14),
        "{lines:?}"
    );
}

#[test]
fn chinese_and_japanese_lines_keep_their_punctuation() {
    let texts = [
        "「東京」は日本の首都です。人口は約千四百万人で、ちょっと多いです。",
        "他没有设定削减数额，只是表示将根据中国的经济产出来决定数额。",
    ];
    for text in texts {
        for max_chars in 3..12 {
            let lines = wrapped_lines(text, max_chars);
            assert_eq!(lines.concat(), text);
            for line in &lines {
                let first = line.chars().next().expect("no empty line");
                let last = line.chars().next_back().expect("no empty line");
                assert!(
                    !"、。，」）ょっー".contains(first),
                    "{max_chars}: {lines:?} starts a line with {first}"
                );
                assert!(
                    !"「（".contains(last),
                    "{max_chars}: {lines:?} ends a line with {last}"
                );
                assert!(
                    line.chars().count() <= max_chars + 1,
                    "{max_chars}: {lines:?}"
                );
            }
        }
    }
}

#[test]
fn korean_wraps_at_its_spaces() {
    let lines = wrapped_lines("한국어는 띄어쓰기를 합니다", 6);
    assert_eq!(lines, ["한국어는", "띄어쓰기를", "합니다"]);
}
