# LazyList Implementation

Last Updated: 2026-01-05

Virtualized lazy layouts for Compose-RS with 1:1 API and architecture parity with Jetpack Compose (JC). This document tracks current alignment gaps, refactor tasks, and verification steps.

---

## Status

| Feature | Status | Notes |
|---------|--------|-------|
| SubcomposeLayout | VERIFIED | JC-style reuse + precompose path in place. Validated against `SubcomposeLayout.kt`. Rust uses `subcompose_slot` on composer. |
| LazyColumn/LazyRow | OK | Layout size constrained to content. Arrangement logic verified. |
| LazyListState | OK | Core state logic matches JC. Hybrid reactive `stats` approach implemented. |
| LazyListIntervalContent | OK | Matches JC interval model. |
| SlotReusePool | OK | Removed; SubcomposeState is single source of truth. |
| Lifecycle (compose/dispose) | OK | Dispose non-retained slots immediately. `dispose_or_reuse_starting_from_index` matches JC. |
| PrefetchScheduler | WIP | Implemented in `LazyListState` and `prefetch.rs`. Needs validation of idle-time execution in Rust async context. |
| Scrollable constraints | OK | LazyList asserts on infinite constraints (JC parity). |
| measure_lazy_list | OK | JC scroll/backfill flow + visible/beyond-bounds separation. Logic verified against `LazyListMeasure.kt`. |
| canScrollForward/Backward | OK | Parity. |
| IntrinsicSize modifiers | OK | Wrap-content uses IntrinsicSize; no unbounded constraints. |
| Item animations | BLOCKED | Needs coroutine API. |
| Fling/animated scroll | BLOCKED | Needs coroutine API. |

---

## Architecture

### Files

| File | Purpose |
|------|---------|
| `compose-foundation/src/lazy/lazy_list_state.rs` | Scroll state + stats. Implements `LazyListState` with `Rc<RefCell<Inner>>` and reactive `stats`. |
| `compose-foundation/src/lazy/lazy_list_scope.rs` | DSL + IntervalContent |
| `compose-foundation/src/lazy/lazy_list_measure.rs` | Measurement algorithm. `measure_lazy_list` function. |
| `compose-foundation/src/lazy/prefetch.rs` | Prefetch scheduler |
| `compose-ui/src/widgets/lazy_list.rs` | LazyColumn/LazyRow widgets |
| `compose-ui/src/subcompose_layout.rs` | SubcomposeLayoutNode implementation. Uses `SubcomposeMeasureScopeImpl`. |
| `compose-ui/src/modifier/scroll.rs` | Scroll gestures |
| `compose-core/src/subcompose.rs` | SubcomposeState + lifecycle. Tracks active/reusable/precomposed slots. |

### JC Reference (`/media/huge/composerepo/`)

| JC File | Path |
|---------|------|
| SubcomposeLayout | `compose/ui/ui/src/commonMain/.../SubcomposeLayout.kt` |
| LazyLayout | `compose/foundation/.../lazy/layout/LazyLayout.kt` |
| LazyListState | `compose/foundation/.../lazy/LazyListState.kt` |
| LazyListMeasure | `compose/foundation/.../lazy/LazyListMeasure.kt` |
| LazyLayoutPrefetchState | `compose/foundation/.../lazy/layout/LazyLayoutPrefetchState.kt` |

---

### Architectural Alignment & Code Quality Deep Dive

1.  **Shortcuts & "Laziness" (Ease over Rigor)**:
    *   **RefCell Proliferation**: `LazyListState` heavily relies on `Rc<RefCell<Inner>>` for all state. While standard for Rust GUIs to handle interior mutability, it risks runtime panics if `borrow_mut()` usage isn't strictly controlled (e.g., during nested calls). This is the "easy path" compared to more robust, potentially lock-free or message-passing state architectures, but acceptable for single-threaded UI.
    *   **Magic Numbers**:
        *   `MAX_VISIBLE_ITEMS = 500` in `measure_lazy_list` prevents infinite loops. This is a hardcoded limit that could bite users with massive screens or tiny items.
        *   `MAX_CACHE_SIZE = 100` in `LazyListState` is a fixed LRU size. Simple, but might thrash on large screens/lists.


2.  **Architectural Choices (The Good & The Risky)**:
    *   **Key Separation (Good)**: `LazyLayoutKey` enum (User vs Index) is a solid choice to prevent ID collisions, a common "shoot in the foot" problem in list frameworks.
    *   **Binary Search (Good)**: `find_interval` uses `partition_point` (O(log n)), fixing a previous O(n) bottleneck.
    *   **Subcompose Bridge (Acceptable Risk)**: The `RefCell` bridge in `LazyColumn` widget (`content_cell`) to pass updated closures to the measure policy is a standard Compose-RS pattern. It allows stable policy pointers but relies on imperative updates.

3.  **"Shoot in the Foot" Potential**:
    *   **Average Size Estimation**: `measure_lazy_list` relies on `state.average_item_size()` when scrolling to random locations. If item sizes vary significantly, this heuristic will cause the scrollbar to jump or position to be inaccurate. This is a known trade-off but undocumented in the API surface.
    *   **Interior Mutability Panic**: The mix of `dispatch_scroll_delta` (mutates state) and layout (reads state) needs careful sequencing to avoid `RefCell` borrowing errors.

4.  **Performance & Correctness**:
    *   **Subcompose Slot Management**: Strong alignment with JC. `SlotId` (u64) adaptation is valid.
    *   **Prefetch Execution**: Logic exists but needs verification of idle-time execution behavior in the Rust runtime.
    *   **Infinite Constraint Handling**: Correctly handles infinite constraints (horizontal/vertical separation) similar to JC.

---

## JC Alignment Notes

Key JC behavior to match (sources above):

-   **Slot Reuse**: Owned by `SubcomposeState` (aligned).
-   **Content Types**: `ContentTypeReusePolicy` implements per-type caps and compatibility (aligned).
-   **Immediate Disposal**: Slots not retained are disposed immediately (aligned).
-   **Prefetching**: Logic exists, validation of "idle" behavior pending.
-   **Structure**: Lazy measure logic (`measure_lazy_list`) closely follows `LazyListMeasure.kt` (aligned).

---

## Verification

Robot tests (run independently):

```
cargo run --package desktop-app --example robot_lazy_list --features robot-app
cargo run --package desktop-app --example robot_lazy_lifecycle --features robot-app
cargo run --package desktop-app --example robot_lazy_perf_validation --features robot-app
cargo run --package desktop-app --example robot_lazy_complex_scroll --features robot-app
cargo run --package desktop-app --example robot_lazy_list_after_modifiers --features robot-app
cargo run --package desktop-app --example robot_lazy_list_end_start_dup --features robot-app
cargo run --package desktop-app --example robot_lazy_list_end_alignment --features robot-app
cargo run --package desktop-app --example robot_positioned_boxes_after_lazy_list --features robot-app
cargo run --package desktop-app --example robot_recursive_layout --features robot-app
```

Workspace verification:

```
cargo fmt
cargo test --workspace
cargo clippy --workspace
cargo tree --duplicates
```

Last verified: 2026-01-05 (Code review and Architecture validation performed)
