# Modifier System: Jetpack Compose Parity Checkpoint

**Status**: Upstream `main` advertises parity; the work branch is validating and fixing remaining gaps.

This version reconciles `main`'s parity claims with the outstanding correctness issues discovered locally so the rebase keeps both sets of facts.

## ✅ What `main` reports as complete (Nov 2025)
- **Live Node References**: Coordinators hold `Rc<RefCell<Box<dyn ModifierNode>>>` directly, matching Kotlin's object references.
- **Placement Control**: `LayoutModifierNode::measure` returns `LayoutModifierMeasureResult` with size and placement offsets.
- **Node Lifecycle**: Proper `on_attach()`, `on_detach()`, `on_reset()` callbacks.
- **Capability Dispatch**: Bitflag-based capability system for traversal.
- **Node Reuse**: Zero allocations when modifier chains remain stable across recompositions.

Broader `main` follow-ups (performance, ergonomics, advanced modifiers, and deep testing) resume once the gaps below are closed.

## ⚠️ Reality checks on the work branch
- **Coordinator bypass**: `LayoutModifierCoordinator::measure` still treats the absence of a measurement proxy as "skip the node" rather than measuring the live node.
- **Missing placement API**: `LayoutModifierNode::measure` returns only `Size` in practice; placement is pass-through, preventing modifiers from affecting child placement (e.g., offset/alignment).
- **Flattened resolution**: `ModifierChainHandle::compute_resolved` aggregates padding/size/offset into a single `ResolvedModifiers`, losing ordering (e.g., `padding.background.padding`).
- **Slice coalescing**: `ModifierNodeSlices` collapses text content and graphics layers to last-write-wins, blocking composition of multiple layers.
- **Proxy dependency**: `MeasurementProxy` stays public even though coordinators aim to measure nodes directly, leaving an unused surface area.

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
