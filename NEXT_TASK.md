# Next Task: Modifier Diagnostics & Inspector Parity

## Context
All modifiers are now node-backed, `ModifierState` has been removed, and every runtime subsystem pulls data from reconciled node chains. The remaining parity gap versus Jetpack Compose is diagnostic tooling: Kotlin exposes rich inspector strings, delegate-depth dumps, and targeted tracing hooks (e.g., `Modifier.inspectable`, `ModifierNodeElement#inspectorInfo`, `NodeChain#trace`). Compose-RS currently has a single global switch (`COMPOSE_DEBUG_MODIFIERS`) that dumps every chain, which makes focused debugging impractical and leaves devtools without structured data.

## Current State
- `Modifier::fmt` and `compose_ui::debug::log_modifier_chain` emit textual dumps, but they lack capability masks, delegate depth, and inspector metadata beyond element names.
- Debug logging is all-or-nothing: setting `COMPOSE_DEBUG_MODIFIERS=1` prints every chain each reconciliation. There is no per-layout-node toggle or tracing hook downstream systems can use.
- `InspectorMetadata` exists in Rust but only stores arbitrary properties; we do not expose a structured API comparable to Kotlin’s `Modifier.inspectable` utilities or Android Studio’s “Modifier Inspector”.
- Pointer/focus stacks would benefit from targeted tracing (similar to Kotlin’s `PointerInputModifierNode#debug` routes) but there is no plumbing for enabling those on a single node chain.

## Goals
1. **Per-node debug toggles** — Allow layout/render nodes to opt-in to modifier logging/tracing without enabling global environment variables.
2. **Inspector parity** — Surface Kotlin-style inspector strings and structured metadata per modifier element so devtools (or CLI tooling) can display friendly names/arguments.
3. **Capability-aware chain dumps** — Expand the debug output to include delegate depth, capability masks, modifier-local providers, semantics/focus flags, and resolved inspector info, matching Jetpack Compose’s `NodeChain#trace` output.

## Jetpack Compose Reference
- `Modifier.kt` & `Modifier.inspectable` helpers (`androidx/compose/ui/Modifier.kt`)
- `ModifierInspector.kt` (`androidx/compose/ui/platform`)
- `NodeChain.kt#trace` and `DelegatableNode.kt` for delegate-depth dumps
- Devtools hooks under `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/platform/InspectableValue.kt`

## Implementation Plan

### Phase 1 — Node-Scoped Debug Flags
1. Add an opt-in flag on `LayoutNode` / `SubcomposeLayoutNodeInner` (e.g., `debug_modifiers: bool`).
2. Plumb that flag through `ModifierChainHandle::update_with_resolver` so chain logging only runs when either the env var is set **or** the node requested debugging.
3. Expose a public-facing toggle (temporary API under `compose_ui::debug` or `LayoutNodeHandle::set_debug_modifiers(bool)`) so tests/tools can enable debugging per node.
4. Tests: enable the flag for a single node, ensure only that node’s chain prints/logs.

### Phase 2 — Structured Inspector Metadata
1. Introduce a `ModifierInspectorRecord` struct mirroring Kotlin’s `InspectorInfo` (name + argument list).
2. Extend `ModifierChainHandle::compute_resolved` (or a new helper) to fold the reconciled chain and collect inspector info, including delegate depth and capability masks per element.
3. Provide an API such as `Modifier::collect_inspector_info()` or `debug::modifier_chain_inspector(handle)` returning structured data (JSON-friendly).
4. Update existing modifiers to record richer metadata (weights, alignments, graphics layers, pointerInput keys, semantics roles) using helper functions similar to Kotlin’s `inspectable`.
5. Tests: assert inspector output for representative modifiers (padding, weight, pointerInput) matches expected names/values.

### Phase 3 — Capability & Tracing Dumps
1. Add a reusable formatter (e.g., `debug::format_modifier_chain`) that prints:
   - delegate depth (head → delegates)
   - capability mask per entry (LAYOUT/DRAW/etc.)
   - inspector metadata
   - modifier-local providers / semantics / focus flags
2. Hook the formatter into both the global env var path and the node-scoped debug toggle introduced in Phase 1.
3. Provide a lightweight tracing hook (closure or channel) so pointer/focus subsystems can subscribe to chain events similar to Kotlin’s `NodeChain#trace`.
4. Tests: create fake chains with nested delegates and assert the formatter output includes depth/capabilities; add a regression test ensuring DRAW-only nodes appear in the dump with their masks.

## Acceptance Criteria
- `LayoutNode` / `SubcomposeLayoutNode` can log modifier chains independently of other nodes.
- Inspector metadata lists modifier names and arguments (e.g., `padding(start=8.0)`), matching Kotlin semantics.
- Chain dumps show delegate depth and capability masks; DRAW-only updates no longer require global logging to debug.
- Pointer/focus stacks can opt into tracing a single node chain for debugging.
- `cargo test` (workspace) passes.

## Notes
- Keep the new APIs `#[cfg(feature = "debug")]` if needed, but default to no-std-friendly data structures.
- Avoid `unsafe`; rely on existing node state/capability infrastructure.
- Prefer structured data over formatted strings so future devtools integrations can re-use it.
