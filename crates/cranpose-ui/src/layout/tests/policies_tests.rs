use super::*;
use crate::layout::core::Placeable;

struct MockMeasurable {
    width: f32,
    height: f32,
    node_id: usize,
}

impl MockMeasurable {
    fn new(width: f32, height: f32, node_id: usize) -> Self {
        Self {
            width,
            height,
            node_id,
        }
    }
}

impl Measurable for MockMeasurable {
    fn measure(&self, _constraints: Constraints) -> Placeable {
        Placeable::value(self.width, self.height, self.node_id)
    }

    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        self.width
    }

    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        self.width
    }

    fn min_intrinsic_height(&self, _width: f32) -> f32 {
        self.height
    }

    fn max_intrinsic_height(&self, _width: f32) -> f32 {
        self.height
    }
}

#[test]
fn box_measure_policy_takes_max_size() {
    let policy = BoxMeasurePolicy::new(Alignment::TOP_START, false);
    let measurables: Vec<Box<dyn Measurable>> = vec![
        Box::new(MockMeasurable::new(40.0, 20.0, 1)),
        Box::new(MockMeasurable::new(60.0, 30.0, 2)),
    ];

    let result = policy.measure(
        &measurables,
        Constraints {
            min_width: 0.0,
            max_width: 100.0,
            min_height: 0.0,
            max_height: 100.0,
        },
    );

    assert_eq!(result.size.width, 60.0);
    assert_eq!(result.size.height, 30.0);
    assert_eq!(result.placements.len(), 2);
}

#[test]
fn column_measure_policy_sums_heights() {
    let policy = FlexMeasurePolicy::column(LinearArrangement::Start, HorizontalAlignment::Start);
    let measurables: Vec<Box<dyn Measurable>> = vec![
        Box::new(MockMeasurable::new(40.0, 20.0, 1)),
        Box::new(MockMeasurable::new(60.0, 30.0, 2)),
    ];

    let result = policy.measure(
        &measurables,
        Constraints {
            min_width: 0.0,
            max_width: 100.0,
            min_height: 0.0,
            max_height: 100.0,
        },
    );

    assert_eq!(result.size.width, 60.0);
    assert_eq!(result.size.height, 50.0);
    assert_eq!(result.placements.len(), 2);
    assert_eq!(result.placements[0].y, 0.0);
    assert_eq!(result.placements[1].y, 20.0);
}

#[test]
fn column_spaced_by_preserves_spacing_when_content_overflows() {
    let policy = FlexMeasurePolicy::column(
        LinearArrangement::SpacedBy(12.0),
        HorizontalAlignment::Start,
    );
    let measurables: Vec<Box<dyn Measurable>> = vec![
        Box::new(MockMeasurable::new(80.0, 48.0, 1)),
        Box::new(MockMeasurable::new(80.0, 200.0, 2)),
    ];

    let result = policy.measure(
        &measurables,
        Constraints {
            min_width: 0.0,
            max_width: 100.0,
            min_height: 0.0,
            max_height: 120.0,
        },
    );

    assert_eq!(result.size.height, 120.0);
    assert_eq!(result.placements.len(), 2);
    assert_eq!(result.placements[0].y, 0.0);
    assert_eq!(
        result.placements[1].y, 60.0,
        "fixed SpacedBy gaps must not disappear just because the column overflows"
    );
}

#[test]
fn row_measure_policy_sums_widths() {
    let policy = FlexMeasurePolicy::row(
        LinearArrangement::Start,
        VerticalAlignment::CenterVertically,
    );
    let measurables: Vec<Box<dyn Measurable>> = vec![
        Box::new(MockMeasurable::new(40.0, 20.0, 1)),
        Box::new(MockMeasurable::new(60.0, 30.0, 2)),
    ];

    let result = policy.measure(
        &measurables,
        Constraints {
            min_width: 0.0,
            max_width: 200.0,
            min_height: 0.0,
            max_height: 100.0,
        },
    );

    assert_eq!(result.size.width, 100.0);
    assert_eq!(result.size.height, 30.0);
    assert_eq!(result.placements.len(), 2);
    assert_eq!(result.placements[0].x, 0.0);
    assert_eq!(result.placements[1].x, 40.0);
}

#[test]
fn built_in_policies_measure_into_reuses_caller_placements() {
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 200.0,
        min_height: 0.0,
        max_height: 100.0,
    };
    let measurables: Vec<Box<dyn Measurable>> = vec![
        Box::new(MockMeasurable::new(40.0, 20.0, 1)),
        Box::new(MockMeasurable::new(60.0, 30.0, 2)),
    ];
    let mut placements = Vec::with_capacity(8);
    placements.push(Placement::new(999, 1.0, 1.0, 0));
    let original_capacity = placements.capacity();

    let policy = FlexMeasurePolicy::column(LinearArrangement::Start, HorizontalAlignment::Start);
    let size = policy.measure_into(&measurables, constraints, &mut placements);

    assert_eq!(size.width, 60.0);
    assert_eq!(size.height, 50.0);
    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0].node_id, 1);
    assert_eq!(placements[1].node_id, 2);
    assert_eq!(placements.capacity(), original_capacity);

    let box_policy = BoxMeasurePolicy::new(Alignment::CENTER, false);
    let size = box_policy.measure_into(&measurables, constraints, &mut placements);

    assert_eq!(size.width, 60.0);
    assert_eq!(size.height, 30.0);
    assert_eq!(placements.len(), 2);
    assert_eq!(placements.capacity(), original_capacity);

    let leaf_policy = LeafMeasurePolicy::new(crate::modifier::Size {
        width: 25.0,
        height: 10.0,
    });
    let size = leaf_policy.measure_into(&[], constraints, &mut placements);

    assert_eq!(size.width, 25.0);
    assert_eq!(size.height, 10.0);
    assert!(placements.is_empty());
    assert_eq!(placements.capacity(), original_capacity);

    let empty_policy = EmptyMeasurePolicy::new();
    let size = empty_policy.measure_into(&[], constraints, &mut placements);

    assert_eq!(size.width, 0.0);
    assert_eq!(size.height, 0.0);
    assert!(placements.is_empty());
    assert_eq!(placements.capacity(), original_capacity);
}
