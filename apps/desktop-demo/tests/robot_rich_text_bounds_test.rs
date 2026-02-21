use cranpose::prelude::*;
use cranpose::text::*;
use cranpose_testing::robot::create_headless_robot_test;

#[test]
fn test_rich_text_bounds() {
    // Inject the real WGPU text measurer for accurate rich text bounds
    cranpose_render_wgpu::setup_headless_text_measurer();

    let mut robot = create_headless_robot_test(800, 600, || {
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
    println!("Text bounds: {:?}", bounds);

    // "BIG" is 30sp, "small" is 10sp. Total height should be >=30.
    assert!(
        bounds.height >= 30.0,
        "Height should wrap largest span. Expected >= 30, got {}",
        bounds.height
    );
    // Width should not take the whole screen!
    assert!(
        bounds.width < 800.0,
        "Width should not take the full space. Got {}",
        bounds.width
    );
}
