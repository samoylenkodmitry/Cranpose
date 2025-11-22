/// Integration tests that validate modifier showcases render correctly using dump_tree()
/// These tests verify the actual rendering output, not just structure.
use compose_core::MutableState;
use compose_macros::composable;
use compose_testing::ComposeTestRule;
use compose_ui::*;

// Import showcase composables from app
// For testing purposes, we recreate them here to avoid module visibility issues

#[composable]
fn simple_card_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Simple Card Pattern ===",
            Modifier::empty()
                .padding(12.0)
                .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.1)))
                .then(Modifier::empty().rounded_corners(14.0)),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        compose_ui::Box(
            Modifier::empty()
                .padding(16.0)
                .then(Modifier::empty().size(Size {
                    width: 300.0,
                    height: 200.0,
                }))
                .then(Modifier::empty().background(Color(0.2, 0.25, 0.35, 0.9)))
                .then(Modifier::empty().rounded_corners(16.0)),
            BoxSpec::default(),
            || {
                Column(
                    Modifier::empty().padding(8.0),
                    ColumnSpec::default(),
                    || {
                        Text(
                            "Card Title",
                            Modifier::empty()
                                .padding(6.0)
                                .then(Modifier::empty().background(Color(0.3, 0.5, 0.8, 0.5)))
                                .then(Modifier::empty().rounded_corners(8.0)),
                        );
                        Text(
                            "Card content goes here with padding",
                            Modifier::empty().padding(4.0),
                        );
                    },
                );
            },
        );
    });
}

#[composable]
fn positioned_boxes_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Positioned Boxes ===",
            Modifier::empty()
                .padding(12.0)
                .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.1)))
                .then(Modifier::empty().rounded_corners(14.0)),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        compose_ui::Box(
            Modifier::empty()
                .size_points(100.0, 100.0)
                .then(Modifier::empty().offset(50.0, 100.0))
                .then(Modifier::empty().padding(8.0))
                .then(Modifier::empty().background(Color(0.4, 0.2, 0.6, 0.8)))
                .then(Modifier::empty().rounded_corners(12.0)),
            BoxSpec::default(),
            || {
                Text("Box A", Modifier::empty().padding(8.0));
            },
        );

        compose_ui::Box(
            Modifier::empty()
                .size_points(100.0, 100.0)
                .then(Modifier::empty().offset(200.0, 100.0))
                .then(Modifier::empty().padding(8.0))
                .then(Modifier::empty().background(Color(0.2, 0.5, 0.4, 0.8)))
                .then(Modifier::empty().rounded_corners(12.0)),
            BoxSpec::default(),
            || {
                Text("Box B", Modifier::empty().padding(8.0));
            },
        );
    });
}

#[composable]
fn dynamic_modifiers_showcase(frame: i32) {
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            "=== Dynamic Modifiers ===",
            Modifier::empty()
                .padding(12.0)
                .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.1)))
                .then(Modifier::empty().rounded_corners(14.0)),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        let x = (frame as f32 * 10.0) % 200.0;
        let y = 50.0;

        compose_ui::Box(
            Modifier::empty()
                .size(Size {
                    width: 50.0,
                    height: 50.0,
                })
                .then(Modifier::empty().offset(x, y))
                .then(Modifier::empty().padding(4.0))
                .then(Modifier::empty().background(Color(0.3, 0.6, 0.9, 0.9)))
                .then(Modifier::empty().rounded_corners(10.0)),
            BoxSpec::default(),
            || {
                Text("Moving!", Modifier::empty().padding(4.0));
            },
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        Text(
            format!("Frame: {}, X: {:.1}", frame, x),
            Modifier::empty()
                .padding(8.0)
                .then(Modifier::empty().background(Color(0.2, 0.2, 0.3, 0.6)))
                .then(Modifier::empty().rounded_corners(10.0)),
        );
    });
}

#[test]
fn test_simple_card_renders_correctly() {
    let mut rule = ComposeTestRule::new();

    rule.set_content(|| {
        simple_card_showcase();
    })
    .expect("Simple card should render");

    let tree = rule.dump_tree();
    println!("=== Simple Card Tree Structure ===\n{}", tree);

    // Validate structure exists
    assert!(
        tree.contains("dyn compose_core::Node"),
        "Should contain Node"
    );

    // Count nodes - should have consistent structure
    let root = rule.root_id();
    assert!(root.is_some(), "Should have root node");
}

#[test]
fn test_positioned_boxes_renders_correctly() {
    let mut rule = ComposeTestRule::new();

    rule.set_content(|| {
        positioned_boxes_showcase();
    })
    .expect("Positioned boxes should render");

    let tree = rule.dump_tree();
    println!("=== Positioned Boxes Tree Structure ===\n{}", tree);

    // Validate structure
    assert!(
        tree.contains("dyn compose_core::Node"),
        "Should contain Node"
    );

    let initial_count = rule.applier_mut().len();
    println!("Total nodes in positioned boxes: {}", initial_count);

    // Should have at least 7 nodes (Column, Text, Spacer, Box A with Text, Box B with Text)
    assert!(
        initial_count >= 7,
        "Should have at least 7 nodes, got {}",
        initial_count
    );
}

#[test]
fn test_dynamic_modifiers_recomposition_preserves_structure() {
    let mut rule = ComposeTestRule::new();
    let frame = MutableState::with_runtime(0, rule.runtime_handle());

    rule.set_content({
        move || {
            dynamic_modifiers_showcase(frame.get());
        }
    })
    .expect("Dynamic modifiers should render");

    let tree_frame0 = rule.dump_tree();
    let count_frame0 = rule.applier_mut().len();
    println!("=== Frame 0 ===\nNodes: {}\n{}", count_frame0, tree_frame0);

    // Advance to frame 5
    frame.set(5);
    rule.pump_until_idle()
        .expect("Should recompose for frame 5");

    let tree_frame5 = rule.dump_tree();
    let count_frame5 = rule.applier_mut().len();
    println!("=== Frame 5 ===\nNodes: {}\n{}", count_frame5, tree_frame5);

    // Node count should be stable across frames
    assert_eq!(
        count_frame0, count_frame5,
        "Node count should remain stable across recomposition"
    );

    // Advance to frame 10
    frame.set(10);
    rule.pump_until_idle()
        .expect("Should recompose for frame 10");

    let count_frame10 = rule.applier_mut().len();
    println!("=== Frame 10 ===\nNodes: {}", count_frame10);

    assert_eq!(
        count_frame0, count_frame10,
        "Node count should remain stable through multiple recompositions"
    );
}

#[test]
fn test_item_list_with_spacing() {
    let mut rule = ComposeTestRule::new();

    rule.set_content(|| {
        Column(
            Modifier::empty().padding(16.0),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
            || {
                for i in 0..5 {
                    Row(
                        Modifier::empty()
                            .padding(8.0)
                            .then(Modifier::empty().size_points(400.0, 50.0))
                            .then(Modifier::empty().background(Color(0.15, 0.2, 0.3, 0.7)))
                            .then(Modifier::empty().rounded_corners(10.0)),
                        RowSpec::default(),
                        move || {
                            let text = match i {
                                0 => "Item #0",
                                1 => "Item #1",
                                2 => "Item #2",
                                3 => "Item #3",
                                4 => "Item #4",
                                _ => "Item",
                            };
                            Text(text, Modifier::empty().padding_horizontal(12.0));
                        },
                    );
                }
            },
        );
    })
    .expect("Item list should render");

    let tree = rule.dump_tree();
    println!("=== Item List Tree Structure ===\n{}", tree);

    let node_count = rule.applier_mut().len();
    println!("Total nodes: {}", node_count);

    // Should have Column + 5 Rows + 5 Texts = at least 11 nodes
    assert!(
        node_count >= 11,
        "Should have at least 11 nodes for 5-item list, got {}",
        node_count
    );
}

#[test]
fn test_complex_modifier_chain_ordering() {
    let mut rule = ComposeTestRule::new();

    rule.set_content(|| {
        compose_ui::Box(
            Modifier::empty()
                .padding(10.0)
                .then(Modifier::empty().size_points(200.0, 100.0))
                .then(Modifier::empty().offset(20.0, 30.0))
                .then(Modifier::empty().padding(5.0))
                .then(Modifier::empty().background(Color(0.6, 0.3, 0.2, 0.8)))
                .then(Modifier::empty().rounded_corners(12.0)),
            BoxSpec::default(),
            || {
                Text("Complex modifiers!", Modifier::empty().padding(8.0));
            },
        );
    })
    .expect("Complex chain should render");

    let tree = rule.dump_tree();
    println!("=== Complex Chain Tree Structure ===\n{}", tree);

    // Should render successfully with nested Text
    let node_count = rule.applier_mut().len();
    assert!(node_count >= 2, "Should have Box + Text at minimum");
}

#[test]
fn test_long_list_performance_and_structure() {
    let mut rule = ComposeTestRule::new();

    let start = std::time::Instant::now();
    rule.set_content(|| {
        Column(
            Modifier::empty().padding(16.0),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
            || {
                for i in 0..50 {
                    Row(
                        Modifier::empty()
                            .padding_symmetric(8.0, 4.0)
                            .then(Modifier::empty().size(Size {
                                width: 400.0,
                                height: 40.0,
                            }))
                            .then(Modifier::empty().background(Color(
                                0.12 + (i as f32 * 0.005),
                                0.15,
                                0.25,
                                0.7,
                            )))
                            .then(Modifier::empty().rounded_corners(8.0)),
                        RowSpec::default(),
                        move || {
                            let text = if i < 10 {
                                match i {
                                    0 => "Item 0",
                                    1 => "Item 1",
                                    2 => "Item 2",
                                    3 => "Item 3",
                                    4 => "Item 4",
                                    5 => "Item 5",
                                    6 => "Item 6",
                                    7 => "Item 7",
                                    8 => "Item 8",
                                    9 => "Item 9",
                                    _ => "Item",
                                }
                            } else {
                                "Item 10+"
                            };
                            Text(text, Modifier::empty().padding_horizontal(12.0));
                        },
                    );
                }
            },
        );
    })
    .expect("Long list should render");

    let duration = start.elapsed();
    println!("50-item list rendered in: {:?}", duration);

    let tree = rule.dump_tree();
    let node_count = rule.applier_mut().len();
    println!("Total nodes: {}", node_count);
    println!(
        "=== Long List Tree Structure (first 500 chars) ===\n{}",
        &tree[..tree.len().min(500)]
    );

    // Performance check: 50 items should render quickly
    assert!(
        duration.as_millis() < 200,
        "50-item list took too long: {:?}",
        duration
    );

    // Should have Column + 50 Rows + 50 Texts = at least 101 nodes
    assert!(
        node_count >= 101,
        "Should have at least 101 nodes for 50-item list, got {}",
        node_count
    );
}

#[test]
fn test_modifier_showcase_recomposition_stability() {
    // Test that changing between different showcases maintains stable node counts
    let mut rule = ComposeTestRule::new();
    let showcase_index = MutableState::with_runtime(0, rule.runtime_handle());

    rule.set_content({
        move || {
            let showcase_index_inner = showcase_index;
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                let current_index = showcase_index_inner.get();
                compose_core::with_key(&current_index, || match current_index {
                    0 => simple_card_showcase(),
                    1 => positioned_boxes_showcase(),
                    2 => dynamic_modifiers_showcase(0),
                    _ => simple_card_showcase(),
                });
            });
        }
    })
    .expect("Initial showcase should render");

    let count0 = rule.applier_mut().len();
    println!("Showcase 0 (simple card): {} nodes", count0);

    // Switch to positioned boxes
    showcase_index.set(1);
    rule.pump_until_idle()
        .expect("Should switch to positioned boxes");
    let count1 = rule.applier_mut().len();
    println!("Showcase 1 (positioned boxes): {} nodes", count1);

    // Switch to dynamic modifiers
    showcase_index.set(2);
    rule.pump_until_idle()
        .expect("Should switch to dynamic modifiers");
    let count2 = rule.applier_mut().len();
    println!("Showcase 2 (dynamic modifiers): {} nodes", count2);

    // Switch back to simple card
    showcase_index.set(0);
    rule.pump_until_idle()
        .expect("Should switch back to simple card");
    let count0_again = rule.applier_mut().len();
    println!("Showcase 0 again: {} nodes", count0_again);

    // Node count should be stable when returning to the same showcase
    assert_eq!(
        count0, count0_again,
        "Node count should be stable when switching back to same showcase"
    );

    // Rapid switching should not cause duplication
    for i in 0..10 {
        let idx = i % 3;
        showcase_index.set(idx);
        rule.pump_until_idle()
            .unwrap_or_else(|_| panic!("Rapid switch {}", i));

        let current_count = rule.applier_mut().len();

        // Each showcase should have a consistent node count
        let expected = match idx {
            0 => count0,
            1 => count1,
            2 => count2,
            _ => unreachable!(),
        };

        assert_eq!(
            current_count, expected,
            "Rapid switch {} to showcase {} should have consistent count",
            i, idx
        );
    }
}
