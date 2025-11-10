# Modifier ≈ Jetpack Compose Parity Plan

Goal: match Jetpack Compose’s `Modifier` API surface and `Modifier.Node` runtime semantics so Kotlin samples and mental models apply 1:1 in Compose-RS.

---

## Current Gaps (Compose-RS)
- `Modifier` is still an Rc-backed builder with cached layout/draw state for legacy APIs. Renderers now read reconciled node slices, but `ModifierState` continues to provide padding/layout caches and must be removed once every factory is node-backed.
- `compose_foundation::ModifierNodeChain` now owns safe sentinel head/tail nodes plus parent/child metadata, yet it still lacks Kotlin’s delegate-chain surface (`DelegatableNode`, `NodeChain#headToTail`, `aggregateChildKindSet` propagation into layout nodes). Capability filtering remains per-chain rather than per ancestor traversal, so focus/modifier-local semantics can drift from Android.
- Modifier locals and semantics have an initial port (`ModifierLocalKey`, provider/consumer nodes, `Modifier::semantics`, `LayoutNode::semantics_configuration`), but invalidation, ancestor lookup, and semantics tree construction still rely on ad-hoc metadata instead of Kotlin’s `ModifierLocalManager`/`SemanticsOwner`.
- Diagnostics exist (`Modifier::fmt`, `debug::log_modifier_chain`, `COMPOSE_DEBUG_MODIFIERS`), but we still lack parity tooling such as Kotlin’s inspector strings, capability dumps with delegate depth, and targeted tracing hooks used by focus/pointer stacks.

## Jetpack Compose Reference Anchors
- `Modifier.kt`: immutable interface (`EmptyModifier`, `CombinedModifier`) plus `foldIn`, `foldOut`, `any`, `all`, `then`.
- `ModifierNodeElement.kt`: node-backed elements with `create`/`update`/`key`/`equals`/`hashCode`/inspector hooks.
- `NodeChain.kt`, `DelegatableNode.kt`, `NodeKind.kt`: sentinel-based chain, capability masks, delegate links, targeted invalidations, and traversal helpers.
- Pointer input stack under `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/pointer`.

## Recent Progress
- `ModifierNodeChain` now stores safe sentinel head/tail nodes, parent/child links, and aggregate capability masks without `unsafe`, enabling deterministic traversal order and `COMPOSE_DEBUG_MODIFIERS` dumps.
- Modifier locals landed (`ModifierLocalKey`, provider/consumer elements, runtime manager sync), semantics nodes can be defined via `Modifier::semantics`, and `LayoutNode` can surface aggregated semantics configurations.
- Diagnostics improved: `Modifier` implements `Display`, `compose_ui::debug::log_modifier_chain` enumerates nodes/capabilities, and DEBUG env flags print chains after reconciliation.
- Core modifier factories (`padding`, `background`, `draw*`, `clipToBounds`, `pointerInput`, `clickable`) are node-backed, and pointer input runs on coroutine-driven scaffolding mirroring Kotlin. Renderers and pointer dispatch now operate exclusively on reconciled node slices.

## Migration Plan
1. **Mirror the `Modifier` data model (Kotlin: `Modifier.kt`)**  
   Keep the fluent API identical (fold helpers, `any`/`all`, inspector metadata) and delete the remaining runtime responsibilities of `ModifierState` once all factories are node-backed.
2. **Adopt `ModifierNodeElement` / `Modifier.Node` parity (Kotlin: `ModifierNodeElement.kt`)**  
   Implement the full lifecycle contract: `onAttach`, `onDetach`, `onReset`, coroutine scope ownership, and equality/key-driven reuse.
3. **Implement delegate traversal + capability plumbing (Kotlin: `NodeChain.kt`, `NodeKind.kt`, `DelegatableNode.kt`)**  
   Finish delegate links, ancestor aggregation, and traversal helpers so semantics/focus/modifier locals can walk nodes exactly like Android.
4. **Wire all runtime subsystems through chains**  
   Layout/draw/pointer already read reconciled nodes; remaining work includes semantics tree extraction, modifier locals invalidation, focus chains, and removal of the residual `ModifierState` caches.
5. **Migrate modifier factories + diagnostics**  
   Finish porting the remaining factories off `ModifierState`, add Kotlin-style inspector dumps/trace hooks, and grow the parity test matrix to compare traversal order/capabilities against the Android reference.

## Near-Term Next Steps
1. **Delegate + ancestor traversal parity**  
   - Port Kotlin’s delegate APIs (`DelegatableNode`, `DelegatingNode`, `node.parent/child` contract) so every `ModifierNode` exposes the same traversal surface as Android.  
   - Propagate `aggregateChildKindSet` (capability bitmasks) from nodes into `LayoutNode` so ancestor/descendant queries short-circuit exactly like `NodeChain.kt`.  
   - Mirror Kotlin’s `NodeChain` head/tail iteration helpers (`headToTail`, `tailToHead`, `trimChain/padChain`) for diffing, ensuring keyed reuse + capability recompute follow the reference semantics.
2. **Modifier locals parity**  
   - Flesh out a `ModifierLocalManager` that registers providers/consumers, invalidates descendants on insert/remove, and mirrors Kotlin’s `ModifierLocalConsumer` contract.  
   - Implement ancestor lookups that walk parent layout nodes (not just the current chain) and add parity tests based on `ModifierLocalTest.kt`.  
   - Connect modifier-local invalidations into `LayoutNode` dirty flags so layout/draw updates fire exactly as on Android.
3. **Semantics stack parity**  
   - Replace `RuntimeNodeMetadata` semantics fields with direct traversal of modifier nodes, build a `SemanticsOwner`/`SemanticsTree` identical to Kotlin’s implementation, and add parity tests (clickable semantics, content descriptions, custom actions).  
   - Wire semantics invalidations through modifier nodes + layout nodes, and feed the resulting semantics tree into accessibility/focus layers once available.
4. **Diagnostics + focus-ready infrastructure**  
   - Extend debugging helpers (`Modifier.toString()`, chain dumps) to include delegate depth, modifier locals provided, semantics flags, and capability masks.  
   - Port Kotlin’s snapshot tests/logging (`trace`, `NodeChain#trace`, `Modifier.toString()`) to prevent regressions once focus/focusRequester stacks land.
5. **Modifier factory + `ModifierState` removal**  
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

1. **Wire `ModifierLocalManager` to layout nodes**

   * **Goal:** provider changes cause consumer invalidations, including across layout boundaries.
   * **Actions:**

     * When `ModifierChainHandle` calls `modifier_locals.sync(...)`, have that return “here are the consumers that must be invalidated.”
     * Bubble that list to the owning `LayoutNode` so it can mark itself dirty (layout or semantics depending on what the local influences).
   * **Acceptance:**

     * A provider on parent → change value → child consumer runs its closure again in the next frame.

2. **Add ancestor walking for locals**

   * **Goal:** match Kotlin’s “walk parent layout nodes” behaviour.
   * **Actions:**

     * Expose a helper on the chain like `resolve_local(key)` that first checks the current chain, then a callback to walk parent layout nodes.
     * On the layout side, provide that parent-walk callback.
   * **Acceptance:**

     * A consumer in a grandchild can see a provider in an ancestor layout node.

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

1. **Unify semantics extraction**

   * **Goal:** stop mixing “runtime node metadata” semantics with “modifier-node” semantics.
   * **Actions:**

     * In `LayoutNode::semantics_configuration()`, you already gather from modifier nodes — make the tree builder prefer this over the old metadata fields.
     * Keep the metadata path only for widgets that don’t have modifier nodes yet (like your current Button shim).
   * **Acceptance:**

     * A node with `.semantics { is_clickable = true }` ends up clickable in the built semantics tree without needing `RuntimeNodeMetadata` to say so.

2. **Respect capability-based traversal**

   * **Goal:** don’t walk the whole chain if `aggregate_child_capabilities` says “nothing semantic down here.”
   * **Actions:**

     * Add tiny traversal helpers on `ModifierNodeChain`:

       * `visit_from_head(kind_mask, f)`
       * `visit_ancestors(from, kind_mask, f)`
     * Use those in semantics extraction so you only look where SEMANTICS is present.
   * **Acceptance:**

     * Semantics building only touches entries that have the semantics bit (or have it in children).

3. **Separate draw vs layout invalidations from semantics**

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

**Targets:** wifn 5, supports gaps 5–6

1. **Add chain-level helpers that mirror Kotlin (`headToTail`, `tailToHead`, filtered)**

   * **Goal:** get rid of hand-rolled parent/child Option-walking at call sites.
   * **Actions:**

     * On `ModifierNodeChain`, add:

       * `fn for_each_forward<F>(&self, f: F)`
       * `fn for_each_forward_with_mask<F>(&self, mask: NodeCapabilities, f: F)`
       * optionally `fn for_each_backward...`
     * Implement them using the `ChainPosition` you already set up.
   * **Acceptance:**

     * Semantics, modifier locals, and (later) focus can all call the same traversal helpers.

2. **Document the traversal contract**

   * **Goal:** this is where your “parity with Jetpack Compose” actually lives.
   * **Actions:**

     * In the md, add “we guarantee sentinel head/tail, stable parent/child, aggregate child set, and filtered traversal helpers.”
     * Note what you *don’t* yet guarantee (no delegate stacks yet).
   * **Acceptance:**

     * Anyone adding a new node kind knows which traversal to call.

---

