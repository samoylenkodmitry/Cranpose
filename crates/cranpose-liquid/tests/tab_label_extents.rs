use cranpose_liquid::prelude::*;
use cranpose_render_common::{
    graph::{LayerNode, PrimitiveNode, RenderNode, TextPrimitiveNode},
    text_fonts::TextFonts,
};
use cranpose_testing::create_headless_robot_test_with_fonts;
use cranpose_ui::Modifier;

fn find_text<'a>(layer: &'a LayerNode, label: &str) -> Option<&'a TextPrimitiveNode> {
    layer.children.iter().find_map(|node| match node {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Text(text) if text.text.text == label => Some(text.as_ref()),
            _ => None,
        },
        RenderNode::Layer(layer) => find_text(layer, label),
        _ => None,
    })
}

#[test]
fn multilingual_tab_labels_fit_their_text_area() {
    let fonts = TextFonts::from_fonts_or_default(&[
        include_bytes!("../../cranpose-render/common/assets/NotoSansMerged.ttf"),
        include_bytes!("../../cranpose-render/common/tests/fixtures/localization/Arabic.ttf"),
        include_bytes!("../../cranpose-render/common/tests/fixtures/localization/Devanagari.ttf"),
    ]);
    let mut robot = create_headless_robot_test_with_fonts(400, 120, fonts, || {
        LiquidTheme(LiquidThemeSpec::default(), || {
            LiquidTabBar(
                Modifier::empty().width(390.0),
                LiquidTabBarSpec::default(),
                0,
                |_| {},
                |tabs| {
                    for label in ["Settings", "الإعدادات", "सेटिंग्स"] {
                        tabs.tab(icons::SEARCH, label);
                    }
                },
            );
        });
    });
    robot.wait_for_idle();
    let graph = robot.shell_mut().scene().graph.as_ref().expect("tab scene");
    for label in ["Settings", "الإعدادات", "सेटिंग्स"] {
        let text = find_text(&graph.root, label).expect("visible tab label");
        let height = text.layout.height();
        assert!(
            height <= text.rect.height + 0.01,
            "{label} needs {height} pixels but its area has {}",
            text.rect.height,
        );
    }
}
