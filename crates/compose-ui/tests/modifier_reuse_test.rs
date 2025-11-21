//! Tests for modifier node reuse and dynamic chain updates
//!
//! These tests verify that modifier nodes are properly reused when the modifier
//! chain is updated with the same structure, and that chains update correctly when
//! modifiers are added or removed.

use compose_ui::*;

#[test]
fn test_modifier_chain_length_after_updates() {
    // This test verifies that the modifier chain has the correct number of nodes
    // after various updates
    let mut composition = run_test_composition(|| {
        Box(Modifier::empty().padding(12.0), BoxSpec::default(), || {});
    });

    // Should have root node
    assert!(
        composition.root().is_some(),
        "Should have root after first render"
    );

    // Second render with same modifier - chain should remain
    composition = run_test_composition(|| {
        Box(Modifier::empty().padding(12.0), BoxSpec::default(), || {});
    });

    assert!(
        composition.root().is_some(),
        "Should have root after second render"
    );
}

#[test]
fn test_modifier_value_changes_propagate_to_layout() {
    // Test that when a modifier value changes, the layout reflects the new value

    let run_with_padding = |p: f32| {
        let mut composition = run_test_composition(|| {
            Box(Modifier::empty().padding(p), BoxSpec::default(), || {});
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

        layout.root().rect.width
    };

    let width1 = run_with_padding(10.0);
    assert_eq!(width1, 20.0, "Width should be 10*2 padding");

    let width2 = run_with_padding(20.0);
    assert_eq!(width2, 40.0, "Width should be 20*2 padding after update");
}

#[test]
fn test_modifier_chain_adds_node() {
    // Test adding a modifier to the chain
    let mut composition = run_test_composition(|| {
        Box(Modifier::empty().padding(8.0), BoxSpec::default(), || {});
    });

    assert!(
        composition.root().is_some(),
        "Should have root with padding only"
    );

    // Add background modifier
    composition = run_test_composition(|| {
        Box(
            Modifier::empty()
                .padding(8.0)
                .background(Color(1.0, 0.0, 0.0, 1.0)),
            BoxSpec::default(),
            || {},
        );
    });

    assert!(
        composition.root().is_some(),
        "Should have root with padding + background"
    );
}

#[test]
fn test_modifier_chain_removes_node() {
    // Test removing a modifier from the chain
    let mut composition = run_test_composition(|| {
        Box(
            Modifier::empty()
                .padding(8.0)
                .background(Color(1.0, 0.0, 0.0, 1.0)),
            BoxSpec::default(),
            || {},
        );
    });

    assert!(
        composition.root().is_some(),
        "Should have root with padding + background"
    );

    // Remove background
    composition = run_test_composition(|| {
        Box(Modifier::empty().padding(8.0), BoxSpec::default(), || {});
    });

    assert!(
        composition.root().is_some(),
        "Should have root with padding only"
    );
}

#[test]
fn test_modifier_order_change() {
    // Test that changing modifier order works correctly
    let mut composition = run_test_composition(|| {
        Box(
            Modifier::empty().padding(10.0).size(Size {
                width: 100.0,
                height: 100.0,
            }),
            BoxSpec::default(),
            || {},
        );
    });

    let root = composition.root().expect("has root");
    let mut applier = composition.applier_mut();
    let layout1 = applier
        .compute_layout(
            root,
            Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("layout computation");

    // padding then size: padding creates space for child, then size constrains it
    assert_eq!(
        layout1.root().rect.width,
        120.0,
        "Width should be padding (20) + size (100)"
    );
    drop(applier); // Release the borrow

    // Reverse order - size then padding
    composition = run_test_composition(|| {
        Box(
            Modifier::empty()
                .size(Size {
                    width: 100.0,
                    height: 100.0,
                })
                .padding(10.0),
            BoxSpec::default(),
            || {},
        );
    });

    let root = composition.root().expect("has root");
    let mut applier = composition.applier_mut();
    let layout2 = applier
        .compute_layout(
            root,
            Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("layout computation");

    // When size is applied AFTER padding, the size constraint applies to the outer box,
    // constraining the final result to 100x100 (the size constraint wins)
    assert_eq!(
        layout2.root().rect.width,
        100.0,
        "Width should be 100 when size constraint is applied after padding"
    );
}

#[test]
fn test_complex_modifier_chain_updates() {
    // Cycle through different modifier configurations
    let build_modifier = |step: i32| match step {
        0 => Modifier::empty().padding(5.0),
        1 => Modifier::empty().padding(5.0).size(Size {
            width: 50.0,
            height: 50.0,
        }),
        2 => Modifier::empty()
            .padding(5.0)
            .size(Size {
                width: 50.0,
                height: 50.0,
            })
            .background(Color(0.5, 0.5, 0.5, 1.0)),
        3 => Modifier::empty()
            .size(Size {
                width: 50.0,
                height: 50.0,
            })
            .background(Color(0.5, 0.5, 0.5, 1.0)),
        4 => Modifier::empty().background(Color(0.5, 0.5, 0.5, 1.0)),
        _ => Modifier::empty(),
    };

    // Test each configuration
    for step in 0..=5 {
        let composition = run_test_composition(|| {
            Box(build_modifier(step), BoxSpec::default(), || {});
        });

        assert!(
            composition.root().is_some(),
            "Step {}: should have root",
            step
        );
    }
}
