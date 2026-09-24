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
#[path = "tests/highlight_theme_tests.rs"]
mod tests;
