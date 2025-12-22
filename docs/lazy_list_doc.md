# LazyList Implementation

**Last Updated**: 2025-12-23

Virtualized lazy layouts for Compose-RS with 1:1 API parity with Jetpack Compose.

---

## Status

| Feature | Status |
|---------|--------|
| SubcomposeLayout | ✅ Complete |
| LazyColumn/LazyRow | ✅ Complete |
| LazyListState | ✅ Complete |
| LazyListIntervalContent | ✅ Complete |
| SlotReusePool | ⚠️ Metadata only |
| PrefetchScheduler | ✅ Basic implementation |
| measure_lazy_list | ✅ Complete |
| canScrollForward/Backward | ✅ Complete |
| Item animations | ❌ Blocked (coroutines) |
| Fling/animated scroll | ❌ Blocked (coroutines) |

---

## Architecture

### Files

| File | Purpose |
|------|---------|
| `compose-foundation/src/lazy/lazy_list_state.rs` | Scroll state |
| `compose-foundation/src/lazy/lazy_list_scope.rs` | DSL + IntervalContent |
| `compose-foundation/src/lazy/lazy_list_measure.rs` | Measurement algorithm |
| `compose-foundation/src/lazy/slot_reuse.rs` | Slot pool |
| `compose-foundation/src/lazy/prefetch.rs` | Prefetch |
| `compose-ui/src/widgets/lazy_list.rs` | LazyColumn/LazyRow |
| `compose-ui/src/subcompose_layout.rs` | SubcomposeLayout |
| `compose-ui/src/modifier/scroll.rs` | Scroll gestures |

### JC Reference (`/media/huge/composerepo/`)

| JC File | Path |
|---------|------|
| SubcomposeLayout | `compose/ui/ui/src/commonMain/.../SubcomposeLayout.kt` |
| LazyLayout | `compose/foundation/.../lazy/layout/LazyLayout.kt` |
| LazyListMeasure | `compose/foundation/.../lazy/LazyListMeasure.kt` |
| LazyLayoutPrefetchState | `compose/foundation/.../lazy/layout/LazyLayoutPrefetchState.kt` |

---

## Roadmap

### P0: Urgent 🔴

| Feature | JC Equivalent | Notes |
|---------|---------------|-------|
| **True slot reuse** | `SubcomposeSlotReusePolicy` | Currently metadata-only; actual node recycling needed for perf |
| **Dynamic arrangements** | `Arrangement.SpaceEvenly/Around/Between` | Currently only `SpacedBy` (fixed spacing) |
| **Collision-safe keys** | `getDefaultLazyLayoutKey()` | Current `index as u64` can collide with user keys |

### P1: Blocked on Coroutines API

> Require async/coroutine support matching JC.

| Feature | JC Equivalent | Notes |
|---------|---------------|-------|
| **ScrollableState trait** | `ScrollableState` | Foundation for fling, velocity, nested scroll |
| **Item animations** | `LazyLayoutItemAnimator` | Appearance/disappearance/position animations |
| **Animated scroll** | `animateScrollToItem()` | Basic UX requirement |
| **Fling velocity** | `ScrollableState.scroll()` | With `MutatePriority` |

### P2: Features

| Feature | JC Equivalent | Notes |
|---------|---------------|-------|
| **Sticky headers** | `StickyItemsPlacement` | Common use case |
| **Pinned items** | `LazyLayoutPinnedItemList` | For focus retention |
| **Remeasurement hooks** | `RemeasurementModifier` | Scroll optimization |
| **Nested scrolling** | Nested scroll connection | Parent-child scroll coordination |

### P3: Optimizations

| Feature | JC Equivalent | Notes |
|---------|---------------|-------|
| **Velocity-based prefetch** | `LazyListPrefetchStrategy` | Current is direction-based only, no velocity |
| **Lookahead** | `isLookingAhead` | Animation coordination between passes |
| **State restoration** | `Saver` integration | Keys don't survive configuration changes |
