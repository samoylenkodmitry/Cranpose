use super::*;

const NODE: Size = Size {
    width: 50.0,
    height: 40.0,
};

#[test]
fn an_unplaced_coordinator_draws_inside_the_padding_before_it() {
    let parts = Rc::new(LayoutParts::empty());
    let coordinator = CoordinatorRect::new(Some(&parts), 1, EdgeInsets::uniform(3.0));
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
fn a_default_coordinator_fills_its_node() {
    let coordinator = CoordinatorRect::default();
    assert_eq!(coordinator.insets(NODE), EdgeInsets::default());
    assert_eq!(coordinator.origin(), Point::default());
}
