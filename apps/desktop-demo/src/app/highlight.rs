#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TokenKind {
    Plain,
    Keyword,
    Type,
    Str,
    Number,
    Comment,
    Attribute,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Language {
    Rust,
    Toml,
    Json,
    Shell,
    Kotlin,
    Java,
    CFamily,
    Python,
    Plain,
}

/// Reads the language out of a fence tag.
///
/// Only the leading word counts, so an annotated fence such as
/// `kotlin(heap)` or `rust [-Rust 20ms]` still names its language. `+` and
/// `#` stay part of the word, because `c++` and `c#` are spelled with them.
pub(crate) fn language_from_fence(tag: &str) -> Language {
    let tag = tag.trim().to_lowercase();
    let head = tag
        .split(['(', ')', '[', ']', '{', '}', ',', ':', ' ', '\t'])
        .next()
        .unwrap_or_default();
    match head {
        "rust" | "rs" => Language::Rust,
        "toml" => Language::Toml,
        "json" => Language::Json,
        "sh" | "bash" | "shell" | "console" | "zsh" => Language::Shell,
        "kotlin" | "kt" | "kts" => Language::Kotlin,
        "java" | "j" => Language::Java,
        "c" | "h" | "c++" | "cpp" | "cxx" | "cc" | "hpp" | "c#" | "cs" | "csharp" | "js"
        | "javascript" | "jsx" | "ts" | "typescript" | "tsx" | "swift" | "go" | "scala"
        | "dart" | "groovy" => Language::CFamily,
        "python" | "python3" | "py" => Language::Python,
        _ => Language::Plain,
    }
}

pub(crate) fn tokenize(language: Language, code: &str) -> Vec<Token> {
    if code.is_empty() {
        return Vec::new();
    }

    match language {
        Language::Rust => tokenize_rust(code),
        Language::Toml => tokenize_toml(code),
        Language::Json => tokenize_json(code),
        Language::Shell => tokenize_shell(code),
        Language::Kotlin => tokenize_c_family(code, KOTLIN_KEYWORDS, NO_WORDS),
        Language::Java => tokenize_c_family(code, JAVA_KEYWORDS, NO_WORDS),
        Language::CFamily => tokenize_c_family(code, C_KEYWORDS, NO_WORDS),
        Language::Python => tokenize_python(code),
        Language::Plain => vec![Token {
            kind: TokenKind::Plain,
            start: 0,
            end: code.len(),
        }],
    }
}

fn is_whitespace(c: char) -> bool {
    c.is_whitespace()
}

fn is_id_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_id_cont(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

fn is_hex_digit(c: char) -> bool {
    c.is_ascii_hexdigit()
}

fn scan_whitespace_run(code: &str, mut pos: usize) -> Option<(usize, usize)> {
    let ch = code[pos..].chars().next()?;
    if !is_whitespace(ch) {
        return None;
    }
    let start = pos;
    pos += ch.len_utf8();
    while pos < code.len() {
        let ch = code[pos..].chars().next().unwrap_or('\0');
        if !is_whitespace(ch) {
            break;
        }
        pos += ch.len_utf8();
    }
    Some((start, pos))
}

fn scan_word(code: &str, mut pos: usize) -> Option<(usize, usize)> {
    let ch = code[pos..].chars().next()?;
    if !is_id_start(ch) {
        return None;
    }
    let start = pos;
    pos += ch.len_utf8();
    while pos < code.len() && is_id_cont(code[pos..].chars().next().unwrap_or('\0')) {
        pos += code[pos..].chars().next().unwrap_or('\0').len_utf8();
    }
    Some((start, pos))
}

fn scan_string_escaped(code: &str, mut pos: usize, quote: char) -> Option<(usize, usize)> {
    let ch = code[pos..].chars().next()?;
    if ch != quote {
        return None;
    }
    let start = pos;
    pos += 1;
    let mut escaped = false;
    while pos < code.len() {
        let ch = code[pos..].chars().next().unwrap_or('\0');
        if ch == quote && !escaped {
            pos += 1;
            break;
        }
        escaped = ch == '\\' && !escaped;
        pos += ch.len_utf8();
    }
    Some((start, pos))
}

fn scan_string_unescaped(code: &str, mut pos: usize, quote: char) -> Option<(usize, usize)> {
    let ch = code[pos..].chars().next()?;
    if ch != quote {
        return None;
    }
    let start = pos;
    pos += 1;
    while pos < code.len() && code[pos..].chars().next().unwrap_or('\0') != quote {
        pos += code[pos..].chars().next().unwrap_or('\0').len_utf8();
    }
    if pos < code.len() {
        pos += 1;
    }
    Some((start, pos))
}

fn scan_line_to_end(code: &str, mut pos: usize) -> Option<(usize, usize)> {
    let ch = code[pos..].chars().next()?;
    let start = pos;
    pos += ch.len_utf8();
    while pos < code.len() && code[pos..].chars().next().unwrap_or('\0') != '\n' {
        pos += code[pos..].chars().next().unwrap_or('\0').len_utf8();
    }
    Some((start, pos))
}

fn token_from_bounds(kind: TokenKind, start: usize, end: usize) -> (Token, usize) {
    (Token { kind, start, end }, end)
}

fn scan_slash_line_comment(code: &str, pos: usize) -> Option<(Token, usize)> {
    let ch = code[pos..].chars().next()?;
    if ch != '/' || pos + 1 >= code.len() {
        return None;
    }
    let next = code[pos + 1..].chars().next().unwrap_or('\0');
    if next != '/' {
        return None;
    }
    let (start, end) = scan_line_to_end(code, pos)?;
    Some(token_from_bounds(TokenKind::Comment, start, end))
}

fn scan_block_comment(code: &str, mut pos: usize, nests: bool) -> Option<(Token, usize)> {
    let ch = code[pos..].chars().next()?;
    if ch != '/' || pos + 1 >= code.len() {
        return None;
    }
    let next = code[pos + 1..].chars().next().unwrap_or('\0');
    if next != '*' {
        return None;
    }
    let start = pos;
    pos += 2;
    let mut depth = 1;
    while depth > 0 && pos < code.len() {
        let ch = code[pos..].chars().next().unwrap_or('\0');
        let ch_len = ch.len_utf8();
        if nests && ch == '/' && pos + 1 < code.len() {
            let next = code[pos + 1..].chars().next().unwrap_or('\0');
            if next == '*' {
                depth += 1;
                pos += 2;
            } else {
                pos += ch_len;
            }
        } else if ch == '*' && pos + 1 < code.len() {
            let next = code[pos + 1..].chars().next().unwrap_or('\0');
            if next == '/' {
                depth -= 1;
                pos += 2;
            } else {
                pos += ch_len;
            }
        } else {
            pos += ch_len;
        }
    }
    Some((
        Token {
            kind: TokenKind::Comment,
            start,
            end: pos,
        },
        pos,
    ))
}

fn scan_char_literal(code: &str, mut pos: usize) -> Option<(Token, usize)> {
    let ch = code[pos..].chars().next()?;
    if ch != '\'' {
        return None;
    }
    let start = pos;
    pos += 1;
    let mut escaped = false;
    let mut count = 0;
    while pos < code.len() && count < 4 {
        let ch = code[pos..].chars().next().unwrap_or('\0');
        if ch == '\'' && !escaped {
            pos += 1;
            break;
        }
        escaped = ch == '\\' && !escaped;
        pos += ch.len_utf8();
        count += 1;
    }
    Some((
        Token {
            kind: TokenKind::Str,
            start,
            end: pos,
        },
        pos,
    ))
}

fn scan_rust_raw_string(code: &str, mut pos: usize) -> Option<(Token, usize)> {
    let ch = code[pos..].chars().next()?;
    if ch != 'r' || pos + 1 >= code.len() {
        return None;
    }
    if code[pos + 1..].chars().next().unwrap_or('\0') != '"' {
        return None;
    }
    let start = pos;
    pos += 1;
    let mut hash_count = 0;
    while pos < code.len() && code[pos..].chars().next().unwrap_or('\0') == '#' {
        hash_count += 1;
        pos += 1;
    }
    if pos < code.len() && code[pos..].chars().next().unwrap_or('\0') == '"' {
        pos += 1;
        let closing = format!("\"{}\"", "#".repeat(hash_count));
        let closing_bytes = closing.as_bytes();
        while pos + closing_bytes.len() <= code.len() {
            if &code.as_bytes()[pos..pos + closing_bytes.len()] == closing_bytes {
                pos += closing_bytes.len();
                break;
            }
            let ch = code[pos..].chars().next().unwrap_or('\0');
            pos += ch.len_utf8();
        }
    }
    Some((
        Token {
            kind: TokenKind::Str,
            start,
            end: pos,
        },
        pos,
    ))
}

fn scan_rust_attribute(code: &str, mut pos: usize) -> Option<(Token, usize)> {
    let ch = code[pos..].chars().next()?;
    if ch != '#' || pos + 1 >= code.len() {
        return None;
    }
    let next = code[pos + 1..].chars().next().unwrap_or('\0');
    let is_attr = next == '['
        || (next == '!'
            && pos + 2 < code.len()
            && code[pos + 2..].chars().next().unwrap_or('\0') == '[');
    if !is_attr {
        return None;
    }
    let start = pos;
    pos += 1;
    let mut depth = 0;
    while pos < code.len() {
        let ch = code[pos..].chars().next().unwrap_or('\0');
        if ch == '[' {
            depth += 1;
        } else if ch == ']' {
            depth -= 1;
            if depth == 0 {
                pos += 1;
                break;
            }
        }
        pos += ch.len_utf8();
    }
    Some((
        Token {
            kind: TokenKind::Attribute,
            start,
            end: pos,
        },
        pos,
    ))
}

fn scan_hex_binary_int(code: &str, mut pos: usize) -> usize {
    if pos >= code.len() {
        return pos;
    }
    let next = code[pos..].chars().next().unwrap_or('\0');
    if next == 'x' {
        pos += 2;
        while pos < code.len()
            && (is_hex_digit(code[pos..].chars().next().unwrap_or('\0'))
                || code[pos..].chars().next().unwrap_or('\0') == '_')
        {
            pos += code[pos..].chars().next().unwrap_or('\0').len_utf8();
        }
    } else if next == 'b' {
        pos += 2;
        while pos < code.len()
            && (code[pos..].chars().next().unwrap_or('\0') == '0'
                || code[pos..].chars().next().unwrap_or('\0') == '1'
                || code[pos..].chars().next().unwrap_or('\0') == '_')
        {
            pos += code[pos..].chars().next().unwrap_or('\0').len_utf8();
        }
    }
    pos
}

fn scan_decimal_int(code: &str, mut pos: usize) -> usize {
    while pos < code.len()
        && (is_digit(code[pos..].chars().next().unwrap_or('\0'))
            || code[pos..].chars().next().unwrap_or('\0') == '_')
    {
        pos += code[pos..].chars().next().unwrap_or('\0').len_utf8();
    }
    pos
}

fn scan_fractional_part(code: &str, mut pos: usize) -> usize {
    if pos >= code.len() || code[pos..].chars().next().unwrap_or('\0') != '.' {
        return pos;
    }
    let next_pos = pos + 1;
    if next_pos >= code.len() || !is_digit(code[next_pos..].chars().next().unwrap_or('\0')) {
        return pos;
    }
    pos += 1;
    scan_decimal_int(code, pos)
}

fn scan_exponent_part(code: &str, mut pos: usize) -> usize {
    if pos >= code.len() {
        return pos;
    }
    let ch = code[pos..].chars().next().unwrap_or('\0');
    if ch != 'e' && ch != 'E' {
        return pos;
    }
    pos += 1;
    if pos < code.len()
        && (code[pos..].chars().next().unwrap_or('\0') == '+'
            || code[pos..].chars().next().unwrap_or('\0') == '-')
    {
        pos += 1;
    }
    scan_decimal_int(code, pos)
}

fn scan_number(code: &str, mut pos: usize) -> Option<(Token, usize)> {
    let ch = code[pos..].chars().next()?;
    let is_hex_start = ch == '0'
        && pos + 1 < code.len()
        && (code[pos + 1..].chars().next().unwrap_or('\0') == 'x'
            || code[pos + 1..].chars().next().unwrap_or('\0') == 'b');
    if !is_digit(ch) && !is_hex_start {
        return None;
    }
    let start = pos;
    if ch == '0' && pos + 1 < code.len() {
        let next = code[pos + 1..].chars().next().unwrap_or('\0');
        if next == 'x' || next == 'b' {
            pos = scan_hex_binary_int(code, pos + 1);
        } else {
            pos += 1;
            pos = scan_decimal_int(code, pos);
        }
    } else {
        pos = scan_decimal_int(code, pos);
    }
    pos = scan_fractional_part(code, pos);
    pos = scan_exponent_part(code, pos);
    while pos < code.len() && is_id_cont(code[pos..].chars().next().unwrap_or('\0')) {
        pos += code[pos..].chars().next().unwrap_or('\0').len_utf8();
    }
    Some((
        Token {
            kind: TokenKind::Number,
            start,
            end: pos,
        },
        pos,
    ))
}

fn tokenize_impl<F>(code: &str, scan_next: F) -> Vec<Token>
where
    F: Fn(&str, usize) -> Option<(Token, usize)>,
{
    let mut tokens = Vec::new();
    let mut pos = 0;
    while pos < code.len() {
        if let Some((start, new_pos)) = scan_whitespace_run(code, pos) {
            tokens.push(Token {
                kind: TokenKind::Plain,
                start,
                end: new_pos,
            });
            pos = new_pos;
        } else if let Some((token, new_pos)) = scan_next(code, pos) {
            tokens.push(token);
            pos = new_pos;
        } else {
            let ch = code[pos..].chars().next().unwrap_or('\0');
            tokens.push(Token {
                kind: TokenKind::Plain,
                start: pos,
                end: pos + ch.len_utf8(),
            });
            pos += ch.len_utf8();
        }
    }
    normalize_tokens(tokens, code.len())
}

const RUST_KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "const", "static", "struct", "enum", "trait", "impl", "for", "while",
    "loop", "if", "else", "match", "return", "use", "mod", "pub", "crate", "self", "super",
    "where", "as", "in", "ref", "move", "box", "dyn", "async", "await", "unsafe", "extern", "type",
    "continue", "break", "true", "false",
];

const RUST_TYPES: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64", "i128", "usize", "isize", "f32",
    "f64", "bool", "char", "str",
];

const KOTLIN_KEYWORDS: &[&str] = &[
    "fun",
    "val",
    "var",
    "class",
    "object",
    "interface",
    "data",
    "sealed",
    "enum",
    "companion",
    "init",
    "constructor",
    "override",
    "open",
    "abstract",
    "final",
    "private",
    "protected",
    "public",
    "internal",
    "if",
    "else",
    "when",
    "while",
    "for",
    "do",
    "return",
    "break",
    "continue",
    "try",
    "catch",
    "finally",
    "throw",
    "is",
    "as",
    "in",
    "out",
    "by",
    "lateinit",
    "suspend",
    "inline",
    "reified",
    "typealias",
    "package",
    "import",
    "null",
    "true",
    "false",
    "this",
    "super",
    "where",
    "vararg",
    "operator",
    "infix",
    "crossinline",
    "noinline",
    "annotation",
    "external",
    "const",
    "get",
    "set",
    "field",
    "it",
    "run",
    "let",
    "also",
    "apply",
    "with",
];

const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "record",
    "return",
    "sealed",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "var",
    "void",
    "volatile",
    "while",
    "yield",
    "true",
    "false",
    "null",
];

const C_KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "auto",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "constexpr",
    "continue",
    "decltype",
    "default",
    "delete",
    "do",
    "double",
    "dynamic_cast",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "final",
    "float",
    "for",
    "friend",
    "function",
    "goto",
    "if",
    "implements",
    "import",
    "inline",
    "instanceof",
    "int",
    "interface",
    "let",
    "long",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "nullptr",
    "operator",
    "override",
    "private",
    "protected",
    "public",
    "readonly",
    "register",
    "restrict",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "static_cast",
    "struct",
    "switch",
    "template",
    "this",
    "throw",
    "true",
    "try",
    "typedef",
    "typename",
    "typeof",
    "union",
    "unsigned",
    "using",
    "var",
    "virtual",
    "void",
    "volatile",
    "while",
    "async",
    "await",
    "yield",
    "from",
    "of",
    "null",
    "undefined",
    "string",
    "number",
    "any",
];

const PYTHON_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda",
    "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield", "True",
    "False", "None", "self", "match", "case",
];

const PYTHON_TYPES: &[&str] = &[
    "int",
    "float",
    "str",
    "bool",
    "list",
    "dict",
    "set",
    "tuple",
    "bytes",
    "complex",
    "frozenset",
    "object",
];

const NO_WORDS: &[&str] = &[];

fn classify_word(word: &str, keywords: &[&str], types: &[&str]) -> TokenKind {
    if keywords.contains(&word) {
        return TokenKind::Keyword;
    }
    if types.contains(&word) {
        return TokenKind::Type;
    }
    if word.chars().next().is_some_and(char::is_uppercase) {
        return TokenKind::Type;
    }
    TokenKind::Plain
}

fn scan_identifier_in(
    code: &str,
    pos: usize,
    keywords: &[&str],
    types: &[&str],
) -> Option<(Token, usize)> {
    let (start, end) = scan_word(code, pos)?;
    let kind = classify_word(&code[start..end], keywords, types);
    Some((Token { kind, start, end }, end))
}

fn scan_triple_quoted(code: &str, pos: usize, quote: char) -> Option<(Token, usize)> {
    let fence: String = std::iter::repeat_n(quote, 3).collect();
    if !code[pos..].starts_with(&fence) {
        return None;
    }
    let body = pos + fence.len();
    let end = match code[body..].find(&fence) {
        Some(offset) => body + offset + fence.len(),
        None => code.len(),
    };
    Some(token_from_bounds(TokenKind::Str, pos, end))
}

fn scan_at_annotation(code: &str, pos: usize) -> Option<(Token, usize)> {
    if code[pos..].chars().next()? != '@' {
        return None;
    }
    let (_, end) = scan_word(code, pos + 1)?;
    Some(token_from_bounds(TokenKind::Attribute, pos, end))
}

fn scan_preprocessor_line(code: &str, pos: usize) -> Option<(Token, usize)> {
    if code[pos..].chars().next()? != '#' {
        return None;
    }
    let line_start = code[..pos].rfind('\n').map(|index| index + 1).unwrap_or(0);
    if code[line_start..pos].chars().any(|c| !is_whitespace(c)) {
        return None;
    }
    scan_word(code, pos + 1)?;
    let (start, end) = scan_line_to_end(code, pos)?;
    Some(token_from_bounds(TokenKind::Attribute, start, end))
}

fn scan_rust_identifier(code: &str, pos: usize) -> Option<(Token, usize)> {
    scan_identifier_in(code, pos, RUST_KEYWORDS, RUST_TYPES)
}

fn tokenize_rust(code: &str) -> Vec<Token> {
    tokenize_impl(code, |code, pos| {
        scan_slash_line_comment(code, pos)
            .or_else(|| scan_block_comment(code, pos, true))
            .or_else(|| {
                let (start, end) = scan_string_escaped(code, pos, '"')?;
                Some(token_from_bounds(TokenKind::Str, start, end))
            })
            .or_else(|| scan_char_literal(code, pos))
            .or_else(|| scan_rust_raw_string(code, pos))
            .or_else(|| scan_rust_attribute(code, pos))
            .or_else(|| scan_number(code, pos))
            .or_else(|| scan_rust_identifier(code, pos))
    })
}

fn tokenize_c_family<'a>(code: &str, keywords: &'a [&'a str], types: &'a [&'a str]) -> Vec<Token> {
    tokenize_impl(code, move |code, pos| {
        scan_slash_line_comment(code, pos)
            .or_else(|| scan_block_comment(code, pos, false))
            .or_else(|| scan_triple_quoted(code, pos, '"'))
            .or_else(|| {
                let (start, end) = scan_string_escaped(code, pos, '"')?;
                Some(token_from_bounds(TokenKind::Str, start, end))
            })
            .or_else(|| scan_char_literal(code, pos))
            .or_else(|| scan_at_annotation(code, pos))
            .or_else(|| scan_preprocessor_line(code, pos))
            .or_else(|| scan_number(code, pos))
            .or_else(|| scan_identifier_in(code, pos, keywords, types))
    })
}

fn tokenize_python(code: &str) -> Vec<Token> {
    tokenize_impl(code, |code, pos| {
        scan_hash_comment(code, pos)
            .or_else(|| scan_triple_quoted(code, pos, '"'))
            .or_else(|| scan_triple_quoted(code, pos, '\''))
            .or_else(|| {
                let (start, end) = scan_string_escaped(code, pos, '"')?;
                Some(token_from_bounds(TokenKind::Str, start, end))
            })
            .or_else(|| {
                let (start, end) = scan_string_escaped(code, pos, '\'')?;
                Some(token_from_bounds(TokenKind::Str, start, end))
            })
            .or_else(|| scan_at_annotation(code, pos))
            .or_else(|| scan_number(code, pos))
            .or_else(|| scan_identifier_in(code, pos, PYTHON_KEYWORDS, PYTHON_TYPES))
    })
}

fn scan_toml_section(code: &str, mut pos: usize) -> Option<(Token, usize)> {
    let ch = code[pos..].chars().next()?;
    if ch != '[' {
        return None;
    }
    let start = pos;
    pos += 1;
    let mut depth = 1;
    while pos < code.len() && depth > 0 {
        let ch = code[pos..].chars().next().unwrap_or('\0');
        if ch == '[' {
            depth += 1;
        } else if ch == ']' {
            depth -= 1;
        }
        pos += ch.len_utf8();
    }
    Some((
        Token {
            kind: TokenKind::Attribute,
            start,
            end: pos,
        },
        pos,
    ))
}

fn scan_toml_number(code: &str, mut pos: usize) -> Option<(Token, usize)> {
    let ch = code[pos..].chars().next()?;
    let has_sign = ch == '-' || ch == '+';
    let digit_ahead = if has_sign {
        pos + 1 < code.len() && is_digit(code[pos + 1..].chars().next().unwrap_or('\0'))
    } else {
        is_digit(ch)
    };
    if !digit_ahead {
        return None;
    }
    let start = pos;
    if has_sign {
        pos += 1;
    }
    pos = scan_decimal_int(code, pos);
    pos = scan_fractional_part(code, pos);
    Some((
        Token {
            kind: TokenKind::Number,
            start,
            end: pos,
        },
        pos,
    ))
}

fn toml_assignment_follows(code: &str, mut pos: usize) -> bool {
    while pos < code.len() {
        let ch = code[pos..].chars().next().unwrap_or('\0');
        match ch {
            ' ' | '\t' => pos += ch.len_utf8(),
            '=' => return true,
            _ => return false,
        }
    }
    false
}

fn toml_classify_word(word: &str, code: &str, pos: usize) -> TokenKind {
    let is_bool = word == "true" || word == "false";
    let is_key = !word.chars().all(|c| c == '_') && toml_assignment_follows(code, pos);
    if is_bool || is_key {
        TokenKind::Keyword
    } else {
        TokenKind::Plain
    }
}

fn scan_toml_identifier(code: &str, pos: usize) -> Option<(Token, usize)> {
    let (start, end) = scan_word(code, pos)?;
    let word = &code[start..end];
    let kind = toml_classify_word(word, code, end);
    Some((Token { kind, start, end }, end))
}

fn scan_toml_comment(code: &str, pos: usize) -> Option<(Token, usize)> {
    let ch = code[pos..].chars().next()?;
    if ch != '#' {
        return None;
    }
    let (start, end) = scan_line_to_end(code, pos)?;
    Some(token_from_bounds(TokenKind::Comment, start, end))
}

fn tokenize_toml(code: &str) -> Vec<Token> {
    tokenize_impl(code, |code, pos| {
        scan_toml_comment(code, pos)
            .or_else(|| scan_toml_section(code, pos))
            .or_else(|| {
                let (start, end) = scan_string_escaped(code, pos, '"')?;
                Some(token_from_bounds(TokenKind::Str, start, end))
            })
            .or_else(|| {
                let (start, end) = scan_string_escaped(code, pos, '\'')?;
                Some(token_from_bounds(TokenKind::Str, start, end))
            })
            .or_else(|| scan_toml_number(code, pos))
            .or_else(|| scan_toml_identifier(code, pos))
    })
}

fn scan_json_string(code: &str, pos: usize) -> Option<(Token, usize)> {
    let (start, end) = scan_string_escaped(code, pos, '"')?;
    let mut check_ws = end;
    while check_ws < code.len() && is_whitespace(code[check_ws..].chars().next().unwrap_or('\0')) {
        check_ws += code[check_ws..].chars().next().unwrap_or('\0').len_utf8();
    }
    let kind = if check_ws < code.len() && code[check_ws..].chars().next().unwrap_or('\0') == ':' {
        TokenKind::Keyword
    } else {
        TokenKind::Str
    };
    Some((Token { kind, start, end }, end))
}

fn scan_json_int_part(code: &str, mut pos: usize) -> usize {
    if pos < code.len() && code[pos..].chars().next().unwrap_or('\0') == '0' {
        return pos + 1;
    }
    while pos < code.len() && is_digit(code[pos..].chars().next().unwrap_or('\0')) {
        pos += 1;
    }
    pos
}

fn scan_json_frac_exp(code: &str, mut pos: usize) -> usize {
    if pos < code.len() && code[pos..].chars().next().unwrap_or('\0') == '.' {
        pos += 1;
        while pos < code.len() && is_digit(code[pos..].chars().next().unwrap_or('\0')) {
            pos += 1;
        }
    }
    if pos < code.len()
        && (code[pos..].chars().next().unwrap_or('\0') == 'e'
            || code[pos..].chars().next().unwrap_or('\0') == 'E')
    {
        pos += 1;
        if pos < code.len()
            && (code[pos..].chars().next().unwrap_or('\0') == '+'
                || code[pos..].chars().next().unwrap_or('\0') == '-')
        {
            pos += 1;
        }
        while pos < code.len() && is_digit(code[pos..].chars().next().unwrap_or('\0')) {
            pos += 1;
        }
    }
    pos
}

fn scan_json_number(code: &str, mut pos: usize) -> Option<(Token, usize)> {
    let ch = code[pos..].chars().next()?;
    let has_minus = ch == '-';
    let digit_ahead = if has_minus {
        pos + 1 < code.len() && is_digit(code[pos + 1..].chars().next().unwrap_or('\0'))
    } else {
        is_digit(ch)
    };
    if !digit_ahead {
        return None;
    }
    let start = pos;
    if has_minus {
        pos += 1;
    }
    pos = scan_json_int_part(code, pos);
    pos = scan_json_frac_exp(code, pos);
    Some((
        Token {
            kind: TokenKind::Number,
            start,
            end: pos,
        },
        pos,
    ))
}

fn json_classify_word(word: &str) -> TokenKind {
    if word == "true" || word == "false" || word == "null" {
        TokenKind::Keyword
    } else {
        TokenKind::Plain
    }
}

fn scan_json_identifier(code: &str, pos: usize) -> Option<(Token, usize)> {
    let (start, end) = scan_word(code, pos)?;
    let word = &code[start..end];
    let kind = json_classify_word(word);
    Some((Token { kind, start, end }, end))
}

fn tokenize_json(code: &str) -> Vec<Token> {
    tokenize_impl(code, |code, pos| {
        scan_json_string(code, pos)
            .or_else(|| scan_json_number(code, pos))
            .or_else(|| scan_json_identifier(code, pos))
    })
}

fn scan_hash_comment(code: &str, pos: usize) -> Option<(Token, usize)> {
    let ch = code[pos..].chars().next()?;
    if ch != '#' {
        return None;
    }
    let (start, end) = scan_line_to_end(code, pos)?;
    Some(token_from_bounds(TokenKind::Comment, start, end))
}

fn shell_classify_word(word: &str) -> TokenKind {
    match word {
        "if" | "then" | "else" | "fi" | "for" | "while" | "do" | "done" | "case" | "esac"
        | "function" | "return" | "export" | "local" | "echo" | "cd" => TokenKind::Keyword,
        _ => TokenKind::Plain,
    }
}

fn scan_shell_identifier(code: &str, pos: usize) -> Option<(Token, usize)> {
    let (start, end) = scan_word(code, pos)?;
    let word = &code[start..end];
    let kind = shell_classify_word(word);
    Some(token_from_bounds(kind, start, end))
}

fn tokenize_shell(code: &str) -> Vec<Token> {
    tokenize_impl(code, |code, pos| {
        scan_hash_comment(code, pos)
            .or_else(|| {
                let (start, end) = scan_string_escaped(code, pos, '"')?;
                Some(token_from_bounds(TokenKind::Str, start, end))
            })
            .or_else(|| {
                let (start, end) = scan_string_unescaped(code, pos, '\'')?;
                Some(token_from_bounds(TokenKind::Str, start, end))
            })
            .or_else(|| scan_shell_identifier(code, pos))
    })
}

fn normalize_tokens(tokens: Vec<Token>, code_len: usize) -> Vec<Token> {
    if tokens.is_empty() {
        if code_len > 0 {
            return vec![Token {
                kind: TokenKind::Plain,
                start: 0,
                end: code_len,
            }];
        }
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut last_end = 0;

    for token in tokens {
        if token.start > last_end {
            result.push(Token {
                kind: TokenKind::Plain,
                start: last_end,
                end: token.start,
            });
        }
        result.push(token);
        last_end = token.end;
    }

    if last_end < code_len {
        result.push(Token {
            kind: TokenKind::Plain,
            start: last_end,
            end: code_len,
        });
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_tiling(code: &str, tokens: &[Token]) {
        if code.is_empty() {
            assert!(
                tokens.is_empty(),
                "Empty code should produce empty token list"
            );
            return;
        }

        if !tokens.is_empty() {
            assert_eq!(tokens[0].start, 0, "First token must start at 0");
            assert_eq!(
                tokens[tokens.len() - 1].end,
                code.len(),
                "Last token must end at code length"
            );

            for (i, token) in tokens.iter().enumerate() {
                assert!(
                    code.is_char_boundary(token.start),
                    "Token {}: start {} is not a char boundary",
                    i,
                    token.start
                );
                assert!(
                    code.is_char_boundary(token.end),
                    "Token {}: end {} is not a char boundary",
                    i,
                    token.end
                );
                assert!(token.start <= token.end, "Token {}: start > end", i);

                if i > 0 {
                    assert_eq!(
                        tokens[i - 1].end,
                        token.start,
                        "Token {} does not start where token {} ends",
                        i,
                        i - 1
                    );
                }

                let _slice = &code[token.start..token.end];
                for (j, token2) in tokens.iter().enumerate() {
                    if i != j {
                        let _slice2 = &code[token2.start..token2.end];
                        assert!(
                            token.end <= token2.start || token2.end <= token.start,
                            "Token {} and {} overlap",
                            i,
                            j
                        );
                    }
                }
            }

            let mut reconstructed = String::new();
            for token in tokens {
                reconstructed.push_str(&code[token.start..token.end]);
            }
            assert_eq!(
                reconstructed, code,
                "Tokens do not reconstruct original code"
            );
        }
    }

    fn test_tokenizes(language: Language, code: &str, expected_kind: TokenKind) {
        let tokens = tokenize(language, code);
        assert_tiling(code, &tokens);
        let found = tokens.iter().find(|t| t.kind == expected_kind);
        assert!(
            found.is_some(),
            "Expected {:?} token not found",
            expected_kind
        );
    }

    #[test]
    fn test_language_from_fence() {
        assert_eq!(language_from_fence("rust"), Language::Rust);
        assert_eq!(language_from_fence("rs"), Language::Rust);
        assert_eq!(language_from_fence("Rust"), Language::Rust);
        assert_eq!(language_from_fence("toml"), Language::Toml);
        assert_eq!(language_from_fence("json"), Language::Json);
        assert_eq!(language_from_fence("sh"), Language::Shell);
        assert_eq!(language_from_fence("bash"), Language::Shell);
        assert_eq!(language_from_fence("shell"), Language::Shell);
        assert_eq!(language_from_fence("console"), Language::Shell);
        assert_eq!(language_from_fence("python"), Language::Python);
        assert_eq!(language_from_fence("python3"), Language::Python);
        assert_eq!(language_from_fence("kotlin"), Language::Kotlin);
        assert_eq!(language_from_fence("Kotlin"), Language::Kotlin);
        assert_eq!(language_from_fence("kt"), Language::Kotlin);
        assert_eq!(language_from_fence("java"), Language::Java);
        assert_eq!(language_from_fence("c++"), Language::CFamily);
        assert_eq!(language_from_fence("cpp"), Language::CFamily);
        assert_eq!(language_from_fence("c#"), Language::CFamily);
        assert_eq!(language_from_fence("ts"), Language::CFamily);
        assert_eq!(language_from_fence(""), Language::Plain);
        assert_eq!(language_from_fence("brainfuck"), Language::Plain);
        assert_eq!(language_from_fence("kotlin(heap)"), Language::Kotlin);
        assert_eq!(language_from_fence("rust [-Rust 20ms]"), Language::Rust);
        assert_eq!(language_from_fence("  rust  "), Language::Rust);
    }

    #[test]
    fn test_empty_code() {
        let tokens = tokenize(Language::Rust, "");
        assert_tiling("", &tokens);

        let tokens = tokenize(Language::Plain, "");
        assert_tiling("", &tokens);
    }

    #[test]
    fn test_plain_language() {
        let code = "hello world";
        let tokens = tokenize(Language::Plain, code);
        assert_tiling(code, &tokens);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Plain);
    }

    #[test]
    fn test_rust_line_comment() {
        test_tokenizes(
            Language::Rust,
            "let x = 5; // this is a comment",
            TokenKind::Comment,
        );
    }

    #[test]
    fn test_rust_block_comment() {
        test_tokenizes(
            Language::Rust,
            "let x = /* comment */ 5;",
            TokenKind::Comment,
        );
    }

    #[test]
    fn test_rust_nested_block_comment() {
        let code = "/* a /* b */ c */";
        let tokens = tokenize(Language::Rust, code);
        assert_tiling(code, &tokens);
        let comment_token = tokens.iter().find(|t| t.kind == TokenKind::Comment);
        assert!(comment_token.is_some());
        assert_eq!(
            comment_token.unwrap().end - comment_token.unwrap().start,
            code.len()
        );
    }

    #[test]
    fn test_rust_string_simple() {
        test_tokenizes(Language::Rust, "let s = \"hello\";", TokenKind::Str);
    }

    #[test]
    fn test_rust_string_with_escape() {
        test_tokenizes(
            Language::Rust,
            "let s = \"hello\\\"world\";",
            TokenKind::Str,
        );
    }

    #[test]
    fn test_rust_raw_string() {
        test_tokenizes(Language::Rust, "let s = r\"hello\"world\";", TokenKind::Str);
    }

    #[test]
    fn test_rust_raw_string_with_hashes() {
        test_tokenizes(
            Language::Rust,
            "let s = r#\"he said \\\"hi\\\"\"#;",
            TokenKind::Str,
        );
    }

    #[test]
    fn test_rust_char_literal() {
        test_tokenizes(Language::Rust, "let c = 'x';", TokenKind::Str);
    }

    #[test]
    fn test_rust_keywords() {
        let code = "fn let mut const static";
        let tokens = tokenize(Language::Rust, code);
        assert_tiling(code, &tokens);
        let keywords: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Keyword)
            .collect();
        assert!(keywords.len() >= 5);
    }

    #[test]
    fn test_rust_types() {
        let code = "u32 String Vec Option Result";
        let tokens = tokenize(Language::Rust, code);
        assert_tiling(code, &tokens);
        let types: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Type)
            .collect();
        assert!(types.len() >= 5);
    }

    #[test]
    fn test_rust_uppercase_identifier_is_type() {
        let code = "MyType MyStruct";
        let tokens = tokenize(Language::Rust, code);
        assert_tiling(code, &tokens);
        let types: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Type)
            .collect();
        assert_eq!(types.len(), 2);
    }

    fn test_token_count(language: Language, code: &str, kind: TokenKind, min_count: usize) {
        let tokens = tokenize(language, code);
        assert_tiling(code, &tokens);
        let count = tokens.iter().filter(|t| t.kind == kind).count();
        assert!(
            count >= min_count,
            "Expected at least {} {:?} tokens, found {}",
            min_count,
            kind,
            count
        );
    }

    #[test]
    fn test_rust_number_decimal() {
        test_token_count(Language::Rust, "42 3.14 1_000", TokenKind::Number, 3);
    }

    #[test]
    fn test_rust_number_hex() {
        test_token_count(Language::Rust, "0xff 0xAB_CD", TokenKind::Number, 2);
    }

    #[test]
    fn test_rust_number_with_suffix() {
        test_token_count(Language::Rust, "42u32 3.14f64", TokenKind::Number, 2);
    }

    #[test]
    fn test_rust_attribute() {
        test_tokenizes(Language::Rust, "#[derive(Debug)]", TokenKind::Attribute);
    }

    #[test]
    fn test_rust_attribute_bang() {
        test_tokenizes(Language::Rust, "#![allow(dead_code)]", TokenKind::Attribute);
    }

    #[test]
    fn test_rust_non_ascii() {
        let code = "let s = \"héllo wörld — ok\"; // emoji: 🚀";
        let tokens = tokenize(Language::Rust, code);
        assert_tiling(code, &tokens);
    }

    #[test]
    fn test_rust_unterminated_string() {
        let code = "let s = \"unterminated";
        let tokens = tokenize(Language::Rust, code);
        assert_tiling(code, &tokens);
    }

    #[test]
    fn test_rust_unterminated_block_comment() {
        let code = "let x = 5; /* unterminated";
        let tokens = tokenize(Language::Rust, code);
        assert_tiling(code, &tokens);
    }

    #[test]
    fn test_rust_unterminated_raw_string() {
        let code = "let s = r\"unterminated";
        let tokens = tokenize(Language::Rust, code);
        assert_tiling(code, &tokens);
    }

    #[test]
    fn test_rust_lone_backslash_at_end() {
        let code = "let s = \"backslash\\";
        let tokens = tokenize(Language::Rust, code);
        assert_tiling(code, &tokens);
    }

    #[test]
    fn test_rust_lone_quote() {
        let code = "\"";
        let tokens = tokenize(Language::Rust, code);
        assert_tiling(code, &tokens);
    }

    #[test]
    fn test_toml_comment() {
        test_tokenizes(
            Language::Toml,
            "key = \"value\" # comment",
            TokenKind::Comment,
        );
    }

    #[test]
    fn test_toml_section_header() {
        test_tokenizes(Language::Toml, "[section]", TokenKind::Attribute);
    }

    #[test]
    fn test_toml_key_value() {
        let code = "key = \"value\"";
        let tokens = tokenize(Language::Toml, code);
        assert_tiling(code, &tokens);
        let keywords: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Keyword)
            .collect();
        assert!(!keywords.is_empty());
    }

    #[test]
    fn test_toml_bool() {
        let code = "enabled = true";
        let tokens = tokenize(Language::Toml, code);
        assert_tiling(code, &tokens);
        let keywords: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Keyword)
            .collect();
        assert!(keywords.len() >= 2);
    }

    #[test]
    fn test_json_string_as_key() {
        let code = "{\"key\": \"value\"}";
        let tokens = tokenize(Language::Json, code);
        assert_tiling(code, &tokens);
        let keyword_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Keyword)
            .collect();
        assert!(!keyword_tokens.is_empty());
    }

    #[test]
    fn test_json_string_as_value() {
        let code = "{\"key\": \"value\"}";
        let tokens = tokenize(Language::Json, code);
        assert_tiling(code, &tokens);
        let str_tokens: Vec<_> = tokens.iter().filter(|t| t.kind == TokenKind::Str).collect();
        assert!(!str_tokens.is_empty());
    }

    #[test]
    fn test_json_number() {
        test_tokenizes(Language::Json, "{\"count\": 42}", TokenKind::Number);
    }

    #[test]
    fn test_json_keywords() {
        let code = "true false null";
        let tokens = tokenize(Language::Json, code);
        assert_tiling(code, &tokens);
        let keywords: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Keyword)
            .collect();
        assert_eq!(keywords.len(), 3);
    }

    #[test]
    fn test_shell_comment() {
        test_tokenizes(Language::Shell, "echo hello # comment", TokenKind::Comment);
    }

    #[test]
    fn test_shell_single_quoted_string() {
        test_tokenizes(Language::Shell, "echo 'hello world'", TokenKind::Str);
    }

    #[test]
    fn test_shell_double_quoted_string() {
        test_tokenizes(Language::Shell, "echo \"hello world\"", TokenKind::Str);
    }

    #[test]
    fn test_shell_keywords() {
        let code = "if then else fi";
        let tokens = tokenize(Language::Shell, code);
        assert_tiling(code, &tokens);
        let keywords: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Keyword)
            .collect();
        assert!(keywords.len() >= 4);
    }

    #[test]
    fn test_non_ascii_in_all_languages() {
        let code = "héllo wörld — 🚀";
        for lang in &[
            Language::Rust,
            Language::Toml,
            Language::Json,
            Language::Shell,
        ] {
            let tokens = tokenize(*lang, code);
            assert_tiling(code, &tokens);
        }
    }
}
