use cranpose_testing::ComposeTestRule;
use cranpose_ui::{LayoutBox, LayoutEngine, LayoutTree, Size};
use desktop_app::test_screens::scroll_repro::ScrollReproScreen;

fn compute_layout_from_rule(
    rule: &mut ComposeTestRule,
    max_width: f32,
    max_height: f32,
) -> Result<LayoutTree, cranpose_core::NodeError> {
    let root = rule.root_id().expect("should have root");
    let handle = rule.runtime_handle();

    let layout = {
        let mut applier = rule.applier_mut();
        applier.set_runtime_handle(handle);
        let result = applier.compute_layout(
            root,
            Size {
                width: max_width,
                height: max_height,
            },
        )?;
        applier.clear_runtime_handle();
        result
    };

    Ok(layout)
}

fn contains_text(layout_box: &LayoutBox, text: &str) -> bool {
    if let Some(content) = layout_box.node_data.modifier_slices().text_content() {
        if content == text {
            return true;
        }
    }

    layout_box
        .children
        .iter()
        .any(|child| contains_text(child, text))
}

#[test]
fn scroll_repro_renders_header_and_first_item() {
    let mut rule = ComposeTestRule::new();
    rule.set_content(|| {
        ScrollReproScreen();
    })
    .expect("Scroll repro should render");

    let layout = compute_layout_from_rule(&mut rule, 800.0, 600.0).expect("layout should render");
    assert!(contains_text(layout.root(), "Scroll Repro"));
    assert!(contains_text(layout.root(), "Fake Title for Item 1"));
}
