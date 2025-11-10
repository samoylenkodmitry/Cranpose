# Next Task: Rebuild Semantics Extraction on Modifier Nodes

## Context
Traversal parity is in place: every runtime consumer now relies on the delegate-aware helpers that mirror Jetpack Compose’s `NodeChain`. Semantics is still the major outlier: `LayoutNode::semantics_configuration()` mixes modifier-node data with `RuntimeNodeMetadata`, semantics invalidations always bubble through layout, and the tree builder cannot short-circuit on `NodeCapabilities::SEMANTICS`. Kotlin’s `SemanticsNode`, `SemanticsOwner`, and `SemanticsModifierNode` (see `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/semantics`) build their tree exclusively from modifier nodes and respect capability masks when visiting descendants and ancestors. We need to mirror that flow so semantics-only updates stay cheap and the upcoming semantics rewrite has a solid foundation.

## Goals
1. Build the semantics tree exclusively from modifier nodes (delegates included), eliminating the dependency on `RuntimeNodeMetadata`.
2. Short-circuit semantics extraction using `aggregate_child_capabilities` so chains without semantics are skipped entirely.
3. Route semantics invalidations through a dedicated dirty flag/pipeline instead of `mark_needs_layout`, matching Kotlin’s `SemanticsOwner` behavior.
4. Expand diagnostics/tests to cover delegate semantics nodes, semantics-only invalidations, and capability-gated traversal.

## Suggested Steps
1. **Modifier-chain semantics snapshot**
   - Teach `LayoutNode::semantics_configuration` and the tree builder in `crates/compose-ui/src/layout/mod.rs` to iterate with `for_each_forward_matching(NodeCapabilities::SEMANTICS, …)`. Remove the fallback to `RuntimeNodeMetadata` for nodes whose semantics come purely from modifiers.
   - Add helper(s) that materialize a `SemanticsConfiguration` slice per node/delegate, similar to Kotlin’s `SemanticsNode.collectSemantics`.
2. **Semantics dirty plumbing**
   - Introduce a semantics dirty flag on `LayoutNode` (and/or the semantics owner) that is toggled when the chain reports `InvalidationKind::Semantics`. Ensure this flag does **not** trigger layout/measure work; instead, queue a semantics recompute pass similar to Kotlin’s `SemanticsOwner` invalidations.
   - Hook the new flag into the existing layout pipeline so semantics are recomputed before rendering/inspection when dirty.
3. **Tree builder parity**
   - Update `build_semantics_node` to stop reading legacy metadata, use the modifier-node configurations, and respect capability masks when recursing.
   - Ensure delegate stacks contribute semantic children in order, matching Jetpack Compose’s `SemanticsNode`.
4. **Testing & diagnostics**
   - Extend `crates/compose-foundation/src/tests/modifier_tests.rs` and `crates/compose-ui/src/widgets/nodes/layout_node.rs` tests to assert that delegated semantics nodes are discovered, semantics-only updates skip layout, and capability masks short-circuit traversal.
   - Add targeted snapshot tests under `crates/compose-ui/src/layout/tests` (or similar) that build nested semantics delegates and verify the resulting tree/dirty flags.

## Definition of Done
- Semantics extraction/trees no longer depend on `RuntimeNodeMetadata`; they are built entirely from modifier nodes via capability-filtered traversal (delegates included).
- Semantics invalidations toggle a dedicated dirty flag or queue, not `mark_needs_layout`, and semantics-only changes avoid measure/layout work.
- Capability masks short-circuit semantics traversal in both chain iteration and tree building.
- New/updated tests cover delegated semantics nodes, semantics-only invalidations, and the semantics tree builder; the full workspace `cargo test` remains green.
