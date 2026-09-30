use super::*;

#[test]
fn resolve_content_direction_detects_rtl_script() {
    assert_eq!(
        TextDirection::Content.resolve("שלום"),
        ResolvedTextDirection::Rtl
    );
}

#[test]
fn resolve_content_direction_detects_ltr_script() {
    assert_eq!(
        TextDirection::Content.resolve("Compose"),
        ResolvedTextDirection::Ltr
    );
}

#[test]
fn resolve_content_or_rtl_falls_back_to_rtl() {
    assert_eq!(
        TextDirection::ContentOrRtl.resolve("12345"),
        ResolvedTextDirection::Rtl
    );
}

#[test]
fn resolve_text_direction_defaults_to_ltr_for_unspecified() {
    assert_eq!(
        resolve_text_direction("12345", None),
        ResolvedTextDirection::Ltr
    );
}

#[test]
fn resolve_text_direction_uses_content_for_unspecified() {
    assert_eq!(
        resolve_text_direction("שלום", Some(TextDirection::Unspecified)),
        ResolvedTextDirection::Rtl
    );
}

#[test]
fn line_break_take_or_else_uses_fallback_for_unspecified() {
    let value = LineBreak::Unspecified.take_or_else(|| LineBreak::Simple);
    assert_eq!(value, LineBreak::Simple);
    assert!(LineBreak::Simple.is_specified());
}

#[test]
fn hyphens_take_or_else_uses_fallback_for_unspecified() {
    let value = Hyphens::Unspecified.take_or_else(|| Hyphens::None);
    assert_eq!(value, Hyphens::None);
    assert!(Hyphens::Auto.is_specified());
}

#[test]
fn content_direction_uses_strong_bidi_classes_instead_of_script_ranges() {
    for text in [
        "١٢٣ Latin",
        "، Latin",
        "\u{05b0}Latin",
        "\u{feff}Latin",
        "\u{200e}שלום",
        "१שלום",
    ] {
        assert_eq!(
            TextDirection::Content.resolve(text),
            ResolvedTextDirection::Ltr,
            "{text:?}"
        );
    }
    for text in ["\u{200f}Latin", "\u{061c}Latin", "  שלום", "\u{1e900}Latin"] {
        assert_eq!(
            TextDirection::Content.resolve(text),
            ResolvedTextDirection::Rtl,
            "{text:?}"
        );
    }
}

#[test]
fn content_direction_ignores_nested_and_unmatched_isolates() {
    for text in [
        "\u{2067}שלום\u{2069}Latin",
        "\u{2066}\u{2067}שלום\u{2069}abc\u{2069}Latin",
    ] {
        assert_eq!(
            TextDirection::Content.resolve(text),
            ResolvedTextDirection::Ltr,
            "{text:?}"
        );
    }
    for text in ["\u{2066}Latin\u{2069}שלום", "\u{2069}שלום"] {
        assert_eq!(
            TextDirection::Content.resolve(text),
            ResolvedTextDirection::Rtl,
            "{text:?}"
        );
    }
    for text in [
        "\u{2067}שלום",
        "\u{2066}Latin\u{2069}",
        "\u{2068}שלום\u{2069}",
    ] {
        assert_eq!(
            TextDirection::ContentOrLtr.resolve(text),
            ResolvedTextDirection::Ltr,
            "{text:?}"
        );
        assert_eq!(
            TextDirection::ContentOrRtl.resolve(text),
            ResolvedTextDirection::Rtl,
            "{text:?}"
        );
    }
}

#[test]
fn content_direction_keeps_neutral_fallback_and_explicit_direction() {
    for text in [
        "",
        "$1,053.980",
        "١٢٣",
        "،",
        "\u{05b0}",
        "\u{feff}",
        "\u{202b}\u{202c}",
        "🦀",
    ] {
        assert_eq!(
            TextDirection::ContentOrLtr.resolve(text),
            ResolvedTextDirection::Ltr,
            "{text:?}"
        );
        assert_eq!(
            TextDirection::ContentOrRtl.resolve(text),
            ResolvedTextDirection::Rtl,
            "{text:?}"
        );
    }
    for text in ["Latin", "שלום", "\u{200f}", "\u{2067}שלום\u{2069}"] {
        assert_eq!(TextDirection::Ltr.resolve(text), ResolvedTextDirection::Ltr);
        assert_eq!(TextDirection::Rtl.resolve(text), ResolvedTextDirection::Rtl);
    }
}

#[test]
fn every_ascii_byte_uses_only_letters_as_a_strong_direction() {
    for byte in 0u8..=127 {
        let bytes = [byte];
        let text = std::str::from_utf8(&bytes).expect("ASCII is UTF-8");
        let expected = if byte.is_ascii_alphabetic() {
            ResolvedTextDirection::Ltr
        } else {
            ResolvedTextDirection::Rtl
        };
        assert_eq!(
            TextDirection::ContentOrRtl.resolve(text),
            expected,
            "{byte}"
        );
    }
}

#[test]
#[ignore = "manual release-mode timing probe"]
fn content_direction_scan_timing() {
    use std::{hint::black_box, time::Instant};
    let iterations = std::env::var("DIRECTION_SCAN_ITERATIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10_000_000);
    let cases = [
        (
            "numeric",
            [
                "$1,053.980",
                "+12.42%",
                "09:32:01",
                "1,286,430",
                "0.00012",
                "-24.998",
                "100.00",
                "(320)",
            ],
        ),
        (
            "labels",
            [
                "AAPL",
                "Market",
                "Volume",
                "  Holdings",
                "52-week",
                "Last price",
                "NASDAQ",
                "Change",
            ],
        ),
        (
            "unicode",
            [
                "שלום",
                "مرحبا",
                "中文",
                "Ελλάδα",
                "日本語",
                "Résumé",
                "🦀 hello",
                "Москва",
            ],
        ),
    ];
    for (name, texts) in cases {
        let start = Instant::now();
        for index in 0..iterations {
            black_box(TextDirection::Content.resolve(black_box(texts[index % texts.len()])));
        }
        println!(
            "DIRECTION_SCAN {name} iterations={iterations} ns_per_call={:.3}",
            start.elapsed().as_nanos() as f64 / iterations as f64
        );
    }
}
