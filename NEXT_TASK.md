# Next Tasks for RS-Compose

> **Last Updated**: December 2024
> 
> Prioritized roadmap for achieving 1:1 architectural parity with [Jetpack Compose](https://github.com/androidx/androidx/tree/androidx-main/compose)

---

## Architectural Mapping: Compose ↔ RS-Compose

| Jetpack Compose Module | RS-Compose Crate | Status |
|------------------------|------------------|--------|
| `runtime/runtime` | `compose-core` | ✅ Done |
| `runtime/runtime-saveable` | `compose-core` | ⚠️ Partial |
| `ui/ui` | `compose-ui`, `compose-foundation` | ✅ Done |
| `ui/ui-geometry` | `compose-ui` | ✅ Inlined |
| `ui/ui-graphics` | `compose-ui-graphics` | ✅ Done |
| `ui/ui-text` | `compose-ui` | ⚠️ Basic |
| `ui/ui-unit` | `compose-ui-layout` | ✅ Done |
| `foundation/foundation` | `compose-foundation` | ⚠️ Partial |
| `foundation/foundation-layout` | `compose-foundation` | ✅ Done |
| `animation/animation-core` | `compose-animation` | ⚠️ Basic |
| `animation/animation` | — | ❌ Missing |
| `material3/material3` | — | ❌ Missing |

---

## 🔴 P0: Critical Gaps for Production Use

### Text Input System
*Reference: `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/`*

#### Undo/Redo System *(completed)*
*Reference: `text/input/TextUndoManager.kt`, `text/input/UndoState.kt`*
- [ ] `TextUndoManager` with staging area for merging edits
- [ ] Smart merging of consecutive character insertions

#### Keyboard Options *(lower priority)*
*Reference: `text/KeyboardOptions.kt`, `text/KeyboardActions.kt`*
- [ ] `KeyboardOptions` (capitalization, autoCorrect, keyboardType)
- [ ] `KeyboardActions` (onDone, onNext, onSearch, etc.)
- [ ] IME action handling

#### Advanced Features *(future)*
- [ ] Selection handles and magnifier (mobile)
- [ ] Context menu (right-click/long-press)
- [ ] `InputTransformation` for input filtering
- [ ] `OutputTransformation` for display formatting

### Lazy Layout System
*Reference: `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/lazy/`*
- [ ] `LazyColumn` with virtualized rendering
- [ ] `LazyRow` for horizontal lists
- [ ] `LazyListState` (scroll position, layoutInfo)
- [ ] `LazyListMeasure` + item provider
- [ ] Prefetch strategy
- [ ] `stickyHeader` support
- [ ] `LazyGrid` (fixed/adaptive columns)

### Animation High-Level APIs
*Reference: `compose/animation/animation/src/commonMain/kotlin/androidx/compose/animation/`*
- [ ] `AnimatedVisibility` with `EnterTransition`/`ExitTransition`
- [ ] `AnimatedContent` with `ContentTransform`
- [ ] `Crossfade` for simple content switching
- [ ] Shared element transitions

---

## 🟠 P1: Important Subsystems

### Focus System
*Reference: `compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/focus/`*
- [ ] `FocusManager` implementation
- [ ] `FocusRequester` for programmatic focus
- [ ] `FocusTargetModifierNode`
- [ ] Tab navigation (`OneDimensionalFocusSearch`)
- [ ] Arrow key navigation (`TwoDimensionalFocusSearch`)
- [ ] Focus restoration on recomposition

### Nested Scroll
*Reference: `compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/nestedscroll/`*
- [ ] `NestedScrollConnection` interface
- [ ] `NestedScrollDispatcher`
- [ ] `nestedScroll()` modifier
- [ ] Scroll priority coordination

### Pager
*Reference: `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/pager/`*
- [ ] `HorizontalPager` / `VerticalPager`
- [ ] `PagerState` with currentPage, scrollToPage
- [ ] Snap fling behavior

### Subcomposition
*Reference: `compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/layout/SubcomposeLayout.kt`*
- [ ] `SubcomposeLayout` for measurement-time composition
- [ ] `SubcomposeLayoutState`
- [ ] Slot reuse tracking

---

## Reference Files

When implementing, refer to these key Compose source files:

| Subsystem | File Path |
|-----------|-----------|
| LayoutNode | `ui/ui/src/commonMain/kotlin/androidx/compose/ui/node/LayoutNode.kt` |
| NodeChain | `ui/ui/src/commonMain/kotlin/androidx/compose/ui/node/NodeChain.kt` |
| ModifierNodeElement | `ui/ui/src/commonMain/kotlin/androidx/compose/ui/node/ModifierNodeElement.kt` |
| PointerInputModifierNode | `ui/ui/src/commonMain/kotlin/androidx/compose/ui/node/PointerInputModifierNode.kt` |
| HitPathTracker | `ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/pointer/HitPathTracker.kt` |
| LazyList | `foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/lazy/LazyList.kt` |
| BasicTextField | `foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/BasicTextField.kt` |
| TextFieldState | `foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/input/TextFieldState.kt` |
| TextUndoManager | `foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/input/TextUndoManager.kt` |
| TextFieldSelectionManager | `foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/selection/TextFieldSelectionManager.kt` |
| TextFieldKeyInput | `foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/TextFieldKeyInput.kt` |
| ClipboardEventsHandler | `foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/ClipboardEventsHandler.kt` |
| AnimatedVisibility | `animation/animation/src/commonMain/kotlin/androidx/compose/animation/AnimatedVisibility.kt` |

## Cleanup
cargo clippy
cargo test
cargo tree --duplicates
(+robot tests)