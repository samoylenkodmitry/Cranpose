use std::cell::Cell;
use std::ops::Deref;

use cranpose_core::{Composition, MemoryApplier, remember};
use cranpose_macros::composable;

thread_local! {
    static INITS: Cell<i32> = const { Cell::new(0) };
    static SECOND_SEEN: Cell<i32> = const { Cell::new(-1) };
    static ARM_SEEN: Cell<i32> = const { Cell::new(-1) };
}

fn reset_probes() {
    INITS.with(|c| c.set(0));
    SECOND_SEEN.with(|c| c.set(-1));
    ARM_SEEN.with(|c| c.set(-1));
}

struct Holder {
    value: Option<i32>,
    marker: i32,
}

impl Deref for Holder {
    type Target = Option<i32>;

    fn deref(&self) -> &Option<i32> {
        let slot = remember(|| {
            INITS.with(|c| c.set(c.get() + 1));
            self.marker
        });
        if self.marker == 72 {
            SECOND_SEEN.with(|c| c.set(slot.with(|v| *v)));
        }
        &self.value
    }
}

#[composable]
fn short_circuited_place_deref_probe(enabled: bool) {
    let first = Holder {
        value: Some(1),
        marker: 71,
    };
    let second = Holder {
        value: Some(2),
        marker: 72,
    };
    if enabled && let Some(_) = *first {}
    let _got = *second;
}

#[test]
fn a_short_circuited_place_deref_does_not_leak_into_a_later_deref() {
    reset_probes();
    let mut composition = Composition::new(MemoryApplier::new());
    let pass = |composition: &mut Composition<MemoryApplier>, enabled: bool| {
        composition
            .render(7, || short_circuited_place_deref_probe(enabled))
            .expect("render");
    };

    pass(&mut composition, true);
    assert_eq!(
        (INITS.with(|c| c.get()), SECOND_SEEN.with(|c| c.get())),
        (2, 72)
    );

    pass(&mut composition, false);
    assert_eq!(
        (INITS.with(|c| c.get()), SECOND_SEEN.with(|c| c.get())),
        (2, 72),
        "the unconditional deref must keep its own slot, not adopt the short-circuited \
         deref's slot and not reinitialize"
    );
}

#[composable]
fn chain_binding_probe(enabled: bool) {
    let first = Holder {
        value: Some(5),
        marker: 71,
    };
    if enabled
        && let Some(ref x) = *first
        && *x > 0
    {
        ARM_SEEN.with(|c| c.set(*x));
    }
}

#[test]
fn chain_let_bindings_still_reach_the_arm() {
    reset_probes();
    let mut composition = Composition::new(MemoryApplier::new());
    let pass = |composition: &mut Composition<MemoryApplier>, enabled: bool| {
        composition
            .render(8, || chain_binding_probe(enabled))
            .expect("render");
    };

    pass(&mut composition, true);
    assert_eq!(ARM_SEEN.with(|c| c.get()), 5);

    pass(&mut composition, false);
    pass(&mut composition, true);
    assert_eq!(ARM_SEEN.with(|c| c.get()), 5);
}
