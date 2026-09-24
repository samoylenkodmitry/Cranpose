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
            assert!(token.start <= token.end, "Token {i}: start > end");

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
                        "Token {i} and {j} overlap"
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
        "Expected {expected_kind:?} token not found"
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
        "Expected at least {min_count} {kind:?} tokens, found {count}"
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
