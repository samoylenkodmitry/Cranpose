//! Tests for offset semantics in padding and offset modifiers
//!
//! These tests verify that PaddingNode and OffsetNode affect layout placement correctly
//! and that offsets are not applied twice (once in layout, once in placement).

use compose_ui::*;

#[test]
fn test_padding_affects_child_position() {
    let mut composition = run_test_composition(|| {
        // Box with padding - child should be offset by padding amount
        Box(Modifier::empty().padding(20.0), BoxSpec::default(), || {
            Box(
                Modifier::empty().size(Size {
                    width: 50.0,
                    height: 50.0,
                }),
                BoxSpec::default(),
                || {},
            );
        });
    });

    let root = composition.root().expect("has root");
    let mut applier = composition.applier_mut();
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("layout computation");

    // The outer box should be at 0,0 with size = child + padding
    let outer_rect = &layout.root().rect;
    assert_eq!(outer_rect.x, 0.0, "Outer box should be at x=0");
    assert_eq!(outer_rect.y, 0.0, "Outer box should be at y=0");
    assert_eq!(
        outer_rect.width, 90.0,
        "Outer width should be child (50) + padding (20*2)"
    );
    assert_eq!(
        outer_rect.height, 90.0,
        "Outer height should be child (50) + padding (20*2)"
    );

    // The inner child should be offset by the padding amount
    assert_eq!(layout.root().children.len(), 1, "Should have one child");
    let child_rect = &layout.root().children[0].rect;
    assert_eq!(
        child_rect.x, 20.0,
        "Child should be offset by padding.left (20)"
    );
    assert_eq!(
        child_rect.y, 20.0,
        "Child should be offset by padding.top (20)"
    );
    assert_eq!(child_rect.width, 50.0, "Child width should be 50");
    assert_eq!(child_rect.height, 50.0, "Child height should be 50");
}

#[test]
fn test_offset_modifier_affects_position() {
    let mut composition = run_test_composition(|| {
        Column(Modifier::empty(), ColumnSpec::default(), || {
            // First box - no offset
            Box(
                Modifier::empty().size(Size {
                    width: 50.0,
                    height: 50.0,
                }),
                BoxSpec::default(),
                || {},
            );

            // Second box - with offset
            Box(
                Modifier::empty()
                    .size(Size {
                        width: 50.0,
                        height: 50.0,
                    })
                    .offset(30.0, 15.0),
                BoxSpec::default(),
                || {},
            );
        });
    });

    let root = composition.root().expect("has root");
    let mut applier = composition.applier_mut();
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("layout computation");

    assert_eq!(layout.root().children.len(), 2, "Should have two children");

    let first_box = &layout.root().children[0].rect;
    let second_box = &layout.root().children[1].rect;

    // First box should be at column's natural position (0, 0)
    assert_eq!(first_box.x, 0.0, "First box should be at x=0");
    assert_eq!(first_box.y, 0.0, "First box should be at y=0");

    // Second box should be at (0, 50) from column layout + (30, 15) from offset
    assert_eq!(
        second_box.x, 30.0,
        "Second box should be offset by 30 in x (0 + offset.x)"
    );
    assert_eq!(
        second_box.y, 65.0,
        "Second box should be offset by 15 in y (50 from first box + 15 offset)"
    );
}

#[test]
fn test_padding_and_offset_combined() {
    let mut composition = run_test_composition(|| {
        Box(
            Modifier::empty().padding(10.0).offset(20.0, 30.0),
            BoxSpec::default(),
            || {
                Box(
                    Modifier::empty().size(Size {
                        width: 40.0,
                        height: 40.0,
                    }),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });

    let root = composition.root().expect("has root");
    let mut applier = composition.applier_mut();
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("layout computation");

    // The outer box should include both offset and padding
    let outer_rect = &layout.root().rect;
    assert_eq!(
        outer_rect.x, 20.0,
        "Outer box should be offset by offset.x (20)"
    );
    assert_eq!(
        outer_rect.y, 30.0,
        "Outer box should be offset by offset.y (30)"
    );
    assert_eq!(
        outer_rect.width, 60.0,
        "Outer width should be child (40) + padding (10*2)"
    );
    assert_eq!(
        outer_rect.height, 60.0,
        "Outer height should be child (40) + padding (10*2)"
    );

    // The inner child should be offset by padding relative to parent
    assert_eq!(layout.root().children.len(), 1, "Should have one child");
    let child_rect = &layout.root().children[0].rect;
    assert_eq!(
        child_rect.x, 30.0,
        "Child x should be parent offset (20) + padding (10)"
    );
    assert_eq!(
        child_rect.y, 40.0,
        "Child y should be parent offset (30) + padding (10)"
    );
    assert_eq!(child_rect.width, 40.0, "Child width should be 40");
    assert_eq!(child_rect.height, 40.0, "Child height should be 40");
}

#[test]
fn test_no_double_offset_application() {
    // This test verifies that offsets aren't applied twice
    // (once during measure, once during place)
    let mut composition = run_test_composition(|| {
        Column(Modifier::empty(), ColumnSpec::default(), || {
            Box(
                Modifier::empty()
                    .size(Size {
                        width: 100.0,
                        height: 50.0,
                    })
                    .offset(25.0, 10.0),
                BoxSpec::default(),
                || {},
            );

            Box(
                Modifier::empty().size(Size {
                    width: 100.0,
                    height: 50.0,
                }),
                BoxSpec::default(),
                || {},
            );
        });
    });

    let root = composition.root().expect("has root");
    let mut applier = composition.applier_mut();
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("layout computation");

    assert_eq!(layout.root().children.len(), 2, "Should have two children");

    let first_box = &layout.root().children[0].rect;
    let second_box = &layout.root().children[1].rect;

    // First box should be offset ONCE by (25, 10)
    // If offset were applied twice, it would be at (50, 20)
    assert_eq!(
        first_box.x, 25.0,
        "First box offset should be applied exactly once (25, not 50)"
    );
    assert_eq!(
        first_box.y, 10.0,
        "First box offset should be applied exactly once (10, not 20)"
    );

    // Second box should be at (0, 50) - the natural column layout position
    // If first box's offset were applied to layout positioning, this would be wrong
    assert_eq!(second_box.x, 0.0, "Second box should be at x=0");
    assert_eq!(
        second_box.y, 50.0,
        "Second box should be at y=50 (first box height, ignoring its offset)"
    );
}

#[test]
fn test_nested_padding_accumulates() {
    let mut composition = run_test_composition(|| {
        Box(Modifier::empty().padding(10.0), BoxSpec::default(), || {
            Box(Modifier::empty().padding(5.0), BoxSpec::default(), || {
                Box(
                    Modifier::empty().size(Size {
                        width: 30.0,
                        height: 30.0,
                    }),
                    BoxSpec::default(),
                    || {},
                );
            });
        });
    });

    let root = composition.root().expect("has root");
    let mut applier = composition.applier_mut();
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("layout computation");

    // Outer box: child (40) + padding (10*2) = 60
    let outer_rect = &layout.root().rect;
    assert_eq!(outer_rect.width, 60.0, "Outer width should be 60");
    assert_eq!(outer_rect.height, 60.0, "Outer height should be 60");

    // Middle box: child (30) + padding (5*2) = 40
    let middle_rect = &layout.root().children[0].rect;
    assert_eq!(middle_rect.width, 40.0, "Middle width should be 40");
    assert_eq!(middle_rect.height, 40.0, "Middle height should be 40");
    assert_eq!(
        middle_rect.x, 10.0,
        "Middle box should be offset by outer padding (10)"
    );
    assert_eq!(
        middle_rect.y, 10.0,
        "Middle box should be offset by outer padding (10)"
    );

    // Inner box: size is fixed at 30x30
    let inner_rect = &layout.root().children[0].children[0].rect;
    assert_eq!(inner_rect.width, 30.0, "Inner width should be 30");
    assert_eq!(inner_rect.height, 30.0, "Inner height should be 30");
    assert_eq!(
        inner_rect.x, 15.0,
        "Inner box should be at outer padding (10) + middle padding (5)"
    );
    assert_eq!(
        inner_rect.y, 15.0,
        "Inner box should be at outer padding (10) + middle padding (5)"
    );
}
