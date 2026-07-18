use super::*;
use crate::layout::core::Placeable;

struct MockMeasurable {
    width: f32,
    height: f32,
    node_id: usize,
    parent_data: cranpose_ui_layout::ParentData,
}

impl MockMeasurable {
    fn new(width: f32, height: f32, node_id: usize) -> Self {
        Self {
            width,
            height,
            node_id,
            parent_data: cranpose_ui_layout::ParentData::default(),
        }
    }

    fn with_parent_data(mut self, parent_data: cranpose_ui_layout::ParentData) -> Self {
        self.parent_data = parent_data;
        self
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

    fn parent_data(&self) -> cranpose_ui_layout::ParentData {
        self.parent_data
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
fn box_child_alignment_overrides_content_alignment() {
    let policy = BoxMeasurePolicy::new(Alignment::TOP_START, false);
    let measurables: Vec<Box<dyn Measurable>> = vec![Box::new(
        MockMeasurable::new(20.0, 10.0, 1).with_parent_data(cranpose_ui_layout::ParentData {
            box_alignment: Some(Alignment::BOTTOM_END),
            ..Default::default()
        }),
    )];

    let result = policy.measure(&measurables, Constraints::tight(100.0, 100.0));

    assert_eq!(result.placements[0].x, 80.0);
    assert_eq!(result.placements[0].y, 90.0);
}

#[test]
fn row_child_alignment_overrides_vertical_alignment() {
    let policy = FlexMeasurePolicy::row(LinearArrangement::Start, VerticalAlignment::Top);
    let measurables: Vec<Box<dyn Measurable>> = vec![Box::new(
        MockMeasurable::new(20.0, 10.0, 1).with_parent_data(cranpose_ui_layout::ParentData {
            row_alignment: Some(VerticalAlignment::Bottom),
            ..Default::default()
        }),
    )];

    let result = policy.measure(&measurables, Constraints::tight(100.0, 100.0));

    assert_eq!(result.placements[0].y, 90.0);
}

#[test]
fn column_child_alignment_overrides_horizontal_alignment() {
    let policy = FlexMeasurePolicy::column(LinearArrangement::Start, HorizontalAlignment::Start);
    let measurables: Vec<Box<dyn Measurable>> = vec![Box::new(
        MockMeasurable::new(20.0, 10.0, 1).with_parent_data(cranpose_ui_layout::ParentData {
            column_alignment: Some(HorizontalAlignment::End),
            ..Default::default()
        }),
    )];

    let result = policy.measure(&measurables, Constraints::tight(100.0, 100.0));

    assert_eq!(result.placements[0].x, 80.0);
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

// ─────────────────────────────────────────────────────────────────────────────
// FlowRowMeasurePolicy
// ─────────────────────────────────────────────────────────────────────────────

fn loose(max_width: f32, max_height: f32) -> Constraints {
    Constraints {
        min_width: 0.0,
        max_width,
        min_height: 0.0,
        max_height,
    }
}

#[test]
fn flow_row_wraps_children_of_varying_widths() {
    let policy = FlowRowMeasurePolicy::new(0.0, 0.0);
    let measurables: Vec<Box<dyn Measurable>> = vec![
        Box::new(MockMeasurable::new(50.0, 20.0, 1)),
        Box::new(MockMeasurable::new(20.0, 20.0, 2)),
        Box::new(MockMeasurable::new(45.0, 20.0, 3)),
        Box::new(MockMeasurable::new(80.0, 20.0, 4)),
        Box::new(MockMeasurable::new(10.0, 20.0, 5)),
    ];

    let result = policy.measure(&measurables, loose(100.0, 500.0));

    // Line 1: 50 + 20 = 70 (45 would exceed 100).
    assert_eq!((result.placements[0].x, result.placements[0].y), (0.0, 0.0));
    assert_eq!(
        (result.placements[1].x, result.placements[1].y),
        (50.0, 0.0)
    );
    // Line 2: 45 (80 would exceed 100).
    assert_eq!(
        (result.placements[2].x, result.placements[2].y),
        (0.0, 20.0)
    );
    // Line 3: 80 + 10 = 90.
    assert_eq!(
        (result.placements[3].x, result.placements[3].y),
        (0.0, 40.0)
    );
    assert_eq!(
        (result.placements[4].x, result.placements[4].y),
        (80.0, 40.0)
    );

    // Width is the widest line, height is the sum of line heights.
    assert_eq!(result.size.width, 90.0);
    assert_eq!(result.size.height, 60.0);
}

#[test]
fn flow_row_respects_main_and_cross_axis_spacing() {
    let policy = FlowRowMeasurePolicy::new(10.0, 5.0);
    let measurables: Vec<Box<dyn Measurable>> = vec![
        Box::new(MockMeasurable::new(30.0, 20.0, 1)),
        Box::new(MockMeasurable::new(30.0, 25.0, 2)),
        Box::new(MockMeasurable::new(30.0, 20.0, 3)),
    ];

    let result = policy.measure(&measurables, loose(100.0, 500.0));

    // Line 1: [0..30] and [40..70]; the third child would need 70+10+30 = 110.
    assert_eq!((result.placements[0].x, result.placements[0].y), (0.0, 0.0));
    assert_eq!(
        (result.placements[1].x, result.placements[1].y),
        (40.0, 0.0)
    );
    // Line 2 starts below the tallest child of line 1 plus the cross gap.
    assert_eq!(
        (result.placements[2].x, result.placements[2].y),
        (0.0, 30.0)
    );

    assert_eq!(result.size.width, 70.0);
    assert_eq!(result.size.height, 50.0);
}

#[test]
fn flow_row_children_share_line_top_when_heights_differ() {
    let policy = FlowRowMeasurePolicy::new(0.0, 0.0);
    let measurables: Vec<Box<dyn Measurable>> = vec![
        Box::new(MockMeasurable::new(40.0, 10.0, 1)),
        Box::new(MockMeasurable::new(40.0, 30.0, 2)),
        Box::new(MockMeasurable::new(40.0, 20.0, 3)),
    ];

    let result = policy.measure(&measurables, loose(100.0, 500.0));

    // Children on the same line are top-aligned.
    assert_eq!(result.placements[0].y, 0.0);
    assert_eq!(result.placements[1].y, 0.0);
    // The next line sits below the TALLEST child of the first line.
    assert_eq!(result.placements[2].y, 30.0);
    assert_eq!(result.size.height, 50.0);
}

#[test]
fn flow_row_gives_oversized_child_its_own_line() {
    let policy = FlowRowMeasurePolicy::new(0.0, 0.0);
    let measurables: Vec<Box<dyn Measurable>> = vec![
        Box::new(MockMeasurable::new(60.0, 10.0, 1)),
        Box::new(MockMeasurable::new(150.0, 10.0, 2)),
        Box::new(MockMeasurable::new(30.0, 10.0, 3)),
    ];

    let result = policy.measure(&measurables, loose(100.0, 500.0));

    assert_eq!((result.placements[0].x, result.placements[0].y), (0.0, 0.0));
    // Wider than the whole line: gets its own line and overflows.
    assert_eq!(
        (result.placements[1].x, result.placements[1].y),
        (0.0, 10.0)
    );
    assert_eq!(
        (result.placements[2].x, result.placements[2].y),
        (0.0, 20.0)
    );

    // The reported size stays within the constraints.
    assert_eq!(result.size.width, 100.0);
    assert_eq!(result.size.height, 30.0);
}

#[test]
fn flow_row_with_unbounded_width_stays_on_one_line() {
    let policy = FlowRowMeasurePolicy::new(4.0, 4.0);
    let measurables: Vec<Box<dyn Measurable>> = vec![
        Box::new(MockMeasurable::new(50.0, 20.0, 1)),
        Box::new(MockMeasurable::new(60.0, 20.0, 2)),
        Box::new(MockMeasurable::new(70.0, 20.0, 3)),
    ];

    let result = policy.measure(&measurables, loose(f32::INFINITY, 500.0));

    assert!(result.placements.iter().all(|p| p.y == 0.0));
    assert_eq!(result.size.width, 50.0 + 4.0 + 60.0 + 4.0 + 70.0);
    assert_eq!(result.size.height, 20.0);
}

#[test]
fn flow_row_empty_respects_min_constraints() {
    let policy = FlowRowMeasurePolicy::new(8.0, 8.0);
    let result = policy.measure(
        &[],
        Constraints {
            min_width: 25.0,
            max_width: 100.0,
            min_height: 15.0,
            max_height: 100.0,
        },
    );

    assert_eq!(result.size.width, 25.0);
    assert_eq!(result.size.height, 15.0);
    assert!(result.placements.is_empty());
}

#[test]
fn flow_row_intrinsics_follow_the_wrap() {
    let policy = FlowRowMeasurePolicy::new(10.0, 5.0);
    let measurables: Vec<Box<dyn Measurable>> = vec![
        Box::new(MockMeasurable::new(30.0, 20.0, 1)),
        Box::new(MockMeasurable::new(30.0, 25.0, 2)),
        Box::new(MockMeasurable::new(30.0, 20.0, 3)),
    ];

    // Narrowest layout: the widest single child.
    assert_eq!(policy.min_intrinsic_width(&measurables, 100.0), 30.0);
    // Widest layout: a single spaced line.
    assert_eq!(policy.max_intrinsic_width(&measurables, 100.0), 110.0);
    // At width 100 the third child wraps: 25 + 5 + 20.
    assert_eq!(policy.max_intrinsic_height(&measurables, 100.0), 50.0);
    // At the full single-line width nothing wraps.
    assert_eq!(policy.max_intrinsic_height(&measurables, 110.0), 25.0);
}
