/// Practical demonstration of the modifier system showcasing real-world patterns
use compose_core::{location_key, Composition, MemoryApplier};
use compose_ui::{
    composable, Box as ComposeBox, BoxSpec, Column, ColumnSpec, Modifier, Row, RowSpec, Size, Text,
};

#[composable]
fn simple_card() {
    ComposeBox(
        Modifier::empty().padding(16.0).size(Size {
            width: 300.0,
            height: 200.0,
        }),
        BoxSpec::default(),
        || {
            Column(
                Modifier::empty().padding(8.0),
                ColumnSpec::default(),
                || {
                    Text("Card Title", Modifier::empty());
                    Text("Card content goes here", Modifier::empty().padding(4.0));
                },
            );
        },
    );
}

#[composable]
fn positioned_box(label: &'static str, x: f32, y: f32) {
    ComposeBox(
        Modifier::empty()
            .size_points(100.0, 100.0)
            .offset(x, y)
            .padding(8.0),
        BoxSpec::default(),
        move || {
            Text(label, Modifier::empty());
        },
    );
}

#[composable]
fn item_list(count: usize) {
    Column(
        Modifier::empty().padding(16.0),
        ColumnSpec::default(),
        move || {
            for i in 0..count {
                Row(
                    Modifier::empty().padding(8.0).size_points(400.0, 50.0),
                    RowSpec::default(),
                    move || {
                        // Use a closure that captures i to avoid String allocation issues
                        let text = if i < 10 {
                            match i {
                                0 => "Item #0",
                                1 => "Item #1",
                                2 => "Item #2",
                                3 => "Item #3",
                                4 => "Item #4",
                                5 => "Item #5",
                                6 => "Item #6",
                                7 => "Item #7",
                                8 => "Item #8",
                                9 => "Item #9",
                                _ => "Item",
                            }
                        } else {
                            "Item #10+"
                        };
                        Text(text, Modifier::empty().padding_horizontal(12.0));
                    },
                );
            }
        },
    );
}

#[composable]
fn complex_chain() {
    // Demonstrate a deep modifier chain
    let modifier = Modifier::empty()
        .padding(10.0)
        .size_points(200.0, 100.0)
        .offset(20.0, 30.0)
        .padding(5.0);

    ComposeBox(modifier, BoxSpec::default(), || {
        Text("Complex modifiers!", Modifier::empty());
    });
}

#[composable]
fn demo() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Modifier System Demo ===",
            Modifier::empty().padding(10.0),
        );

        simple_card();

        Text("=== Positioned Boxes ===", Modifier::empty().padding(10.0));
        positioned_box("Box A", 50.0, 100.0);
        positioned_box("Box B", 200.0, 100.0);

        Text("=== Item List ===", Modifier::empty().padding(10.0));
        item_list(5);

        Text("=== Complex Chain ===", Modifier::empty().padding(10.0));
        complex_chain();
    });
}

fn main() {
    println!("🎨 Modifier System Demonstration\n");

    let mut composition = Composition::new(MemoryApplier::new());

    // Initial render
    println!("📊 Rendering demo...");
    let start = std::time::Instant::now();
    composition
        .render(location_key(file!(), line!(), column!()), demo)
        .unwrap();
    let duration = start.elapsed();

    println!("✅ Rendered in {:?}\n", duration);

    // Count nodes
    if let Some(root) = composition.root() {
        let count = count_all_nodes(&mut composition.applier_mut(), root);
        println!("📦 Created {} total nodes", count);
    }

    // Test recomposition
    println!("\n🔄 Testing recomposition...");
    let recomp_start = std::time::Instant::now();
    composition
        .render(location_key(file!(), line!(), column!()), || {
            Column(Modifier::empty(), ColumnSpec::default(), || {
                Text("Recomposed!", Modifier::empty().padding(10.0));
                positioned_box("Updated", 75.0, 125.0); // Different position
                item_list(10); // More items
            });
        })
        .unwrap();
    let recomp_duration = recomp_start.elapsed();

    println!("✅ Recomposed in {:?}", recomp_duration);

    // Performance test
    println!("\n💪 Performance test: 100 items...");
    let perf_start = std::time::Instant::now();
    composition
        .render(location_key(file!(), line!(), column!()), || item_list(100))
        .unwrap();
    let perf_duration = perf_start.elapsed();

    println!("✅ 100 items in {:?}", perf_duration);
    println!("📈 Per-item: {:?}", perf_duration / 100);

    println!("\n🎉 Demo complete - modifier system working perfectly!");
}

fn count_all_nodes(applier: &mut MemoryApplier, node_id: usize) -> usize {
    let mut count = 1;

    if let Ok(children) = applier.with_node(node_id, |node: &mut compose_ui::LayoutNode| {
        node.children.clone()
    }) {
        for child in children {
            count += count_all_nodes(applier, child);
        }
    }

    count
}
