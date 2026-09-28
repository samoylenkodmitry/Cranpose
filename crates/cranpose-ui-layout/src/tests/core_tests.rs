use std::{cell::Cell, rc::Rc};

use super::{PlaceTarget, Placeable};

struct Recorder(Cell<Option<(f32, f32)>>);

impl PlaceTarget for Recorder {
    fn place(&self, x: f32, y: f32) {
        self.0.set(Some((x, y)));
    }
}

#[test]
fn a_placeable_tells_its_target_where_it_was_placed() {
    let target = Rc::new(Recorder(Cell::new(None)));
    let placeable =
        Placeable::with_place_target(10.0, 20.0, 7, Rc::clone(&target) as Rc<dyn PlaceTarget>);
    placeable.place(3.0, 4.0);
    assert_eq!(target.0.get(), Some((3.0, 4.0)));
    assert_eq!(
        (placeable.width(), placeable.height(), placeable.node_id()),
        (10.0, 20.0, 7)
    );
}

#[test]
fn a_closure_is_a_place_target() {
    let placed = Rc::new(Cell::new(None));
    let seen = Rc::clone(&placed);
    let placeable =
        Placeable::with_place_target(1.0, 1.0, 1, Rc::new(move |x, y| seen.set(Some((x, y)))));
    placeable.place(5.0, 6.0);
    assert_eq!(placed.get(), Some((5.0, 6.0)));
}

#[test]
fn a_value_placeable_places_nothing() {
    let placeable = Placeable::value(1.0, 2.0, 3);
    placeable.place(1.0, 1.0);
    assert_eq!((placeable.width(), placeable.height()), (1.0, 2.0));
}
