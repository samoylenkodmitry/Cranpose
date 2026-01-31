# Cranpose Macros

Procedural macros that power the Cranpose declarative syntax.

## When to Use

You will rarely interact with this crate directly; it is re-exported by `cranpose`. Identifying how the `#[composable]` macro transforms your code is useful for debugging and understanding performance characteristics.

## Key Concepts

-   **`#[composable]`**: This attribute macro transforms a standard Rust function into a `Composable` function. It injects a hidden `Composer` parameter and wraps the function body in a uniquely identified group.
-   **Skipping**: The macro generates code to compare current arguments with previous arguments. If they haven't changed, the function body execution is skipped during recomposition, significantly improving performance.

## Transformation Example

Conceptual expansion of what `#[composable]` does:

```rust
// Source
#[composable]
fn MyComponent(name: String) {
    Text(name);
}

// Generated (Conceptual)
fn MyComponent(composer: &mut Composer, changed: usize, name: String) {
    composer.start_restart_group(12345); // Unique ID based on location
    
    if changed == 0 && composer.skipping() {
        composer.skip_to_group_end();
    } else {
        Text(composer, changed, name);
    }
    
    composer.end_restart_group(|composer| MyComponent(composer, changed | 1, name));
}
```
