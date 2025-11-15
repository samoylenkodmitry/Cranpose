# Modifier Migration Reality Check

The modifier API surface is moving in the right direction (builder helpers now chain via
`self.then`, `ModifierNodeChain` has capability tracking, and the node-backed factories live in
`crates/compose-ui/src/modifier_nodes.rs`). This document records the current status and remaining
gaps before we can claim full parity with Jetpack Compose.

---

## Current Snapshot

- ✅ `ModifierNodeChain` reconciliation, capability masks, and helper macros exist in
  `crates/compose-foundation/src/modifier.rs` and are used by the built-in nodes under
  `crates/compose-ui/src/modifier_nodes.rs`.
- ✅ Public modifier builders (padding/background/fill/etc.) now consume `self` and use `then(...)`
  so callers can fluently chain them without reaching for ad-hoc constructors.
- ✅ Pointer/focus invalidation managers (`crates/compose-ui/src/pointer_dispatch.rs` and
  `crates/compose-ui/src/focus_dispatch.rs`) are now invoked by the app shell runtime during frame
  processing. The `process_pointer_repasses` / `process_focus_invalidations` functions are called
  in `AppShell::run_dispatch_queues()`, and the `needs_pointer_pass` / `needs_focus_sync` flags on
  `LayoutNode` are properly cleared after processing, matching Jetpack Compose's invalidation pattern.
- ✅ **Legacy widget-specific nodes removed.** `ButtonNode`, `TextNode`, and `SpacerNode` have been
  deleted. All widgets now use `LayoutNode` with appropriate measure policies.
- ✅ **Centralized modifier resolution.** The legacy `measure_spacer`, `measure_text`, and
  `measure_button` functions that rebuilt modifiers via `Modifier::empty().resolved_modifiers()` have
  been removed. All measurement goes through the unified `measure_layout_node` path.
- ✅ **Metadata fallbacks removed.** `runtime_metadata_for` and `compute_semantics_for_node` only
  handle `LayoutNode` and `SubcomposeLayoutNode`, ensuring consistent modifier chain traversal.
- ⚠️ **Text implementation shortcut.** Text content is currently stored in `TextMeasurePolicy` with
  a `text_content()` method added to the `MeasurePolicy` trait. This violates separation of concerns
  and doesn't match Jetpack Compose's architecture (see "Known Shortcuts" section below).
- ⚠️ Tests under `crates/compose-ui/src/tests/pointer_input_integration_test.rs` simply assert node
  counts; no integration test actually drives pointer events through `HitTestTarget`.

---

## Known Shortcuts

### Text Implementation Architecture Mismatch

**Current (Shortcut) Implementation:**
- Text content stored in `TextMeasurePolicy`
- Added `text_content()` method to `MeasurePolicy` trait for extracting text
- `Text()` widget: `Layout(modifier, TextMeasurePolicy::new(text), || {})`

**Problem:**
- Violates separation of concerns - `MeasurePolicy` shouldn't store rendering/semantic content
- Pollutes `MeasurePolicy` trait with domain-specific methods
- Doesn't match Jetpack Compose architecture

**Jetpack Compose Architecture:**
```kotlin
// In BasicText.kt
Layout(finalModifier, EmptyMeasurePolicy)

// Where finalModifier includes:
TextStringSimpleElement(text, style, ...) // Creates TextStringSimpleNode
```

**TextStringSimpleNode** implements:
- `LayoutModifierNode` - for measurement
- `DrawModifierNode` - for drawing
- `SemanticsModifierNode` - for semantics

Text content lives in the **modifier node**, not in MeasurePolicy!

**Proper Fix Required:**
1. Create `TextModifierNode: LayoutModifierNode + DrawModifierNode + SemanticsModifierNode`
2. Create `TextModifierElement` that produces `TextModifierNode`
3. Update `Text()` to: `Layout(modifier.textModifier(text, style, ...), EmptyMeasurePolicy, || {})`
4. Remove `text_content()` from `MeasurePolicy` trait
5. Delete `TextMeasurePolicy` (use empty/simple policy instead)

**Reference Files:**
- `/media/huge/composerepo/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/BasicText.kt`
- `/media/huge/composerepo/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/modifiers/TextStringSimpleNode.kt`

---

## Work Remaining Before Full Parity

1. ✅ **COMPLETED: Hook up the dispatch queues.**
2. ✅ **COMPLETED: Delete the widget-specific node types.**
3. ✅ **COMPLETED: Centralize resolved modifier data.**
4. ⚠️ **Fix Text implementation to use modifier nodes.**
   - Implement `TextModifierNode` following JC's `TextStringSimpleNode` pattern
   - Move text content from `TextMeasurePolicy` to the modifier node
   - Update rendering/semantics to extract text from modifier chain
5. **Add real integration coverage.**
   - Extend the pointer/focus tests to synthesize events through `HitTestTarget` so we can verify
     suspending pointer handlers, `Modifier.clickable`, and focus callbacks operate end-to-end.

---

## Jetpack Compose References

Use these upstream files while implementing the remaining pieces:

| Area | Kotlin Source | Compose-RS Target |
| --- | --- | --- |
| Modifier API | `androidx/compose/ui/Modifier.kt` | `crates/compose-ui/src/modifier/mod.rs` |
| Node lifecycle | `ModifierNodeElement.kt`, `DelegatableNode.kt` | `crates/compose-foundation/src/modifier.rs` |
| Text modifier nodes | `foundation/text/modifiers/TextStringSimpleNode.kt` | *TODO: create `TextModifierNode`* |
| Text widget | `foundation/text/BasicText.kt` | `crates/compose-ui/src/widgets/text.rs` |
| Layout modifier | `ui/layout/LayoutModifier.kt` | `crates/compose-foundation/src/modifier.rs` (LayoutModifierNode) |
| Pointer input | `ui/input/pointer/*` | `crates/compose-ui/src/modifier/pointer_input.rs` |
| Focus system | `FocusInvalidationManager.kt`, `FocusOwner.kt` | `crates/compose-ui/src/modifier/focus.rs` + dispatch managers |
| Semantics | `semantics/*` | `crates/compose-ui/src/semantics` |

Keep this document up to date as we chip away at the remaining tasks so reviewers can clearly see
which parts of the Kotlin contract are satisfied.
