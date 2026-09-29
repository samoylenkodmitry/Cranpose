use super::*;

const NODE: Size = Size {
    width: 50.0,
    height: 40.0,
};

fn padded(ordinal: usize, geometry: &Rc<CoordinatorGeometry>) -> CoordinatorRect {
    CoordinatorRect::new(geometry, ordinal, EdgeInsets::uniform(3.0))
}

#[test]
fn an_unplaced_coordinator_draws_inside_the_padding_before_it() {
    let geometry = Rc::new(CoordinatorGeometry::default());
    let coordinator = padded(1, &geometry);
    assert_eq!(
        coordinator.rect(NODE),
        Rect {
            x: 3.0,
            y: 3.0,
            width: 44.0,
            height: 34.0,
        }
    );
    assert_eq!(coordinator.insets(NODE), EdgeInsets::uniform(3.0));
    assert_eq!(coordinator.origin(), Point { x: 3.0, y: 3.0 });
}

#[test]
fn a_placed_coordinator_draws_where_layout_put_it() {
    let geometry = Rc::new(CoordinatorGeometry::default());
    let placed = Rect {
        x: 20.0,
        y: 15.0,
        width: 10.0,
        height: 10.0,
    };
    geometry.replace([
        Rect {
            x: 0.0,
            y: 0.0,
            width: NODE.width,
            height: NODE.height,
        },
        placed,
    ]);
    let coordinator = padded(1, &geometry);
    assert_eq!(coordinator.rect(NODE), placed);
    assert_eq!(
        coordinator.insets(NODE),
        EdgeInsets {
            left: 20.0,
            top: 15.0,
            right: 20.0,
            bottom: 15.0,
        }
    );
    assert_eq!(coordinator.origin(), Point { x: 20.0, y: 15.0 });
    assert_eq!(
        padded(2, &geometry).rect(NODE),
        EdgeInsets::uniform(3.0).inset_rect(NODE)
    );
}

#[test]
fn a_new_measure_replaces_every_rect() {
    let geometry = Rc::new(CoordinatorGeometry::default());
    let empty = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };
    geometry.replace([empty, empty]);
    geometry.replace([Rect {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
    }]);
    assert_eq!(padded(0, &geometry).origin(), Point { x: 1.0, y: 2.0 });
    assert_eq!(padded(1, &geometry).origin(), Point { x: 3.0, y: 3.0 });
}

#[test]
fn a_default_coordinator_fills_its_node() {
    let coordinator = CoordinatorRect::default();
    assert_eq!(coordinator.insets(NODE), EdgeInsets::default());
    assert_eq!(coordinator.origin(), Point::default());
}

#[test]
fn replacing_rects_reports_only_a_change() {
    let geometry = CoordinatorGeometry::default();
    let outer = Rect {
        x: 0.0,
        y: 0.0,
        width: NODE.width,
        height: NODE.height,
    };
    let inner = Rect {
        x: 3.0,
        y: 3.0,
        width: 44.0,
        height: 34.0,
    };
    assert!(geometry.replace([outer, inner]));
    assert!(!geometry.replace([outer, inner]));
    assert!(geometry.replace([outer, outer]));
    assert_eq!(geometry.rect(1), Some(outer));
    assert!(geometry.replace([outer]));
    assert_eq!(geometry.rect(1), None);
    assert!(geometry.replace([outer, inner, inner]));
    assert_eq!(geometry.rect(2), Some(inner));
    assert!(!geometry.replace([outer, inner, inner]));
}
