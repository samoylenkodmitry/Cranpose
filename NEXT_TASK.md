# Next Task: Implement Focus Chain Parity with Jetpack Compose

## Context
Focus is the last major subsystem that still treats the modifier chain as a flat list. While layout, draw, pointer input, and semantics now use delegate-aware traversal helpers with capability-based short-circuiting, focus management continues to operate on legacy patterns. We need to migrate focus to use `ModifierNodeChain`'s delegate traversal so focus targets, focus requesters, focus order, and ancestor resolution work identically to Jetpack Compose's `androidx.compose.ui.focus.*` implementation.

## Current State
- **What exists:** Basic focus management exists but operates outside the modifier node system
- **What's missing:**
  - Focus modifier nodes (FocusTargetNode, FocusRequesterNode, FocusOrderNode)
  - Capability-gated focus traversal using `NodeCapabilities::FOCUS`
  - Delegate-aware ancestor/descendant search for focus resolution
  - Focus state management integrated with modifier chain lifecycle
  - Focus transaction system mirroring Kotlin's `FocusTransactions.kt`

## Goals
1. **Create focus modifier node types** that implement `ModifierNode` with focus-specific capabilities
2. **Wire focus traversal** through `ModifierNodeChain`'s delegate-aware visitors (`visit_ancestors_matching`, `visit_descendants_matching`)
3. **Implement focus state management** that respects node attachment lifecycle (`onAttach`, `onDetach`)
4. **Add focus capability bit** (`NodeCapabilities::FOCUS`) to enable efficient short-circuiting
5. **Port key Kotlin patterns** from `FocusTransactions.kt`, `FocusTraversalPolicy.kt`, and `FocusProperties.kt`
6. **Test focus behavior** including delegation, traversal order, and capability filtering

## Jetpack Compose Reference
Key files to study in `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/focus/`:
- `FocusModifier.kt` - Core focus modifier implementation
- `FocusTargetNode.kt` - Focus target modifier node
- `FocusRequesterModifierNode.kt` - Focus requester node
- `FocusOrderModifierNode.kt` - Focus ordering node
- `FocusTransactions.kt` - Focus state transitions
- `FocusTraversalPolicy.kt` - Focus navigation (next, previous, up, down, etc.)
- `FocusProperties.kt` - Focus configuration properties

## Suggested Implementation Steps

### Step 1: Define Focus Capability and Node Traits
```rust
// In compose-foundation/src/modifier.rs
impl NodeCapabilities {
    pub const FOCUS: Self = Self(1 << 5);
}

pub trait FocusTargetNode: ModifierNode {
    fn focus_state(&self) -> FocusState;
    fn on_focus_changed(&mut self, focused: bool);
}

pub trait FocusRequesterNode: ModifierNode {
    fn request_focus(&mut self);
}
```

### Step 2: Create Focus Modifier Elements
Create `compose-ui/src/modifier/focus.rs`:
- `FocusTargetElement` - Makes a node focusable
- `FocusRequesterElement` - Allows programmatic focus requests
- `FocusOrderElement` - Controls focus traversal order
- Each should return `NodeCapabilities::FOCUS` from `capabilities()`

### Step 3: Implement Focus Manager
Create or extend focus manager to:
- Track the currently focused node by `NodeId`
- Use `ModifierNodeChain::for_each_forward_matching(NodeCapabilities::FOCUS, ...)` to find focus targets
- Implement `find_focusable(start: NodeId, direction: FocusDirection)` using delegate-aware traversal
- Handle focus requests via `visit_descendants_matching` for finding targets

### Step 4: Port Focus Transactions
Mirror Kotlin's focus transaction logic:
- `requestFocus(node: &dyn ModifierNode)` - Request focus on a specific node
- `clearFocus(force: bool)` - Clear current focus
- `moveFocus(direction: FocusDirection)` - Navigate focus (Next, Previous, Up, Down, Left, Right)
- All operations should traverse via capability-filtered helpers

### Step 5: Integrate with Modifier Chain Lifecycle
- Focus nodes attach/detach via `onAttach`/`onDetach`
- Focus state invalidates using `ModifierNodeContext::invalidate(InvalidationKind::Focus)` (add this variant)
- `LayoutNode` maintains a focus dirty flag similar to semantics

### Step 6: Add Traversal Utilities
Extend `ModifierChainNodeRef` with focus-specific helpers:
```rust
impl ModifierChainNodeRef<'_> {
    pub fn find_parent_focus_target(&self) -> Option<&dyn FocusTargetNode> {
        // Use visit_ancestors_matching with FOCUS capability
    }

    pub fn find_next_focusable(&self, direction: FocusDirection) -> Option<NodeId> {
        // Use capability-aware traversal to find next target
    }
}
```

### Step 7: Testing
Create `compose-ui/src/modifier/focus/tests.rs`:
- `focus_target_receives_events` - Basic focus request/clear
- `focus_traversal_respects_order` - Next/previous navigation
- `delegated_focus_nodes_discovered` - Delegate depth is respected
- `capability_filtering_shortcuts_focus_search` - Non-focus subtrees skipped
- `focus_state_survives_recomposition` - Focus persists across updates
- `focus_invalidation_independent_of_layout` - Focus changes don't trigger layout

## Definition of Done
- ✅ Focus modifier nodes exist (`FocusTargetNode`, `FocusRequesterNode`) with `NodeCapabilities::FOCUS`
- ✅ Focus manager uses `for_each_forward_matching` and `visit_ancestors_matching` for all traversal
- ✅ `Modifier::focusTarget()` and `Modifier::focusRequester()` factories implemented
- ✅ Focus state persists across modifier chain updates via element equality
- ✅ Focus traversal (next/previous/directional) uses capability-filtered helpers
- ✅ Tests verify delegation, traversal order, and capability short-circuiting
- ✅ `cargo test` passes with no regressions
- ✅ Focus behavior matches Jetpack Compose reference tests from `FocusTest.kt`

## Success Criteria
After this task, developers should be able to:
```rust
Modifier::empty()
    .focusTarget()
    .focusRequester(requester)
    .onFocusChanged(|focused| {
        println!("Focus changed: {}", focused);
    })
```

And focus traversal should:
- Use capability masks to skip non-focusable subtrees
- Respect delegate nodes when searching for focus targets
- Work identically to Kotlin's focus system in terms of ordering and lifecycle

## Migration Notes
- Existing focus code (if any) should be audited and migrated to use the new node-based system
- Focus-related invalidations should go through the new `InvalidationKind::Focus` variant
- Add `NodeCapabilities::FOCUS` to the capability debug dumps

## Cross-Reference
- Roadmap: `modifier_match_with_jc.md` - Near-Term Next Steps #1, Phase 4
- Prior Art: Semantics implementation (just completed) provides a pattern for node-based system integration
- Kotlin Reference: `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/focus/`
