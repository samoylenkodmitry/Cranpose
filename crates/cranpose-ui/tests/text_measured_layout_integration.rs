use std::rc::Rc;

use cranpose_foundation::text::{TextFieldLineLimits, TextFieldState};
use cranpose_ui::{
    BasicTextFieldOptions, BasicTextFieldWithOptions, Column, ColumnSpec, LayoutBox, LayoutEngine,
    LayoutTree, Modifier, Size, Text, TextStyle, set_text_measurer,
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

fn lay_out_in_column<R>(content: impl Fn() + 'static, inspect: impl FnOnce(&LayoutTree) -> R) -> R {
    let content: Rc<dyn Fn()> = Rc::new(content);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let content = Rc::clone(&content);
        Column(Modifier::empty(), ColumnSpec::default(), move || content());
    });
    set_text_measurer(ContractMeasurer);
    let root = composition.root().expect("composition root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(root, Size::new(300.0, 300.0))
        .expect("layout");
    let inspected = inspect(&layout);
    applier.clear_runtime_handle();
    inspected
}

fn measured_body_lines(layout: &LayoutTree) -> Vec<String> {
    let body_box = find_box(layout.root(), &|node| {
        node.node_data.modifier_slices().text_content() == Some(BODY)
    })
    .expect("body box");
    body_box
        .node_data
        .modifier_slices()
        .measured_text_layout()
        .expect("a measured body exposes its layout")
        .text
        .text
        .lines()
        .map(str::to_owned)
        .collect()
}

fn text_field_lines(line_limits: TextFieldLineLimits) -> Vec<String> {
    lay_out_in_column(
        move || {
            let state = cranpose_core::remember(|| TextFieldState::new(BODY)).with(|state| *state);
            BasicTextFieldWithOptions(
                state,
                Modifier::empty().width(10.0 * CHAR_WIDTH),
                BasicTextFieldOptions {
                    line_limits,
                    ..BasicTextFieldOptions::default()
                },
            );
        },
        measured_body_lines,
    )
}

#[test]
fn measured_text_layout_is_the_layout_text_measured_under_its_own_constraint() {
    lay_out_in_column(
        || {
            Text(
                BODY,
                Modifier::empty().width(10.0 * CHAR_WIDTH),
                TextStyle::default(),
            );
        },
        |layout| {
            assert_eq!(measured_body_lines(layout), ["aaaa bbbb", "cccc"]);

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
        },
    );
}

#[test]
fn measured_text_layout_of_a_text_field_wraps_at_the_field_s_own_constraint() {
    assert_eq!(
        text_field_lines(TextFieldLineLimits::default()),
        ["aaaa bbbb", "cccc"],
        "a multi-line field wraps at its own width, not the column's"
    );
    assert_eq!(
        text_field_lines(TextFieldLineLimits::SingleLine),
        [BODY],
        "a single-line field never wraps; it pans"
    );
}
