use super::*;

#[test]
fn resolving_start_and_end_follows_the_direction() {
    assert_eq!(LayoutDirection::Ltr.resolve(4.0, 12.0), (4.0, 12.0));
    assert_eq!(LayoutDirection::Rtl.resolve(4.0, 12.0), (12.0, 4.0));
}

#[test]
fn a_direction_knows_its_opposite() {
    assert_eq!(LayoutDirection::Ltr.reversed(), LayoutDirection::Rtl);
    assert!(!LayoutDirection::Ltr.is_rtl());
    assert!(LayoutDirection::Rtl.is_rtl());
}

#[test]
fn a_provided_direction_reaches_the_content_and_ends_with_it() {
    use std::{cell::Cell, rc::Rc};

    use cranpose_core::{Composition, MemoryApplier, location_key};

    let mut composition = Composition::new(MemoryApplier::new());
    let outer = Rc::new(Cell::new(LayoutDirection::Rtl));
    let inside = Rc::new(Cell::new(LayoutDirection::Ltr));
    let nested = Rc::new(Cell::new(LayoutDirection::Rtl));
    let after = Rc::new(Cell::new(LayoutDirection::Rtl));

    let key = location_key(file!(), line!(), column!());
    {
        let (outer, inside, nested, after) = (
            Rc::clone(&outer),
            Rc::clone(&inside),
            Rc::clone(&nested),
            Rc::clone(&after),
        );
        let mut render = move || {
            outer.set(layout_direction());
            ProvideLayoutDirection(LayoutDirection::Rtl, || {
                inside.set(layout_direction());
                ProvideLayoutDirection(LayoutDirection::Ltr, || nested.set(layout_direction()));
            });
            after.set(layout_direction());
        };
        composition.render(key, &mut render).expect("render");
    }

    assert_eq!(outer.get(), LayoutDirection::Ltr);
    assert_eq!(inside.get(), LayoutDirection::Rtl);
    assert_eq!(nested.get(), LayoutDirection::Ltr);
    assert_eq!(after.get(), LayoutDirection::Ltr);
}

#[test]
fn the_composition_local_is_one_instance_per_thread() {
    use std::{cell::Cell, rc::Rc};

    use cranpose_core::{Composition, MemoryApplier, location_key};

    let mut composition = Composition::new(MemoryApplier::new());
    let seen = Rc::new(Cell::new(LayoutDirection::Ltr));
    let recorder = Rc::clone(&seen);
    let key = location_key(file!(), line!(), column!());
    let mut render = move || {
        CompositionLocalProvider(
            vec![local_layout_direction().provides(LayoutDirection::Rtl)],
            || recorder.set(local_layout_direction().current()),
        );
    };
    composition.render(key, &mut render).expect("render");

    assert_eq!(seen.get(), LayoutDirection::Rtl);
}
