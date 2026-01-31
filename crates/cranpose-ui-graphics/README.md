# Cranpose UI Graphics

Graphics primitives and drawing definitions for Cranpose.

## When to Use

This crate contains the mathematical and visual types used for drawing. You will use it when:
-   Defining colors (`Color`).
-   Working with units (`Dp`, `Sp`, `Size`, `Offset`).
-   Creating custom shapes or paths for drawing modifiers.

## Key Concepts

-   **`Density`**: Interface for converting between density-independent pixels (`Dp`), scalable pixels (`Sp`), and raw physical pixels.
-   **`Brush`**: Defines how a shape is filled (e.g., `SolidColor`, `LinearGradient`).
-   **`Shape`**: Defines the outline of a renderable object (e.g., `RoundedCornerShape`).

## Example

```rust
let color = Color::Red;
let size = Size::new(100.0, 100.0);
let rect = Rect::from_origin_size(Point::ZERO, size);
```
