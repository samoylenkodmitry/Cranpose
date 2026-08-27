use cranpose_ui::{
    Modifier, TextLayoutOptions, TextOptions, TextOverflow, TextStyle, TextWithOptions,
    run_test_composition,
};

#[test]
fn text_options_express_single_line_ellipsis_contract() {
    let layout = TextLayoutOptions::from(TextOptions {
        overflow: TextOverflow::Ellipsis,
        soft_wrap: false,
        max_lines: Some(1),
        ..TextOptions::default()
    });

    assert_eq!(layout.overflow, TextOverflow::Ellipsis);
    assert!(!layout.soft_wrap);
    assert_eq!(layout.max_lines, 1);
    assert_eq!(layout.min_lines, 1);
}

#[test]
fn text_options_express_scale_down_contract() {
    let layout = TextLayoutOptions::from(TextOptions {
        overflow: TextOverflow::ScaleDown {
            min_font_size_sp: 9.0,
        },
        soft_wrap: false,
        max_lines: Some(1),
        ..TextOptions::default()
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

#[test]
fn text_with_options_is_available_to_integration_users() {
    let composition = run_test_composition(|| {
        TextWithOptions(
            "Save Cranpose WebP",
            Modifier::empty(),
            TextStyle::default(),
            TextOptions {
                overflow: TextOverflow::Ellipsis,
                soft_wrap: false,
                max_lines: Some(1),
                ..TextOptions::default()
            },
        );
    });

    assert!(composition.root().is_some());
}
