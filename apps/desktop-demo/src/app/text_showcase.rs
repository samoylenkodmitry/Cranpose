#![allow(non_snake_case)]

use cranpose_ui::text::{
    AnnotatedString, BaselineShift, FontFamily, FontStyle, FontSynthesis, FontWeight, Hyphens,
    LineBreak, LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim, LocaleList,
    ParagraphStyle, PlatformParagraphStyle, PlatformSpanStyle, PlatformTextStyle,
    Shadow as TextShadow, SpanStyle, TextAlign, TextDecoration, TextDirection, TextDrawStyle,
    TextGeometricTransform, TextIndent, TextMotion, TextOverflow, TextUnit,
};
use cranpose_ui::{
    composable, BasicText, Box, BoxSpec, Brush, Color, Column, ColumnSpec, LinearArrangement,
    Modifier, Point, Row, RowSpec, Size, Spacer, Text, TextStyle, VerticalAlignment,
};

const OVERFLOW_SAMPLE: &str =
    "Overflow sample: Supercalifragilisticexpialidocious / חדשות / Compose-like text rendering";
const PARAGRAPH_SAMPLE: &str =
    "This paragraph demonstrates textIndent, lineHeight, lineBreak and hyphenation knobs in a constrained width box.";

fn tab_root_modifier() -> Modifier {
    Modifier::empty()
        .fill_max_width()
        .padding(18.0)
        .background(Color(0.06, 0.07, 0.11, 1.0))
        .rounded_corners(18.0)
        .padding(18.0)
}

fn section_modifier() -> Modifier {
    Modifier::empty()
        .fill_max_width()
        .background(Color(0.12, 0.14, 0.2, 0.95))
        .rounded_corners(14.0)
        .padding(12.0)
}

fn card_modifier() -> Modifier {
    Modifier::empty()
        .fill_max_width()
        .background(Color(0.08, 0.09, 0.14, 1.0))
        .rounded_corners(10.0)
        .padding(10.0)
}

fn heading_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.92, 0.94, 0.99, 1.0)),
            font_weight: Some(FontWeight::BOLD),
            font_size: TextUnit::Sp(22.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn section_title_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.83, 0.9, 1.0, 1.0)),
            font_weight: Some(FontWeight::SEMI_BOLD),
            font_size: TextUnit::Sp(16.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn body_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.9, 0.92, 0.97, 1.0)),
            font_size: TextUnit::Sp(14.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn tiny_label_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.72, 0.78, 0.9, 1.0)),
            font_size: TextUnit::Sp(12.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn style_chip_label(text: &'static str) {
    Text(text, Modifier::empty(), tiny_label_style());
}

fn overflow_sample(label: &'static str, overflow: TextOverflow) {
    style_chip_label(label);
    BasicText(
        OVERFLOW_SAMPLE,
        Modifier::empty()
            .width(420.0)
            .background(Color(0.15, 0.17, 0.24, 1.0))
            .rounded_corners(8.0)
            .padding(8.0),
        TextStyle {
            span_style: SpanStyle {
                color: Some(Color(0.95, 0.97, 1.0, 1.0)),
                font_size: TextUnit::Sp(14.0),
                ..Default::default()
            },
            ..Default::default()
        },
        overflow,
        false,
        1,
        1,
    );
}

#[composable]
pub(crate) fn TextShowcaseTab() {
    Column(
        tab_root_modifier(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(14.0)),
        move || {
            Text(
                "Text Rendering Feature Showcase",
                Modifier::empty(),
                heading_style(),
            );
            Text(
                "Covers span styling, paragraph layout knobs, direction/alignment, and overflow handling.",
                Modifier::empty(),
                body_style(),
            );

            Column(
                section_modifier(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
                move || {
                    Text(
                        "SpanStyle Features",
                        Modifier::empty(),
                        section_title_style(),
                    );

                    Column(
                        card_modifier(),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                        move || {
                            style_chip_label(
                                "color + font_size + font_weight + font_style + font_family",
                            );
                            Text(
                                "Serif Bold Italic",
                                Modifier::empty(),
                                TextStyle {
                                    span_style: SpanStyle {
                                        color: Some(Color(0.98, 0.9, 0.6, 1.0)),
                                        font_size: TextUnit::Sp(20.0),
                                        font_weight: Some(FontWeight::BOLD),
                                        font_style: Some(FontStyle::Italic),
                                        font_synthesis: Some(FontSynthesis::All),
                                        font_family: Some(FontFamily::Serif),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                            );

                            style_chip_label(
                                "letter_spacing + text_decoration + background + shadow",
                            );
                            Text(
                                "Decorated shadow text",
                                Modifier::empty(),
                                TextStyle {
                                    span_style: SpanStyle {
                                        color: Some(Color(0.95, 0.98, 1.0, 1.0)),
                                        font_size: TextUnit::Sp(18.0),
                                        letter_spacing: TextUnit::Em(0.18),
                                        background: Some(Color(0.2, 0.3, 0.52, 0.55)),
                                        text_decoration: Some(
                                            TextDecoration::UNDERLINE
                                                .combine(TextDecoration::LINE_THROUGH),
                                        ),
                                        shadow: Some(TextShadow {
                                            color: Color(0.0, 0.0, 0.0, 0.95),
                                            offset: Point::new(2.0, 2.0),
                                            blur_radius: 3.0,
                                        }),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                            );

                            style_chip_label(
                                "baseline_shift + text_geometric_transform + locale_list + font_feature_settings",
                            );
                            Row(
                                Modifier::empty(),
                                RowSpec::new()
                                    .horizontal_arrangement(LinearArrangement::SpacedBy(16.0))
                                    .vertical_alignment(VerticalAlignment::CenterVertically),
                                move || {
                                    Text(
                                        "Shifted ↑",
                                        Modifier::empty(),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(0.8, 1.0, 0.9, 1.0)),
                                                baseline_shift: Some(BaselineShift::SUPERSCRIPT),
                                                text_geometric_transform: Some(
                                                    TextGeometricTransform {
                                                        scale_x: 1.1,
                                                        skew_x: 0.18,
                                                    },
                                                ),
                                                locale_list: Some(LocaleList::from_language_tags(
                                                    "en-US,ja-JP",
                                                )),
                                                font_feature_settings: Some(
                                                    "'liga' 1, 'kern' 1".to_string(),
                                                ),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );
                                    Text(
                                        "Shifted ↓",
                                        Modifier::empty(),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(1.0, 0.82, 0.82, 1.0)),
                                                baseline_shift: Some(BaselineShift::SUBSCRIPT),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );
                                },
                            );

                            style_chip_label("brush + alpha + draw_style + platform_style");
                            Text(
                                "Gradient/alpha/stroke/platform",
                                Modifier::empty(),
                                TextStyle {
                                    span_style: SpanStyle {
                                        brush: Some(Brush::linear_gradient(vec![
                                            Color(0.5, 0.95, 1.0, 1.0),
                                            Color(1.0, 0.72, 0.5, 1.0),
                                        ])),
                                        alpha: Some(0.88),
                                        draw_style: Some(TextDrawStyle::Stroke { width: 1.5 }),
                                        platform_style: Some(PlatformSpanStyle),
                                        ..Default::default()
                                    },
                                    paragraph_style: ParagraphStyle {
                                        platform_style: Some(PlatformParagraphStyle {
                                            include_font_padding: Some(false),
                                        }),
                                        ..Default::default()
                                    },
                                },
                            );
                        },
                    );
                },
            );

            Column(
                section_modifier(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
                move || {
                    Text(
                        "AnnotatedString Features",
                        Modifier::empty(),
                        section_title_style(),
                    );

                    Column(
                        card_modifier(),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                        move || {
                            style_chip_label("Multiple SpanStyles applied to a single text node");

                            let annotated_text = AnnotatedString::builder()
                                .push_style(SpanStyle {
                                    color: Some(Color(0.5, 0.9, 0.6, 1.0)),
                                    font_weight: Some(FontWeight::BOLD),
                                    ..Default::default()
                                })
                                .append("This is bold green ")
                                .pop()
                                .append("and this is normal text. ")
                                .push_style(SpanStyle {
                                    color: Some(Color(0.9, 0.4, 0.4, 1.0)),
                                    font_style: Some(FontStyle::Italic),
                                    text_decoration: Some(TextDecoration::UNDERLINE),
                                    ..Default::default()
                                })
                                .append("This is red, italic, and underlined!")
                                .pop()
                                .to_annotated_string();

                            Text(
                                annotated_text,
                                Modifier::empty()
                                    .background(Color(0.2, 0.16, 0.12, 1.0))
                                    .rounded_corners(8.0)
                                    .padding(8.0),
                                TextStyle {
                                    span_style: SpanStyle {
                                        color: Some(Color(1.0, 0.9, 0.82, 1.0)),
                                        font_size: TextUnit::Sp(14.0),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                            );

                            style_chip_label("Inline Styles inside Builder");

                            let annotated_text2 = AnnotatedString::builder()
                                .append("You can also ")
                                .push_style(SpanStyle {
                                    font_size: TextUnit::Sp(22.0),
                                    color: Some(Color(0.4, 0.8, 1.0, 1.0)),
                                    ..Default::default()
                                })
                                .append("change font size")
                                .pop()
                                .append(" dynamically mid-sentence!")
                                .to_annotated_string();

                            Text(
                                annotated_text2,
                                Modifier::empty()
                                    .background(Color(0.12, 0.15, 0.22, 1.0))
                                    .rounded_corners(8.0)
                                    .padding(8.0),
                                TextStyle {
                                    span_style: SpanStyle {
                                        color: Some(Color(0.95, 0.97, 1.0, 1.0)),
                                        font_size: TextUnit::Sp(14.0),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                            );
                        },
                    );
                },
            );

            Column(
                section_modifier(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
                move || {
                    Text(
                        "ParagraphStyle Features",
                        Modifier::empty(),
                        section_title_style(),
                    );

                    Column(
                        card_modifier(),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                        move || {
                            style_chip_label("text_align + text_direction (LTR / RTL start-end)");

                            Box(
                                Modifier::empty()
                                    .fill_max_width()
                                    .background(Color(0.2, 0.16, 0.12, 1.0))
                                    .rounded_corners(8.0)
                                    .padding(8.0),
                                BoxSpec::default(),
                                move || {
                                    Text(
                                        "Start + LTR",
                                        Modifier::empty().fill_max_width(),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(1.0, 0.9, 0.82, 1.0)),
                                                ..Default::default()
                                            },
                                            paragraph_style: ParagraphStyle {
                                                text_align: TextAlign::Start,
                                                text_direction: TextDirection::Ltr,
                                                ..Default::default()
                                            },
                                        },
                                    );
                                    Spacer(Size {
                                        width: 0.0,
                                        height: 4.0,
                                    });
                                    Text(
                                        "Start + RTL (aligns right)",
                                        Modifier::empty().fill_max_width(),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(0.88, 1.0, 0.86, 1.0)),
                                                ..Default::default()
                                            },
                                            paragraph_style: ParagraphStyle {
                                                text_align: TextAlign::Start,
                                                text_direction: TextDirection::Rtl,
                                                ..Default::default()
                                            },
                                        },
                                    );
                                },
                            );

                            style_chip_label(
                                "line_height + text_indent + line_height_style + line_break + hyphens + text_motion",
                            );
                            BasicText(
                                PARAGRAPH_SAMPLE,
                                Modifier::empty()
                                    .width(440.0)
                                    .background(Color(0.12, 0.15, 0.22, 1.0))
                                    .rounded_corners(8.0)
                                    .padding(8.0),
                                TextStyle {
                                    span_style: SpanStyle {
                                        color: Some(Color(0.95, 0.97, 1.0, 1.0)),
                                        font_size: TextUnit::Sp(14.0),
                                        ..Default::default()
                                    },
                                    paragraph_style: ParagraphStyle {
                                        text_align: TextAlign::Justify,
                                        text_direction: TextDirection::ContentOrLtr,
                                        line_height: TextUnit::Em(1.65),
                                        text_indent: Some(TextIndent {
                                            first_line: TextUnit::Sp(24.0),
                                            rest_line: TextUnit::Sp(8.0),
                                        }),
                                        line_height_style: Some(LineHeightStyle {
                                            alignment: LineHeightAlignment::Center,
                                            trim: LineHeightTrim::Both,
                                            mode: LineHeightMode::Minimum,
                                        }),
                                        line_break: LineBreak::Paragraph,
                                        hyphens: Hyphens::Auto,
                                        text_motion: Some(TextMotion::Animated),
                                        ..Default::default()
                                    },
                                },
                                TextOverflow::Clip,
                                true,
                                4,
                                1,
                            );

                            Spacer(Size {
                                width: 0.0,
                                height: 4.0,
                            });
                            style_chip_label(
                                "Worldwide Hyphenation Dictionaries (fr-FR, de-DE, es-ES locales)",
                            );
                            Row(
                                Modifier::empty(),
                                RowSpec::new()
                                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
                                move || {
                                    BasicText(
                                        "Hyphenation test: configuration super-éléphant transformation magnifique.",
                                        Modifier::empty()
                                            .width(140.0)
                                            .background(Color(0.18, 0.12, 0.16, 1.0))
                                            .rounded_corners(8.0)
                                            .padding(8.0),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(1.0, 0.9, 0.95, 1.0)),
                                                font_size: TextUnit::Sp(14.0),
                                                locale_list: Some(LocaleList::from_language_tags("fr-FR")),
                                                ..Default::default()
                                            },
                                            paragraph_style: ParagraphStyle {
                                                line_break: LineBreak::Paragraph,
                                                hyphens: Hyphens::Auto,
                                                ..Default::default()
                                            },
                                        },
                                        TextOverflow::Clip,
                                        true,
                                        4,
                                        1,
                                    );
                                    BasicText(
                                        "Streichholzschächtelchen Windschutzscheibenschmutz",
                                        Modifier::empty()
                                            .width(140.0)
                                            .background(Color(0.12, 0.15, 0.16, 1.0))
                                            .rounded_corners(8.0)
                                            .padding(8.0),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(0.9, 0.95, 1.0, 1.0)),
                                                font_size: TextUnit::Sp(14.0),
                                                locale_list: Some(LocaleList::from_language_tags(
                                                    "de-DE",
                                                )),
                                                ..Default::default()
                                            },
                                            paragraph_style: ParagraphStyle {
                                                line_break: LineBreak::Paragraph,
                                                hyphens: Hyphens::Auto,
                                                ..Default::default()
                                            },
                                        },
                                        TextOverflow::Clip,
                                        true,
                                        4,
                                        1,
                                    );
                                    BasicText(
                                        "Desoxirribonucleótido electroencefalografista",
                                        Modifier::empty()
                                            .width(140.0)
                                            .background(Color(0.16, 0.14, 0.12, 1.0))
                                            .rounded_corners(8.0)
                                            .padding(8.0),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(1.0, 0.95, 0.9, 1.0)),
                                                font_size: TextUnit::Sp(14.0),
                                                locale_list: Some(LocaleList::from_language_tags(
                                                    "es-ES",
                                                )),
                                                ..Default::default()
                                            },
                                            paragraph_style: ParagraphStyle {
                                                line_break: LineBreak::Paragraph,
                                                hyphens: Hyphens::Auto,
                                                ..Default::default()
                                            },
                                        },
                                        TextOverflow::Clip,
                                        true,
                                        4,
                                        1,
                                    );
                                },
                            );
                        },
                    );
                },
            );

            Column(
                section_modifier(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
                move || {
                    Text(
                        "Overflow and Wrapping",
                        Modifier::empty(),
                        section_title_style(),
                    );

                    Column(
                        card_modifier(),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                        move || {
                            overflow_sample("Clip", TextOverflow::Clip);
                            overflow_sample("Ellipsis", TextOverflow::Ellipsis);
                            overflow_sample("StartEllipsis", TextOverflow::StartEllipsis);
                            overflow_sample("MiddleEllipsis", TextOverflow::MiddleEllipsis);
                            overflow_sample("Visible", TextOverflow::Visible);
                        },
                    );
                },
            );

            Column(
                section_modifier(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    Text(
                        "Composed TextStyle",
                        Modifier::empty(),
                        section_title_style(),
                    );
                    Text(
                        "TextStyle is now strictly composed of SpanStyle and ParagraphStyle, matching Compose's model directly.",
                        Modifier::empty(),
                        body_style(),
                    );
                    Text(
                        "Nested styles can be combined in one value and applied in one Text call.",
                        Modifier::empty()
                            .background(Color(0.14, 0.16, 0.22, 1.0))
                            .rounded_corners(8.0)
                            .padding(8.0),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.98, 0.94, 0.8, 1.0)),
                                font_size: TextUnit::Sp(17.0),
                                font_weight: Some(FontWeight::SEMI_BOLD),
                                font_family: Some(FontFamily::Monospace),
                                ..Default::default()
                            },
                            paragraph_style: ParagraphStyle {
                                text_align: TextAlign::Center,
                                text_direction: TextDirection::Ltr,
                                line_height: TextUnit::Em(1.3),
                                text_motion: Some(TextMotion::Static),
                                ..Default::default()
                            },
                        },
                    );

                    let composed_span = TextStyle::from_span_style(SpanStyle {
                        color: Some(Color(0.92, 0.98, 0.82, 1.0)),
                        font_size: TextUnit::Sp(15.0),
                        ..Default::default()
                    });
                    let composed_paragraph = TextStyle::from_paragraph_style(ParagraphStyle {
                        text_align: TextAlign::Start,
                        text_direction: TextDirection::ContentOrLtr,
                        line_height: TextUnit::Em(1.25),
                        line_break: LineBreak::Heading,
                        hyphens: Hyphens::Auto,
                        text_motion: Some(TextMotion::Animated),
                        ..Default::default()
                    });
                    let merged_style =
                        composed_span
                            .merge(&composed_paragraph)
                            .plus(&TextStyle::new(
                                SpanStyle {
                                    font_weight: Some(FontWeight::MEDIUM),
                                    ..Default::default()
                                },
                                ParagraphStyle::default(),
                            ));
                    let platform_style =
                        merged_style
                            .clone()
                            .with_platform_style(Some(PlatformTextStyle {
                                span_style: Some(PlatformSpanStyle),
                                paragraph_style: Some(PlatformParagraphStyle {
                                    include_font_padding: Some(false),
                                }),
                            }));
                    Text(
                        "TextStyle::from_span_style + from_paragraph_style + merge + plus",
                        Modifier::empty(),
                        merged_style,
                    );
                    let span_has_color = platform_style.to_span_style().color.is_some();
                    let include_font_padding = platform_style
                        .to_paragraph_style()
                        .platform_style
                        .and_then(|style| style.include_font_padding)
                        .unwrap_or(true);
                    Text(
                        format!(
                            "TextStyle::with_platform_style + to_span_style + to_paragraph_style (span color: {span_has_color}, include_font_padding: {include_font_padding})"
                        ),
                        Modifier::empty(),
                        platform_style,
                    );
                },
            );
        },
    );
}
