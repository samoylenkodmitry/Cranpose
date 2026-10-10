use std::rc::Rc;

use cranpose_liquid::prelude::*;
use cranpose_render_common::{
    graph::{LayerNode, PrimitiveNode, RenderNode, TextPrimitiveNode},
    software_text_raster::{SoftwareTextFontSet, SoftwareTextMeasurer},
};
use cranpose_testing::create_headless_robot_test_with_text_measurer;
use cranpose_ui::Modifier;
use cranpose_ui_graphics::{Color, ColorFilter};

const WIDTH: f32 = 300.0;

/// The text primitive showing `label`, and the color filter of the nearest
/// layer above it that has one.
fn find_label<'a>(
    layer: &'a LayerNode,
    label: &str,
    filter: Option<ColorFilter>,
) -> Option<(&'a TextPrimitiveNode, Option<ColorFilter>)> {
    let filter = layer.color_filter().or(filter);
    layer.children.iter().find_map(|node| match node {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Text(text) if text.text.text == label => Some((text.as_ref(), filter)),
            _ => None,
        },
        RenderNode::Layer(layer) => find_label(layer, label, filter),
        _ => None,
    })
}

fn robot(
    style: impl Fn(LiquidColors, LiquidTypography) -> LiquidSegmentedStyle + 'static,
) -> cranpose_testing::RobotTestRule<cranpose_testing::TestRenderer> {
    let fonts = SoftwareTextFontSet::from_fonts_or_default(&[include_bytes!(
        "../../cranpose-render/common/assets/NotoSansMerged.ttf"
    )]);
    let measurer = Rc::new(SoftwareTextMeasurer::from_font_set(fonts, 32));
    let style = Rc::new(style);
    create_headless_robot_test_with_text_measurer(320, 80, measurer, move || {
        let style = Rc::clone(&style);
        LiquidTheme(
            LiquidThemeSpec {
                scheme: SchemeMode::Light,
                ..Default::default()
            },
            move || {
                ProvideLiquidSegmentedStyle(style(liquid_colors(), liquid_typography()), || {
                    LiquidSegmentedControl(
                        Modifier::empty().width(WIDTH),
                        0,
                        |_| {},
                        |scope| {
                            scope.segment("One");
                            scope.segment("Two");
                        },
                    );
                });
            },
        );
    })
}

fn accent(colors: LiquidColors, typography: LiquidTypography) -> LiquidSegmentedStyle {
    LiquidSegmentedStyle {
        indicator: colors.accent,
        selected_label: colors.on_accent,
        height: 44.0,
        ..LiquidSegmentedStyle::system(&colors, &typography)
    }
}

/// Runs `frames` frames. `wait_for_idle` returns while a spring that needs
/// no redraw of its own is still moving.
fn advance_frames(
    robot: &mut cranpose_testing::RobotTestRule<cranpose_testing::TestRenderer>,
    frames: usize,
) {
    for _ in 0..frames {
        robot.advance_time(16_666_667);
    }
}

/// Presses the first segment and keeps it pressed until the lens is lifted.
fn hold_the_first_segment(
    robot: &mut cranpose_testing::RobotTestRule<cranpose_testing::TestRenderer>,
) {
    robot.wait_for_idle();
    let center = robot
        .find_by_text("One")
        .center()
        .expect("the first segment");
    robot.mouse_move(center.x, center.y);
    robot.mouse_down();
    advance_frames(robot, 30);
}

fn label_color(text: &TextPrimitiveNode) -> Option<Color> {
    text.text_style.span_style.color
}

#[test]
fn a_provided_style_sets_the_height_and_the_label_colors() {
    let mut robot = robot(accent);
    robot.wait_for_idle();
    let center = robot
        .find_by_text("One")
        .center()
        .expect("the first segment");
    assert!(
        (center.y - 22.0).abs() <= 1.0,
        "the label sits at {} in a 44 high control",
        center.y
    );
    let colors = LiquidColors::light(LiquidThemeSpec::default().accent);
    let graph = robot.shell_mut().scene().graph.as_ref().expect("a scene");
    let (chosen, filter) = find_label(&graph.root, "One", None).expect("the chosen label");
    assert_eq!(label_color(chosen), Some(colors.on_accent));
    assert_eq!(filter, None, "a resting lens recolors no label");
    let (other, _) = find_label(&graph.root, "Two", None).expect("the other label");
    assert_eq!(label_color(other), Some(colors.label));
}

#[test]
fn a_held_lens_gives_the_chosen_label_the_color_of_the_others() {
    let mut robot = robot(accent);
    hold_the_first_segment(&mut robot);
    let colors = LiquidColors::light(LiquidThemeSpec::default().accent);
    let graph = robot.shell_mut().scene().graph.as_ref().expect("a scene");
    let (_, filter) = find_label(&graph.root, "One", None).expect("the chosen label");
    let Some(ColorFilter::Tint(held)) = filter else {
        panic!("a held lens left the chosen label in {filter:?}");
    };
    let distance = (held.r() - colors.label.r()).abs()
        + (held.g() - colors.label.g()).abs()
        + (held.b() - colors.label.b()).abs();
    assert!(
        distance < 0.15,
        "the held label is {held:?}, the others {:?}",
        colors.label
    );

    robot.mouse_up();
    advance_frames(&mut robot, 40);
    let graph = robot.shell_mut().scene().graph.as_ref().expect("a scene");
    let (chosen, filter) = find_label(&graph.root, "One", None).expect("the chosen label");
    assert_eq!(filter, None, "the lens came to rest");
    assert_eq!(label_color(chosen), Some(colors.on_accent));
}

#[test]
fn the_system_style_draws_every_label_alike_without_a_filter() {
    let mut robot = robot(|colors, typography| LiquidSegmentedStyle::system(&colors, &typography));
    hold_the_first_segment(&mut robot);
    let colors = LiquidColors::light(LiquidThemeSpec::default().accent);
    let graph = robot.shell_mut().scene().graph.as_ref().expect("a scene");
    for label in ["One", "Two"] {
        let (text, filter) = find_label(&graph.root, label, None).expect("a label");
        assert_eq!(label_color(text), Some(colors.label));
        assert_eq!(filter, None, "{label} gained a filter");
    }
    robot.mouse_up();
}
