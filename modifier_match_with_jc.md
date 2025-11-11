# Modifier ≈ Jetpack Compose Parity Plan

Goal: match Jetpack Compose’s `Modifier` API surface and `Modifier.Node` runtime semantics so Kotlin samples and mental models apply 1:1 in Compose-RS.

---

## Current Gaps (Compose-RS)
- `Modifier` is still an Rc-backed builder with cached layout/draw state for legacy APIs. Renderers now read reconciled node slices, but `ModifierState` continues to provide padding/layout caches and must be removed once every factory is node-backed.
- Focus is the last holdout that still treats the modifier chain as a flat list; its ancestor search, invalidations, and capability gating must move onto the delegate-aware traversal helpers.
- Semantics tree building still leans on `RuntimeNodeMetadata` shims instead of walking modifier nodes end-to-end, so we lack a Kotlin-style `SemanticsOwner`, capability-scoped traversal, and full delegate coverage when deriving roles/actions.
- Diagnostics exist (`Modifier::fmt`, `debug::log_modifier_chain`, `COMPOSE_DEBUG_MODIFIERS`), but we still lack parity tooling such as Kotlin’s inspector strings, capability dumps with delegate depth, and targeted tracing hooks used by focus/pointer stacks.

## Jetpack Compose Reference Anchors
- `Modifier.kt`: immutable interface (`EmptyModifier`, `CombinedModifier`) plus `foldIn`, `foldOut`, `any`, `all`, `then`.
- `ModifierNodeElement.kt`: node-backed elements with `create`/`update`/`key`/`equals`/`hashCode`/inspector hooks.
- `NodeChain.kt`, `DelegatableNode.kt`, `NodeKind.kt`: sentinel-based chain, capability masks, delegate links, targeted invalidations, and traversal helpers.
- Pointer input stack under `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/pointer`.

## Recent Progress
- `ModifierNodeChain` now stores safe sentinel head/tail nodes and aggregate capability masks without `unsafe`, enabling deterministic traversal order and `COMPOSE_DEBUG_MODIFIERS` dumps.
- Modifier locals graduated to a Kotlin-style manager: providers/consumers stay registered per chain, invalidations return from `ModifierChainHandle`, layout nodes resolve ancestor values via a registry, and regression tests now cover overrides + ancestor propagation; semantics nodes continue to be defined via `Modifier::semantics`, but the semantics tree still needs to consume the new data.
- Layout nodes expose modifier-local data to ancestors without raw pointers: `ModifierChainHandle` shares a `ModifierLocalsHandle`, `LayoutNode` updates a pointer-free registry entry, and `resolve_modifier_local_from_parent_chain` now mirrors Kotlin’s `ModifierLocalManager` traversal while staying completely safe.
- Diagnostics improved: `Modifier` implements `Display`, `compose_ui::debug::log_modifier_chain` enumerates nodes/capabilities, and DEBUG env flags print chains after reconciliation.
- Core modifier factories (`padding`, `background`, `draw*`, `clipToBounds`, `pointerInput`, `clickable`) are node-backed, and pointer input runs on coroutine-driven scaffolding mirroring Kotlin. Renderers and pointer dispatch now operate exclusively on reconciled node slices.
- `ModifierNodeChain` now mirrors Kotlin’s delegate semantics: every node exposes parent/child links, delegate stacks feed the traversal helpers, aggregate capability masks propagate through delegates, and tests cover ordering, sentinel wiring, and capability short-circuiting without any `unsafe`.
- Runtime consumers (modifier locals, pointer input, semantics helpers, diagnostics, and resolver pipelines) now use the delegate-aware traversal helpers exclusively; the legacy iterator APIs were removed and tests cover delegated capability discovery.
- Semantics modifiers now feed a capability-filtered snapshot straight from `ModifierNodeChain`, and layout nodes own a dedicated semantics dirty flag that queues bubbling through composer commands so semantics-only updates avoid layout.

## Migration Plan
1. **Mirror the `Modifier` data model (Kotlin: `Modifier.kt`)**  
   Keep the fluent API identical (fold helpers, `any`/`all`, inspector metadata) and delete the remaining runtime responsibilities of `ModifierState` once all factories are node-backed.
2. **Adopt `ModifierNodeElement` / `Modifier.Node` parity (Kotlin: `ModifierNodeElement.kt`)**  
   Implement the full lifecycle contract: `onAttach`, `onDetach`, `onReset`, coroutine scope ownership, and equality/key-driven reuse.
3. **Implement delegate traversal + capability plumbing (Kotlin: `NodeChain.kt`, `NodeKind.kt`, `DelegatableNode.kt`)**  
   ✅ Delegate stacks + traversal helpers now match Kotlin. **Remaining:** wire the focus subsystem onto the helpers so capability-aware short-circuiting covers every pipeline.
4. **Wire all runtime subsystems through chains**  
   Layout/draw/pointer already read reconciled nodes; remaining work includes semantics tree extraction, modifier locals invalidation, focus chains, and removal of the residual `ModifierState` caches.
5. **Migrate modifier factories + diagnostics**  
   Finish porting the remaining factories off `ModifierState`, add Kotlin-style inspector dumps/trace hooks, and grow the parity test matrix to compare traversal order/capabilities against the Android reference.

## Near-Term Next Steps
1. **Focus chain parity**  
   - Wire the focus manager/requester utilities through `ModifierNodeChain`’s delegate traversal helpers so focus targets, focus order, and ancestor resolution short-circuit via capability masks.  
   - Port the relevant pieces from `androidx.compose.ui.focus.*` (especially `FocusTransactions.kt`, `FocusTraversalPolicy.kt`) to ensure Kotlin’s `visitAncestors/visitChildren` semantics hold, and add tests that cover delegated focus nodes.
2. **Semantics stack parity**
   - Re-read `androidx/compose/ui/node/Semantics*` and port the Kotlin contract: build the accessibility tree directly from modifier nodes/delegates, cache `SemanticsConfiguration`s per node, and expose a `SemanticsOwner`-equivalent that downstream renderers/tests can query.
   - Replace the `RuntimeNodeMetadata` shims in `layout/mod.rs` with node-chain derived data so role/action synthesis mirrors Kotlin and respects capability-scoped traversal.
   - Expand the parity test matrix (e.g., mirror `SemanticsModifierTest.kt`) to cover ancestor-provided semantics, custom actions, delegated semantics children, and semantics-only updates.
3. **Diagnostics + focus-ready infrastructure**  
   - Extend debugging helpers (`Modifier.to_string()`, chain dumps) to include delegate depth, modifier locals provided, semantics flags, and capability masks.  
   - Port Kotlin’s tracing (`NodeChain#trace`, inspector strings) so modifier/focus debugging has feature parity and can be toggled per-layout-node (not just via `COMPOSE_DEBUG_MODIFIERS`).
4. **Modifier factory + `ModifierState` removal**  
   - Audit every `Modifier` factory to ensure it’s fully node-backed; delete `ModifierState` caches after verifying layout/draw/inspection behavior via tests.  
   - Update docs/examples to emphasize node-backed factories and remove stale ModOp/`ModifierState` guidance.

## Kotlin Reference Playbook
| Area | Kotlin Source | Compose-RS Target |
| --- | --- | --- |
| Modifier API | `androidx/compose/ui/Modifier.kt` | `crates/compose-ui/src/modifier/mod.rs` |
| Node elements & lifecycle | `ModifierNodeElement.kt`, `DelegatableNode.kt` | `crates/compose-foundation/src/modifier.rs` + `compose-ui` node impls |
| Node chain diffing | `NodeChain.kt`, `NodeCoordinator.kt` | `crates/compose-foundation/src/modifier.rs`, upcoming coordinator module |
| Pointer input | `input/pointer/*` | `crates/compose-ui/src/modifier/pointer_input.rs` |
| Semantics | `semantics/*`, `SemanticsNode.kt` | `crates/compose-ui/src/semantics` (to be ported) |

Always cross-check behavior against the Kotlin sources under `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui` to ensure parity.

## Roadmap: Closing Runtime/Parity Gaps

### Phase 1 — Stabilize “where does resolved data come from?”

**Targets:** gap 3, shortcuts 1, wifn 1–3

1. **Centralize resolved-modifier computation**

   * **Goal:** resolved data is computed exactly once per layout-owning thing (`LayoutNode`, `SubcomposeLayoutNode`), never ad-hoc.
   * **Actions:**

     * Keep `LayoutNode`’s current `modifier_chain.update(...)` + `resolved_modifiers` as the **source of truth**.
     * Make `SubcomposeLayoutNodeInner` do the same (it already does, just confirm it mirrors the layout node path).
     * Mark `Modifier::resolved_modifiers()` as “helper/debug-only” and hunt down any call sites in layout/measure/text that still use it.
   * **Acceptance:**

     * No hot path calls `Modifier::resolved_modifiers()` directly.
     * Renderer and layout both consume the snapshot coming from `LayoutNodeData`.

2. **Make all layout-tree builders provide the 3-part node data**

   * **Goal:** every constructed `LayoutNodeData` has

     ```rust
     LayoutNodeData::new(
       modifier,
       resolved_modifiers,
       modifier_slices,
       kind,
     )
     ```
   * **Actions:**

     * Audit places that build layout trees (debug tests, runtime metadata trees, any virtual/layout wrappers) and update them to call the new constructor.
     * Add a tiny test that builds a minimal layout tree and asserts `modifier_slices` is non-None / default.
   * **Acceptance:**

     * `cargo check` over ui + both renderers succeeds after the constructor change.
     * No `LayoutNodeData { modifier, kind }` left.

3. **Make resolved modifiers fully node-first**

   * **Goal:** stop “build from legacy ModifierState and then patch from nodes.”
   * **Actions:**

     * Move the logic from `ModifierChainHandle::compute_resolved(...)` so it **starts** from the chain (layout nodes, draw nodes, shape nodes) and only *optionally* consults legacy fields.
     * Keep the current order for now (padding → background → shape → graphics layer) but document “this is 100% node-backed once all factories are node-backed.”
   * **Acceptance:**

     * The resolved struct can be explained using only “what nodes were in the chain.”

---

### Phase 2 — Modifier locals that actually do something

**Targets:** gap 5, shortcut 3, wifn 4

1. **(✅) Wire `ModifierLocalManager` to layout nodes**

   * Provider changes now surface through `ModifierChainHandle::update_with_resolver`, the manager returns invalidation kinds, and `LayoutNode` bubbles the result into its dirty flags/tests.

2. **(✅) Add ancestor walking for locals**

   * Layout nodes maintain a registry of living parents so modifier-local consumers can resolve ancestors exactly like Kotlin’s `visitAncestors`, with capability short-circuiting tied to `modifier_child_capabilities`.

3. **Make debug toggling less global**

   * **Goal:** avoid “env var = everything logs.”
   * **Actions:**

     * Keep `COMPOSE_DEBUG_MODIFIERS` for now, but add a per-node switch the layout node can set (`layout_node.set_debug_modifiers(true)`).
     * Route chain logging through that.
   * **Acceptance:**

     * You can turn on modifier-debug for one node without spamming the whole tree.

---

### Phase 3 — Semantics on top of modifier nodes

**Targets:** gap 6, shortcuts 4, 5, wifn 5

1. **(✅) Unify semantics extraction**

   * **Goal:** stop mixing “runtime node metadata” semantics with “modifier-node” semantics.
   * **Actions:**

     * In `LayoutNode::semantics_configuration()`, you already gather from modifier nodes — make the tree builder prefer this over the old metadata fields.
     * Keep the metadata path only for widgets that don’t have modifier nodes yet (like your current Button shim).
   * **Acceptance:**

     * A node with `.semantics { is_clickable = true }` ends up clickable in the built semantics tree without needing `RuntimeNodeMetadata` to say so.

2. **(✅) Respect capability-based traversal**

   * **Goal:** don’t walk the whole chain if `aggregate_child_capabilities` says “nothing semantic down here.”
   * **Actions:**

     * Add tiny traversal helpers on `ModifierNodeChain`:

       * `visit_from_head(kind_mask, f)`
       * `visit_ancestors(from, kind_mask, f)`
     * Use those in semantics extraction so you only look where SEMANTICS is present.
   * **Acceptance:**

     * Semantics building only touches entries that have the semantics bit (or have it in children).

3. **(✅) Separate draw vs layout invalidations from semantics**

   * **Goal:** current invalidation routing in `LayoutNode` is coarse.
   * **Actions:**

     * When the chain reports an invalidation of kind `Semantics`, do **not** call `mark_needs_layout()`.
     * Instead, mark a “semantics dirty” flag, or route to whatever layer builds the semantics tree.
   * **Acceptance:**

     * Changing only semantics does not trigger a layout pass.

---

### Phase 4 — Clean up the “shortcut” APIs on nodes

**Targets:** shortcuts 4, 5

1. **Replace per-node `as_*_node` with mask-driven dispatch**

   * **Goal:** not every user node has to implement 4 optional methods.
   * **Actions:**

     * Where you iterate now with `draw_nodes()`, `pointer_input_nodes()`, switch to: use the chain entries’ capability bits as the primary filter, and only downcast the node once.
     * Keep the `as_*` methods for now for built-ins, but don’t require third parties to override them.
   * **Acceptance:**

     * A node with the DRAW capability but no `as_draw_node` still gets visited.

2. **Make invalidation routing match the mask**

   * **Goal:** stop doing “draw → mark_needs_layout.”
   * **Actions:**

     * Add a `mark_needs_redraw()` or equivalent on the node/renderer path and call that for DRAW invalidations.
   * **Acceptance:**

     * DRAW-only updates don’t force layout.

---

### Phase 5 — Finish traversal utilities (the Kotlin-like part)

**Status:** ✅ Done (modifier locals, semantics, pointer input, diagnostics, and tests now rely solely on the capability-filtered visitors; bespoke iterators were removed. Remaining traversal work lives under focus + semantics tree follow-ups.)

---
