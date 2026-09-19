# Windows as roots: one composition, any number of OS windows

Status: plan, being built on `feat/tearable-tabs` (PR #717). Each step below
lands as its own commit on that branch, green at every step. The "as built"
notes at the end are filled in as steps land.

## Goal

An application is one composition, one slot table, one node tree. An OS
window is a root inside that tree, declared by a modifier:

```rust
Box(Modifier::empty().window(WindowConfig::borderless("Player", 275.0, 116.0)
        .with_state(state)), BoxSpec::default(), || PlayerPane(...));
```

Remove the modifier and the subtree is inline again. Nothing in the subtree
is composed "from scratch on arrival": a keyed subtree can move between any
two parents, in one window or in two, and keep its remembered values, its
running effects and its nodes:

```rust
movable(page.id, move || PageBody(page));
```

Chrome-style tabs, Winamp-style snapping windows and IntelliJ-style tool
windows are then ordinary composables over plain state. The framework
provides three primitives, the window modifier, movable content and
per-window pointer facts (screen position, window rectangle), and nothing
else. `Dock`, `Pane`, `DockHost`, `dock_handle` and the per-window
`AppShell` go away.

## What binds shell and window today

From the maps taken before this plan (all paths under `crates/`):

- `AppShell` (`cranpose-app-shell/src/lib.rs:167-208`) owns one composition
  and, in the same struct, one viewport, cursor, pressed buttons, hit path
  tracker, hovered nodes, layout snapshot, semantics snapshot, frame
  scheduler and renderer. The renderer owns the retained `Scene`, which is
  both the draw list and the hit-test structure (`cranpose-render/common/src/graph_scene.rs:385`).
- `AppContext` (`cranpose-ui/src/render_state.rs:34-56`) mixes app-wide
  services with per-window state: density, invalidation flags and repass
  queues, pointer and focus dispatch, the text input session bound to one
  winit window, the pending pointer icon.
- Focus order, keyboard-focus visibility, the modal stack and the popup host
  registry are thread-global while the focus target is per context, so two
  windows already disagree with each other.
- `Composition::root` is one `Option<NodeId>` (`cranpose-core/src/composition.rs:19`),
  set by whichever parentless node was emitted last (`emit.rs:364`).
- A native window leaves the composition entirely: `WindowNode` registers a
  content closure in a thread-local registry and the desktop loop builds a
  whole new shell for it (`cranpose/src/desktop.rs:1609-1620`).
- Everything below the shell's layout and render phases is already
  parameterised by `(applier, root, viewport)`: `measure_layout_with_options`,
  `tree_needs_layout`, `build_graph_from_applier`, `render_from_applier`,
  `update_from_applier`, `scene_node_attached_to`,
  `take_structural_change_parents_attached_to`.

The seam is therefore above those functions: the shell's scalars become
per-root, the scene builder stops at nested roots, and the composition keeps
one primary root.

## Target architecture

### Roots

The composition keeps its single primary root. A **window root** is a
`LayoutNode` inside the tree whose modifier chain carries a
`WindowRootElement`. The element's node:

- measures its content with the window's constraints, taken from the
  window's `WindowState` size, and reports a zero size to its parent, so the
  parent lays out as if the subtree were absent;
- registers its node id and its window configuration in a
  `WindowRootRegistry` on the `AppContext` when attached, and removes them
  when detached, so the desktop loop learns about windows by reading that
  registry after each update, as it reads the native window registry today;
- is skipped by the scene builder when it builds the parent's graph, and is
  the root when the builder builds that window's graph.

Layout stays one pass from the primary root. A window subtree is measured
inside that pass with its own constraints, so invalidation, caches and the
frame arena work unchanged; a resized OS window marks its root node for
measure. Positions in a window subtree are relative to the window root,
which is what a scene built from that root needs.

### Surfaces

`AppShell` becomes the application: runtime, composition, content, clock,
dev options, clipboard, app-wide `AppContext` services. It holds a
`RootSurface` per root, the primary included:

```text
RootSurface {
    root: NodeId, renderer, viewport, buffer_size,
    cursor, buttons_pressed, pointer_source, hit_path_tracker, hovered_nodes,
    layout snapshot, semantics snapshot + enabled + revision,
    layout_requested, force_layout_pass, scene_dirty, scoped_layout_scene_nodes,
    retained_visual_nodes, is_dirty, frame_scheduler, frame_rate_preference,
    dev overlay, pointer icon, text input session, on_rotary_scroll,
}
```

Rendering is per surface: each surface's renderer builds its scene from
`(applier, root, viewport)`, dirty nodes partitioned with
`scene_node_attached_to`, the global invalidation booleans applied to every
surface. Presenting is per surface. Frame scheduling is per surface, and the
runtime's frame waker wakes every surface that owes a frame.

Input is per surface: an event names its root, hit-tests that surface's
scene at that surface's cursor, and keeps its gesture in that surface's
tracker. Keyboard input goes to the app's focus target when that target
lives under the surface's root. Focus order is computed per root on demand,
which also removes the thread-global clobber. The pointer icon and the text
input session live on the surface because the OS owns them per window.

Density is one value for the whole app in this plan. Mixed-DPI monitors are
a follow-up and are noted as such.

### Movable content

`movable(key, content)` opens a group whose identity is the key, app-wide,
not the call site. The core already detaches a departed keyed subtree into
a `DetachedSubtree` that owns its payloads and keeps its nodes alive and its
effects running, and restores it under any parent
(`cranpose-core/src/slot/detach.rs:140`, `:253`). What changes:

- `RetainKey` gains a `Movable(MovableId)` variant that carries no parent
  scope (`retention.rs:69`); movable entries are exempt from budget
  eviction (`retention.rs:379`); the retained-subtree validators stop
  comparing the root group key for movable entries.
- The `MovableId` rides on `RecomposeOptions` and on the scope
  (`lib.rs:517`), so the departure path inserts under the movable key.
- Ordering: the new parent may compose before the old parent sweeps. A
  movable site whose state is not yet retained opens an empty placeholder
  group and records a pending insert; the pass end
  (`composer.rs:791`, after `finalize_pass`) restores the subtree at the
  placeholder, attaches its root nodes under the placeholder's parent, and
  forces the root scope to recompose. If nothing arrived, the placeholder
  composes fresh on the next pass.
- On restore, every scope inside the subtree has its parent scope and
  parent hint rewritten; today only the root gets that (`composer.rs:1093`).
- Movable content does not cross a `SubcomposeLayout` host in this plan,
  because anchors are per slot table; the composer logs and composes fresh.

Node reparenting needs no new code: `emit_node_box` adopts the restored
`NodeId` and `insert_child_with_reparenting` dirties both parents and
records structural changes the scene builder consumes.

### Pointer facts for cross-window gestures

A `PointerEvent` gains a screen position. The shell computes it from the
surface's window origin, which the desktop loop keeps in the window's
`WindowState`. The thread-local dispatch context for surface origin goes
away. Window rectangles are the `WindowState`s the app already holds. A
drag between windows is then a `pointer_input` on any node: press in one
window, moves delivered until release by the OS, screen coordinates from the
event, rectangles from state. Snapping, drop zones and tab strips are app or
library code.

### What is removed, what stays

Removed: `dock.rs` and every `Dock*` export, `SizedPane`, `Pane`,
`dock_handle`; per-window `AppShell`; the content closure in
`NativeWindowRegistry`; `compose_root` per window;
`current_native_window_surface_origin`.

Stays, reimplemented over roots: `WindowConfig` and its builder,
`WindowState`, `rememberWindowState[At]`, `WindowNode`, `Window`,
`WindowGroup` with `WindowAttachPolicy`, `window_drag_area`,
`window_resize_area`, the desktop fixes from this branch (a held press
survives a window opening over it, new windows do not steal focus, a
resized or created window presents before the desktop composites), and
`scripts/dev/drag_window.sh`.

## Steps

Each step compiles, passes its tests and the gates, and is one commit.

1. **Core: movable content.** `MovableId`, `RetainKey::Movable`, eviction
   exemption, the placeholder and pending-insert splice at pass end, scope
   re-parenting on restore, `movable(key, content)` in `hooks.rs` and the
   prelude. Tests: flip
   `keyed_child_moved_between_parents_rebuilds_without_stale_attachment`
   into the movable case (same `NodeId`, zero payload drops, zero node
   unmounts); a remembered counter, a running `LaunchedEffect` and a
   `DisposableEffect` survive a move in both orderings (old parent
   composed first, new parent composed first); a movable emitted twice in
   one pass logs and composes the second fresh.
2. **UI: window roots.** `WindowRootElement` and node in `cranpose-ui`,
   `WindowRootRegistry` on `AppContext`, window constraints in
   `measure_through_modifier_chain`, the scene builder stopping at nested
   roots and starting at a given root. Tests: a window subtree measures
   with the window's size and its parent sees zero; the parent's graph
   excludes the subtree; a graph built from the window root contains it
   with positions relative to the root; resizing the window state
   re-measures only that subtree.
3. **App shell: surfaces.** `RootSurface`, `AppShell::roots`, per-root
   layout and render bookkeeping, per-root input entry points with the old
   signatures kept as the primary-root case, per-root frame scheduling,
   focus order per root, pointer icon and text input per root. Tests: two
   roots, a press in the second hits only the second; a hover in one
   changes only that surface's icon; dirty nodes reach the right surface;
   the frame waker wakes the surface that owes a frame.
4. **Desktop: windows are surfaces.** `NativeWindowSurface` drops its shell
   and gains a root; the sync reads the window root registry; every native
   event names its root; presents call the shell per root, keeping the
   synchronous present on resize and creation; `DesktopTextInput` and
   pointer icons per root; `Focused` names the active root. `WindowNode`,
   `Window` and `WindowGroup` become wrappers over the modifier. Checks:
   `winamp_standalone` still glues and moves its windows; the drag tool's
   launch, tear and snap runs on the rewritten demos.
5. **Demos on the primitives, dock deleted.** `chrome_tabs` and
   `tool_windows` rewritten as plain composables over a `Vec` of windows
   with `movable` pages and panes; snapping as a small helper in the demo;
   `dock.rs`, its exports, tests and docs removed; the drag tool's
   `windows` listing reads the new rest-state trace.
6. **Plumbing.** `WindowState` learns whether its window has presented, so
   an app can keep a pane in its old window until the new one can show it;
   the two busy loops; focus as a policy on `WindowConfig`; borrowing
   reads on `MutableState::with`; lazy trace arguments.
7. **Docs and PR.** This document gains an "as built" section; the PR
   description is rewritten; the AGENTS rule about the drag tool stays.

## Risks and limits

- **Cross-host moves.** A movable subtree cannot leave a `SubcomposeLayout`
  host, so a lazy list item cannot be torn into a window directly. Wrap it
  in a movable outside the list, or accept a fresh composition. Noted in
  the `movable` docs.
- **Density per app.** Windows on monitors with different scale factors
  share one density until the context learns per-root density.
- **Accessibility and the robot** stay bound to the primary root in this
  plan; secondary windows publish semantics but the bridge reads the
  primary. Extending both is a follow-up the surface split makes easy.
- **Popups** register with the nearest `PopupHost`. The modifier alone puts
  a window subtree's popups in the primary window; the `Window` wrapper
  adds a host per window.
- **Shell input signatures.** Every desktop call site changes from
  "this shell" to "this root". The old signatures stay as the primary-root
  case so the change is mechanical and reviewable.

## Validation

`just clippy`, `just clippy-optional-backends`, `just clippy-robot`, the
library tests with the desktop feature set, the workspace tests, the
commit hook's gates, and the drag tool on both demos and on the Winamp
example, with the timing trace proving presents land before the desktop
composites.

## As built

### Step 1: movable content

`movable(key, content)` and `forget_movable(key)` in `cranpose-core`,
re-exported from `cranpose` and its prelude. Differences from the plan
above, all in `crates/cranpose-core/src`:

- No `RetainKey::Movable` variant. A movable group's key is exact (the
  seed skips the branch fold, `slot/types.rs`) with a fixed static half, so
  `RetainKey::for_group` simply drops the parent scope for it and the
  existing validators keep comparing the root key.
- The table indexes attached movables by identity (`slot/movable.rs`), so a
  site can tell "still attached under another parent" in constant time. The
  site then opens a placeholder group under its own exact key and records a
  pending site; at pass end, once the movable is retained, the site's
  enclosing scope is forced to recompose and takes the content back through
  the ordinary restore path. The placeholder is then an unvisited sibling
  and is disposed by the usual sweep. No splice at pass end.
- A pending site survives passes, so a move whose two halves land in
  different frames still completes; a second live site for the same key
  stays empty.
- Every detached subtree gives up the movables nested below its root
  (`DetachedSubtree::split_off_nested_movables`) before it is retained or
  disposed, so a window that closes because its last tab left does not take
  the tab's state with it. The node detach commands run before the
  container's disposal, so the applier keeps the nodes.
- Restored subtrees now reactivate every scope and repoint the parent hint
  of scopes with no node between them and the root; before, a scope two
  levels down behind a skipped composable stayed inactive and attached new
  nodes to the old parent.
- Retained movables are pinned against the retention budget and released
  by `forget_movable`, which works from an event handler through the
  runtime and disposes on the composition's next recompose entry.
- `scripts/dev/mutation_check.sh` proves each test guards its fix.

### Step 2: window roots

`Modifier::window_root(id, descriptor)` in `cranpose-ui`
(`modifier/window_root.rs`), with `NodeCapabilities::WINDOW_ROOT` in the
foundation crate so layout and the scene builder both read one bit.
Differences from the plan:

- The window root's own size is the window's size, not zero: `LayoutState`
  keeps it, so the window's scene has real bounds for clipping and hit
  testing. Only what the parent is handed is zero: `MeasuredNode` carries a
  `window_root` flag and `size_for_parent()` answers the parent's placeable
  and intrinsic queries. Apply `window_root` first in a chain; everything
  after it is inside the window.
- The descriptor is a trait object (`WindowRootDescriptor`) the platform
  implements over its own window configuration. Its `layout_size()` is read
  on every measure, so a resize is `schedule_measure_repass(node)` and no
  new modifier.
- The registry lives on the `AppContext` (`window_roots()`, revision
  counter); the node registers in `on_attach` with the node id the chain
  context supplies and unregisters by the context id it recorded.
- The scene builder skips window roots wherever it lowers a child (one
  check in `build_layer_node_from_applier_internal`) and, when asked to
  build from a window root, starts at the origin. The two layout-tree
  builders accessibility reads skip window-root children the same way.
- `nearest_window_root(applier, node)` tells a shell which surface a dirty
  node belongs to; step 3 partitions dirty nodes with it.
