use cranpose_ui::{
    text::{annotated_string::Builder, AnnotatedString, SpanStyle},
    Color,
};

use super::highlight::{tokenize, Language, TokenKind};

fn token_color(kind: TokenKind) -> Option<Color> {
    match kind {
        TokenKind::Plain => None,
        TokenKind::Keyword => Some(Color(0.86, 0.70, 0.96, 1.0)),
        TokenKind::Type => Some(Color(0.52, 0.84, 0.96, 1.0)),
        TokenKind::Str => Some(Color(0.70, 0.92, 0.74, 1.0)),
        TokenKind::Number => Some(Color(0.98, 0.86, 0.76, 1.0)),
        TokenKind::Comment => Some(Color(0.72, 0.76, 0.83, 1.0)),
        TokenKind::Attribute => Some(Color(0.97, 0.88, 0.78, 1.0)),
    }
}

fn token_style(kind: TokenKind) -> Option<SpanStyle> {
    token_color(kind).map(|color| SpanStyle {
        color: Some(color),
        ..Default::default()
    })
}

/// Appends `code`, coloured by `language`, to an in-progress builder.
///
/// Any style already pushed on the builder — the markdown code block's
/// monospace face and background, for one — stays in effect, because each
/// token pushes and pops one style of its own.
pub(crate) fn append_highlighted(mut builder: Builder, language: Language, code: &str) -> Builder {
    for token in tokenize(language, code) {
        let Some(slice) = code.get(token.start..token.end) else {
            continue;
        };
        builder = match token_style(token.kind) {
            Some(style) => builder.push_style(style).append(slice).pop(),
            None => builder.append(slice),
        };
    }
    builder
}

/// Splits `code` into one coloured [`AnnotatedString`] per line.
///
/// The whole text is tokenized before it is split, so a block comment or a
/// raw string spanning several lines keeps its colour on every one of them —
/// which tokenizing each line separately could not do. Line terminators are
/// dropped; each entry is the line's own content.
pub(crate) fn highlight_lines(language: Language, code: &str) -> Vec<AnnotatedString> {
    let tokens = tokenize(language, code);
    let mut lines = Vec::new();
    let mut cursor = 0usize;
    let mut offset = 0usize;

    for raw_line in code.split_inclusive('\n') {
        let start = offset;
        offset += raw_line.len();
        let text = raw_line.trim_end_matches(['\n', '\r']);
        let end = start + text.len();

        while cursor < tokens.len() && tokens[cursor].end <= start {
            cursor += 1;
        }

        let mut builder = AnnotatedString::builder();
        let mut index = cursor;
        while index < tokens.len() && tokens[index].start < end {
            let token = tokens[index];
            let from = token.start.max(start);
            let to = token.end.min(end);
            if from < to {
                if let Some(slice) = code.get(from..to) {
                    builder = match token_style(token.kind) {
                        Some(style) => builder.push_style(style).append(slice).pop(),
                        None => builder.append(slice),
                    };
                }
            }
            index += 1;
        }
        lines.push(builder.to_annotated_string());
    }

    if code.is_empty() || code.ends_with('\n') {
        lines.push(AnnotatedString::builder().to_annotated_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(lines: &[AnnotatedString]) -> String {
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn lines_reconstruct_the_source_without_terminators() {
        let code = "let a = 1;\nlet b = \"two\";\n";
        let lines = highlight_lines(Language::Rust, code);
        assert_eq!(joined(&lines), "let a = 1;\nlet b = \"two\";\n");
    }

    #[test]
    fn a_source_without_a_trailing_newline_keeps_its_last_line() {
        let lines = highlight_lines(Language::Rust, "let a = 1;");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "let a = 1;");
    }

    #[test]
    fn empty_source_is_one_empty_line() {
        let lines = highlight_lines(Language::Rust, "");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].text.is_empty());
    }

    #[test]
    fn carriage_returns_are_not_rendered() {
        let lines = highlight_lines(Language::Rust, "let a = 1;\r\nlet b = 2;\r\n");
        assert_eq!(lines[0].text, "let a = 1;");
        assert_eq!(lines[1].text, "let b = 2;");
    }

    #[test]
    fn non_ascii_source_survives_the_split() {
        let code = "let s = \"héllo wörld — ok\";\n// comment with 🌍\n";
        let lines = highlight_lines(Language::Rust, code);
        assert_eq!(lines[0].text, "let s = \"héllo wörld — ok\";");
        assert_eq!(lines[1].text, "// comment with 🌍");
    }

    #[test]
    fn a_multi_line_comment_is_coloured_on_every_line() {
        let code = "/* one\n two\n three */\n";
        let lines = highlight_lines(Language::Rust, code);
        for (index, line) in lines.iter().take(3).enumerate() {
            assert!(
                !line.span_styles.is_empty(),
                "line {index} lost the comment colour: {:?}",
                line.text
            );
        }
    }

    #[test]
    fn appending_preserves_the_exact_code_text() {
        let code = "fn main() { println!(\"hi\"); }";
        let annotated = append_highlighted(AnnotatedString::builder(), Language::Rust, code)
            .to_annotated_string();
        assert_eq!(annotated.text, code);
    }

    /// How far a colour sits inside the nearest ink range the markdown visual
    /// contracts accept, in 0-255 channel steps.
    ///
    /// A colour that only just clears a threshold is not enough: glyph edges
    /// blend toward the code block's dark ground, so an anti-aliased pixel of
    /// a barely-qualifying colour falls back outside the range and the row
    /// stops counting as rendered content.
    fn ink_margin(color: Color) -> i32 {
        let channel = |value: f32| i32::from((value.clamp(0.0, 1.0) * 255.0).round() as u8);
        let (r, g, b) = (channel(color.0), channel(color.1), channel(color.2));
        let bright_text = (r - 150).min(g - 150).min(b - 160);
        let link_blue = (b - 145).min(g - 110).min(179 - r);
        let math_yellow = (r - 165).min(g - 160).min(109 - b);
        bright_text.max(link_blue).max(math_yellow)
    }

    const MINIMUM_INK_MARGIN: i32 = 20;

    #[test]
    fn every_token_colour_counts_as_ink_for_the_visual_contracts() {
        let kinds = [
            TokenKind::Keyword,
            TokenKind::Type,
            TokenKind::Str,
            TokenKind::Number,
            TokenKind::Comment,
            TokenKind::Attribute,
        ];
        for kind in kinds {
            let color = token_color(kind).expect("every highlighted kind has a colour");
            let margin = ink_margin(color);
            assert!(
                margin >= MINIMUM_INK_MARGIN,
                "{kind:?} sits {margin} channel steps inside the markdown visual contracts' ink \
                 ranges, under the {MINIMUM_INK_MARGIN} an anti-aliased glyph needs: {color:?}"
            );
        }
    }

    #[test]
    fn plain_language_appends_one_unstyled_run() {
        let code = "no language here";
        let annotated = append_highlighted(AnnotatedString::builder(), Language::Plain, code)
            .to_annotated_string();
        assert_eq!(annotated.text, code);
        assert!(annotated.span_styles.is_empty());
    }

    #[test]
    fn tokenizing_real_source_files_reconstructs_them_exactly() {
        let sources: [(Language, &str); 3] = [
            (Language::Rust, include_str!("markdown.rs")),
            (Language::Rust, include_str!("highlight.rs")),
            (Language::Rust, include_str!("source_view.rs")),
        ];
        for (language, code) in sources {
            let tokens = tokenize(language, code);
            assert!(!tokens.is_empty(), "no tokens for a non-empty source");
            assert_eq!(tokens[0].start, 0);
            assert_eq!(tokens[tokens.len() - 1].end, code.len());
            let mut rebuilt = String::with_capacity(code.len());
            for pair in tokens.windows(2) {
                assert_eq!(pair[0].end, pair[1].start, "tokens must tile without gaps");
            }
            for token in &tokens {
                assert!(code.is_char_boundary(token.start));
                assert!(code.is_char_boundary(token.end));
                rebuilt.push_str(&code[token.start..token.end]);
            }
            assert_eq!(rebuilt, code, "tokens did not reconstruct the source");
        }
    }

    #[test]
    fn highlighting_real_source_preserves_every_line() {
        let code = include_str!("markdown.rs");
        let lines = highlight_lines(Language::Rust, code);
        let original: Vec<&str> = code.lines().collect();
        assert!(lines.len() >= original.len());
        for (index, expected) in original.iter().enumerate() {
            assert_eq!(
                lines[index].text,
                *expected,
                "line {} differs after highlighting",
                index + 1
            );
        }
    }
}
