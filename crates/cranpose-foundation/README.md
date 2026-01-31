# Cranpose Foundation

Fundamental building blocks and system services for Cranpose UI.

## When to Use

This crate provides the essential primitives that `cranpose-ui` is built upon. It contains:
-   **The Modifier System**: The core logic for `Modifier` chains and node delegation.
-   **Basic Layouts**: `Box`, `Row`, `Column` (the layout algorithms themselves).
-   **Input APIs**: `PointerInput`, `FocusManager`.

Library authors building their own widget sets or design systems might depend on `cranpose-foundation` to avoid pulling in the opinionated widgets of `cranpose-ui`.

## Key Concepts

-   **Modifier Node**: A stateful object attached to a layout node that can participate in layout, drawing, and input handling. This is more efficient than the stateless `Modifier` chain object, which is just a factory configuration.
-   **Semantics**: Accessibility and testing information attached to the UI tree.
-   **Focus System**: Manages keyboard navigation and focus request propagation.

## Example: Custom Modifier

Creating a custom modifier involves defining a `ModifierNode` and a fluent builder method.

```rust
use cranpose::prelude::*;

// 1. Define the Node
struct MyModifierNode;

impl ModifierNode for MyModifierNode {}

// 2. Define the Element (Factory)
struct MyModifierElement;

impl ModifierElement<MyModifierNode> for MyModifierElement {
    fn create(&self) -> MyModifierNode {
        MyModifierNode
    }
    
    fn update(&self, _node: &mut MyModifierNode) {
        // Update node properties if needed
    }
}

// 3. Extension method for fluent API
trait MyModifierExt {
    fn my_custom_modifier(self) -> Modifier;
}

impl MyModifierExt for Modifier {
    fn my_custom_modifier(self) -> Modifier {
        self.then(MyModifierElement)
    }
}
```
