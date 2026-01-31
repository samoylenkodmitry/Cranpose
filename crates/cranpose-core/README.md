# Cranpose Core

The core runtime engine for Cranpose. This crate implements the fundamental algorithms for managing the composition tree, state snapshots, and change detection.

## When to Use

Use `cranpose-core` directly if you are:
-   Building a custom tree management system unrelated to UI (e.g., a reactive scene graph).
-   Implementing low-level state primitives.
-   Developing a renderer for a strictly non-standard environment where the higher-level `cranpose-ui` might be too opinionated.

## Key Concepts

-   **Slot Table**: A linear, gap-buffer-based data structure that stores the composition tree efficiently in memory. It optimizes for locality and minimizes allocations compared to pointer-based trees.
-   **Composer**: The primary interface for building and updating the slot table. It tracks the current position in the tree and handles inserting, updating, or removing nodes.
-   **Snapshot System**: A multi-version concurrency control (MVCC) system for state. It allows `MutableState` to be read and written transactions, enabling atomic updates and ensuring UI consistency during concurrent operations.
-   **Recomposition**: The process of re-executing composable functions when their dependencies change. The runtime tracks dependencies at a fine-grained level (scopes) to minimize re-execution.

## Example: Manual State Transaction

The following example shows how to perform atomic state updates using the snapshot system explicitly.

```rust
use cranpose_core::{mutableStateOf, run_in_mutable_snapshot};

fn main() {
    let count = mutableStateOf(0);

    // Any modification to state must happen within a snapshot.
    // This ensures that observers (like the UI recomposer) see a consistent view of data.
    run_in_mutable_snapshot(|| {
        let current = count.value();
        count.set(current + 1);
    });

    assert_eq!(count.value(), 1);
}
```
