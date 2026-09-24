use cranpose_ui::{
    Column, ColumnSpec, LayoutBox, LayoutEngine, Modifier, Size, Text, TextStyle, set_text_measurer,
};

use crate::text_contract_measurer::{CHAR_WIDTH, ContractMeasurer};

const BODY: &str = "aaaa bbbb cccc";

fn find_box<'a>(
    node: &'a LayoutBox,
    matches: &impl Fn(&LayoutBox) -> bool,
) -> Option<&'a LayoutBox> {
    if matches(node) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_box(child, matches))
}

#[test]
fn measured_text_layout_is_the_layout_text_measured_under_its_own_constraint() {
    let mut composition = cranpose_ui::run_test_composition(|| {
        Column(Modifier::empty(), ColumnSpec::default(), || {
            Text(
                BODY,
                Modifier::empty().width(10.0 * CHAR_WIDTH),
                TextStyle::default(),
            );
        });
    });
    set_text_measurer(ContractMeasurer);
    let root = composition.root().expect("composition root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(root, Size::new(300.0, 300.0))
        .expect("layout");
    applier.clear_runtime_handle();

    let text_box = find_box(layout.root(), &|node| {
        node.node_data.modifier_slices().text_content() == Some(BODY)
    })
    .expect("text box");
    let measured = text_box
        .node_data
        .modifier_slices()
        .measured_text_layout()
        .expect("a measured Text exposes its layout");
    assert_eq!(
        measured.text.text.lines().collect::<Vec<_>>(),
        ["aaaa bbbb", "cccc"]
    );

    let column_box = find_box(layout.root(), &|node| {
        node.node_data.modifier_slices().text_content().is_none()
    })
    .expect("column box");
    assert!(
        column_box
            .node_data
            .modifier_slices()
            .measured_text_layout()
            .is_none()
    );
}
