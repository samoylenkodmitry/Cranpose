# Modifier System: Jetpack Compose Parity Checkpoint

**Status**: ⚠️ Parity claim from origin/main is under validation; outstanding gaps remain on the work branch.

This document merges the parity narrative from origin/main with the current reality checks discovered while exercising the modifier runtime.

## What origin/main reports as complete (Nov 2025)

- ✅ **Live Node References**: Coordinators hold `Rc<RefCell<Box<dyn ModifierNode>>>` directly, matching Kotlin's object references.
- ✅ **Placement Control**: `LayoutModifierNode::measure` returns `LayoutModifierMeasureResult` with size and placement offsets.
- ✅ **Node Lifecycle**: Proper `on_attach()`, `on_detach()`, `on_reset()` callbacks.
- ✅ **Capability Dispatch**: Bitflag-based capability system for traversal.
- ✅ **Node Reuse**: Zero allocations when modifier chains remain stable across recompositions.

## Reality checks on the work branch

- **Flattened resolution**: `ModifierChainHandle::compute_resolved` still aggregates padding/size/offset into a single `ResolvedModifiers`, losing ordering (e.g., `padding.background.padding`).
- **Slice coalescing**: `ModifierNodeSlices` collects draw commands and pointer inputs but collapses text content and graphics layers to "last write wins", blocking composition of multiple layers.
- **Unused measurement proxy**: The `MeasurementProxy` API remains in the public surface even though `LayoutModifierCoordinator` measures nodes directly. Keeping it without integration adds maintenance overhead.

## Reconciliation plan (merge of origin/main and work-branch needs)

1. **Eliminate layout flattening**
   - Route padding/size/offset/intrinsic behavior through live nodes and coordinators instead of `ResolvedModifiers`.
   - Add coverage for mixed chains (e.g., `padding.background.padding`) to ensure ordering is preserved.

2. **Make draw/text slices composable**
   - Allow multiple text entries and graphics layers to stack instead of last-write-wins semantics.
   - Preserve chain order when emitting draw commands and pointer handlers.

3. **Decide the measurement proxy story**
   - Either remove `MeasurementProxy` and related implementations or integrate it meaningfully (e.g., borrow-safe async measurement).
   - Update documentation and tests so the public API matches runtime behavior.

4. **Continue origin/main focus areas once parity is re-validated**
   - Performance optimization of modifier traversal and capability caching.
   - Advanced features such as animated/conditional modifiers and custom coordinators.
   - Developer experience: clearer errors, guides, and examples.

## Reference documentation

- **[MODIFIERS.md](./MODIFIERS.md)** - Complete modifier system internals (37KB, Nov 2025)
- **[SNAPSHOTS_AND_SLOTS.md](./SNAPSHOTS_AND_SLOTS.md)** - Snapshot and slot table system (54KB, Nov 2025)
- **[NEXT_TASK.md](./NEXT_TASK.md)** - Broader project roadmap with modifier corrections highlighted
