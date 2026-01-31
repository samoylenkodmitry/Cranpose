# Cranpose UI Layout

The layout protocol and constraints system for Cranpose.

## When to Use

This crate defines the contract between parent and child layouts. You will use this crate when implementing custom `Layout` logic or creating new layout modifiers. It provides the types necessary to measure content and place it within the available space.

## Key Concepts

-   **`Constraints`**: Immutable constraints passed from parent to child, defining the minimum and maximum width and height a child is allowed to be.
-   **`Measurable`**: A trait for any node that can be measured.
-   **`Placeable`**: The result of a measurement pass. It holds the measured size and provides a method to position the content relative to its parent.

## Example: Fixed Size Measurement

```rust
impl Measurable for MyNode {
    fn measure(&self, constraints: Constraints) -> Placeable {
        // Coerce the requested size to be within the constraints
        let width = constraints.constrain_width(100);
        let height = constraints.constrain_height(100);
        
        layout(width, height, |_| {})
    }
}
```
