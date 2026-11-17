// `Measurables`, the children a measure policy gets, through its public API.

use cranpose_core::NodeId;
use cranpose_ui_graphics::Size;
use cranpose_ui_layout::{
    Constraints, ConstraintsHold, FlexParentData, Measurable, Measurables, MeasurePolicy,
    MeasureResult, MeasureScope, ParentData, Placeable, Placement,
};

// A child of a fixed size that reports a weight and a hold of its last
// measure.
struct Fixed {
    id: usize,
    width: f32,
    weight: f32,
}

impl Fixed {
    fn new(id: usize, width: f32) -> Self {
        Self {
            id,
            width,
            weight: 0.0,
        }
    }
}

impl Measurable for Fixed {
    fn measure(&self, constraints: Constraints) -> Placeable {
        let (width, height) = constraints.constrain(self.width, 10.0);
        Placeable::value(width, height, NodeId(self.id))
    }

    fn measured_hold(&self) -> Option<(Size, ConstraintsHold)> {
        Some((
            Size::new(self.width, 10.0),
            ConstraintsHold::sized(self.width, 10.0),
        ))
    }

    // A child that wraps to half its width at twice its height.
    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        self.width / 2.0
    }

    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        self.width
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        if width < self.width { 20.0 } else { 10.0 }
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.min_intrinsic_height(width)
    }

    fn flex_parent_data(&self) -> Option<FlexParentData> {
        (self.weight > 0.0).then(|| FlexParentData::new(self.weight, true))
    }
}

// A child of another type, for a list of children of several types.
struct Square;

impl Measurable for Square {
    fn measure(&self, _constraints: Constraints) -> Placeable {
        Placeable::value(5.0, 5.0, NodeId(99))
    }

    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        5.0
    }

    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        5.0
    }

    fn min_intrinsic_height(&self, _width: f32) -> f32 {
        5.0
    }

    fn max_intrinsic_height(&self, _width: f32) -> f32 {
        5.0
    }
}

fn ids(children: impl Iterator<Item = usize>) -> Vec<usize> {
    children.collect()
}

fn id_of(child: &dyn Measurable) -> usize {
    child.measure(Constraints::loose(100.0, 100.0)).node_id().0
}

#[test]
fn a_list_reads_its_children_in_order_from_either_end() {
    let list = vec![
        Fixed::new(1, 10.0),
        Fixed::new(2, 20.0),
        Fixed::new(3, 30.0),
    ];
    let children = Measurables::new(&list);
    assert_eq!(children.len(), 3);
    assert!(!children.is_empty());
    assert_eq!(children.first().map(id_of), Some(1));
    assert_eq!(children.last().map(id_of), Some(3));
    assert_eq!(id_of(&children[1]), 2);
    assert!(children.get(3).is_none());
    assert_eq!(ids(children.iter().map(id_of)), [1, 2, 3]);
    assert_eq!(ids(children.iter().rev().map(id_of)), [3, 2, 1]);
    assert_eq!(children.iter().len(), 3);
    let mut iter = children.iter();
    assert_eq!(iter.next().map(id_of), Some(1));
    assert_eq!(iter.next_back().map(id_of), Some(3));
    assert_eq!(iter.len(), 1);
    assert_eq!(ids(iter.map(id_of)), [2]);
}

#[test]
fn a_list_without_children_has_no_ends() {
    let none: Vec<Fixed> = Vec::new();
    for children in [Measurables::empty(), Measurables::new(&none)] {
        assert_eq!(children.len(), 0);
        assert!(children.is_empty());
        assert!(children.first().is_none());
        assert!(children.last().is_none());
        assert!(children.get(0).is_none());
        assert_eq!(children.iter().count(), 0);
        assert!(children.iter().next_back().is_none());
    }
}

#[test]
#[should_panic(expected = "index 2 is past the end of 2 measurables")]
fn an_index_past_the_end_panics() {
    let list = [Fixed::new(1, 10.0), Fixed::new(2, 20.0)];
    let children = Measurables::new(&list);
    children[2].max_intrinsic_width(0.0);
}

#[test]
fn a_list_of_references_reaches_each_child_whole() {
    let weighted = Fixed {
        weight: 2.0,
        ..Fixed::new(1, 10.0)
    };
    let square = Square;
    let list: [&dyn Measurable; 2] = [&weighted, &square];
    let children = Measurables::new(&list);
    let first = &children[0];
    assert_eq!(first.flex_parent_data().map(|data| data.weight), Some(2.0));
    assert_eq!(first.parent_data().weight, 2.0);
    assert!(first.parent_data().fill);
    assert_eq!(
        first.measured_hold(),
        Some((Size::new(10.0, 10.0), ConstraintsHold::sized(10.0, 10.0)))
    );
    let second = &children[1];
    assert_eq!(second.parent_data(), ParentData::default());
    assert_eq!(second.measured_hold(), None);
    assert_eq!(
        second.measure(Constraints::loose(100.0, 100.0)).node_id(),
        NodeId(99)
    );
}

#[test]
fn a_slice_of_children_is_a_list() {
    let list = [
        Fixed::new(1, 10.0),
        Fixed::new(2, 20.0),
        Fixed::new(3, 30.0),
    ];
    let tail: &[Fixed] = &list[1..];
    let children = Measurables::new(&tail);
    assert_eq!(ids(children.iter().map(id_of)), [2, 3]);
}

// Stacks its children one under another.
struct Stack;

impl MeasurePolicy for Stack {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        measurables: Measurables<'_>,
        constraints: Constraints,
    ) -> MeasureResult {
        let mut y = 0.0;
        let mut width = 0.0_f32;
        let placements = measurables
            .iter()
            .map(|child| {
                let placeable = child.measure(constraints);
                let placement = Placement::new(placeable.node_id(), 0.0, y, 0.0);
                y += placeable.height();
                width = width.max(placeable.width());
                placement
            })
            .collect();
        MeasureResult::new(Size::new(width, y), placements)
    }

    fn min_intrinsic_width(&self, measurables: Measurables<'_>, height: f32) -> f32 {
        measurables
            .iter()
            .map(|child| child.min_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_width(&self, measurables: Measurables<'_>, height: f32) -> f32 {
        measurables
            .iter()
            .map(|child| child.max_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn min_intrinsic_height(&self, measurables: Measurables<'_>, width: f32) -> f32 {
        measurables
            .iter()
            .map(|child| child.min_intrinsic_height(width))
            .sum()
    }

    fn max_intrinsic_height(&self, measurables: Measurables<'_>, width: f32) -> f32 {
        measurables
            .iter()
            .map(|child| child.max_intrinsic_height(width))
            .sum()
    }
}

struct Scope;

impl MeasureScope for Scope {
    fn density(&self) -> f32 {
        1.0
    }

    fn font_scale(&self) -> f32 {
        1.0
    }
}

#[test]
fn a_policy_measures_its_children_through_a_shared_policy() {
    let list = [Fixed::new(1, 10.0), Fixed::new(2, 30.0)];
    let policy: &dyn MeasurePolicy = &Stack;
    let children = Measurables::new(&list);
    let mut placements = Vec::new();
    let measurement = policy.measure_into(
        &Scope,
        children,
        Constraints::loose(100.0, 100.0),
        &mut placements,
    );
    assert_eq!(measurement.size, Size::new(30.0, 20.0));
    let at: Vec<(NodeId, f32)> = placements
        .iter()
        .map(|placement| (placement.node_id, placement.y))
        .collect();
    assert_eq!(at, [(NodeId(1), 0.0), (NodeId(2), 10.0)]);
    assert_eq!(policy.max_intrinsic_width(children, 0.0), 30.0);
    assert_eq!(policy.min_intrinsic_height(Measurables::empty(), 0.0), 0.0);
}
