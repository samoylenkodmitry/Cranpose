# Modifier System: Jetpack Compose Parity Checkpoint
# (Main-branch reality snapshot merged with work-branch gaps)

**Status**: `main` advertises parity; the work branch is validating and fixing remaining gaps.

This edition merges `main`'s parity claims with the file-path-specific gaps previously documented so rebasing keeps both sets of facts.

## ✅ What `main` reports as complete (Nov 2025)
- **Live Node References**: Coordinators hold `Rc<RefCell<Box<dyn ModifierNode>>>` directly, matching Kotlin's object references.
- **Placement Control**: `LayoutModifierNode::measure` returns `LayoutModifierMeasureResult` with size and placement offsets.
- **Node Lifecycle**: Proper `on_attach()`, `on_detach()`, `on_reset()` callbacks.
- **Capability Dispatch**: Bitflag-based capability system for traversal.
- **Node Reuse**: Zero allocations when modifier chains remain stable across recompositions.

Broader `main` follow-ups (performance, ergonomics, advanced modifiers, and deep testing) resume once the gaps below are closed.

## ⚠️ Reality checks on the work branch
- **Coordinator bypass** (`crates/compose-ui/src/layout/coordinator.rs:120`): `LayoutModifierCoordinator::measure` still treats the absence of a measurement proxy as "skip the node" rather than measuring the live node.
- **Missing placement API** (`crates/compose-ui/src/layout/coordinator.rs:112`): `LayoutModifierNode::measure` returns only `Size`; placement is pass-through, preventing modifiers from affecting child placement (e.g., offset/alignment).
- **Flattened resolution** (`crates/compose-ui/src/modifier/chain.rs:188`): `ModifierChainHandle::compute_resolved` aggregates padding/size/offset into a single `ResolvedModifiers`, losing ordering (e.g., `padding.background.padding`).
- **Slice coalescing** (`crates/compose-ui/src/modifier/slices.rs`): `ModifierNodeSlices` collapses text content and graphics layers to last-write-wins, blocking composition of multiple layers.
- **Proxy dependency**: `MeasurementProxy` stays public even though coordinators aim to measure nodes directly, leaving an unused surface area.

## 🚧 Mismatches vs Jetpack Compose (context)
- **Live node vs snapshot proxy**: Kotlin's `LayoutModifierNodeCoordinator` measures the live `LayoutModifierNode`. The Rust implementation relies on proxies and can ignore nodes if none are provided.
- **Placement control**: Kotlin's `measure` returns a `MeasureResult` containing a `placeChildren` lambda. Rust currently returns `Size` only, with placement handled by pass-through coordinator logic.
- **Chain traversal**: Kotlin traverses the actual node chain for all operations. Rust flattens layout-affecting modifiers into `ResolvedModifiers`, losing ordering in complex chains.

## 🛠️ Reconciliation plan
1. **Fix layout modifier protocol**  
   Measure live nodes (or meaningful proxies) and return placement-aware results; remove the "no proxy = skip" behavior.
2. **Remove flattening**  
   Route padding/size/offset/intrinsics through the node chain and add coverage for interleaved modifier ordering.
3. **Make draw/text slices composable**  
   Allow stacking of graphics layers and multiple text entries; preserve chain order for draw and pointer handlers.
4. **Decide the proxy story**  
   Either remove `MeasurementProxy` or integrate it for a real use case (e.g., borrow-safe async measurement), then align docs/tests.
5. **Continue `main` priorities after parity is validated**  
   Resume performance/ergonomics/advanced feature work once the above correctness items are landed.

## Reference documentation
- **[MODIFIERS.md](./MODIFIERS.md)** — modifier system internals (37KB, Nov 2025)
- **[SNAPSHOTS_AND_SLOTS.md](./SNAPSHOTS_AND_SLOTS.md)** — snapshot and slot table system (54KB, Nov 2025)
- **[NEXT_TASK.md](./NEXT_TASK.md)** — roadmap with merged `main` snapshot and local parity fixes
