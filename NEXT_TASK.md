# Next Task: Modifier Local Manager Parity & Ancestor Resolution

## Context
`ModifierNodeChain` now mirrors Kotlin’s traversal surface: sentinel head/tail nodes, `head_to_tail` / `tail_to_head` iterators, filtered visitors, and cached `aggregate_child_capabilities` are wired through `ModifierChainHandle` into each `LayoutNode` (`layout_node.modifier_child_capabilities()`). This unlocks ancestor-aware queries, but our modifier-local plumbing still behaves like a single-chain demo:

- `ModifierLocalManager` only walks the current chain once, immediately invoking consumers without tracking invalidations.
- Providers/consumers have no notion of parent layout nodes, so locals cannot flow across the tree the way Kotlin’s `ModifierLocalManager` + `DelegatableNode.visitAncestors` allow.
- `LayoutNode` has no way to mark itself dirty when a provider appears/disappears or when a provider’s value changes, so modifier locals never trigger measure/draw/semantics updates.

To move toward Jetpack Compose parity we need to port the Kotlin behavior from `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/modifier/ModifierLocal*` plus the management hooks in `NodeChain.kt` / `DelegatableNode.kt`.

## Goals
1. Implement a real `ModifierLocalManager` that tracks provider registration, consumer dependencies, and invalidation scopes just like Kotlin’s `ModifierLocalManager`.
2. Allow modifier-local consumers to resolve values by walking ancestor layout nodes, using the cached `modifier_child_capabilities` mask to short-circuit when no providers exist.
3. Bubble provider/consumer insert/remove/update events back to `LayoutNode` so relevant dirty flags (layout, draw, semantics) flip when modifier locals change.

## Suggested Steps
1. **Manager data model**
   - Port the concepts from `ModifierLocalManager.kt`: maintain maps of providers by `ModifierLocalId`, keep weak handles (or `NodeId` references) to consumers, and produce an `InvalidationKind`/dirty flag list when a provider changes.
   - Update `ModifierChainHandle::update` to receive the invalidation list from the manager (e.g. `manager.sync(&mut chain)` → `ModifierLocalSyncResult { consumers_to_invalidate, requery }`).
2. **Consumer resolution + ancestor walk**
   - Add a method like `ModifierLocalManager::resolve(key, chain, parent_lookup)` that first checks providers in the current chain (using the new traversal helpers) and otherwise calls a closure supplied by `LayoutNode` to walk parent chains.
   - When notifying consumers, pass a scope that defers reads until the consumer re-runs, mirroring Kotlin’s `ModifierLocalReadScope`.
3. **LayoutNode integration**
   - Store the per-node manager or, at minimum, receive invalidation results from `ModifierChainHandle` and flip `needs_measure`/`needs_layout`/semantics-dirty flags accordingly.
   - Expose a callback on `LayoutNode` (or via `ModifierChainHandle`) that knows how to walk parent nodes using `modifier_child_capabilities` to prune branches (parity with `DelegatableNode.visitAncestors`).
4. **Testing**
   - Add Rust tests mirroring scenarios from `ModifierLocalTest.kt`: intra-chain overrides, ancestor lookup across layout boundaries, provider removal invalidating children, etc.
   - Ensure tests cover capability short-circuiting (no ancestor walk when `modifier_child_capabilities` lacks `MODIFIER_LOCALS`).

## Definition of Done
- `ModifierLocalManager` persists provider state, tracks consumers, and surfaces invalidation info instead of immediately invoking callbacks.
- Consumers can observe providers defined on ancestor layout nodes; lookups short-circuit when `modifier_child_capabilities()` shows no modifier-local capability above.
- Changing a provider’s value (or removing/adding one) marks the owning `LayoutNode` dirty in the same way Kotlin does (layout vs draw vs semantics as appropriate).
- New tests under `crates/compose-ui` (and/or `compose-foundation`) cover sibling overrides, ancestor propagation, and invalidations; they pass alongside `cargo test`.
