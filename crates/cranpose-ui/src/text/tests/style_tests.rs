use super::*;
use crate::{
    modifier::Brush,
    text::{FontFamily, TextDirection},
};

#[test]
fn baseline_shift_reports_specified() {
    assert!(BaselineShift::SUPERSCRIPT.is_specified());
    assert!(!BaselineShift::UNSPECIFIED.is_specified());
}

#[test]
fn locale_list_parses_language_tags() {
    let locale_list = LocaleList::from_language_tags("en-US, ar-EG, ja-JP");
    assert_eq!(locale_list.locales(), &["en-US", "ar-EG", "ja-JP"]);
}

#[test]
fn span_style_merge_prefers_incoming_specified_values() {
    let base = SpanStyle {
        font_size: TextUnit::Sp(14.0),
        font_family: Some(FontFamily::Serif),
        ..Default::default()
    };
    let incoming = SpanStyle {
        font_size: TextUnit::Unspecified,
        letter_spacing: TextUnit::Em(0.1),
        ..Default::default()
    };

    let merged = base.merge(&incoming);
    assert_eq!(merged.font_size, TextUnit::Sp(14.0));
    assert_eq!(merged.letter_spacing, TextUnit::Em(0.1));
    assert_eq!(merged.font_family, Some(FontFamily::Serif));
}

#[test]
fn span_style_merge_switches_foreground_kind() {
    let base = SpanStyle {
        color: Some(Color(1.0, 0.0, 0.0, 1.0)),
        ..Default::default()
    };
    let incoming = SpanStyle {
        brush: Some(Brush::solid(Color(0.0, 1.0, 0.0, 1.0))),
        ..Default::default()
    };

    let merged = base.merge(&incoming);
    assert_eq!(merged.color, None);
    assert_eq!(merged.brush, incoming.brush);
}

#[test]
fn span_style_plus_matches_merge() {
    let base = SpanStyle {
        font_size: TextUnit::Sp(12.0),
        ..Default::default()
    };
    let incoming = SpanStyle {
        letter_spacing: TextUnit::Em(0.2),
        ..Default::default()
    };
    assert_eq!(base.plus(&incoming), base.merge(&incoming));
}

#[test]
fn paragraph_style_merge_prefers_specified_values() {
    let base = ParagraphStyle {
        text_direction: TextDirection::Ltr,
        line_height: TextUnit::Sp(18.0),
        ..Default::default()
    };
    let incoming = ParagraphStyle {
        text_direction: TextDirection::Unspecified,
        line_height: TextUnit::Em(1.4),
        ..Default::default()
    };

    let merged = base.merge(&incoming);
    assert_eq!(merged.text_direction, TextDirection::Ltr);
    assert_eq!(merged.line_height, TextUnit::Em(1.4));
}

#[test]
fn paragraph_style_plus_matches_merge() {
    let base = ParagraphStyle {
        text_align: TextAlign::Start,
        ..Default::default()
    };
    let incoming = ParagraphStyle {
        text_direction: TextDirection::Rtl,
        ..Default::default()
    };
    assert_eq!(base.plus(&incoming), base.merge(&incoming));
}

#[test]
fn resolve_font_size_uses_specified_value() {
    let style = TextStyle::new(
        SpanStyle {
            font_size: TextUnit::Sp(18.0),
            ..Default::default()
        },
        ParagraphStyle::default(),
    );
    assert_eq!(style.resolve_font_size(14.0), 18.0);
}

#[test]
fn resolve_font_size_handles_em_units() {
    let style = TextStyle::new(
        SpanStyle {
            font_size: TextUnit::Em(1.5),
            ..Default::default()
        },
        ParagraphStyle::default(),
    );
    assert_eq!(style.resolve_font_size(16.0), 24.0);
}

#[test]
fn resolve_line_height_uses_style_value() {
    let style = TextStyle::new(
        SpanStyle {
            font_size: TextUnit::Sp(20.0),
            ..Default::default()
        },
        ParagraphStyle {
            line_height: TextUnit::Em(1.2),
            ..Default::default()
        },
    );
    assert_eq!(style.resolve_line_height(14.0, 18.0), 24.0);
}

#[test]
fn resolve_foreground_color_supports_solid_brush_with_alpha() {
    let style = SpanStyle {
        brush: Some(Brush::solid(Color(0.2, 0.4, 0.6, 1.0))),
        alpha: Some(0.5),
        ..Default::default()
    };
    assert_eq!(
        style.resolve_foreground_color(Color(1.0, 1.0, 1.0, 1.0)),
        Color(0.2, 0.4, 0.6, 0.5)
    );
}

#[test]
fn resolve_foreground_color_keeps_default_color_for_gradient_brush() {
    let style = SpanStyle {
        brush: Some(Brush::linear_gradient(vec![
            Color(0.1, 0.2, 0.3, 1.0),
            Color(0.9, 0.8, 0.7, 1.0),
        ])),
        alpha: Some(0.25),
        ..Default::default()
    };

    assert_eq!(
        style.resolve_foreground_color(Color(1.0, 1.0, 1.0, 1.0)),
        Color(1.0, 1.0, 1.0, 0.25)
    );
}

#[test]
fn text_style_merge_combines_span_and_paragraph() {
    let base = TextStyle::new(
        SpanStyle {
            font_family: Some(FontFamily::SansSerif),
            ..Default::default()
        },
        ParagraphStyle {
            text_direction: TextDirection::Ltr,
            ..Default::default()
        },
    );
    let incoming = TextStyle::new(
        SpanStyle {
            letter_spacing: TextUnit::Em(0.2),
            ..Default::default()
        },
        ParagraphStyle {
            line_height: TextUnit::Sp(22.0),
            ..Default::default()
        },
    );

    let merged = base.merge(&incoming);
    assert_eq!(merged.span_style.font_family, Some(FontFamily::SansSerif));
    assert_eq!(merged.span_style.letter_spacing, TextUnit::Em(0.2));
    assert_eq!(merged.paragraph_style.text_direction, TextDirection::Ltr);
    assert_eq!(merged.paragraph_style.line_height, TextUnit::Sp(22.0));
}

#[test]
fn text_style_from_and_to_style_helpers_work() {
    let span_style = SpanStyle {
        font_size: TextUnit::Sp(12.0),
        ..Default::default()
    };
    let from_span = TextStyle::from_span_style(span_style.clone());
    assert_eq!(from_span.to_span_style(), span_style);

    let paragraph_style = ParagraphStyle {
        text_direction: TextDirection::Rtl,
        ..Default::default()
    };
    let from_paragraph = TextStyle::from_paragraph_style(paragraph_style.clone());
    assert_eq!(from_paragraph.to_paragraph_style(), paragraph_style);
}

#[test]
fn text_style_plus_matches_merge() {
    let base = TextStyle::from_span_style(SpanStyle {
        font_size: TextUnit::Sp(10.0),
        ..Default::default()
    });
    let incoming = TextStyle::from_paragraph_style(ParagraphStyle {
        text_direction: TextDirection::Ltr,
        ..Default::default()
    });
    assert_eq!(base.plus(&incoming), base.merge(&incoming));
}

#[test]
fn text_style_platform_style_helpers_roundtrip() {
    let style = TextStyle::default().with_platform_style(Some(PlatformTextStyle {
        span_style: Some(PlatformSpanStyle),
        paragraph_style: Some(PlatformParagraphStyle {
            include_font_padding: Some(false),
            shaping: Some(TextShaping::Basic),
        }),
    }));
    assert_eq!(
        style.platform_style(),
        Some(PlatformTextStyle {
            span_style: Some(PlatformSpanStyle),
            paragraph_style: Some(PlatformParagraphStyle {
                include_font_padding: Some(false),
                shaping: Some(TextShaping::Basic),
            }),
        })
    );
}

#[test]
fn measurement_hash_changes_when_measurement_attributes_change() {
    let style_a = TextStyle::default();
    let style_b = TextStyle::new(
        SpanStyle {
            font_family: Some(FontFamily::SansSerif),
            ..Default::default()
        },
        ParagraphStyle {
            text_direction: TextDirection::Rtl,
            ..Default::default()
        },
    );

    assert_ne!(style_a.measurement_hash(), style_b.measurement_hash());
}

#[test]
fn measurement_hash_includes_platform_style() {
    let style_a = TextStyle::default();
    let style_b = TextStyle::new(
        SpanStyle {
            platform_style: Some(PlatformSpanStyle),
            ..Default::default()
        },
        ParagraphStyle::default(),
    );
    assert_ne!(style_a.measurement_hash(), style_b.measurement_hash());
}

#[test]
fn measurement_hash_includes_platform_paragraph_shaping() {
    let style_a = TextStyle::default();
    let style_b = TextStyle::from_paragraph_style(ParagraphStyle {
        platform_style: Some(PlatformParagraphStyle {
            include_font_padding: None,
            shaping: Some(TextShaping::Basic),
        }),
        ..Default::default()
    });
    assert_ne!(style_a.measurement_hash(), style_b.measurement_hash());
}

#[test]
fn font_feature_settings_reach_every_text_cache_hash() {
    let featured = |settings: &str| SpanStyle {
        font_feature_settings: Some(settings.to_owned()),
        ..Default::default()
    };
    let plain = SpanStyle::default();
    let spanned = |span: SpanStyle| crate::text::AnnotatedString {
        text: "0123".to_owned(),
        span_styles: vec![crate::text::RangeStyle {
            item: span,
            range: 0..2,
        }],
        ..Default::default()
    };
    for (a, b) in [
        (&plain, &featured("zero")),
        (&featured("zero"), &featured("smcp")),
    ] {
        let (style_a, style_b) = (
            TextStyle::from_span_style(a.clone()),
            TextStyle::from_span_style(b.clone()),
        );
        assert_ne!(style_a.measurement_hash(), style_b.measurement_hash());
        assert_ne!(style_a.render_hash(), style_b.render_hash());
        assert_ne!(a.render_hash(), b.render_hash());
        let (text_a, text_b) = (spanned(a.clone()), spanned(b.clone()));
        assert_ne!(
            text_a.span_measurement_hash(),
            text_b.span_measurement_hash()
        );
        assert_ne!(text_a.render_hash(), text_b.render_hash());
    }
}

#[test]
fn span_style_render_hash_changes_for_visual_attributes() {
    let plain = SpanStyle::default();
    let decorated = SpanStyle {
        shadow: Some(Shadow {
            color: Color(1.0, 0.0, 0.0, 0.5),
            offset: crate::modifier::Point::new(2.0, 3.0),
            blur_radius: 4.0,
        }),
        draw_style: Some(TextDrawStyle::Stroke { width: 2.0 }),
        ..Default::default()
    };

    assert_ne!(plain.render_hash(), decorated.render_hash());
}

#[test]
fn paragraph_style_render_hash_changes_for_paragraph_attributes() {
    let base = ParagraphStyle::default();
    let aligned = ParagraphStyle {
        text_align: TextAlign::Center,
        text_direction: TextDirection::Rtl,
        ..Default::default()
    };

    assert_ne!(base.render_hash(), aligned.render_hash());
}

#[test]
fn text_style_render_hash_includes_visual_attributes() {
    let base = TextStyle::default();
    let tinted = TextStyle::from_span_style(SpanStyle {
        color: Some(Color(0.1, 0.2, 0.3, 1.0)),
        background: Some(Color(0.9, 0.8, 0.7, 1.0)),
        ..Default::default()
    });

    assert_ne!(base.render_hash(), tinted.render_hash());
}

#[test]
fn text_align_fraction_puts_the_slack_before_the_text_by_alignment_and_direction() {
    let aligned = |text_align| {
        let mut style = TextStyle::default();
        style.paragraph_style.text_align = text_align;
        style
    };
    assert_eq!(text_align_fraction(&aligned(TextAlign::Start), "abc"), 0.0);
    assert_eq!(text_align_fraction(&aligned(TextAlign::Center), "abc"), 0.5);
    assert_eq!(text_align_fraction(&aligned(TextAlign::End), "abc"), 1.0);
    assert_eq!(text_align_fraction(&aligned(TextAlign::Right), "abc"), 1.0);
    assert_eq!(
        text_align_fraction(&aligned(TextAlign::Start), "שלום"),
        1.0,
        "a right-to-left paragraph starts at the right edge"
    );
}

#[test]
fn text_alignment_respects_physical_edges_and_paragraph_direction() {
    let cases = [
        (TextDirection::Ltr, "שלום", false),
        (TextDirection::Rtl, "hello", true),
        (TextDirection::Content, "hello", false),
        (TextDirection::Content, "שלום", true),
        (TextDirection::ContentOrLtr, "123.45", false),
        (TextDirection::ContentOrRtl, "123.45", true),
        (TextDirection::Unspecified, "hello", false),
        (TextDirection::Unspecified, "שלום", true),
        (TextDirection::Content, "١٢٣ Latin", false),
        (TextDirection::Content, "\u{200f}Latin", true),
        (TextDirection::Content, "\u{200e}שלום", false),
        (TextDirection::Content, "\u{2067}שלום\u{2069}Latin", false),
        (TextDirection::Content, "\u{2066}Latin\u{2069}שלום", true),
        (TextDirection::ContentOrLtr, "١٢٣", false),
        (TextDirection::ContentOrRtl, "١٢٣", true),
    ];
    let alignments = [
        (TextAlign::Left, 0.0, 0.0),
        (TextAlign::Right, 1.0, 1.0),
        (TextAlign::Center, 0.5, 0.5),
        (TextAlign::Start, 0.0, 1.0),
        (TextAlign::End, 1.0, 0.0),
        (TextAlign::Justify, 0.0, 1.0),
        (TextAlign::Unspecified, 0.0, 1.0),
    ];
    for (text_direction, text, rtl) in cases {
        for (text_align, ltr_fraction, rtl_fraction) in alignments {
            let mut style = TextStyle::default();
            style.paragraph_style.text_direction = text_direction;
            style.paragraph_style.text_align = text_align;
            let expected = if rtl { rtl_fraction } else { ltr_fraction };
            assert_eq!(
                text_align_fraction(&style, text),
                expected,
                "{text_align:?}, {text_direction:?}, {text:?}"
            );
        }
    }
}

#[test]
fn span_measurement_hash_ignores_paint_and_follows_metrics() {
    let spanned = |span: SpanStyle| crate::text::AnnotatedString {
        text: "0123".to_owned(),
        span_styles: vec![crate::text::RangeStyle {
            item: span,
            range: 0..2,
        }],
        ..Default::default()
    };
    let plain = spanned(SpanStyle::default());
    let painted = spanned(SpanStyle {
        color: Some(Color(1.0, 0.0, 0.0, 1.0)),
        background: Some(Color(0.0, 0.0, 1.0, 0.5)),
        brush: Some(Brush::solid(Color(0.0, 1.0, 0.0, 1.0))),
        text_decoration: Some(TextDecoration::UNDERLINE),
        ..Default::default()
    });
    let larger = spanned(SpanStyle {
        font_size: TextUnit::Sp(30.0),
        ..Default::default()
    });
    assert_eq!(
        plain.span_measurement_hash(),
        painted.span_measurement_hash()
    );
    assert_ne!(
        plain.span_measurement_hash(),
        larger.span_measurement_hash()
    );
    assert_ne!(plain.render_hash(), painted.render_hash());
}
