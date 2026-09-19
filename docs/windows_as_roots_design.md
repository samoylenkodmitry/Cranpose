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
  builders accessibility reads skip window-root children the same way,
  and so does the semantics builder over the applier: CI's screen reader
  audit of the Winamp tab found it walking into the three windows, naming
  nodes the primary layout tree does not hold.
- `nearest_window_root(applier, node)` tells a shell which surface a dirty
  node belongs to; step 3 partitions dirty nodes with it.

### Step 3: surfaces

`AppShell` is now the application (`ShellApp` inside the crate: runtime,
composition, content, clock, layout flags, dev options, clipboard, text
input routes) plus one `RootSurface` per window, the primary first. A
platform hands a window root a renderer with
`add_window_surface(id, renderer, buffer_size, viewport)`, reads the
window's frame and delivers its events through `surface(RootId::Window(id))`,
and takes the renderer back with `remove_window_surface`. Every shell
method that names no root still acts on the primary surface, so the
existing platforms compile and behave unchanged. Differences from the plan:

- The per-root input entry points are methods of a `SurfaceMut` handle
  rather than a second family of `*_on(root, ...)` methods. The handle
  borrows the shell and the surface index, so a press can still reach
  whole-app operations such as the dev overlay's pacing switch.
- A window surface follows the registry by id: after each recomposition
  the shell points it at the node registered under its id, and at nothing
  while the window is out of the tree. Node ids are recycled, so a window
  composed anew may get its old id back; the surface reads the registry,
  not the id.
- Dirty nodes are sorted by `nearest_window_root`: draw repass nodes,
  structural parents, repass and geometry nodes all go to the surface
  owning the window root above them, or to the primary when there is none,
  and are dropped when their window has no surface yet. The retained
  redraw walk stops at nested window roots. The frame-wide "draw repass
  pending" flag became "this surface received repass nodes", which is what
  it always meant for one surface.
- A render invalidation is one app-wide boolean. When the frame's dirt
  named nodes, the surfaces that received none report nothing to present;
  when it named none, every surface presents its retained scene again.
  This keeps a caret blink in one window from presenting every window.
- The layout pass sets `scene_dirty` on every surface for a global pass,
  and only on the surfaces that received geometry nodes for a scoped one;
  a scoped pass that named no node at all marks every surface, as one
  surface was marked before.
- Draw observations are pruned to the union of every surface's retained
  visual nodes, once per frame.
- The pointer icon session moved onto the surface (`PointerIconState` is
  public in `cranpose-ui` for that); the app context keeps its own for the
  free functions.
- Text input: one router sits in the app context's text input session and
  forwards a show to the handler of the active surface, a hide to the
  handler that showed. A press activates its surface; a platform names the
  focused surface with `set_active_root` otherwise.
- Focus order per root falls out of the per-surface layout snapshot, since
  the keyboard paths collect the order from the surface's own tree.
- `SurfaceMut::set_viewport` requests a measure repass of the window root
  and leaves the frame to the next update, unlike `AppShell::set_viewport`,
  which keeps its synchronous frame for the primary.
- Semantics snapshots are per surface but the accessibility bridge still
  reads the primary, as planned.

Tests in `crates/cranpose-app-shell/src/tests/surface_tests.rs`: a press in
a window reaches only that window's content and activates it; a hover
changes only that surface's pointer icon; each surface snapshots its own
root; a surface follows its root out of the tree and back; removing a
surface hands back its renderer; the soft keyboard belongs to the active
surface; a draw change in a window updates only that window's scene and
only that surface reports a frame. Six mutations of the partition,
attribution, retained walk, activation and routing were killed by them.
`scripts/dev/function_complexity.sh` lists function complexities as the
gate measures them, for splitting a body before it moves under a new name.

### Step 4: windows are surfaces

The desktop owns one `AppShell` again. A declared window composes its
content under `Modifier::window_root(id, descriptor)` inside a `PopupHost`,
where the descriptor is the window's `NativeWindowRoot`, a cell holding the
logical size the desktop keeps equal to the window's surface. The registry
carries that root instead of a content closure; the id is the `WindowId`
hash, exposed as `WindowId::raw`. Creating an OS window hands the shell a
renderer with `add_window_surface`, resizing sets the root's size and the
surface's buffer and viewport, closing takes the renderer back with
`remove_window_surface`. Every native event reaches its surface through
`app.surface(RootId::Window(id))`; the wrappers that used to call the
window's shell (`dispatch_mouse_wheel`, `dispatch_keyboard_input`,
`dispatch_ime_event`, `dispatch_middle_click_paste`, `cancel_surface_input`,
`DesktopTextInput::install`) take a `SurfaceMut`, and the primary window
passes `app.primary()`. Differences from the plan:

- **One update, many presents.** A native redraw runs the whole-app
  `update` and presents its own surface when that surface owes a frame.
  `RootSurface::frame_owed` remembers a visual change until the platform
  takes it with `take_frame_owed`, so a change to window B produced while
  window A was redrawing is not lost, and the primary presents a frame a
  native redraw produced for it. The hidden primary of a declaration host
  takes the flag in its direct update. `about_to_wait` asks a native window
  to redraw when its schedule needs a frame or it owes one, and no longer
  runs per-window updates.
- **The event handler returns what to settle.** `dispatch_native_window_event`
  takes the app and the window out of their maps; `native_window_event` answers whether the
  window stays plus a `NativeWindowEventSettlement` (graph moves, graph
  drag, finish, sync). `settle_native_window_event` applies it once the
  window is back in the map, which is what the old tail of the function
  did inline.
- **Modifiers and pointer source** are set on the shell and the surface
  respectively; the frame pacing mode is set once on the app, and
  `apply_frame_pacing_mode` configures a wgpu surface from the device.
- **Scale factor** of a native window sets the platform's scale and the
  surface renderer's root scale; density stays app-wide as the plan's
  limits say.
- `Focused(true)` on any window calls `set_active_root`, so the soft
  keyboard and the text input router follow the OS focus.
- The dock still reads `current_native_window_surface_origin`; step 5
  removes both.

Checks: `chrome_tabs` tears a pressed tab into a new window with its
counter intact and joins it back; `tool_windows` tears a pane out and snaps
it back in line; `winamp_standalone` moves its equalizer and playlist with
the main window. All through `scripts/dev/drag_window.sh`, with an idle
log that stops growing once the windows are up. The shell test
`a_surface_keeps_owing_its_frame_until_the_platform_takes_it` covers the
owed-frame flag.

### Step 5: demos on the primitives, dock deleted

`crates/cranpose/src/dock.rs` and every `Dock*` export are gone, and with
them the thread-local surface origin: `current_native_window_surface_origin`
and the dispatch context field behind it. In their place a `PointerEvent`
carries `screen_position`, the pointer on the screen in logical pixels
when the platform told the shell where the window sits. Each `RootSurface`
keeps a `screen_origin` the platform sets through
`SurfaceMut::set_screen_origin` (`AppShell::set_screen_origin` for the
primary); the desktop sets it from the OS position before every pointer
sample, as the dispatch context used to be set, and on every `Moved`. The
shell adds it to the root-relative position when it builds an event, so a
cross-window gesture reads one field and compares it with the windows'
`WindowState`s.

The two demos are plain app code in `apps/desktop-demo/src/app/`:

- `torn_windows.rs` is what an app writes for tabs or tool windows: a pure
  model of windows, panes and the drag in flight (the dock's model, with
  its tests), and a `TornWindowsHost` composable that opens one
  `WindowNode` per window with a `rememberWindowStateAt`, gives the chrome
  a `WindowView`, and puts a `pointer_input` on each window's root that
  steps the drag from the events' screen positions. `Windows::grip` marks
  a pane's grip; it names only the pane and looks the holding window up at
  the press, so it may sit inside the pane's movable content.
- `chrome_tabs.rs` composes each page's body under `movable`, keyed by the
  page. The click counter is remembered inside that body, so nothing above
  the page holds its state and the count survives the tear; closing a tab
  calls `forget_movable`. `tool_windows.rs` composes each tool under
  `movable` with the grip in its title.

Tearing the first tab out showed a core defect the earlier tests had not
reached: a restored subtree's root node record still named its old parent,
so the next pass that skipped the enclosing group (a window moving after
the release) took the page for one of that group's own root nodes and hung
it a level up, under the drag box, where it covered the strip.
`DetachedSubtree::set_root_nodes_parent` now records the node the composer
attaches a restored subtree under, passed through `begin_group`. Tests at
three levels cover it: the core moves content into a fresh parent and
skips that parent on the next pass; the shell tears a page into a second
window root and recomposes that root with its chrome skipped; the demo
module tears through its own model and moves the new window. The first
two fail without the fix; the demo test drives the glue end to end and
passes either way, so it guards the demo, not the core.

Joining a tab back left a ghost: the torn window, hidden at the join,
stayed painted on the screen at its last position, owned by no window and
taking no clicks. AppKit had ordered the window out, but the redraw winit
queues itself for a content change still ran, and a frame presented to an
ordered-out window puts its surface back on the screen. A hidden native
window now holds its redraw (`native_window_redraw_held_while_hidden`),
keeping the frame owed until the window shows again. In the tabs demo a
parked window also composes no page: a window holds no pane once it is
parked, so the page moves into the joined window at the join itself
rather than after the release.

`scripts/dev/drag_window.sh` launches with `CRANPOSE_DEMO_TRACE` and reads
the `demo trace:` lines the host prints, gained `key` and `oswindows`
(what the desktop lists for the example, for a window the demo believes
is gone), and the two examples start the demo's logger when built with
`logging`, so the debug key's layout dump reaches the tool's log.
`scripts/dev/mutation_check.sh` takes cargo arguments for a package whose
tests need features, as the desktop tests do.

### Step 6: plumbing

Five items, each small, each with a number or a test.

- **A window's state knows when it is on the screen.** `WindowState`
  gained `presented()`, set by the desktop after a present and cleared
  when the window is hidden or let go. An app that moves content into a
  new window can keep it in the old one until the new one has a frame up,
  so the content is never in neither. The demo's window trace prints it.
- **The loop spun while a button was held.** A pressed pointer keeps a
  surface's schedule asking for a frame, so its platform's frame driver
  stays awake; the desktop turned that into a redraw request the moment
  the previous one returned, and each redraw found nothing to present and
  recorded no frame time to pace the next. Measured with the drag tool's
  new `cpu` command on the tabs demo: a button held for two seconds cost
  2.7 CPU seconds and 110,000 native redraws a second. An attempt that
  presents nothing now paces the next attempt like a frame
  (`pace_after_empty_redraw`, for the primary and the native path); the
  same two seconds cost 0.07 CPU seconds after.
- **The loop spun during a platform drag.** A window drag polled the
  global pointer with a zero interval and made the loop poll for as long
  as the drag lasted; on every platform but X11 there is no global
  pointer, so each poll only wrote `drag poll skipped`. One second of
  dragging the strip wrote 210,000 of those and cost 2.9 CPU seconds. A
  build that cannot read the pointer on the screen never polls a drag
  (`native_window_drag_poll_deadline`); one that can polls every 16 ms,
  and a poll ahead is a deadline the loop waits for, not a reason to
  spin. The choice of `Poll`, `WaitUntil` and `Wait` moved out of
  `about_to_wait` into `event_loop_control_flow`, which is tested. The
  same drag costs 0.10 CPU seconds and writes 128 lines after.
- **Focus is a policy.** `WindowConfig::with_focus` takes a
  `WindowFocus`: `Never`, `WhenNoneFocused` (the default and the old
  constant) or `Always`, applied when the desktop creates the window.
- **Reads that borrow.** `MutableState::read` and `State::read` run the
  closure on the stored value in place, so a read of the window model on
  a pointer move no longer copies it. `with` keeps copying: its contract,
  held by a test, lets the closure write the state it reads, which a
  borrow cannot allow without touching the record chain the snapshots
  keep. The traces are macros that evaluate their arguments only when the
  trace is on, and read their environment variable once.

## Second cut: windows are modifiers, nothing else

The first cut left three gaps the review named: the launcher and the shell
still command windows, the demos carry a model and a host of their own to
tear a page out, and the framework grew helpers a developer has to learn.
The second cut removes all three. What an application writes:

```rust
let torn = rememberMutableStateOf(|| false);
let modifier = if torn.get() {
    Modifier::empty().window(WindowConfig::borderless("Tab 2", 520.0, 360.0))
} else {
    Modifier::empty()
};
Column(modifier, ColumnSpec::default(), || { ... });
```

A subtree is in its own window while the modifier is applied and back in
place when it is not; its remembered state and running effects survive
either way, because the modifier only changes where the subtree is laid out
and drawn. Everything a window can do is a modifier on that subtree, or a
field of its `WindowConfig`:

- `window(config)`: be a window. `config` carries title, size, position,
  decorations, transparency, focus policy, and a `group(id, policy)` for
  windows that snap to and move with their peers, which is what
  `WindowGroup` did as a composable.
- `window_drag_area()` and `window_resize_area(direction)`: move and resize
  the window from this node, as today. A window created while the pointer
  is still down in another window takes that press as its drag, so a page
  torn out under the pointer follows it without the application moving
  anything.
- `drag_and_drop_source(payload)` and `drag_and_drop_target(handlers)`:
  Compose's pair, routed across windows. The shell tracks one transfer by
  screen position and finds targets in every surface; the target's handlers
  see enter, move, exit and drop with the payload. The source's handlers
  see the drag start, where it is, and that it ended outside every target.

What leaves: `Window`, `WindowNode`, `WindowGroup`, `AppLauncher::run_windows`
and `try_run_windows`, `WindowView`, `TornWindowsHost` and the torn-windows
model. The primary window shows the root composition; when the root
composition places every visible node in a window of its own, the primary
window stays hidden, which the launcher decided by a flag before. The shell
keeps its surfaces, but what a platform loop calls to open, close and
address them lives on one `Surfaces` handle documented as the platform
contract, not on the application-facing shell.

The tabs demo becomes pages, each inline or in a window of its own, a strip
that is a drop target, and tabs that are drag sources; the tool windows
demo becomes three panes with `window(config.group(...))`, snapped and
carried by the framework's window group. Neither has a model.

### Steps

1. **Tests out of implementation files.** Every inline `#[cfg(test)] mod`
   in a file this branch touched moves to a `tests/` file beside it,
   declared through `#[path]`, so `use super::*` keeps its reach
   (`scripts/dev/move_inline_tests.py`).
2. **Comments.** Only documentation of public items in published crates
   stays; every other comment goes (`scripts/dev/strip_private_docs.py`).
3. **`window(config)` and `WindowConfig::group`.** The composable wrappers
   are deleted; Winamp and the demos apply the modifier. A window's
   identity is the node that carries the modifier, so an application names
   nothing: the node keeps its identity across recompositions and moves,
   and the modifier's node registers the window request on attach, on
   every config change, and unregisters on detach, where the wrappers ran
   effects. A group's drag leader is a config flag (`leads_group`), since
   no application can name another window's node. Popups inside a window
   subtree still register with the primary window's host; a host per
   window is a follow-up the surfaces make possible.
4. **The launcher sets the root composition only.** `run_windows` and
   `try_run_windows` go; the primary window hides itself when the root
   composition has nothing of its own to show. The shell's surface
   management moves onto a `Surfaces` handle for platform loops.
5. **A new window under a held press follows the pointer.** The desktop's
   drag session starts from the press already in flight when the node
   under it now lives in a window that just appeared.
6. **Drag and drop across windows.** The two modifiers, the shell's transfer
   tracking across surfaces, and tests for enter, exit, drop and a drop
   outside every target.
7. **Demos on the modifiers.** Tabs and tool windows rewritten as above,
   `torn_windows.rs` deleted, the drag tool reading the demos' new trace.
8. **Docs and PR.** As built, the PR description, and the gates.

