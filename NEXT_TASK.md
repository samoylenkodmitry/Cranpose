# Next Task: Build a Modifier-Driven Semantics Tree

## Context
Semantics data now comes straight from modifier nodes: `LayoutNode::semantics_configuration` pulls from the reconciled `ModifierNodeChain`, capability filters keep traversal tight, and semantics invalidations raise a dedicated dirty flag that bubbles through the composer without touching layout. The remaining gap lives in the tree builder: `layout/mod.rs` still assembles semantics using `RuntimeNodeMetadata` shims, synthesizing roles/actions per widget instead of mirroring Kotlin’s `SemanticsNode`/`SemanticsOwner` pipeline. We need to replace that scaffolding with modifier-node traversal so delegated semantics, ancestor merges, and capability masks behave like Jetpack Compose.

## Goals
1. Rebuild the semantics tree exclusively from modifier nodes (including delegates), removing `RuntimeNodeMetadata` from the extraction path.
2. Introduce a SemanticsOwner-style cache keyed by `NodeId` that stores the most recent `SemanticsConfiguration` per layout node and is refreshed only when `needs_semantics()` is true.
3. Ensure role/action synthesis (e.g., click handlers, button roles, content descriptions) flows from the modifier-provided configuration, not widget-specific fallbacks.
4. Expand tests to cover delegated semantics children, semantics-only updates, and capability-filtered traversal parity with Jetpack Compose expectations.

## Suggested Steps
1. **Refactor the semantics snapshot**
   - Mirror Kotlin’s `SemanticsNode#collectSemantics`: walk each `LayoutNode`’s modifier chain with `for_each_forward_matching(NodeCapabilities::SEMANTICS, …)` to gather configurations and cache them alongside the node id.
   - Replace the `RuntimeNodeMetadata` structs with a lightweight owner map (e.g., `HashMap<NodeId, SemanticsEntry>`) that records the cached configuration, child ordering hints, and any synthesized actions derived from the config.
2. **Build a SemanticsOwner**
   - Create a dedicated struct (or reuse an existing container) that holds the node map, exposes `invalidate(node_id)` when `mark_needs_semantics` fires, and rebuilds the tree lazily before reads.
   - Clear each `LayoutNode`’s semantics dirty flag after the owner refreshes the configuration, mirroring Kotlin’s contract.
3. **Rework tree assembly**
   - Update `build_semantics_node` and related helpers to read from the new owner cache, compose delegated children in modifier order, and derive roles/actions solely from the configuration (falling back to widget shims only when no semantics nodes exist).
   - Ensure capability masks gate traversal: skip subtrees whose layout nodes advertise no semantics capabilities.
4. **Testing & diagnostics**
   - Extend `crates/compose-ui/src/layout/tests/layout_tests.rs` (and related modules) with cases that cover modifier delegates adding semantics, semantics-only updates avoiding layout, and ancestor merges.
   - Add debug assertions or logs (guarded behind existing flags) that dump the rebuilt semantics tree so parity comparisons with Kotlin samples are straightforward.

## Definition of Done
- The semantics tree no longer relies on `RuntimeNodeMetadata`; all roles/actions/descriptions originate from modifier-node traversal and cached configurations.
- The new semantics owner invalidation flow respects `LayoutNode::needs_semantics()` and clears the flag after refresh, leaving layout/measure dirty flags untouched on semantics-only updates.
- Tests exercise delegated semantics, capability short-circuiting, and semantics-only invalidations; the full workspace `cargo test` suite remains green.
