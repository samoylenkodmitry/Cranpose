//! Content that a composable starts to show takes its place among the
//! siblings, also when the composable showed nothing before and recomposes
//! on its own.

use cranpose_core::MutableState;
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, Modifier, Size, composable};

use crate::{directional_layout::tagged, text_measured_layout_integration::layout_composition};

const VIEWPORT: Size = Size {
    width: 100.0,
    height: 100.0,
};

fn bar(tag: &'static str) {
    Box(
        Modifier::empty()
            .size_points(10.0, 10.0)
            .content_description(tag),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn Maybe(show: MutableState<bool>) {
    if show.get() {
        bar("shown");
    }
}

#[test]
fn content_shown_by_an_empty_composable_lays_out_in_place() {
    let state = std::rc::Rc::new(std::cell::Cell::new(None));
    let slot = std::rc::Rc::clone(&state);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let show = cranpose_core::rememberMutableStateOf(|| false);
        slot.set(Some(show));
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            bar("before");
            Maybe(show);
            bar("after");
        });
    });
    layout_composition(&mut composition, VIEWPORT);
    let show = state.get().expect("show state");

    show.set(true);
    while composition.process_invalid_scopes().expect("recompose") {}
    let layout = layout_composition(&mut composition, VIEWPORT);

    let y = |tag| tagged(layout.root(), tag).rect.y;
    assert_eq!((y("before"), y("shown"), y("after")), (0.0, 10.0, 20.0));
}
