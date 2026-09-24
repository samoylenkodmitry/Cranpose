use super::*;

#[test]
fn normalized_enforces_minimum_one_line() {
    let options = TextLayoutOptions {
        min_lines: 0,
        max_lines: 0,
        ..Default::default()
    }
    .normalized();

    assert_eq!(options.min_lines, 1);
    assert_eq!(options.max_lines, 1);
}

#[test]
fn normalized_ensures_max_not_smaller_than_min() {
    let options = TextLayoutOptions {
        min_lines: 3,
        max_lines: 1,
        ..Default::default()
    }
    .normalized();

    assert_eq!(options.min_lines, 3);
    assert_eq!(options.max_lines, 3);
}

#[test]
fn normalized_limits_unwrapped_ellipsis_text_to_one_line() {
    for overflow in [
        TextOverflow::Ellipsis,
        TextOverflow::StartEllipsis,
        TextOverflow::MiddleEllipsis,
    ] {
        let options = TextLayoutOptions {
            overflow,
            soft_wrap: false,
            max_lines: 4,
            min_lines: 2,
        }
        .normalized();

        assert_eq!(options.max_lines, 1, "{overflow:?}");
        assert_eq!(options.min_lines, 2, "{overflow:?}");
    }
}

#[test]
fn normalized_keeps_max_lines_of_wrapped_or_non_ellipsis_text() {
    for (overflow, soft_wrap) in [
        (TextOverflow::Ellipsis, true),
        (TextOverflow::Clip, false),
        (TextOverflow::Visible, false),
        (
            TextOverflow::ScaleDown {
                min_font_size_sp: 9.0,
            },
            false,
        ),
    ] {
        let options = TextLayoutOptions {
            overflow,
            soft_wrap,
            max_lines: 4,
            min_lines: 1,
        }
        .normalized();

        assert_eq!(options.max_lines, 4, "{overflow:?} soft_wrap={soft_wrap}");
    }
}

#[test]
fn normalized_sanitizes_scale_down_min_font_size() {
    let options = TextLayoutOptions {
        overflow: TextOverflow::ScaleDown {
            min_font_size_sp: f32::NAN,
        },
        ..Default::default()
    }
    .normalized();

    assert_eq!(
        options.overflow,
        TextOverflow::ScaleDown {
            min_font_size_sp: 1.0
        }
    );
}

#[test]
fn text_options_default_maps_to_unlimited_layout() {
    let layout = TextLayoutOptions::from(TextOptions::default());

    assert_eq!(layout.overflow, TextOverflow::Clip);
    assert!(layout.soft_wrap);
    assert_eq!(layout.max_lines, usize::MAX);
    assert_eq!(layout.min_lines, 1);
}

#[test]
fn text_options_maps_optional_max_lines_to_layout_limit() {
    let layout = TextLayoutOptions::from(TextOptions {
        overflow: TextOverflow::Ellipsis,
        soft_wrap: false,
        max_lines: Some(1),
        min_lines: 1,
    });

    assert_eq!(layout.overflow, TextOverflow::Ellipsis);
    assert!(!layout.soft_wrap);
    assert_eq!(layout.max_lines, 1);
    assert_eq!(layout.min_lines, 1);
}

#[test]
fn text_options_preserve_scale_down_overflow() {
    let layout = TextLayoutOptions::from(TextOptions {
        overflow: TextOverflow::ScaleDown {
            min_font_size_sp: 9.0,
        },
        soft_wrap: false,
        max_lines: Some(1),
        min_lines: 1,
    });

    assert_eq!(
        layout.overflow,
        TextOverflow::ScaleDown {
            min_font_size_sp: 9.0
        }
    );
    assert!(!layout.soft_wrap);
    assert_eq!(layout.max_lines, 1);
}
