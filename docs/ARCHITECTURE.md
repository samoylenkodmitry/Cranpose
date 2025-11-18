# Proposal: A Jetpack Compose‑Inspired Declarative UI Framework for Rust

## Background and motivation

Modern mobile and desktop UIs have largely moved toward **declarative** APIs. Kotlin’s **Jetpack Compose** lets Android developers describe the UI as a tree of functions that react to state changes. Similar ideas power **SwiftUI** and Flutter.  Rust’s GUI ecosystem, however, is still fragmented.  A 2025 survey of Rust GUI libraries notes that there isn’t yet a “super‑easy slam‑dunk” choice: you can pick **Dioxus** for a WebView‑backed Diet Electron‑style approach, **Slint** if you like DSL‑driven UIs with solid tooling, or **egui** if you prefer to avoid macros and work with an immediate‑mode API【478399327936865†L1738-L1755】.  Each option has trade‑offs and none follow Jetpack Compose’s slot‑table and recomposition model.

Slint, for example, compiles declarative UI descriptions to native code and keeps a tiny runtime under 300 KiB, with a reactive property system and cross‑platform support【268503358500627†L170-L187】.  Despite this, its DSL and code‑behind separation differ from Compose’s function‑centric API, and it isn’t designed to mirror Compose’s recomposition semantics.  To explore whether Rust can adopt Compose’s **declarative functions, snapshot state and effects** model while still taking advantage of Rust’s safety and performance, this proposal outlines a new framework tentatively called **Compose‑RS**.

## Goals and design principles

The proposed framework aims to:

1. **Mirror Jetpack Compose’s ergonomics.** UI elements are described as pure Rust functions annotated with a `#[composable]` attribute.  They can call other composable functions and use a `Modifier` chain for styling and behaviour, just like Compose.
2. **Provide first‑class state and recomposition.** A lightweight **slot table** stores persistent state, remembered values and keys.  When a `State<T>` changes, the framework marks the corresponding composable scope dirty and schedules a frame; during recomposition only invalid scopes re‑run, avoiding unnecessary work.
3. **Offer a flexible modifier and layout system.** A chain of modifiers (padding, background, size, click handlers, transforms, clipping, etc.) composes behaviour in a predictable order.  Layout nodes implement a two‑phase measure/place protocol similar to Compose’s `Layout` composable.
4. **Be portable and efficient.** The runtime will run on desktop via `winit` and **Skia**, with future back‑ends for Android (via `ndk_glue` and Skia) and Web (via WebAssembly).  The core should have minimal dependencies and compile down to a small native binary.  Inspired by Slint’s lightweight runtime【268503358500627†L170-L187】, Compose‑RS will aim for a similarly small footprint.

## Architecture overview

The framework consists of several crates:

- **`compose_core`**: implements the slot table, `Composer`, `Recomposer`, `State`, `Effects` and `CompositionLocal`.  The slot table stores a tape of groups containing keys, arity, remembered values and node references.  The `Composer` traverses this tape during recomposition, comparing keys to decide whether to skip or re‑run a subtree.
- **`compose_macros`**: a procedural macro crate providing the `#[composable]` attribute.  It rewrites a function into one that accepts `&mut Composer`, wraps the body in `start_group`/`end_group` calls, and lowers builder blocks into lambdas that emit nodes with their own groups.
- **`compose_ui`**: defines UI primitives (e.g. `Text`, `Image`, `Column`, `Row`, `Button`) and the **modifier** types.  Modifiers are implemented as an immutable chain of operations (padding, background, size, click).  Layout nodes implement a two‑phase `measure`/`place` API, returning sizes and placing children.
- **Back‑end crates** (`compose_skia`, `compose_wgpu`): implement the **`Applier`** trait to manage native nodes and draw via a rendering library.  The desktop back‑end uses `winit` for the event loop and `skia‑safe` for GPU‑accelerated drawing.  Android and iOS back‑ends can use `ndk_glue` and `wgpu`/Metal.
- **`compose_platform`**: a thin platform layer providing event loops, timers, clipboard access and input handling.

## Core runtime and component model

At the heart of the framework is the **slot table**, a compact vector of slots that record groups, values and node identifiers.  Each composable function call records a **group** with a **key** (derived from `file!()` and `line!()` or an explicit `key(id)`).  The group stores its arity (number of child calls), skip flags and remembered values.  When recomposing, the `Composer` checks whether the upcoming group in the table has the same key and whether its inputs are stable; if so, it can **skip** executing the function and reuse the previously emitted nodes.

Persistent state is represented by `State<T>`, which wraps a value in `Rc<RefCell<T>>` and maintains a list of watching group indices.  Calling `state.set(new_value)` marks those groups dirty and schedules a frame.  The `Recomposer` collects dirty groups and processes them during the next frame, executing their bodies and applying changes to the UI tree.

### Effects and coroutines

Effects allow you to perform side‑effects in response to recomposition:

- `side_effect { … }` runs a closure after the frame is applied.
- `disposable_effect(key) { on_dispose = … }` runs a cleanup when the key changes or the scope leaves.
- `launched_effect(key) { async { … } }` launches an asynchronous task (e.g. a network call) that is cancelled when the key changes.

### CompositionLocals

Composition locals provide scoped ambient values (e.g. colours, typography, density).  A `CompositionLocal<T>` has a default provider and can be overridden by a `Provider` composable within a subtree.

## Modifiers and layout

The **Modifier** type is a persistent chain of operations.  Each modifier carries a `ModOp` (padding, background, size, click, clip, transform, etc.) and a pointer to the next modifier.  Composables accept a `modifier` argument and apply it in order.  During drawing, the back‑end interprets the modifier chain to apply paint, clipping and input handling.

Layout nodes implement two methods:

1. **`measure(ctx, constraints) → Size`**: given minimum and maximum width/height constraints, measure each child (recursively calling `measure` on child nodes) and decide the size of the node.  For example, a `Column` adds the heights of its children plus spacing and takes the maximum width.
2. **`place(ctx, origin)`**: position each child relative to an origin.  The `ctx` can perform translations or apply modifiers on a per‑child basis.

This separation allows for flexible layouts, such as `Row`, `Column`, `Box` and custom `Layout` implementations, mirroring Compose’s `Layout` composable.

## Back‑ends and platform support

For the desktop back‑end, the framework will integrate with **winit** for window creation and input and **Skia** for rendering.  Skia provides high‑quality text, vector and image drawing and can target multiple graphics back‑ends (OpenGL, Vulkan, Metal).  On Android, `ndk_glue` provides the event loop, and the same Skia renderer can draw to a native surface.  Future back‑ends could use `wgpu` or integrate with the **Metal** API on iOS.

## Comparison with existing frameworks

While the Rust community already has multiple GUI libraries, none directly replicate Jetpack Compose’s architecture.  Slint offers a declarative markup language and compiles to native code with a tiny runtime and reactive properties【268503358500627†L170-L187】, but its DSL differs significantly from Compose’s function‑centric API.  The 2025 survey of Rust GUI libraries points out that **Dioxus** (Diet Electron), **Slint** (DSL‑driven), **egui** (immediate mode), **Freya** and **Xilem** are all viable but come with trade‑offs【478399327936865†L1738-L1755】.  Compose‑RS aims to complement this landscape by bringing Jetpack Compose’s **slot table**, **recomposition**, **modifiers** and **effects** model into Rust, thereby offering an alternative to both DSL‑driven and immediate‑mode libraries.

## Implementation plan and milestones

Development can be staged in the following milestones:

1. **Initial window and rendering**: create a `winit` window, integrate `skia‑safe` and draw a “Hello World” text.  Set up the event loop and frame scheduling.
2. **Slot table and composer**: implement `SlotTable::start_group`, `end_group`, `remember`, and `Composer` traversal.  Write a `#[composable]` macro that injects a composer argument and records groups.  Implement `Text`, `Box`, `Column`, `Row` and `Spacer` as basic primitives.
3. **State and recomposition**: implement `State<T>` with watcher tracking and frame invalidation.  Demonstrate a `Counter()` example that recomposes on click.
4. **Modifiers**: design the `Modifier` chain with padding, background, size, and clickable operations.  Integrate with hit‑testing and input handling.  Add `Button` and improve styling.
5. **Layout polish**: implement constraint handling, alignment, spacing, and intrinsic sizing.  Add `Row`, `Column`, `Box`, and `Stack` with alignment options.
6. **Effects and coroutines**: implement `side_effect`, `disposable_effect`, `launched_effect`, and integrate an async executor (e.g. `async‑executor` or `tokio`, behind a feature flag).
7. **Composition locals and theming**: add `CompositionLocal<T>`, define `LocalColors`, `LocalTypography` and implement a `Theme` composable that propagates these values.
8. **Advanced widgets**: add text editing, lists (`LazyColumn`), scroll views and gestures.  Implement viewport recycling for lists using stable keys.  Create sample applications.
9. **Additional back‑ends**: explore Android and Web targets by using `ndk_glue` and `wgpu`/WebGL.  Evaluate the feasibility of iOS support.

By following this plan, Compose‑RS can evolve into a production‑ready framework that brings Jetpack Compose’s productivity and ergonomics to Rust developers.

## Code sketches

Below are a few minimal sketches illustrating how Compose‑RS’s core runtime and API might look in code.  These examples are not complete but they demonstrate the core concepts described above.

```rust
// A simplified slot table storing groups, values and node handles.
use std::{any::Any, rc::Rc, cell::RefCell};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key(pub u64);
pub type Ix = u32;

pub enum Slot {
    Group { key: Key, arity: u16, skip: bool },
    Value(Box<dyn Any>),        // remember values and state
    Node(Ix),                   // index into the applier’s arena
}

pub struct SlotTable {
    tape: Vec<Slot>,
    sp: usize,
}

impl SlotTable {
    pub fn start_group(&mut self, key: Key) -> usize {
        // push a new group onto the tape; return its index so we can mark arity later
        let idx = self.tape.len();
        self.tape.push(Slot::Group { key, arity: 0, skip: false });
        idx
    }
    pub fn end_group(&mut self, start_idx: usize) {
        // update the group's arity based on how many slots were emitted in between
        if let Slot::Group { arity, .. } = &mut self.tape[start_idx] {
            *arity = (self.tape.len() - start_idx - 1) as u16;
        }
    }
    pub fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> &mut T {
        if let Some(Slot::Value(boxed)) = self.tape.get_mut(self.sp) {
            // reuse remembered value
            return boxed.downcast_mut::<T>().expect("type mismatch");
        }
        // insert a new slot with the initial value
        let value = Box::new(init());
        self.tape.insert(self.sp, Slot::Value(value));
        match &mut self.tape[self.sp] {
            Slot::Value(boxed) => boxed.downcast_mut::<T>().unwrap(),
            _ => unreachable!(),
        }
    }
    pub fn record_node(&mut self, id: Ix) {
        self.tape.push(Slot::Node(id));
    }
}

// A composer traverses the slot table during composition and recomposition.
pub trait Node: 'static {
    fn mount(&mut self, ctx: &mut dyn Applier);
    fn update(&mut self, ctx: &mut dyn Applier);
    fn unmount(&mut self, ctx: &mut dyn Applier);
}

pub trait Applier {
    fn create<N: Node>(&mut self, init: N) -> Ix;
    fn get_mut(&mut self, ix: Ix) -> &mut dyn Node;
    fn remove(&mut self, ix: Ix);
}

pub struct Composer<'a> {
    pub slots: &'a mut SlotTable,
    pub applier: &'a mut dyn Applier,
}

impl<'a> Composer<'a> {
    pub fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> &mut T {
        self.slots.remember(init)
    }
    pub fn emit<N: Node + 'static>(&mut self, init: impl FnOnce() -> N) -> &mut N {
        // allocate a node via the applier and record its index in the slot table
        let ix = self.applier.create(init());
        self.slots.record_node(ix);
        self.applier.get_mut(ix).downcast_mut::<N>().unwrap()
    }
}

// Persistent state wraps a value and notifies watchers when it changes.
pub struct State<T> {
    inner: Rc<RefCell<T>>,
    watchers: Rc<RefCell<Vec<usize>>>,
}

impl<T> State<T> {
    pub fn new(v: T) -> Self {
        Self { inner: Rc::new(RefCell::new(v)), watchers: Rc::new(RefCell::new(Vec::new())) }
    }
    pub fn get(&self) -> T where T: Copy {
        *self.inner.borrow()
    }
    pub fn set(&self, v: T) {
        *self.inner.borrow_mut() = v;
        // mark each watching group dirty (implementation omitted)
    }
}

// Example composable counter component.
#[composable]
pub fn Counter() {
    let count = remember(|| State::new(0));
    Column(Modifier::empty().padding(16.0)) {
        Text(format!("Count = {}", count.get()));
        Row(Modifier::empty().gap(8.0)) {
            Button(on_click = move || count.set(count.get() - 1)) { Text("-") }
            Button(on_click = move || count.set(count.get() + 1)) { Text("+") }
        }
    }
}

// Modifier definition showing a chain of operations.
use std::rc::Rc;
pub enum ModOp {
    Padding(f32),
    Background(Color),
    Clickable(Rc<dyn Fn(Point)>),
    // other operations: size, clip, transform, etc.
}

struct NodeMod {
    op: ModOp,
    next: Option<Rc<NodeMod>>,
}

pub struct Modifier(Option<Rc<NodeMod>>);

impl Modifier {
    pub fn empty() -> Self { Self(None) }
    fn from(op: ModOp) -> Self {
        Self(Some(Rc::new(NodeMod { op, next: None })))
    }
    pub fn padding(p: f32) -> Self { Self::from(ModOp::Padding(p)) }
    pub fn background(c: Color) -> Self { Self::from(ModOp::Background(c)) }
    pub fn clickable(on_click: impl Fn(Point) + 'static) -> Self {
        Self::from(ModOp::Clickable(Rc::new(on_click)))
    }
    pub fn then(self, next: Modifier) -> Self {
        match (self.0, next.0) {
            (Some(mut this), Some(next_node)) => {
                // find the end of this chain and append
                let mut tail = Rc::get_mut(&mut this).unwrap();
                tail.next = Some(next_node);
                Self(Some(this))
            }
            (None, _) => next,
            (some, None) => Self(some),
        }
    }
}
```

These sketches illustrate the core patterns: a slot table records compositional structure; a `Composer` manages slots and node creation; `State<T>` ties values to reactive recomposition; and `Modifier` chains describe styling and interactions.  A full implementation would flesh out error handling, effect management, layout measurement and drawing via a concrete `Applier` back‑end.
