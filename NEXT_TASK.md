# RS-Compose Development Roadmap

**Last Updated**: April 2026  
**Modifier System Status**: Upstream main claims parity; work branch has active parity fixes in flight.  
**Documentation Status**: ✅ Comprehensive internals documented

---

## 🔎 Upstream Main Snapshot (Nov 2025)
These items mirror the roadmap that already shipped on `main` so they remain visible after rebasing.

### Modifier System Parity Claims
- ✅ `ModifierNodeEntry` stores `Rc<RefCell<Box<dyn ModifierNode>>>`
- ✅ `LayoutModifierNode::measure` returns `LayoutModifierMeasureResult` with placement control
- ✅ `LayoutModifierCoordinator` holds live node references (no proxy bypass)
- ✅ Chain preservation intended — flattening marked as removed in `main`
- ✅ Node lifecycle: `on_attach()`, `on_detach()`, `on_reset()`
- ✅ Capability-based dispatch with bitflags
- ✅ Core modifiers implemented: Padding, Size, Offset, Background, Clickable, etc.

### Completion Milestones (main)
- **Phase 1: Shared Ownership & Protocol** — live node references, direct coordinator calls
- **Phase 2: Eliminating Flattening** — single measurement path, padding/offset/size handled by nodes
- **Jetpack Compose Parity Achieved** — ordering preserved, placement control, no panicking APIs
- **Production Ready** — 460+ workspace tests reported green in main

### Other Main-Branch Priorities
- **Performance Optimization**: benchmark modifier traversal, cache aggregated capabilities, pool allocations in `update_from_slice`, reusable benchmark suite
- **API Ergonomics**: builder patterns, common helpers, clearer RefCell errors, richer documentation/examples
- **Advanced Features**: animated/conditional modifiers, modifier scopes, custom coordinator hooks
- **Testing & Validation**: property-based tests, integration scenarios, stress tests for deep chains, performance regression tracking

---

## ⚠️ Reality Checks (work branch)
Observed divergences that must be addressed before the parity claim is trustworthy after rebasing.

- `ResolvedModifiers` still flattens padding/size/offset, losing modifier ordering (e.g., `padding.background.padding`).
- `ModifierNodeSlices` coalesces text and graphics layers to the last writer instead of composing them.
- `MeasurementProxy` remains in the public API even though coordinators measure nodes directly.

---

## 🧭 Reconciliation Plan (merge-friendly)
These steps combine the upstream claims with the work-branch findings so rebasing on `main` keeps both perspectives.

1. **Eliminate layout flattening**  
   Route padding/size/offset/intrinsics through live nodes and coordinators; add regression tests for mixed chains.
2. **Make draw/text slices composable**  
   Stack graphics layers and allow multiple text entries in chain order; keep pointer handlers ordered.
3. **Resolve the measurement proxy story**  
   Remove the unused surface or integrate it meaningfully (e.g., borrow-safe async measurement) and update docs/tests.
4. **Resume upstream priorities once parity is validated**  
   Continue performance, ergonomics, advanced features, and testing work from the main snapshot.

---

## 🎯 Active Focus Areas (post-rebase)

### A) Modifier System Corrections
- Remove layout flattening and keep ordering semantics.
- Compose draw/text slices instead of last-write-wins.
- Decide and implement the measurement proxy direction.
- Add regression tests for mixed chains and slice composition.

### B) Real-World Application Development
- **Complex Desktop Application**: multi-window, nested layouts, drag-and-drop, keyboard shortcuts, custom rendering.
- **Dashboard/Data Visualization App**: charts/graphs, large-data LazyColumn/LazyRow, scrolling performance.
- **Form-Heavy Application**: text validation, focus management, tab navigation, error/accessibility states.

### C) Performance Optimization & Benchmarking
- **Benchmark Suite**: traversal depth, recomposition patterns, complex layout measurement, draw command generation, allocation profiling.
- **Optimization Targets**: cache aggregated node capabilities, pool allocations in `update_from_slice`, optimize `SnapshotIdSet`, reduce `RefCell` overhead where safe.
- **Profiling Infrastructure**: flamegraphs for layout/draw, frame time tracking, memory usage tracking.

### D) Missing Core Features
- **Intrinsic Measurements**: implement `IntrinsicMeasureScope` for Row/Column, `IntrinsicSize.Min/Max`, baseline alignment.
- **Text Editing**: TextField variants, cursor/selection, IME integration, selection gestures.
- **Lazy Layouts Performance**: large dataset handling, viewport-aware recycling, smooth scrolling.
- **Draw/Graphics Enhancements**: layered draw modifiers, blend modes, shaders; composition of text and graphics slices.
- **Pointer/Input**: richer gesture recognizers, multi-pointer coordination, scroll/drag/zoom handling.
- **Accessibility & Semantics**: semantics tree parity with Jetpack Compose, focus traversal, screen reader annotations.

---

## ✅ Documentation References
- **MODIFIERS.md** — modifier system internals (37KB)
- **SNAPSHOTS_AND_SLOTS.md** — snapshot and slot table internals (54KB)
- **modifier_match_with_jc.md** — parity status tracking
