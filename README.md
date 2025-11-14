
[compose-rs.webm](https://github.com/user-attachments/assets/b96a83f0-4739-4d0d-9dc2-e2194d63df78)

# RS-Compose 

Compose-RS is a Jetpack Compose–inspired declarative UI framework. The repository accompanies the architectural proposal documented in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and provides crate scaffolding for the core runtime, procedural macros, UI primitives, and example applications.

## Examples

Run the interactive desktop example:
```bash
cargo run --bin desktop-app
```
```rust

fn main() {
    compose_app::ComposeApp!(
        options: ComposeAppOptions::default()
            .WithTitle("Compose Counter")
            .WithSize(800, 600),
        {
            recursive_layout_example();
        }
    );
}

#[composable]
fn recursive_layout_example() {
    let depth_state = compose_core::useState(|| 3usize);

    Column(
        Modifier::empty().padding(32.0)
            .then(Modifier::empty().background(Color(0.08, 0.10, 0.18, 1.0)))
            .then(Modifier::empty().rounded_corners(24.0))
            .then(Modifier::empty().padding(20.0)),
        ColumnSpec::default(),
        move || {
            Text(
                "Recursive Layout Playground",
                Modifier::empty().padding(12.0)
                    .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.08)))
                    .then(Modifier::empty().rounded_corners(16.0)),
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            Row(
                Modifier::empty().fill_max_width().then(Modifier::empty().padding(8.0)),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                {
                    let depth_state = depth_state.clone();
                    move || {
                        let depth = depth_state.get();
                        Button(
                            Modifier::empty().rounded_corners(16.0)
                                .then(Modifier::empty().draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.35, 0.45, 0.85, 1.0)),
                                        CornerRadii::uniform(16.0),
                                    );
                                }))
                                .then(Modifier::empty().padding(10.0)),
                            {
                                let depth_state = depth_state.clone();
                                move || {
                                    let next = (depth_state.get() + 1).min(16);
                                    if next != depth_state.get() {
                                        depth_state.set(next);
                                    }
                                }
                            },
                            || {
                                Text("Increase depth", Modifier::empty().padding(6.0));
                            },
                        );

                        Button(
                            Modifier::empty().rounded_corners(16.0)
                                .then(Modifier::empty().draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.65, 0.35, 0.35, 1.0)),
                                        CornerRadii::uniform(16.0),
                                    );
                                }))
                                .then(Modifier::empty().padding(10.0)),
                            {
                                let depth_state = depth_state.clone();
                                move || {
                                    let next = depth_state.get().saturating_sub(1).max(1);
                                    if next != depth_state.get() {
                                        depth_state.set(next);
                                    }
                                }
                            },
                            || {
                                Text("Decrease depth", Modifier::empty().padding(6.0));
                            },
                        );

                        Text(
                            format!("Current depth: {}", depth.max(1)),
                            Modifier::empty().padding(8.0)
                                .then(Modifier::empty().background(Color(0.12, 0.16, 0.28, 0.8)))
                                .then(Modifier::empty().rounded_corners(12.0)),
                        );
                    }
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            let depth = depth_state.get().max(1);
            Column(
                Modifier::empty().fill_max_width()
                    .then(Modifier::empty().padding(8.0))
                    .then(Modifier::empty().background(Color(0.06, 0.08, 0.16, 0.9)))
                    .then(Modifier::empty().rounded_corners(20.0))
                    .then(Modifier::empty().padding(12.0)),
                ColumnSpec::default(),
                move || {
                    recursive_layout_node(depth, true, 0);
                },
            );
        },
    );
}

#[composable]
fn recursive_layout_node(depth: usize, horizontal: bool, index: usize) {
    let palette = [
        Color(0.25, 0.32, 0.58, 0.75),
        Color(0.30, 0.20, 0.45, 0.75),
        Color(0.20, 0.40, 0.32, 0.75),
        Color(0.45, 0.28, 0.24, 0.75),
    ];
    let accent = palette[index % palette.len()];

    Column(
        Modifier::empty().rounded_corners(18.0)
            .then(Modifier::empty().draw_behind({
                move |scope| {
                    scope.draw_round_rect(Brush::solid(accent), CornerRadii::uniform(18.0));
                }
            }))
            .then(Modifier::empty().padding(12.0)),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                format!("Depth {}", depth),
                Modifier::empty().padding(6.0)
                    .then(Modifier::empty().background(Color(0.0, 0.0, 0.0, 0.25)))
                    .then(Modifier::empty().rounded_corners(10.0)),
            );

            if depth <= 1 {
                Text(
                    format!("Leaf node #{index}"),
                    Modifier::empty().padding(6.0)
                        .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.12)))
                        .then(Modifier::empty().rounded_corners(10.0)),
                );
            } else if horizontal {
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                    move || {
                        for child_idx in 0..2 {
                            recursive_layout_node(depth - 1, false, index * 2 + child_idx + 1);
                        }
                    },
                );
            } else {
                Column(
                    Modifier::empty().fill_max_width(),
                    ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                    move || {
                        for child_idx in 0..2 {
                            recursive_layout_node(depth - 1, true, index * 2 + child_idx + 1);
                        }
                    },
                );
            }
        },
    );
}

```

## Roadmap

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for detailed progress tracking, implementation status, and upcoming milestones.
## Contributing

This repository is currently a design playground; issues and pull requests are welcome for discussions, experiments, and early prototypes that move the Jetpack Compose–style experience forward in Rust.

## License

This project is available under the terms of the Apache License (Version 2.0). See [`LICENSE-APACHE`](LICENSE-APACHE) for the full license text.
