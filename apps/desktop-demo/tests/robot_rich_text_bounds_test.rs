use cranpose::{prelude::*, text::*};
use cranpose_render_common::text_fonts::TextFonts;
use cranpose_testing::robot::create_headless_robot_test_with_fonts;

#[test]
fn test_rich_text_bounds() {
    let fonts = TextFonts::from_fonts_or_default(&[]);

    let mut robot = create_headless_robot_test_with_fonts(800, 600, fonts, || {
        let annotated_text = AnnotatedString::builder()
            .push_style(SpanStyle {
                font_size: TextUnit::Sp(30.0),
                ..Default::default()
            })
            .append("BIG ")
            .pop()
            .push_style(SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            })
            .append("small")
            .pop()
            .to_annotated_string();

        Column(Modifier::empty(), ColumnSpec::new(), move || {
            Text(
                annotated_text.clone(),
                Modifier::empty(),
                TextStyle::default(),
            );
        });
    });

    let bounds = robot
        .find_by_text("BIG small")
        .bounds()
        .expect("text bounds");
    println!("Text bounds: {bounds:?}");

    assert!(
        bounds.height >= 30.0,
        "Height should wrap largest span. Expected >= 30, got {}",
        bounds.height
    );
    assert!(
        bounds.width < 800.0,
        "Width should not take the full space. Got {}",
        bounds.width
    );
}
