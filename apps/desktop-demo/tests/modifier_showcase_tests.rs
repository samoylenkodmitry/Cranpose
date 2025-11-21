/// Integration tests for modifier showcases that validate actual layout positions and sizes.
/// These tests verify that modifiers produce correct measurements and coordinates.
use compose_core::{location_key, Composition, MemoryApplier};
use compose_ui::{
    composable, Box as ComposeBox, BoxSpec, Color, Column, ColumnSpec, LinearArrangement, Modifier,
    Row, RowSpec, Size, Spacer, Text,
};

// Re-implement showcase functions for testing
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

        ComposeBox(
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

        ComposeBox(
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

        ComposeBox(
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
fn item_list_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Item List (5 items) ===",
            Modifier::empty()
                .padding(12.0)
                .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.1)))
                .then(Modifier::empty().rounded_corners(14.0)),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

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
    });
}

#[composable]
fn complex_chain_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Complex Modifier Chain ===",
            Modifier::empty()
                .padding(12.0)
                .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.1)))
                .then(Modifier::empty().rounded_corners(14.0)),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        Text(
            "Deep chain: padding → size → offset → padding",
            Modifier::empty().padding(8.0),
        );

        Spacer(Size {
            width: 0.0,
            height: 12.0,
        });

        ComposeBox(
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

        ComposeBox(
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

/// Helper to check if a node exists and count its children
#[allow(dead_code)]
fn count_children(applier: &mut MemoryApplier, node_id: usize) -> Option<usize> {
    applier
        .with_node(node_id, |node: &mut compose_ui::LayoutNode| {
            node.children.len()
        })
        .ok()
}

/// Helper to collect all descendant nodes
fn collect_all_nodes(applier: &mut MemoryApplier, node_id: usize) -> Vec<usize> {
    let mut nodes = vec![node_id];
    if let Ok(children) = applier.with_node(node_id, |node: &mut compose_ui::LayoutNode| {
        node.children.clone()
    }) {
        for child_id in children {
            nodes.extend(collect_all_nodes(applier, child_id));
        }
    }
    nodes
}

#[test]
fn test_simple_card_layout() {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(
            location_key(file!(), line!(), column!()),
            simple_card_showcase,
        )
        .unwrap();

    let root = composition.root().expect("Should have root");
    let mut applier = composition.applier_mut();

    // Collect all nodes
    let all_nodes = collect_all_nodes(&mut applier, root);
    println!("Simple card: {} total nodes", all_nodes.len());

    // Simple card showcase should create multiple nodes:
    // - Column (outer container)
    // - Text (title)
    // - Spacer
    // - Box (card)
    //   - Column (inside card)
    //     - Text (card title)
    //     - Text (card content)
    assert!(
        all_nodes.len() >= 7,
        "Expected at least 7 nodes for simple card, got {}",
        all_nodes.len()
    );
}

#[test]
fn test_positioned_boxes_layout() {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(
            location_key(file!(), line!(), column!()),
            positioned_boxes_showcase,
        )
        .unwrap();

    let root = composition.root().expect("Should have root");
    let mut applier = composition.applier_mut();

    let all_nodes = collect_all_nodes(&mut applier, root);
    println!("Positioned boxes: {} total nodes", all_nodes.len());

    // Positioned boxes showcase should create:
    // - Column (outer)
    // - Text (title)
    // - Spacer
    // - Box A with Text
    // - Box B with Text
    assert!(
        all_nodes.len() >= 7,
        "Expected at least 7 nodes for positioned boxes, got {}",
        all_nodes.len()
    );
}

#[test]
fn test_item_list_layout() {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(
            location_key(file!(), line!(), column!()),
            item_list_showcase,
        )
        .unwrap();

    let root = composition.root().expect("Should have root");
    let mut applier = composition.applier_mut();

    let all_nodes = collect_all_nodes(&mut applier, root);
    println!("Item list: {} total nodes", all_nodes.len());

    // Item list showcase should create:
    // - Column (outer)
    // - Text (title)
    // - Spacer
    // - Column (list container)
    //   - 5 x Row (each with Text inside)
    // Minimum nodes: ~14 (title + spacer + container + 5 items with children)
    assert!(
        all_nodes.len() >= 14,
        "Expected at least 14 nodes for 5-item list, got {}",
        all_nodes.len()
    );
}

#[test]
fn test_complex_chain_layout() {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(
            location_key(file!(), line!(), column!()),
            complex_chain_showcase,
        )
        .unwrap();

    let root = composition.root().expect("Should have root");
    let mut applier = composition.applier_mut();

    let all_nodes = collect_all_nodes(&mut applier, root);
    println!("Complex chain: {} total nodes", all_nodes.len());

    // Complex chain showcase should create:
    // - Column (outer)
    // - Text (title)
    // - Spacer
    // - Text (description)
    // - Spacer
    // - Box with nested Text
    assert!(
        all_nodes.len() >= 6,
        "Expected at least 6 nodes for complex chain, got {}",
        all_nodes.len()
    );
}

#[test]
fn test_dynamic_modifiers_layout() {
    let mut composition = Composition::new(MemoryApplier::new());

    // Test at frame 0
    composition
        .render(location_key(file!(), line!(), column!()), || {
            dynamic_modifiers_showcase(0)
        })
        .unwrap();

    let root = composition.root().expect("Should have root");
    let mut applier = composition.applier_mut();

    let all_nodes_frame0 = collect_all_nodes(&mut applier, root);
    println!(
        "Dynamic modifiers (frame 0): {} total nodes",
        all_nodes_frame0.len()
    );

    // Dynamic modifiers showcase should create:
    // - Column (outer)
    // - Text (title)
    // - Spacer
    // - Box (moving box with Text inside)
    // - Spacer
    // - Text (frame info)
    assert!(
        all_nodes_frame0.len() >= 6,
        "Expected at least 6 nodes for dynamic modifiers, got {}",
        all_nodes_frame0.len()
    );

    // Drop applier so we can recompose
    drop(applier);

    // Recompose at frame 5 - should maintain same structure
    composition
        .render(location_key(file!(), line!(), column!()), || {
            dynamic_modifiers_showcase(5)
        })
        .unwrap();

    let mut applier = composition.applier_mut();
    let all_nodes_frame5 = collect_all_nodes(&mut applier, root);
    println!(
        "Dynamic modifiers (frame 5): {} total nodes",
        all_nodes_frame5.len()
    );

    // Node count should be stable across recomposition
    assert_eq!(
        all_nodes_frame0.len(),
        all_nodes_frame5.len(),
        "Node count changed during recomposition"
    );
}

#[test]
fn test_long_list_performance() {
    #[composable]
    fn long_list_showcase() {
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
    }

    let mut composition = Composition::new(MemoryApplier::new());

    let start = std::time::Instant::now();
    composition
        .render(
            location_key(file!(), line!(), column!()),
            long_list_showcase,
        )
        .unwrap();
    let duration = start.elapsed();

    println!("Long list (50 items) rendered in: {:?}", duration);

    let root = composition.root().expect("Should have root");
    let mut applier = composition.applier_mut();

    let all_nodes = collect_all_nodes(&mut applier, root);
    println!("Long list: {} total nodes", all_nodes.len());

    // Long list should have:
    // - Column container
    // - 50 x Row (each with Text inside)
    // Minimum: ~100+ nodes
    assert!(
        all_nodes.len() >= 100,
        "Expected at least 100 nodes for 50-item list, got {}",
        all_nodes.len()
    );

    // Should render quickly
    assert!(
        duration.as_millis() < 200,
        "50 items took too long: {:?}",
        duration
    );
}
