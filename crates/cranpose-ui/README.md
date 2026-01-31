# Cranpose UI

High-level UI widgets and layout components for the Cranpose framework.

## When to Use

This crate is the primary building block for application UIs. It provides:
-   **Standard Widgets**: `Text`, `Button`, `TextField`, `Image`.
-   **Layout Containers**: `Row`, `Column`, `Box`, `Spacer`.
-   **Lazy Lists**: `LazyColumn`, `LazyRow` for efficiently rendering large datasets.

Most applications will consume these via the `cranpose` crate re-exports. You would interact directly with `cranpose-ui` constructs when implementing custom layout logic or new widget primitives.

## Key Concepts

-   **Widget**: A composable function that emits UI nodes.
-   **Layout Phase**: The pass where parents measure children and determine their positions.
-   **Draw Phase**: The pass where visible content is rendered to the screen/canvas.
-   **Modifier**: A fluent API for configuring layout behavior, appearance, and interaction.

## Example: Custom Layout

The `Layout` primitive allows you to define custom measurement and placement logic.

```rust
use cranpose::prelude::*;

#[composable]
fn CustomColumn(modifier: Modifier, content: impl Fn()) {
    Layout(
        modifier, 
        content,
        |measurable_list, constraints| {
            // Measure children
            let placeables: Vec<_> = measurable_list
                .iter()
                .map(|m| m.measure(constraints))
                .collect();
            
            // Calculate total size
            let width = placeables.iter().map(|p| p.width()).max().unwrap_or(0);
            let height = placeables.iter().map(|p| p.height()).sum();
            
            // Layout children
            layout(width, height, move |placeable_list| {
                let mut y = 0;
                for placeable in placeables {
                    placeable.place(0, y);
                    y += placeable.height();
                }
            })
        }
    )
}
```
