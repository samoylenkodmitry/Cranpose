use std::{cell::Cell, rc::Rc};

use cranpose_core::NodeId;

use super::{ChildPlacer, Placeable};

struct Recorder(Cell<Option<(u32, NodeId, f32, f32)>>);

impl ChildPlacer for Recorder {
    fn place(&self, slot: u32, child: NodeId, x: f32, y: f32) {
        self.0.set(Some((slot, child, x, y)));
    }
}

#[test]
fn a_placeable_tells_its_placer_which_child_it_placed_where() {
    let placer = Rc::new(Recorder(Cell::new(None)));
    let placeable = Placeable::with_placer(
        10.0,
        20.0,
        NodeId(7),
        Rc::clone(&placer) as Rc<dyn ChildPlacer>,
        2,
    );
    placeable.place(3.0, 4.0);
    assert_eq!(placer.0.get(), Some((2, NodeId(7), 3.0, 4.0)));
    assert_eq!(
        (placeable.width(), placeable.height(), placeable.node_id()),
        (10.0, 20.0, NodeId(7))
    );
}

#[test]
fn a_value_placeable_places_nothing() {
    let placeable = Placeable::value(1.0, 2.0, NodeId(3));
    placeable.place(1.0, 1.0);
    assert_eq!((placeable.width(), placeable.height()), (1.0, 2.0));
}
