# LazyColumn / LazyRow / SubcomposeLayout Implementation

**Goal**: Virtualized lazy layouts for Compose-RS with 1:1 API parity with Jetpack Compose.

**Last Updated**: 2025-12-22

---

## Architecture Comparison

### Layer Mapping

| Compose-RS | JC Source | Path | Status |
|------------|-----------|------|--------|
| `SubcomposeLayout` | `SubcomposeLayout` | `compose/ui/ui/.../SubcomposeLayout.kt` | ✅ |
| `LazyColumn`/`LazyRow` | `LazyList` | `compose/foundation/.../lazy/LazyList.kt` | ✅ |
| `LazyListState` | `LazyListState` | `compose/foundation/.../lazy/LazyListState.kt` | ⚠️ |
| `LazyListIntervalContent` | `LazyLayoutItemProvider` | `compose/foundation/.../lazy/layout/LazyLayoutItemProvider.kt` | ✅ |
| `SlotReusePool` | `LazyLayoutItemReusePolicy` | `compose/foundation/.../lazy/layout/LazyLayout.kt:147-164` | ✅ |
| `PrefetchScheduler` | `LazyLayoutPrefetchState` | `compose/foundation/.../lazy/layout/LazyLayoutPrefetchState.kt` | ✅ |
| `measure_lazy_list` | `measureLazyList` | `compose/foundation/.../lazy/LazyListMeasure.kt` | ✅ |
| — | `LazyListScrollPosition` | `compose/foundation/.../lazy/LazyListScrollPosition.kt` | ❌ |
| — | `LazyLayoutItemAnimator` | `compose/foundation/.../lazy/layout/LazyLayoutItemAnimator.kt` | ❌ |

---

## Key Implementation Patterns

### Content Freshness

**Problem**: `LazyColumn` must update content on each recomposition, but `compose_node` reuses nodes.

**JC Solution** (`LazyLayout.kt:113-118`):
```kotlin
val currentItemProvider = rememberUpdatedState(itemProvider)
val itemContentFactory = remember {
    LazyLayoutItemContentFactory(saveableStateHolder) { currentItemProvider.value() }
}
```

**Our Solution** (`lazy_list.rs:287-295`):
```rust
let content_cell = compose_core::remember(|| Rc::new(RefCell::new(LazyListIntervalContent::new())))
    .with(|cell| cell.clone());
*content_cell.borrow_mut() = content;  // Update on each recomposition

let content_for_policy = content_cell.clone();
let policy = Rc::new(move |scope, constraints| {
    let content_ref = content_for_policy.borrow();
    measure_lazy_list_internal(scope, constraints, true, &*content_ref, &state_clone, &config)
});
```

---

### Slot Reuse Policy (JC - Not Yet Integrated)

**JC** (`LazyLayout.kt:147-164`):
```kotlin
private class LazyLayoutItemReusePolicy(private val factory: LazyLayoutItemContentFactory) :
    SubcomposeSlotReusePolicy {
    private val countPerType = mutableObjectIntMapOf<Any?>()

    override fun getSlotsToRetain(slotIds: SlotIdsSet) {
        countPerType.clear()
        slotIds.fastForEach { slotId ->
            val type = factory.getContentType(slotId)
            val currentCount = countPerType.getOrDefault(type, 0)
            if (currentCount == MaxItemsToRetainForReuse) {
                slotIds.remove(slotId)
            } else {
                countPerType[type] = currentCount + 1
            }
        }
    }

    override fun areCompatible(slotId: Any?, reusableSlotId: Any?): Boolean =
        factory.getContentType(slotId) == factory.getContentType(reusableSlotId)
}

private const val MaxItemsToRetainForReuse = 7
```

**Status**: ✅ Integrated — `SlotReusePool` wired into `LazyListState` with `mark_in_use()` and `release_non_visible()`.

---

### Measurement Algorithm

**JC** (`LazyListMeasure.kt:50-77`):
```kotlin
internal fun measureLazyList(
    itemsCount: Int,
    measuredItemProvider: LazyListMeasuredItemProvider,
    mainAxisAvailableSize: Int,
    beforeContentPadding: Int,
    afterContentPadding: Int,
    spaceBetweenItems: Int,
    firstVisibleItemIndex: Int,
    firstVisibleItemScrollOffset: Int,
    scrollToBeConsumed: Float,
    constraints: Constraints,
    isVertical: Boolean,
    beyondBoundsItemCount: Int,
    // ... more params
): LazyListMeasureResult
```

**Our** (`lazy_list_measure.rs:65-72`):
```rust
pub fn measure_lazy_list<F>(
    items_count: usize,
    state: &LazyListState,
    viewport_size: f32,
    _cross_axis_size: f32,
    config: &LazyListMeasureConfig,
    mut measure_item: F,
) -> LazyListMeasureResult
```

---

## Roadmap

### ✅ Completed
- SlotReusePool integration
- PrefetchScheduler integration
- Scroll gesture DRY refactor
- Key-based scroll stability
- Cached sizes for estimation
- Configurable beyond_bounds
- NearestRangeState for O(1) key lookup

### Planned (P3)
- [ ] Item animations (`animateItemPlacement`)
- [ ] Sticky headers (`stickyHeader {}`)
- [ ] canScrollForward/Backward API

---

## Files Reference

### Compose-RS
| File | Purpose |
|------|---------|
| `compose-foundation/src/lazy/lazy_list_state.rs` | Scroll state |
| `compose-foundation/src/lazy/lazy_list_scope.rs` | DSL + IntervalContent |
| `compose-foundation/src/lazy/lazy_list_measure.rs` | Measurement algorithm |
| `compose-foundation/src/lazy/slot_reuse.rs` | Slot pool ✅ |
| `compose-foundation/src/lazy/prefetch.rs` | Prefetch ✅ |
| `compose-ui/src/widgets/lazy_list.rs` | LazyColumn/LazyRow |
| `compose-ui/src/subcompose_layout.rs` | SubcomposeLayout |
| `compose-ui/src/modifier/scroll.rs` | Scroll gestures |

### Jetpack Compose (Reference: `/media/huge/composerepo/`)
| File | Path |
|------|------|
| SubcomposeLayout | `compose/ui/ui/src/commonMain/.../SubcomposeLayout.kt` |
| LazyLayout | `compose/foundation/.../lazy/layout/LazyLayout.kt` |
| LazyListMeasure | `compose/foundation/.../lazy/LazyListMeasure.kt` |
| LazyLayoutPrefetchState | `compose/foundation/.../lazy/layout/LazyLayoutPrefetchState.kt` |
