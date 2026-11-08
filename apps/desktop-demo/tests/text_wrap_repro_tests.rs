use cranpose_render_common::{
    graph::TextPrimitiveNode,
    scene_builder::build_from_applier,
    scene_store::{ItemRef, LayerRef},
};
use cranpose_testing::ComposeTestRule;
use cranpose_ui::{LayoutBox, LayoutEngine, Size};
use desktop_app::test_screens::text_wrap_repro::TextWrapReproScreen;

fn squashed(value: &str) -> String {
    value.chars().filter(|c| !c.is_whitespace()).collect()
}

fn find_box<'a>(node: &'a LayoutBox, needle: &str) -> Option<&'a LayoutBox> {
    if node
        .node_data
        .modifier_slices()
        .text_content()
        .is_some_and(|text| squashed(&text).starts_with(&squashed(needle)))
    {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_box(child, needle))
}

fn find_painted<'a>(layer: LayerRef<'a>, needle: &str) -> Option<&'a TextPrimitiveNode> {
    layer.items().find_map(|item| match item {
        ItemRef::Text(text) if squashed(text.text.text()).starts_with(&squashed(needle)) => {
            Some(text)
        }
        ItemRef::Layer(child) | ItemRef::Leaf(child) => find_painted(child, needle),
        ItemRef::Text(_) | ItemRef::Draw(_) | ItemRef::Run(_) => None,
    })
}

#[test]
fn repro_screen_paragraphs_paint_the_height_they_measured() {
    const PARAGRAPHS: [&str; 2] = ["fed back картица scored", "fp32 back ПДВ under"];

    let mut rule = ComposeTestRule::new();
    rule.set_content(|| {
        TextWrapReproScreen();
    })
    .expect("text wrap repro should render");

    let root = rule.root_id().expect("root");
    let handle = rule.runtime_handle();
    let mut applier = rule.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 760.0,
                height: 720.0,
            },
        )
        .expect("layout");
    let measured: Vec<f32> = PARAGRAPHS
        .iter()
        .map(|needle| {
            find_box(layout.root(), needle)
                .unwrap_or_else(|| panic!("measured box for {needle:?}"))
                .rect
                .height
        })
        .collect();
    let graph = build_from_applier(&applier, root).expect("render graph");
    applier.clear_runtime_handle();
    let painted_root = graph.root_layer().expect("a scene");

    for (needle, measured_height) in PARAGRAPHS.iter().zip(measured) {
        let painted = find_painted(painted_root, needle)
            .unwrap_or_else(|| panic!("painted paragraph for {needle:?}"));
        assert!(
            measured_height > 40.0,
            "the repro screen must keep these paragraphs multi-line; {needle:?} measured \
             {measured_height}"
        );
        assert!(
            (painted.rect.height - measured_height).abs() < 0.5,
            "{needle:?} painted {:.2} tall into a box layout measured at {measured_height:.2}",
            painted.rect.height
        );
    }
}
