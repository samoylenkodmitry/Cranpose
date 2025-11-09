# Next Task: Pointer & Draw Pipeline Parity

## Context
Capability masks now mirror Jetpack Compose’s `NodeKind` flags and `LayoutNode` / `SubcomposeLayoutNode` only dirty the phases that have interested nodes, but the runtime still treats draw and pointer behavior as value-based `ModifierState`. Renderers continue to read cached background/graphics-layer state and the pointer dispatcher calls the legacy closures on `ModifierState` instead of traversing `ModifierNodeChain` slices. To reach Kotlin parity we need to drive draw + pointer passes directly from reconciled nodes, reuse Kotlin’s coroutine-backed pointer input lifecycle, and prove the new traversal order matches `androidx.compose.ui` (`ModifierNodeChain.kt`, `DelegatableNode.kt`, `PointerInputModifierNode.kt`).

## Goals
1. Teach every renderer (`compose-render/*`) to walk `ModifierNodeChain::draw_nodes()` / `pointer_input_nodes()` (via `LayoutNode` helpers) so draw order, clipping, and hit testing mirror Kotlin.
2. Port the pointer-input coroutine machinery (`PointerInputModifierNode`, `AwaitPointerEventScope`, cancellation semantics) from `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/pointer`, and rebase `ClickableElement`/gesture modifiers on the new API.
3. Expose capability-gated iterators (`layout_nodes()`, `draw_nodes()`, `pointer_input_nodes()`) through `LayoutNode` / `SubcomposeLayoutNode` handles so subsystems can short-circuit when a slice is empty.
4. Add tests that validate draw/pointer traversal order, pointer cancellation, and capability gating (unit tests under `crates/compose-ui/src/tests/` plus renderer smoke tests).

## Suggested Steps
1. **Surface node slices to subsystems**  
   - Add helpers such as `LayoutNode::draw_nodes()` / `pointer_input_nodes()` (wrapping the existing `ModifierChainHandle` iterators) and mirror them on `SubcomposeLayoutNodeHandle`.  
   - Update `ResolvedModifiers` consumers to call these helpers instead of touching `ModifierState`.
2. **Update rendering / pointer dispatch**  
   - Walk `draw_nodes()` inside each renderer and run `DrawModifierNode::draw` hooks before/after child content, mirroring Kotlin’s `NodeCoordinator`.  
   - Replace the pointer dispatcher’s direct click handler invocation with traversal over `pointer_input_nodes()`, using capability checks to skip nodes quickly.
3. **Port pointer-input coroutine flow**  
   - Translate `AwaitPointerEventScope`, `PointerInputModifierNode`, and cancellation rules from the Kotlin sources into `crates/compose-ui/src/modifier/pointer_input.rs`.  
   - Reimplement `ClickableElement` (and any existing gesture modifiers) so they create proper pointer-input nodes instead of storing closures in `ModifierState`.
4. **Tests & parity validation**  
   - Extend `modifier_nodes_tests.rs` with fixtures that enqueue multiple draw + pointer nodes and assert traversal order / invalidations.  
   - Add integration tests (and, if feasible, renderer snapshots) that simulate pointer sequences to ensure cancellation + restart behavior matches Kotlin.

## Definition of Done
- Renderers and pointer dispatchers consume draw/pointer modifier nodes via capability-filtered iterators; no remaining draw/pointer logic relies on `ModifierState`.
- Pointer-input coroutine scaffolding (`PointerInputModifierNode`, `AwaitPointerEventScope`, cancellation) exists in `compose-ui`, and `ClickableElement` (plus any existing gesture modifiers) uses it.
- `LayoutNode` / `SubcomposeLayoutNode` expose ergonomic helpers (`has_pointer_input_modifier_nodes`, iterators) that short-circuit subsystem work when slices are empty.
- New tests cover traversal order, pointer cancellation semantics, and capability gating; `cargo test -p compose-ui` passes.
