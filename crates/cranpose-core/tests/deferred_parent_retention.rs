// A group kept by `cranpose_with_reuse` leaves the root of a subcomposition
// host, which retains it under no parent scope. A later pass composes the
// same key under a new composable parent whose scope is deferred.
use std::{cell::Cell, rc::Rc};

use cranpose_core::{
    Composition, Key, MemoryApplier, RecomposeOptions, SlotTable, SlotsHost, location_key,
    remember, with_current_composer,
};

use crate::tracked_parent::TrackedParent;

thread_local! {
    static DEFERRED_PARENT_SEEN: Cell<&'static str> = const { Cell::new("") };
}

const DEFERRED_PARENT_REUSED_KEY: Key = Key(0x7265_7573_6564);

fn reused_group(label: &'static str) {
    with_current_composer(|composer| {
        composer.cranpose_with_reuse(
            DEFERRED_PARENT_REUSED_KEY,
            RecomposeOptions::default(),
            |_| {
                let kept = remember(|| label);
                DEFERRED_PARENT_SEEN.with(|seen| seen.set(kept.with(|value| *value)));
            },
        );
    });
}

#[cranpose_macros::composable]
fn DeferredParent() {
    reused_group("nested");
}

#[cranpose_macros::composable]
fn OuterParent() {
    DeferredParent();
}

#[test]
fn root_retained_group_stays_out_of_a_deferred_parent() {
    let mut composition = Composition::new(MemoryApplier::new());
    let host = Rc::new(SlotsHost::new(SlotTable::new()));
    let root_key = location_key(file!(), line!(), column!());
    let mut render = |step: u8| {
        let host = Rc::clone(&host);
        composition
            .render(root_key, move || {
                let root =
                    with_current_composer(|composer| composer.emit_node(TrackedParent::default));
                with_current_composer(|composer| {
                    composer
                        .subcompose_in(&host, Some(root), |_| match step {
                            0 => reused_group("root"),
                            1 => {}
                            _ => OuterParent(),
                        })
                        .expect("subcompose host");
                });
            })
            .expect("render composition");
    };
    render(0);
    assert_eq!(DEFERRED_PARENT_SEEN.with(Cell::get), "root");
    render(1);
    render(2);
    assert_eq!(
        DEFERRED_PARENT_SEEN.with(Cell::get),
        "nested",
        "the group under the new parent starts with its own state"
    );
}
