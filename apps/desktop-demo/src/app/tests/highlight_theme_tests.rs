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
    let annotated =
        append_highlighted(AnnotatedString::builder(), Language::Rust, code).to_annotated_string();
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
    let annotated =
        append_highlighted(AnnotatedString::builder(), Language::Plain, code).to_annotated_string();
    assert_eq!(annotated.text, code);
    assert!(annotated.span_styles.is_empty());
}

#[test]
fn tokenizing_real_source_files_reconstructs_them_exactly() {
    let sources: [(Language, &str); 3] = [
        (Language::Rust, include_str!("../markdown.rs")),
        (Language::Rust, include_str!("../highlight.rs")),
        (Language::Rust, include_str!("../source_view.rs")),
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
    let code = include_str!("../markdown.rs");
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
