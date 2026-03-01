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
