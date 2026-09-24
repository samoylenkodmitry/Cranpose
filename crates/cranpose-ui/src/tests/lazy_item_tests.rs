use std::{cell::Cell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, location_key};

use super::*;

fn composed<T: Copy + Default + 'static>(body: impl Fn(&Cell<T>) + 'static) -> T {
    let mut composition = Composition::new(MemoryApplier::new());
    let seen = Rc::new(Cell::new(T::default()));
    let recorder = Rc::clone(&seen);
    let mut render = move || body(&recorder);
    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("render");
    seen.get()
}

#[test]
fn an_item_key_reaches_what_the_row_composes() {
    let seen: (Option<u64>, Option<u64>, Option<u64>) = composed(|cell| {
        let before = lazy_item_key();
        let inside = Cell::new(None);
        ProvideLazyItemKey(Some(17), || inside.set(lazy_item_key()));
        cell.set((before, inside.get(), lazy_item_key()));
    });
    assert_eq!(seen, (None, Some(17), None));
}

#[test]
fn an_unkeyed_list_reports_no_identity_rather_than_its_position() {
    let seen: Option<u64> = composed(|cell| {
        cell.set(Some(0));
        ProvideLazyItemKey(None, || cell.set(lazy_item_key()));
    });
    assert_eq!(seen, None);
}

#[test]
fn the_composition_local_is_one_instance_per_thread() {
    let seen: bool = composed(|cell| {
        ProvideLazyItemKey(Some(3), || {
            cell.set(local_lazy_item_key().current() == Some(3));
        });
    });
    assert!(seen);
}
