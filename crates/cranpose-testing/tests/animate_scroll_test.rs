//! A lazy column animates to an item far away and lands on it, through the
//! public shell.

use std::{cell::Cell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_testing::{robot::TestRenderer, robot_assertions::find_semantics_node};
use cranpose_ui::{AnimateScroll, Box, BoxSpec, LazyColumn, LazyColumnSpec, Modifier};

#[test]
fn animate_scroll_to_item_lands_on_an_item_beyond_the_viewport() {
    let state: Rc<Cell<Option<LazyListState>>> = Rc::new(Cell::new(None));
    let captured = Rc::clone(&state);
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let list = rememberLazyListState();
            captured.set(Some(list));
            LazyColumn(
                Modifier::empty().size_points(200.0, 100.0),
                list,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(LazyItems::new(100), |index| {
                        Box(
                            Modifier::empty()
                                .fill_max_width()
                                .height(if index % 2 == 0 { 20.0 } else { 30.0 })
                                .content_description(format!("row {index}")),
                            BoxSpec::default(),
                            || {},
                        );
                    });
                },
            );
        },
    );
    shell.set_viewport(200.0, 200.0);
    shell.set_buffer_size(200, 200);
    shell.set_semantics_enabled(true);
    let mut frame = 16_000_000_u64;
    shell.update_at_frame_time_nanos(frame);
    let list = state.get().expect("composed list");
    list.animate_scroll_to_item(60, 0.0);
    for _ in 0..150 {
        frame += 16_000_000;
        shell.update_at_frame_time_nanos(frame);
    }
    assert_eq!(list.first_visible_item_index(), 60);
    let tree = shell.semantics_tree().expect("semantics");
    let row = find_semantics_node(tree.root(), &|node| {
        node.description.as_deref() == Some("row 60")
    })
    .expect("row 60 shows");
    assert_eq!(row.bounds.y, 0.0, "it lands at the top");
}
