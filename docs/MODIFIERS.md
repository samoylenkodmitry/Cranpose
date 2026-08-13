# Modifier System Internals

This document provides comprehensive technical documentation of the modifier system implementation in cranpose, which achieves 1:1 parity with Jetpack Compose's Modifier.Node architecture.

## Table of Contents

- [Overview](#overview)
- [Core Architecture](#core-architecture)
- [Type System](#type-system)
- [Modifier Composition](#modifier-composition)
- [The Modifier Node Chain](#the-modifier-node-chain)
- [Built-in Modifiers](#built-in-modifiers)
- [Modifier Element System](#modifier-element-system)
- [Integration with Layout System](#integration-with-layout-system)
- [Modifier Slices](#modifier-slices)
- [Technical Implementation Details](#technical-implementation-details)
- [Key Implementation Files](#key-implementation-files)

---

## Overview

The modifier system is a **node-based architecture** inspired by Jetpack Compose that enables high-performance, composable UI modifications. The system prioritizes:

- **Node Reuse**: Node instances are recycled across recompositions (zero allocations when modifiers are stable)
- **Capability-Driven Dispatch**: Nodes declare capabilities via bitflags; only relevant nodes participate in each pipeline stage
- **Targeted Invalidation**: Changes trigger invalidation only for affected subsystems (Layout/Draw/PointerInput/Semantics/Focus)
- **Lifecycle Management**: Nodes receive `on_attach()`, `on_detach()`, `on_reset()` callbacks

### Key Design Principles

1. **Element-Node Duality**: Immutable `ModifierNodeElement` descriptors create/update stateful `ModifierNode` instances
2. **Flat Composition**: `.then()` eagerly concatenates element vectors, keeping modifiers as a single flat list
3. **Capability Filtering**: Traversal methods accept capability masks to skip irrelevant nodes
4. **Retained Measurement**: Layout modifier coordinators invoke reconciled node instances by identity

---

## Core Architecture

### Node-Based Architecture Pattern

The system follows Jetpack Compose's **Modifier.Node** pattern where modifiers create reusable node instances rather than eagerly computing values.

```rust
// Modifier chain creates persistent nodes
Modifier::empty()
    .padding(16.0)           // Creates PaddingNode (once)
    .background(Color::RED)  // Creates BackgroundNode (once)
    .clickable(handler)      // Creates ClickableNode (once)

// On recomposition with same modifiers:
// - Existing nodes are reused (no allocations)
// - Only changed modifiers trigger update()
// - Invalidation is targeted to affected systems
```

### Flat Modifier Composition

The `Modifier` type eagerly concatenates elements on `.then()`:

```rust
enum ModifierKind {
    Empty,
    Single {
        elements: Rc<Vec<DynModifierElement>>,
        inspector: Rc<Vec<InspectorMetadata>>,
    },
}
```

**Benefits**:
- No recursive Rc tree — zero drop overhead, flat comparison
- Simple slice iteration for element traversal
- Fingerprint-based fast equality rejection

---

## Type System

### The Foundation Traits

**Location**: `crates/cranpose-foundation/src/modifier.rs`

#### ModifierNode (Base Trait)

All modifier nodes implement this trait:

```rust
pub trait ModifierNode: Any + DelegatableNode {
    // Lifecycle callbacks
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {}
    fn on_detach(&mut self) {}
    fn on_reset(&mut self) {}

    // Capability dispatchers - return Some if node implements capability
    fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> { None }
    fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> { None }

    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> { None }
    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> { None }

    fn as_pointer_input_node(&self) -> Option<&dyn PointerInputNode> { None }
    fn as_pointer_input_node_mut(&mut self) -> Option<&mut dyn PointerInputNode> { None }

    fn as_semantics_node(&self) -> Option<&dyn SemanticsNode> { None }
    fn as_semantics_node_mut(&mut self) -> Option<&mut dyn SemanticsNode> { None }

    fn as_focus_node(&self) -> Option<&dyn FocusNode> { None }
    fn as_focus_node_mut(&mut self) -> Option<&mut dyn FocusNode> { None }
}
```

#### Capability-Specific Traits

**LayoutModifierNode**:
```rust
pub trait LayoutModifierNode: ModifierNode {
    fn measure(
        &self,
        context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints
    ) -> LayoutModifierMeasureResult;
}
```

**DrawModifierNode**:
```rust
pub trait DrawModifierNode: ModifierNode {
    fn draw(
        &mut self,
        context: &mut dyn ModifierNodeContext,
        draw_scope: &mut dyn DrawScope
    );
}
```

**PointerInputNode**:
```rust
pub trait PointerInputNode: ModifierNode {
    fn on_pointer_event(
        &mut self,
        context: &mut dyn ModifierNodeContext,
        event: &PointerEvent
    ) -> bool; // true = consumed event
}
```

### ModifierNodeElement (Element Trait)

Elements are immutable descriptors that create and update nodes:

```rust
pub trait ModifierNodeElement: Debug + Hash + PartialEq + 'static {
    type Node: ModifierNode;

    // Create new node instance
    fn create(&self) -> Self::Node;

    // Update existing node (called when element changes)
    fn update(&self, node: &mut Self::Node);

    // Declare capabilities for fast dispatch
    fn capabilities(&self) -> NodeCapabilities;

    // Optional key for disambiguation (for same type/hash)
    fn key(&self) -> Option<u64> { None }
}
```

#### Example: PaddingElement/PaddingNode

```rust
// Element: immutable, hashable, comparable
#[derive(Debug, Clone, PartialEq, Hash)]
pub struct PaddingElement {
    padding: EdgeInsets,
}

impl ModifierNodeElement for PaddingElement {
    type Node = PaddingNode;

    fn create(&self) -> PaddingNode {
        PaddingNode::new(self.padding)
    }

    fn update(&self, node: &mut PaddingNode) {
        if node.padding != self.padding {
            node.padding = self.padding;
            // Node will be invalidated by reconciliation
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

// Node: stateful, attached to tree
pub struct PaddingNode {
    padding: EdgeInsets,
    // Internal state managed by framework
}

impl LayoutModifierNode for PaddingNode {
    fn measure(&self, context: &mut dyn ModifierNodeContext,
               measurable: &dyn Measurable,
               constraints: Constraints) -> LayoutModifierMeasureResult {
        // Deflate constraints by padding
        let inner_constraints = constraints.deflate(self.padding);
        let placeable = measurable.measure(inner_constraints);

        // Add padding to result size
        LayoutModifierMeasureResult::new(
            Size {
                width: placeable.width() + self.padding.horizontal_sum(),
                height: placeable.height() + self.padding.vertical_sum(),
            },
            self.padding.left,  // Placement offset X
            self.padding.top,   // Placement offset Y
        )
    }
}
```

---

## Modifier Composition

### The Modifier Type

**Location**: `crates/cranpose-ui/src/modifier/mod.rs`

The `Modifier` struct is the user-facing API:

```rust
pub struct Modifier {
    kind: ModifierKind,
}

impl Modifier {
    // Create empty modifier
    pub fn empty() -> Self {
        Modifier { kind: ModifierKind::Empty }
    }

    // Compose modifiers (O(1) operation)
    pub fn then(&self, next: Modifier) -> Modifier {
        // Fast paths for empty modifiers
        if self.is_trivially_empty() { return next; }
        if next.is_trivially_empty() { return self.clone(); }

        // Eagerly concatenate elements into a single flat vec
        let merged = concat_elements(self, &next);
        Modifier { kind: ModifierKind::Single { elements: Rc::new(merged), .. } }
    }
}
```

### Chaining Pattern

Extension methods enable fluent API:

```rust
impl Modifier {
    pub fn padding(self, padding: impl Into<EdgeInsets>) -> Self {
        self.then(Modifier::from_element(PaddingElement::new(padding.into())))
    }

    pub fn background(self, color: Color) -> Self {
        self.then(Modifier::from_element(BackgroundElement::new(color)))
    }

    pub fn clickable<F>(self, on_click: F) -> Self
    where F: Fn(Point) + 'static
    {
        self.then(Modifier::from_element(ClickableElement::new(on_click)))
    }

    pub fn size(self, width: f32, height: f32) -> Self {
        self.then(Modifier::from_element(SizeElement::new(width, height)))
    }
}
```

### Usage Example

```rust
let modifier = Modifier::empty()
    .padding(16.0)                    // Adds PaddingElement
    .background(Color::RED)            // Adds BackgroundElement
    .corner_shape(RoundedCornerShape::all(8.0))  // Adds CornerShapeElement
    .clickable(|pos| println!("Clicked at {:?}", pos))  // Adds ClickableElement
    .size(100.0, 100.0);              // Adds SizeElement

// Creates flat element list:
// Single [PaddingElement, BackgroundElement, CornerShapeElement, ClickableElement, SizeElement]
```

### Fold Operations

Jetpack Compose-style traversal:

```rust
impl Modifier {
    // Fold from left to right (outer to inner)
    pub fn fold_in<R, F>(&self, initial: R, operation: F) -> R
    where F: Fn(R, &dyn AnyModifierElement) -> R;

    // Fold from right to left (inner to outer)
    pub fn fold_out<R, F>(&self, initial: R, operation: F) -> R
    where F: Fn(&dyn AnyModifierElement, R) -> R;
}

// Example: Collect all elements
let elements: Vec<DynModifierElement> = modifier.fold_in(Vec::new(), |mut acc, elem| {
    acc.push(elem.to_dyn());
    acc
});
```

---

## The Modifier Node Chain

### ModifierNodeChain

**Location**: `crates/cranpose-foundation/src/modifier.rs`

The `ModifierNodeChain` is the runtime representation that:
- Owns `Box<dyn ModifierNode>` instances (wrapped in `Rc<RefCell<>>`)
- Reconciles elements against existing nodes (reuses by type/key/hash)
- Manages node lifecycle (attach/detach/reset)
- Maintains aggregated capabilities for fast dispatch
- Provides traversal methods with capability filtering

```rust
pub struct ModifierNodeChain {
    entries: Vec<ChainEntry>,
    aggregated_capabilities: NodeCapabilities,
}

struct ChainEntry {
    node: Rc<RefCell<Box<dyn ModifierNode>>>,
    element: DynModifierElement,
    capabilities: NodeCapabilities,
}
```

### Reconciliation Algorithm

The `update_from_slice()` method implements O(n) reconciliation:

```rust
pub fn update_from_slice(
    &mut self,
    elements: &[DynModifierElement],
    context: &mut dyn ModifierNodeContext
) {
    // Step 1: Build index for O(1) lookups
    let index = EntryIndex::build(&self.entries);
    // Index tracks:
    // - keyed_entries: Map<(TypeId, u64), usize>
    // - hashed_entries: Map<(TypeId, u64), usize>
    // - typed_entries: Map<TypeId, Vec<usize>>

    let mut new_entries = Vec::with_capacity(elements.len());
    let mut reused_indices = HashSet::new();

    // Step 2: Match each new element to existing node
    for element in elements {
        let match_idx = index.find_match(element, &reused_indices);

        if let Some(old_idx) = match_idx {
            // Reuse existing node
            let old_entry = &self.entries[old_idx];

            // Update node if element changed
            if !element.equals(&*old_entry.element) {
                element.update_node(&mut *old_entry.node.borrow_mut());
            }

            new_entries.push(old_entry.clone());
            reused_indices.insert(old_idx);
        } else {
            // Create new node
            let node = element.create_node();
            attach_node_tree(node.clone(), context);

            new_entries.push(ChainEntry {
                node,
                element: element.clone(),
                capabilities: element.capabilities(),
            });
        }
    }

    // Step 3: Detach unused nodes
    for (idx, old_entry) in self.entries.iter().enumerate() {
        if !reused_indices.contains(&idx) {
            detach_node_tree(old_entry.node.clone());
        }
    }

    // Step 4: Rebuild chain
    self.entries = new_entries;
    self.rebuild_capability_aggregation();
}
```

### Capability-Based Traversal

```rust
impl ModifierNodeChain {
    // Forward traversal with capability filter
    pub fn for_each_forward_matching<F>(
        &self,
        capabilities: NodeCapabilities,
        mut f: F
    ) where F: FnMut(&Rc<RefCell<Box<dyn ModifierNode>>>)
    {
        if !self.aggregated_capabilities.contains(capabilities) {
            return; // Early exit if no matching nodes
        }

        for entry in &self.entries {
            if entry.capabilities.contains(capabilities) {
                f(&entry.node);
            }
        }
    }

    // Visit only layout nodes
    pub fn for_each_layout_node<F>(&self, f: F)
    where F: FnMut(&dyn LayoutModifierNode) {
        self.for_each_forward_matching(NodeCapabilities::LAYOUT, |node_ref| {
            let node = node_ref.borrow();
            if let Some(layout_node) = node.as_layout_node() {
                f(layout_node);
            }
        });
    }
}
```

---

## Built-in Modifiers

### Layout Modifiers

**Location**: `crates/cranpose-ui/src/modifier_nodes.rs`

#### PaddingNode

- **Capability**: `LAYOUT`
- **Behavior**: Deflates constraints, adds padding to result size, offsets child placement

```rust
impl LayoutModifierNode for PaddingNode {
    fn measure(&self, context: &mut dyn ModifierNodeContext,
               measurable: &dyn Measurable,
               constraints: Constraints) -> LayoutModifierMeasureResult {
        let inner_constraints = constraints.deflate(self.padding);
        let placeable = measurable.measure(inner_constraints);

        LayoutModifierMeasureResult::new(
            Size {
                width: placeable.width() + self.padding.horizontal_sum(),
                height: placeable.height() + self.padding.vertical_sum(),
            },
            self.padding.left,  // X offset
            self.padding.top,   // Y offset
        )
    }
}
```

#### SizeNode

- **Capability**: `LAYOUT`
- **Behavior**: Enforces size constraints (min/max width/height)
- **Supports**: `enforce_incoming` flag (like Compose's `size()` vs `requiredSize()`)

```rust
impl LayoutModifierNode for SizeNode {
    fn measure(&self, context: &mut dyn ModifierNodeContext,
               measurable: &dyn Measurable,
               constraints: Constraints) -> LayoutModifierMeasureResult {
        let mut new_constraints = constraints;

        if let Some(width) = self.min_width {
            new_constraints = new_constraints.with_min_width(width);
        }
        if let Some(width) = self.max_width {
            new_constraints = new_constraints.with_max_width(width);
        }
        // Similar for height...

        let placeable = measurable.measure(new_constraints);
        LayoutModifierMeasureResult::from_placeable(placeable)
    }
}
```

#### FillNode

- **Capability**: `LAYOUT`
- **Behavior**: Fills available space (fillMaxWidth/fillMaxHeight/fillMaxSize)
- **Supports**: Fractional fills (e.g., 0.5 = fill half of available space)

```rust
impl LayoutModifierNode for FillNode {
    fn measure(&self, context: &mut dyn ModifierNodeContext,
               measurable: &dyn Measurable,
               constraints: Constraints) -> LayoutModifierMeasureResult {
        let mut placeable = measurable.measure(constraints);

        if self.fill_width {
            placeable.set_width(constraints.max_width * self.fraction);
        }
        if self.fill_height {
            placeable.set_height(constraints.max_height * self.fraction);
        }

        LayoutModifierMeasureResult::from_placeable(placeable)
    }
}
```

#### OffsetNode

- **Capability**: `LAYOUT`
- **Behavior**: Translates child placement by (x, y), doesn't affect size
- **Supports**: RTL-aware vs absolute offset

```rust
impl LayoutModifierNode for OffsetNode {
    fn measure(&self, context: &mut dyn ModifierNodeContext,
               measurable: &dyn Measurable,
               constraints: Constraints) -> LayoutModifierMeasureResult {
        let placeable = measurable.measure(constraints);

        LayoutModifierMeasureResult::new(
            placeable.size(),
            self.x_offset,  // Additional X offset
            self.y_offset,  // Additional Y offset
        )
    }
}
```

### Draw Modifiers

#### BackgroundNode

- **Capability**: `DRAW`
- **Behavior**: Stores color/brush, collected in modifier slices
- **Integration**: Used with `CornerShapeNode` to create `DrawPrimitive::RoundRect`

```rust
pub struct BackgroundNode {
    color: Color,
}

// Collected during slice extraction:
// if let Some(background_node) = node.as_background_node() {
//     let color = background_node.color;
//     // Combine with shape from CornerShapeNode if present
// }
```

#### CornerShapeNode

- **Capability**: `DRAW`
- **Behavior**: Stores `RoundedCornerShape`, combined with background for rendering

```rust
pub struct CornerShapeNode {
    shape: RoundedCornerShape,
}

// Collected in slices:
// DrawPrimitive::RoundRect {
//     bounds: node_bounds,
//     color: background_color,
//     corner_radius: shape.top_left, // etc.
// }
```

### Input Modifiers

#### ClickableNode

- **Capability**: `POINTER_INPUT`
- **Behavior**: Captures pointer down events, invokes handler with position
- **Auto-adds**: Semantics (`is_clickable = true`)

```rust
pub struct ClickableNode {
    on_click: Rc<dyn Fn(Point)>,
}

impl PointerInputNode for ClickableNode {
    fn on_pointer_event(&mut self, context: &mut dyn ModifierNodeContext,
                        event: &PointerEvent) -> bool {
        if matches!(event.kind, PointerEventKind::Down) {
            (self.on_click)(Point {
                x: event.position.x,
                y: event.position.y,
            });
            true  // Consume event
        } else {
            false
        }
    }
}
```

### Semantics Modifiers

#### SemanticsModifierNode

- **Capability**: `SEMANTICS`
- **Behavior**: Runs a recorder closure that fills a `SemanticsConfiguration`;
  the layout pass folds every SEMANTICS node in the chain into one config and
  turns it into the node's `SemanticsNode`.
- **Re-recording**: the element declares `always_update`, so a recorder that
  captured new state re-runs and dirties the semantics snapshot. Nothing about
  the modifier's *shape* changes when a toggle flips, so without that the
  screen reader would keep reading the first frame's answer (regression test:
  `re_recording_semantics_reopens_the_snapshot`).

The vocabulary mirrors Jetpack Compose's `SemanticsProperties`:

| `SemanticsConfiguration` | Compose |
| --- | --- |
| `content_description` | `contentDescription` |
| `state_description` | `stateDescription` |
| `on_click_label` | `onClick(label = …)` — implies the click action |
| `role` | `Role` (`Button`, `Checkbox`, `Switch`, `RadioButton`, `Tab`, `Image`, `Header`) |
| `selected` | `selected` |
| `toggled` | `toggleableState` |
| `enabled` | the inverse of `disabled()` |
| `custom_actions` | `customActions = listOf(CustomAccessibilityAction(…))` |
| `canvas_children` | *(no Compose equivalent — see below)* |

#### Canvas semantics

An immediate-mode surface — one `Canvas` painting a whole screen — has exactly
one layout node, so the semantics tree built from layout has exactly one node
to offer a screen reader. `canvas_children` is the escape hatch: the drawing
code already knows where it put every control, so it publishes those rectangles
as semantics directly.

```rust
Modifier::empty().fill_max_size().semantics(move |config| {
    config.canvas_children = rows
        .iter()
        .map(|row| {
            CanvasSemanticsNode::control(row.key, row.rect, &row.label)
                .with_role(SemanticsWidgetRole::Switch)
                .with_toggled(row.checked)
                .with_state_description(if row.checked { "On" } else { "Off" })
        })
        .collect();
});
```

- `bounds` are in the publishing node's own coordinates, because that is what a
  draw scope works in; the accessibility projection translates them.
- `key` must survive a redraw. A screen reader parks its cursor on a virtual
  view id, and the id is derived from `(layout node, key)` — derive the key from
  what the control *is*, never from where it currently sits.
- Activation is a synthesised tap at the published rect's centre, so a control
  the app already hit-tests by geometry needs no extra wiring. A custom action
  has no position and is instead routed back by identity and run against the
  live semantics tree.
- Android's own answer for a canvas-drawn `View` has the same shape
  (`ExploreByTouchHelper` feeding virtual view ids into an
  `AccessibilityNodeProvider`), and Cranpose's Android bridge already *is* an
  `AccessibilityNodeProvider`.

---

## Modifier Element System

### Creating Custom Elements

Pattern for creating new modifier elements:

```rust
// 1. Define element (immutable descriptor)
#[derive(Debug, Clone, PartialEq, Hash)]
pub struct MyModifierElement {
    pub param1: f32,
    pub param2: Color,
}

// 2. Define node (stateful instance)
pub struct MyModifierNode {
    state: NodeState,
    param1: f32,
    param2: Color,
}

impl DelegatableNode for MyModifierNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

// 3. Implement ModifierNodeElement
impl ModifierNodeElement for MyModifierElement {
    type Node = MyModifierNode;

    fn create(&self) -> Self::Node {
        MyModifierNode::new(self.param1, self.param2)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.param1 != self.param1 || node.param2 != self.param2 {
            node.param1 = self.param1;
            node.param2 = self.param2;
            // Invalidation happens automatically
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT | NodeCapabilities::DRAW
    }
}

// 4. Implement capability traits on node
impl ModifierNode for MyModifierNode {
    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
        Some(self)
    }

    // Use helper macro to reduce boilerplate:
    // impl_modifier_node!(layout, draw);
}

impl LayoutModifierNode for MyModifierNode {
    fn measure(
        &self,
        context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> LayoutModifierMeasureResult {
        // Implementation
    }
}

impl DrawModifierNode for MyModifierNode {
    fn draw(&self, draw_scope: &mut dyn DrawScope) {
        // Implementation
    }
}

// 5. Add extension method to Modifier
impl Modifier {
    pub fn my_modifier(self, param1: f32, param2: Color) -> Self {
        self.then(Modifier::from_element(MyModifierElement { param1, param2 }))
    }
}
```

### Type Erasure

The system uses type erasure to store heterogeneous elements:

```rust
// Type-erased wrapper
pub type DynModifierElement = Rc<dyn AnyModifierElement>;

// Helper trait for type erasure
pub trait AnyModifierElement: Debug {
    fn type_id(&self) -> TypeId;
    fn hash_value(&self) -> u64;
    fn equals(&self, other: &dyn AnyModifierElement) -> bool;
    fn create_node(&self) -> Rc<RefCell<Box<dyn ModifierNode>>>;
    fn update_node(&self, node: &mut Box<dyn ModifierNode>);
    fn capabilities(&self) -> NodeCapabilities;
    fn key(&self) -> Option<u64>;
}

// Concrete wrapper
struct TypedModifierElement<E: ModifierNodeElement> {
    element: E,
    hash_cache: u64,
}

impl<E: ModifierNodeElement> AnyModifierElement for TypedModifierElement<E> {
    fn create_node(&self) -> Rc<RefCell<Box<dyn ModifierNode>>> {
        let node: E::Node = self.element.create();
        Rc::new(RefCell::new(Box::new(node)))
    }

    fn update_node(&self, node: &mut Box<dyn ModifierNode>) {
        // Downcast to concrete type
        if let Some(typed_node) = node.downcast_mut::<E::Node>() {
            self.element.update(typed_node);
        }
    }

    // ... other methods
}
```

---

## Integration with Layout System

### ModifierChainHandle

**Location**: `crates/cranpose-ui/src/modifier/chain.rs`

The `ModifierChainHandle` bridges the `Modifier` API and the layout tree:

```rust
pub struct ModifierChainHandle {
    chain: ModifierNodeChain,
    context: RefCell<BasicModifierNodeContext>,
    resolved: ResolvedModifiers,
    capabilities: NodeCapabilities,
}

impl ModifierChainHandle {
    pub fn new(modifier: &Modifier) -> Self {
        let elements = modifier.elements();
        let mut context = BasicModifierNodeContext::new();
        let mut chain = ModifierNodeChain::new();

        chain.update_from_slice(&elements, &mut context);

        let resolved = Self::compute_resolved(&chain);
        let capabilities = chain.aggregated_capabilities();

        ModifierChainHandle {
            chain,
            context: RefCell::new(context),
            resolved,
            capabilities,
        }
    }

    pub fn update(&mut self, modifier: &Modifier) -> Vec<ModifierInvalidation> {
        // 1. Flatten modifier tree to element list
        let elements = modifier.elements();

        // 2. Reconcile node chain (reuses existing nodes)
        self.chain.update_from_slice(&elements, &mut *self.context.borrow_mut());

        // 3. Recompute resolved modifiers
        let new_resolved = Self::compute_resolved(&self.chain);
        let changed = self.resolved != new_resolved;
        self.resolved = new_resolved;

        // 4. Update capabilities
        self.capabilities = self.chain.aggregated_capabilities();

        // 5. Return invalidations
        self.context.borrow_mut().take_invalidations()
    }

    pub fn resolved(&self) -> &ResolvedModifiers {
        &self.resolved
    }
}
```

### ResolvedModifiers

Cached layout properties extracted from the chain:

```rust
pub struct ResolvedModifiers {
    pub padding: EdgeInsets,
    pub offset: Offset,
    pub min_size: Option<Size>,
    pub max_size: Option<Size>,
    pub fill_width: Option<f32>,
    pub fill_height: Option<f32>,
    // ... other cached properties
}

impl ModifierChainHandle {
    fn compute_resolved(chain: &ModifierNodeChain) -> ResolvedModifiers {
        let mut resolved = ResolvedModifiers::default();

        // Traverse chain and accumulate properties
        chain.for_each_layout_node(|node| {
            if let Some(padding_node) = node.downcast_ref::<PaddingNode>() {
                resolved.padding = resolved.padding + padding_node.padding;
            }
            if let Some(offset_node) = node.downcast_ref::<OffsetNode>() {
                resolved.offset.x += offset_node.x_offset;
                resolved.offset.y += offset_node.y_offset;
            }
            // ... other properties
        });

        resolved
    }
}
```

### Retained Coordinator Measurement

**Location**: `crates/cranpose-ui/src/layout/mod.rs`

The layout system reconciles layout modifier nodes into a retained
`CoordinatorChain`. Each coordinator stores the retained modifier node handle and
invokes `LayoutModifierNode::measure()` on the live node during measurement.

**Measurement Flow**:
1. Layout collects retained layout modifier node handles from the `ModifierNodeChain`.
2. `LayoutRuntimeState` reconciles those handles into a retained `CoordinatorChain`.
3. Each coordinator invokes the live `LayoutModifierNode` and passes a wrapped
   measurable for the next coordinator.
4. Node-local measurement state stays on the retained node instead of being copied
   into detached snapshot objects.

---

## Modifier Slices

**Location**: `crates/cranpose-ui/src/modifier/slices.rs`

**Purpose**: Pre-collect capabilities for rendering/input without traversing chain repeatedly during hot paths.

```rust
pub struct ModifierNodeSlices {
    pub draw_commands: Vec<DrawCommand>,
    pub pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pub click_handlers: Vec<Rc<dyn Fn(Point)>>,
    pub clip_to_bounds: bool,
    pub text_content: Option<String>,
    pub graphics_layer: Option<GraphicsLayer>,
}

pub fn collect_modifier_slices(chain: &ModifierNodeChain) -> ModifierNodeSlices {
    let mut slices = ModifierNodeSlices::default();

    let mut background_color: Option<Color> = None;
    let mut corner_shape: Option<RoundedCornerShape> = None;

    // Single traversal collects all capabilities
    chain.for_each_node(|node_ref| {
        let node = node_ref.borrow();

        // Collect background
        if let Some(bg) = node.downcast_ref::<BackgroundNode>() {
            background_color = Some(bg.color);
        }

        // Collect shape
        if let Some(shape) = node.downcast_ref::<CornerShapeNode>() {
            corner_shape = Some(shape.shape);
        }

        // Collect draw nodes
        if let Some(draw_node) = node.as_draw_node() {
            // Custom draw commands...
        }

        // Collect pointer input
        if let Some(clickable) = node.downcast_ref::<ClickableNode>() {
            slices.click_handlers.push(clickable.on_click.clone());
        }
    });

    // Combine background + shape into single draw primitive
    if let (Some(color), Some(shape)) = (background_color, corner_shape) {
        slices.draw_commands.push(DrawCommand::RoundRect {
            color,
            corner_radius: shape.top_left,
            // ... other corners
        });
    } else if let Some(color) = background_color {
        slices.draw_commands.push(DrawCommand::Rect { color });
    }

    slices
}
```

**Usage in Rendering**:
```rust
// Layout node stores slices
pub struct LayoutNode {
    modifier_slices: ModifierNodeSlices,
    // ...
}

// During draw phase:
impl LayoutNode {
    fn draw(&self, canvas: &mut Canvas) {
        // Execute all draw commands without chain traversal
        for command in &self.modifier_slices.draw_commands {
            match command {
                DrawCommand::Rect { color } => {
                    canvas.draw_rect(self.bounds, *color);
                }
                DrawCommand::RoundRect { color, corner_radius } => {
                    canvas.draw_rounded_rect(self.bounds, *corner_radius, *color);
                }
            }
        }
    }
}
```

---

## Technical Implementation Details

### NodeCapabilities Bitflags

**Location**: `crates/cranpose-foundation/src/modifier.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeCapabilities(u32);

impl NodeCapabilities {
    pub const LAYOUT: Self = Self(1 << 0);
    pub const DRAW: Self = Self(1 << 1);
    pub const POINTER_INPUT: Self = Self(1 << 2);
    pub const SEMANTICS: Self = Self(1 << 3);
    pub const PARENT_DATA: Self = Self(1 << 4);
    pub const FOCUS: Self = Self(1 << 5);

    pub fn contains(&self, other: NodeCapabilities) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn union(&self, other: NodeCapabilities) -> NodeCapabilities {
        NodeCapabilities(self.0 | other.0)
    }
}
```

**Fast Dispatch**:
```rust
// Check if any layout nodes exist before traversal
if chain.aggregated_capabilities().contains(NodeCapabilities::LAYOUT) {
    chain.for_each_layout_node(|node| {
        // Only called if LAYOUT capability present
    });
}
```

### Invalidation System

```rust
pub enum InvalidationKind {
    Layout,
    Draw,
    PointerInput,
    Semantics,
    Focus,
}

pub struct ModifierInvalidation {
    pub kind: InvalidationKind,
    pub node_id: Option<usize>,
}

// Nodes request invalidation via context
impl ModifierNode for MyNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(InvalidationKind::Layout);
    }
}

// Context accumulates invalidations
pub struct BasicModifierNodeContext {
    invalidations: Vec<ModifierInvalidation>,
}

impl ModifierNodeContext for BasicModifierNodeContext {
    fn invalidate(&mut self, kind: InvalidationKind) {
        self.invalidations.push(ModifierInvalidation {
            kind,
            node_id: None,
        });
    }
}

// Layout system processes invalidations
let invalidations = modifier_handle.update(&new_modifier);
for invalidation in invalidations {
    match invalidation.kind {
        InvalidationKind::Layout => request_layout(),
        InvalidationKind::Draw => request_draw(),
        InvalidationKind::PointerInput => update_hit_testing(),
        // ...
    }
}
```

### Helper Macros

**Location**: `crates/cranpose-foundation/src/modifier_helpers.rs`

Reduce boilerplate for capability declarations:

```rust
#[macro_export]
macro_rules! impl_modifier_node {
    ($($capability:ident),*) => {
        $(
            impl_modifier_node!(@single $capability);
        )*
    };

    (@single layout) => {
        fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
            Some(self)
        }
        fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
            Some(self)
        }
    };

    (@single draw) => {
        fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
            Some(self)
        }
        fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
            Some(self)
        }
    };

    // ... other capabilities
}

// Usage:
impl ModifierNode for MyNode {
    impl_modifier_node!(layout, draw, pointer_input);
}
```

### Inspector Metadata

For debugging and developer tools:

```rust
pub struct InspectorInfo {
    pub name: String,
    pub properties: HashMap<String, String>,
}

// Attached to modifier elements
let modifier = Modifier::empty()
    .padding(16.0)
    .with_inspector_info(InspectorInfo {
        name: "padding".to_string(),
        properties: [("value".to_string(), "16.0".to_string())].into(),
    });
```

---

## Key Implementation Files

| File Path | Responsibilities |
|-----------|------------------|
| `cranpose-foundation/src/modifier.rs` | Core traits (`ModifierNode`, `LayoutModifierNode`, `DrawModifierNode`, `PointerInputNode`, `SemanticsNode`, `FocusNode`), `ModifierNodeChain` reconciliation and lifecycle, capability bitflags, node context traits |
| `cranpose-ui/src/modifier/mod.rs` | `Modifier` type definition, composition via `then()`, fold operations (`fold_in`, `fold_out`), element extraction, empty modifier singleton |
| `cranpose-ui/src/modifier/chain.rs` | `ModifierChainHandle` integration bridge, `ResolvedModifiers` caching, invalidation propagation, measurement chain setup |
| `cranpose-ui/src/modifier_nodes.rs` | All built-in modifier nodes: `PaddingNode`, `SizeNode`, `FillNode`, `OffsetNode`, `BackgroundNode`, `CornerShapeNode`, `ClickableNode`, etc. |
| `cranpose-ui/src/modifier/slices.rs` | `ModifierNodeSlices` collection, draw command aggregation, background+shape combination, pointer input collection |
| `cranpose-ui/src/modifier/padding.rs` | Padding modifier factory methods (`padding()`, `padding_symmetric()`, `padding_all()`), `PaddingElement` definition |
| `cranpose-ui/src/modifier/size.rs` | Size modifier factory methods (`size()`, `width()`, `height()`, `required_size()`), `SizeElement` definition |
| `cranpose-ui/src/modifier/fill.rs` | Fill modifier factory methods (`fill_max_width()`, `fill_max_height()`, `fill_max_size()`), `FillElement` definition |
| `cranpose-ui/src/modifier/background.rs` | Background/corner shape factory methods (`background()`, `corner_shape()`), element definitions |
| `cranpose-ui/src/modifier/clickable.rs` | Clickable modifier factory method, `ClickableElement` and `ClickableNode` implementations |
| `cranpose-ui/src/modifier/offset.rs` | Offset modifier factory methods (`offset()`, `absolute_offset()`), RTL support |
| `cranpose-foundation/src/modifier_helpers.rs` | Helper macros (`impl_modifier_node!`), boilerplate reduction utilities |

---

## Performance Characteristics

### Time Complexity

| Operation | Complexity | Notes |
|-----------|------------|-------|
| `modifier.then(other)` | O(n+m) | Concatenates element vectors (n + m elements) |
| Chain reconciliation | O(n) | Where n = number of elements, with O(1) lookups via indexing |
| Node reuse check | O(1) | Hash-based + type-based + key-based index lookups |
| Capability-filtered traversal | O(m) | Where m = nodes matching capability (aggregation enables early exit) |
| Resolved modifier computation | O(n) | Single pass accumulation |
| Slice collection | O(n) | Single pass with capability filtering |

### Space Complexity

| Structure | Space | Notes |
|-----------|-------|-------|
| `Modifier` | O(n) | Flat element vector per modifier |
| `ModifierNodeChain` | O(n) | One entry per element |
| Node instances | O(n) | Reused across recompositions (zero new allocations when stable) |
| Coordinator chain | O(n) | Retained layout modifier chain reconciled by element identity |
| Modifier slices | O(n) | Cached pre-computed capabilities |

### Optimization Strategies

1. **Node Reuse**: Existing nodes are updated in-place when elements match (type + hash + key)
2. **Capability Aggregation**: Chain maintains union of all node capabilities for early exit
3. **Lazy Evaluation**: Nodes are only created/updated when modifier changes
4. **Retained Coordinators**: Layout measurement walks stable coordinator nodes and live retained modifier state
5. **Flat Composition**: `.then()` concatenates elements eagerly, eliminating recursive Rc overhead
6. **Slice Caching**: Draw/input capabilities pre-collected to avoid hot-path traversal

---

## Summary

The cranpose modifier system achieves **complete parity with Jetpack Compose's Modifier.Node architecture** while adapting to Rust's ownership model. Key achievements:

✅ **Zero-allocation node reuse** across recompositions
✅ **Capability-driven dispatch** for optimal performance
✅ **Flat modifier composition** with fingerprint-based fast equality
✅ **Borrow-checker-safe measurement** via snapshot proxies
✅ **Full lifecycle management** (attach/detach/reset)
✅ **Targeted invalidation** per subsystem
✅ **Complete built-in modifier suite** (padding, size, fill, offset, background, clickable, etc.)
✅ **Inspector metadata** for developer tools

The architecture elegantly solves Rust's ownership constraints while maintaining Compose's performance characteristics and developer ergonomics.
