use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_ui::{
    Box as ComposeBox, BoxSpec, Column, ColumnSpec, Modifier, Row, RowSpec, Size, Text, TextStyle,
    composable,
};

#[composable]
fn card(title: &'static str, content: &'static str) {
    ComposeBox(
        Modifier::empty()
            .padding(16.0)
            .size(Size {
                width: 300.0,
                height: 200.0,
            })
            .offset(0.0, 0.0),
        BoxSpec::default(),
        move || {
            Column(
                Modifier::empty().padding(8.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        title,
                        Modifier::empty().padding_each(0.0, 0.0, 0.0, 12.0),
                        TextStyle::default(),
                    );

                    Text(
                        content,
                        Modifier::empty().padding(4.0),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
}

#[composable]
fn animated_box(frame: i32) {
    let x = (frame as f32 * 10.0) % 200.0;
    let y = 50.0;

    ComposeBox(
        Modifier::empty()
            .size(Size {
                width: 50.0,
                height: 50.0,
            })
            .offset(x, y)
            .padding(4.0),
        BoxSpec::default(),
        || {
            Text("Moving!", Modifier::empty(), TextStyle::default());
        },
    );
}

#[composable]
fn long_list(item_count: usize) {
    Column(
        Modifier::empty().padding(16.0),
        ColumnSpec::default(),
        move || {
            for i in 0..item_count {
                Row(
                    Modifier::empty().padding_symmetric(8.0, 4.0).size(Size {
                        width: 400.0,
                        height: 60.0,
                    }),
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
                        Text(
                            text,
                            Modifier::empty().padding_horizontal(12.0),
                            TextStyle::default(),
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn reorderable_modifiers(use_large_padding: bool) {
    let padding = if use_large_padding { 32.0 } else { 8.0 };

    ComposeBox(
        Modifier::empty()
            .padding(padding)
            .size(Size {
                width: 200.0,
                height: 100.0,
            })
            .offset(10.0, 10.0),
        BoxSpec::default(),
        || {
            Text("Dynamic padding!", Modifier::empty(), TextStyle::default());
        },
    );
}

#[composable]
fn showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Card Pattern ===",
            Modifier::empty().padding_each(0.0, 0.0, 0.0, 16.0),
            TextStyle::default(),
        );
        card(
            "Welcome Card",
            "This demonstrates a typical card UI with nested padding and size constraints.",
        );

        Text(
            "=== Dynamic Modifiers ===",
            Modifier::empty()
                .padding_symmetric(0.0, 32.0)
                .padding_each(0.0, 0.0, 0.0, 16.0),
            TextStyle::default(),
        );
        animated_box(5);

        Text(
            "=== Performance: 50 Items ===",
            Modifier::empty()
                .padding_symmetric(0.0, 32.0)
                .padding_each(0.0, 0.0, 0.0, 16.0),
            TextStyle::default(),
        );
        long_list(50);

        Text(
            "=== Modifier Reordering ===",
            Modifier::empty()
                .padding_symmetric(0.0, 32.0)
                .padding_each(0.0, 0.0, 0.0, 16.0),
            TextStyle::default(),
        );
        reorderable_modifiers(true);
    });
}

fn main() {
    println!("🚀 Modifier System Showcase");
    println!("============================\n");

    let mut composition = Composition::new(MemoryApplier::new());

    println!("📊 Rendering initial composition...");
    let start = std::time::Instant::now();
    composition
        .render(location_key(file!(), line!(), column!()), showcase)
        .unwrap();
    let initial_duration = start.elapsed();
    println!("✅ Initial render: {:?}\n", initial_duration);

    if let Some(root) = composition.root() {
        let mut applier = composition.applier_mut();
        let node_count = count_nodes(&mut applier, root, 0);
        println!("📦 Total nodes created: {}", node_count);
        println!("🎯 Demonstrates: Complex nesting, dynamic modifiers, performance\n");
    }

    println!("🔄 Testing recomposition with modifier changes...");
    let recomp_start = std::time::Instant::now();
    composition
        .render(location_key(file!(), line!(), column!()), || {
            Column(Modifier::empty(), ColumnSpec::default(), || {
                Text("Updated!", Modifier::empty(), TextStyle::default());
                animated_box(10);
                reorderable_modifiers(false);
            });
        })
        .unwrap();
    let recomp_duration = recomp_start.elapsed();
    println!("✅ Recomposition: {:?}", recomp_duration);
    println!(
        "⚡ Speedup vs initial: {:.2}x\n",
        initial_duration.as_secs_f64() / recomp_duration.as_secs_f64()
    );

    println!("💪 Performance stress test: 1000 items...");
    let stress_start = std::time::Instant::now();
    composition
        .render(location_key(file!(), line!(), column!()), || {
            long_list(1000);
        })
        .unwrap();
    let stress_duration = stress_start.elapsed();
    println!("✅ 1000 items rendered: {:?}", stress_duration);
    println!("📈 Per-item average: {:?}", stress_duration / 1000);

    println!("\n🎉 Showcase complete!");
    println!("All modifier system features working correctly.");
}

fn count_nodes(applier: &mut MemoryApplier, node_id: usize, _depth: usize) -> usize {
    let mut count = 1;

    if let Ok(children) = applier.with_node(node_id, |node: &mut cranpose_ui::LayoutNode| {
        node.children.clone()
    }) {
        for child_id in children {
            count += count_nodes(applier, child_id, _depth + 1);
        }
    }

    count
}
