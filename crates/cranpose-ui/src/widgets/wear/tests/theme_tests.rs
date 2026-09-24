use super::*;

#[test]
fn the_indicator_colours_come_from_on_background_and_nothing_else() {
    let colors = WearColors {
        on_background: Color::from_rgb_u8(0xDF, 0xF6, 0xFF),
        ..WearColors::default()
    }
    .with_wear_scroll_indicator();
    assert_eq!(colors.indicator_thumb, Color::from_rgb_u8(180, 202, 211));
    assert_eq!(colors.indicator_track, Color::from_rgb_u8(30, 51, 58));
    let default = WearColors::default();
    assert_eq!(default, default.with_wear_scroll_indicator());
}

#[test]
fn a_bare_text_is_white_and_a_header_is_not() {
    let colors = WearColors::default();
    assert_eq!(colors.content, Color::WHITE);
    assert_ne!(colors.content, colors.on_background);
}

#[test]
fn the_type_scale_carries_the_sizes_wear_declares() {
    assert_eq!(WearTextStyle::TITLE_MEDIUM.size_sp, 16.0);
    assert_eq!(WearTextStyle::TITLE_MEDIUM.line_height_sp, 18.0);
    assert_eq!(WearTextStyle::TITLE_MEDIUM.weight, 550);
    assert_eq!(WearTextStyle::LABEL_MEDIUM.size_sp, 15.0);
    assert_eq!(WearTextStyle::LABEL_SMALL.size_sp, 13.0);
    assert_eq!(WearTextStyle::LABEL_SMALL.line_height_sp, 16.0);
    assert_eq!(WearTextStyle::BODY_LARGE.weight, 450);
    for style in [
        WearTextStyle::TITLE_MEDIUM,
        WearTextStyle::LABEL_MEDIUM,
        WearTextStyle::LABEL_SMALL,
        WearTextStyle::BODY_LARGE,
    ] {
        assert_eq!(style.tracking_sp, 0.4);
    }
}

#[test]
fn overriding_the_size_keeps_the_line_height_it_inherited() {
    let small = WearTextStyle::BODY_LARGE.at_size(12.0);
    assert_eq!(small.size_sp, 12.0);
    assert_eq!(small.line_height_sp, 18.0);
    let credit_line = small.with_line_height(16.0);
    assert_eq!(credit_line.line_height_sp, 16.0);
}

#[test]
fn the_scale_itself_states_no_alignment_and_a_call_site_can() {
    for style in [
        WearTextStyle::TITLE_MEDIUM,
        WearTextStyle::LABEL_MEDIUM,
        WearTextStyle::LABEL_SMALL,
        WearTextStyle::BODY_LARGE,
    ] {
        assert_eq!(style.align, TextAlign::Unspecified);
        assert_eq!(
            style.resolve(Color::WHITE).paragraph_style.text_align,
            TextAlign::Unspecified
        );
    }
    let centred = WearTextStyle::BODY_LARGE
        .at_size(12.0)
        .with_line_height(16.0)
        .aligned(TextAlign::Center);
    assert_eq!(centred.align, TextAlign::Center);
    assert_eq!(
        centred.resolve(Color::WHITE).paragraph_style.text_align,
        TextAlign::Center
    );
    assert_eq!(centred.size_sp, 12.0);
    assert_eq!(centred.line_height_sp, 16.0);
    assert_eq!(centred.weight, WearTextStyle::BODY_LARGE.weight);
}

#[test]
fn every_entry_names_a_family_so_its_text_has_a_face_to_draw_with() {
    for style in [
        WearTextStyle::TITLE_MEDIUM,
        WearTextStyle::LABEL_MEDIUM,
        WearTextStyle::LABEL_SMALL,
        WearTextStyle::BODY_LARGE,
    ] {
        assert_eq!(
            style.resolve(Color::WHITE).span_style.font_family,
            Some(FontFamily::SansSerif),
            "Wear's brand token resolves to sans-serif on a device with no \
             RobotoFlex-Regular.ttf, which is every Wear OS 5 image measured"
        );
    }
    assert_eq!(
        WearTextStyle::BODY_LARGE
            .resolve_in(Color::WHITE, FontFamily::Monospace)
            .span_style
            .font_family,
        Some(FontFamily::Monospace),
        "a caller can still name its own"
    );
}

#[test]
fn a_resolved_style_keeps_its_sizes_in_sp_for_the_framework_to_scale() {
    let style = WearTextStyle::LABEL_MEDIUM.resolve(Color::WHITE);
    assert_eq!(style.span_style.font_size, TextUnit::Sp(15.0));
    assert_eq!(style.paragraph_style.line_height, TextUnit::Sp(18.0));
    assert_eq!(style.span_style.font_weight, Some(FontWeight(500)));
    assert_eq!(style.span_style.letter_spacing, TextUnit::Sp(0.4));
}

#[test]
fn a_resolved_style_asks_for_the_wear_line_box_rule() {
    let style = WearTextStyle::TITLE_MEDIUM.resolve(Color::WHITE);
    let policy = style
        .paragraph_style
        .line_height_style
        .expect("a Wear style names its line-height policy");
    assert_eq!(policy.alignment, LineHeightAlignment::Center);
    assert_eq!(policy.trim, LineHeightTrim::None);
    assert_eq!(policy.mode, LineHeightMode::Minimum);
    assert_eq!(
        style
            .paragraph_style
            .platform_style
            .and_then(|platform| platform.include_font_padding),
        Some(false)
    );
}
