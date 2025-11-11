# Modifier ≈ Jetpack Compose Parity Plan

Goal: match Jetpack Compose’s `Modifier` API surface and `Modifier.Node` runtime semantics so Kotlin samples and mental models apply 1:1 in Compose-RS.

---

## Current Gaps (Compose-RS)
- Diagnostics exist (`Modifier::fmt`, `debug::log_modifier_chain`, `COMPOSE_DEBUG_MODIFIERS`), but we still lack parity tooling such as Kotlin's inspector strings, capability dumps with delegate depth, per-node tracing toggles, and focused tracing hooks used by focus/pointer stacks.
- Node capability dispatch still leans on the optional `as_*` downcasts; we need Kotlin’s mask-driven iteration and invalidation routing so DRAW-only changes stop forcing layout.

## Jetpack Compose Reference Anchors
- `Modifier.kt`: immutable interface (`EmptyModifier`, `CombinedModifier`) plus `foldIn`, `foldOut`, `any`, `all`, `then`.
- `ModifierNodeElement.kt`: node-backed elements with `create`/`update`/`key`/`equals`/`hashCode`/inspector hooks.
- `NodeChain.kt`, `DelegatableNode.kt`, `NodeKind.kt`: sentinel-based chain, capability masks, delegate links, targeted invalidations, and traversal helpers.
- Pointer input stack under `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/pointer`.

## Recent Progress
- `ModifierNodeChain` now stores safe sentinel head/tail nodes and aggregate capability masks without `unsafe`, enabling deterministic traversal order and `COMPOSE_DEBUG_MODIFIERS` dumps.
- Modifier locals graduated to a Kotlin-style manager: providers/consumers stay registered per chain, invalidations return from `ModifierChainHandle`, layout nodes resolve ancestor values via a registry, and regression tests now cover overrides + ancestor propagation.
- Layout nodes expose modifier-local data to ancestors without raw pointers: `ModifierChainHandle` shares a `ModifierLocalsHandle`, `LayoutNode` updates a pointer-free registry entry, and `resolve_modifier_local_from_parent_chain` now mirrors Kotlin's `ModifierLocalManager` traversal while staying completely safe.
- Diagnostics improved: `Modifier` implements `Display`, `compose_ui::debug::log_modifier_chain` enumerates nodes/capabilities, and DEBUG env flags print chains after reconciliation.
- Core modifier factories (`padding`, `background`, `draw*`, `clipToBounds`, `pointerInput`, `clickable`) are node-backed, and pointer input runs on coroutine-driven scaffolding mirroring Kotlin. Renderers and pointer dispatch now operate exclusively on reconciled node slices.
- `ModifierNodeChain` now mirrors Kotlin's delegate semantics: every node exposes parent/child links, delegate stacks feed the traversal helpers, aggregate capability masks propagate through delegates, and tests cover ordering, sentinel wiring, and capability short-circuiting without any `unsafe`.
- Runtime consumers (modifier locals, pointer input, semantics helpers, diagnostics, and resolver pipelines) now use the delegate-aware traversal helpers exclusively; the legacy iterator APIs were removed and tests cover delegated capability discovery.
- **Semantics tree is now fully modifier-driven:** `SemanticsOwner` caches configurations by `NodeId`, `build_semantics_node` derives roles/actions exclusively from `SemanticsConfiguration` flags, semantics dirty flag is independent of layout, and capability-filtered traversal respects delegate depth. `RuntimeNodeMetadata` removed from the semantics extraction path.
- **Focus chain parity achieved:** `FocusTargetNode` and `FocusRequesterNode` implement full `ModifierNode` lifecycle, focus traversal uses `NodeCapabilities::FOCUS` with delegate-aware visitors (`find_parent_focus_target`, `find_first_focus_target`), `FocusManager` tracks state without unsafe code, focus invalidations are independent of layout/draw, and all 6 tests pass covering lifecycle, callbacks, chain integration, and state predicates.
- **✅ Layout modifier migration complete:** `OffsetElement`/`OffsetNode` (offset.rs), `FillElement`/`FillNode` (fill.rs), and enhanced `SizeElement`/`SizeNode` now provide full 1:1 parity with Kotlin's foundation-layout modifiers. All three implement `LayoutModifierNode` with proper `measure()`, intrinsic measurement support, and `enforce_incoming` constraint handling. Code is organized into separate files (offset.rs, fill.rs, size.rs). All 118 tests pass ✅.
- **✅ `ModifierState` removed:** `Modifier` now carries only elements + inspector metadata, all factories emit `ModifierNodeElement`s, and `ModifierChainHandle::compute_resolved()` derives padding/layout/background/graphics-layer data directly from the reconciled chain.
- **✅ Weight/alignment/intrinsic parity:** `WeightElement`, `AlignmentElement`, `IntrinsicSizeElement`, and `GraphicsLayerElement` keep Row/Column/Box/Flex + rendering behavior node-driven, matching Jetpack Compose APIs while keeping the public builder surface unchanged.

## Migration Plan
1. **(✅) Mirror the `Modifier` data model (Kotlin: `Modifier.kt`)**  
   Modifiers now only store elements + inspector metadata; runtime data lives exclusively on nodes and resolved state is aggregated via `ModifierChainHandle`.
2. **(✅) Adopt `ModifierNodeElement` / `Modifier.Node` parity (Kotlin: `ModifierNodeElement.kt`)**  
   All public factories emit `ModifierNodeElement`s, nodes reuse via equality/hash, and lifecycle hooks drive invalidations.
3. **(✅) Implement delegate traversal + capability plumbing (Kotlin: `NodeChain.kt`, `NodeKind.kt`, `DelegatableNode.kt`)**  
   Sentinel chains, capability masks, and delegate-aware traversal power layout/draw/pointer/focus/semantics.
4. **(🚧) Surface Kotlin-level diagnostics + tooling parity**  
   Need richer inspector strings, delegate-depth dumps, per-node debug toggles, and tracing hooks that match Android Studio tooling.
5. **(🚧) Remove shortcut APIs + align invalidation routing with capability masks**  
   Replace `as_*` downcasts with mask-driven iteration and ensure DRAW-only invalidations propagate without forcing layout.

## Near-Term Next Steps
1. **Diagnostics & inspector parity (new Phase 6)**  
   - Add Kotlin-style inspector strings per element (including weights, alignments, graphics layers) and bubble them through debug tooling.  
   - Implement per-layout-node debug toggles so we can trace a single node chain without enabling `COMPOSE_DEBUG_MODIFIERS` globally.  
   - Extend chain dumps to show delegate depth, capability masks, modifier-local providers, semantics/focus flags, and the resolved inspector metadata to match Android Studio Modifier Inspector output.
2. **Capability-driven dispatch & invalidation (Phase 4)**  
   - Replace the `as_*` shortcut APIs with mask-driven visitors everywhere (draw/pointer/focus/semantics/layout) so third-party nodes only implement the traits they need.  
   - Introduce explicit DRAW invalidations (`mark_needs_draw`) and ensure redraw-only updates stop forcing layout passes.  
   - Add targeted tests comparing traversal order + invalidation behavior against the Kotlin reference chain in `/media/huge/composerepo/.../NodeChain.kt`.

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

**Status:** ✅ Done (semantics tree is fully modifier-driven; `SemanticsOwner` caches configurations, roles/actions derive from `SemanticsConfiguration` flags, and semantics invalidations are independent of layout. Tests cover caching, role synthesis, and capability-filtered traversal.)

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
