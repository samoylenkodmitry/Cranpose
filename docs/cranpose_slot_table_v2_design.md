# Cranpose Slot Table V2 — Full Rearchitecture Design Doc

Status: active implementation
Target crate: `crates/cranpose-core`  
Primary files affected: `lib.rs`, `subcompose.rs`, `retention.rs`, `slot/*`, tests
Principle: keep one slot-table architecture; do not preserve obsolete gap-based surfaces or wrapper modules.

---

## 1. Executive summary

The current Cranpose slot table mixes four concerns in one linear `Vec<Slot>`:

1. active group structure,
2. remembered payload storage,
3. emitted node identity,
4. inactive/restorable group retention.

The rewrite must separate these concerns.

The new architecture is:

```text
Composer
  owns semantic lifecycle, recomposition scopes, retention decisions, node attach/detach policy

SlotTable V2 write/read APIs
  expose structural storage operations, not storage hacks

SlotTable V2
  owns active tree storage, payload storage, anchors, scope lookup, structural moves

RetentionManager
  owned by Composer; stores detached subtrees that should survive while inactive

Applier integration
  owns actual UI node create/attach/detach/remove commands
```

The most important rule:

> A gap, free slot, or spare capacity is never a dormant group. A dormant group is an explicit `DetachedSubtree` owned by the composer-side retention system.

This rewrite intentionally removes the old concepts:

```text
gap slots carrying group metadata
gap-restoration flags
last_start_was_gap
has_gap_children
stale group len as physical extent
SEARCH_BUDGET / EXTENDED_SEARCH_BUDGET rescue scans
step_back cursor repair
advance_after_node_read cursor repair
scope lookup by scanning all slots
```

---

## 2. Historical baseline and current-state observations

This section keeps the pre-V2 baseline for rationale only. The current repository implementation is the source of truth for active architecture decisions.

Relevant source URLs:

- `slot_table.rs`: https://docs.rs/cranpose-core/latest/src/cranpose_core/slot_table.rs.html
- `slot_storage.rs`: https://docs.rs/cranpose-core/latest/src/cranpose_core/slot_storage.rs.html
- `slot_backend.rs`: https://docs.rs/cranpose-core/latest/src/cranpose_core/slot_backend.rs.html
- `lib.rs`: https://docs.rs/cranpose-core/latest/src/cranpose_core/lib.rs.html
- `subcompose.rs`: https://docs.rs/cranpose-core/latest/src/cranpose_core/subcompose.rs.html
- repository README: https://github.com/samoylenkodmitry/Cranpose

Historical source-level observations:

- `slot_table.rs` describes the baseline implementation as gap-buffer based and claims support for gap-based slot reuse, anchors, group skipping, scope-based recomposition, and batch anchor rebuilds.
- The current `Slot` enum contains `Group`, `Value`, `Node`, and `Gap`. `Gap` preserves `group_key`, `group_scope`, and `group_len`, which makes a storage hole double as semantic retention state.
- `GroupFrame` stores physical `start` and `end`, and the source comments that those physical positions should eventually be phased out.
- `SlotTable::start` includes fast paths, parent-forced recomposition, gap conversion, group-to-gap conversion, limited sibling scans, recursive gap scans, and extended rescue scans.
- `trim_to_cursor` marks unreachable slots as gaps and intentionally keeps group length as physical extent rather than active subtree size.
- `SlotStorage::begin_group` returned a gap-restoration boolean; `Composer::with_group` checked that flag and forced recomposition.
- `SlotStorage` exposed cursor-repair methods such as `step_back` and `advance_after_node_read`.
- `begin_recranpose_at_scope` starts from a `ScopeId`; the current slot table finds a group by scanning slots for a matching scope.
- `SubcomposeState` already shows a cleaner lifecycle pattern: it owns active slots, reusable pools, slot compositions, precomposed nodes, and a reuse policy outside the main slot table.

---

## 3. Goals

### 3.1 Correctness goals

The slot table must have simple invariants that can be validated after every composition pass in debug builds.

Required correctness properties:

1. Active groups form one preorder forest.
2. Every group has an exact active subtree size.
3. Parent/child boundaries are exact and parent-bounded.
4. Payload ranges are exact and owned by a group.
5. Group identity is matched only among siblings of the current parent.
6. A detached group is not present in the active table.
7. Retained groups are explicit detached objects, not gaps.
8. Anchors resolve to active groups/payloads/nodes or fail cleanly.
9. Scope lookup is indexed, not a slot scan.
10. Node lifecycle is explicit: active, retained-detached, or disposed.

### 3.2 Architectural goals

- The slot table stores structure and payloads.
- The composer owns lifecycle decisions.
- The applier owns concrete node objects.
- Retention is policy-driven and explicit.
- Skipping and recomposition must not depend on stale physical lengths.
- Keyed sibling reordering must be deterministic.
- The first V2 implementation should optimize for clarity and invariants over raw speed.
- Later performance work may add chunking, packed arrays, or gap buffers internally, but those mechanisms must not leak into semantics.

### 3.3 API goals

- Replace the gap-restoration boolean with semantic `GroupStartKind`.
- Replace `finalize_current_group() -> bool` with `finish_group_body() -> FinishGroupResult` returning detached children.
- Remove `step_back` and `advance_after_node_read` from the storage trait.
- Add explicit detach/restore operations.
- Add storage validation methods for tests/debugging.
- Keep public user-facing `remember`, `useState`, `with_key`, and composable macro behavior as stable as possible, but allow internal APIs to break.

---

## 4. Non-goals

This rewrite does not need to preserve the old slot table internals.

Do not try to:

- keep semantic gap-slot behavior;
- keep old group length behavior;
- preserve old rescue-scan logic;
- preserve old experimental backend behavior;
- make the storage trait object-safe;
- optimize memory packing before correctness;
- keep undocumented internal debug output unchanged.

User-facing Cranpose APIs should remain mostly stable, but internal slot APIs can change aggressively.

---

## 5. Key concepts

### 5.1 Active group

A group currently present in the active composition tree.

### 5.2 Detached subtree

An owned, inactive subtree removed from the active table. It includes group records, payload records, node IDs, scope IDs, and anchor metadata needed to restore or dispose it.

Detached subtrees have no active parent.

### 5.3 Retained group

A detached subtree that the composer intentionally keeps alive because a retention policy said so.

Examples:

- tab pages that should keep remembered state;
- reusable lazy/subcompose items;
- precomposed content waiting to be activated.

### 5.4 Disposed group

A detached subtree that is dropped. Its payloads are dropped, effects clean up through `Drop`/effect state cleanup, anchors are invalidated, scopes are deactivated/unregistered, and nodes are removed from the applier.

### 5.5 Group key

A key used to match groups among siblings.

The old `Key = u64` can remain as the raw hash, but V2 should internally distinguish:

```rust
pub struct GroupKey {
    pub static_key: Key,
    pub explicit_key: Option<Key>,
    pub ordinal: u32,
}
```

`static_key` is usually the call-site key.  
`explicit_key` comes from `with_key` / list item keys.  
`ordinal` disambiguates duplicate unkeyed sibling calls under the same parent.

Long term, `with_key` should combine source-location key + user key instead of replacing source location with only the user hash.

---

## 6. Target module layout

The current slot table implementation uses this layout:

```text
crates/cranpose-core/src/
  retention.rs                 // Composer-owned detached-subtree retention manager
  slot/
    mod.rs
    types.rs                   // semantic handles, cursors, and operation result types
    table.rs                   // SlotTable struct and high-level methods
    writer.rs                  // mutation/traversal state machine
    table/                     // SlotTable metadata, mutation, and value helpers
    writer/                    // writer state-machine helper modules
    reader.rs                  // read-only traversal and debug dumps
    groups.rs                  // GroupRecord and group-table helpers
    payload.rs                 // PayloadRecord storage
    payload_anchors.rs         // PayloadAnchorRegistry and value-slot resolution
    nodes.rs                   // node ranges / node identity helpers
    anchors.rs                 // AnchorRegistry
    scope_index.rs             // ScopeId -> AnchorId map
    detach.rs                  // DetachedSubtree and detach/restore helpers
    validate/                  // invariant checking
    lifecycle.rs               // deferred drop coordination
    debug.rs                   // public debug snapshot structs
```

`lib.rs` re-exports `SlotTable`, handle types, and public debug APIs from the active modules. There is no active alternate slot-table backend.

These old/experimental backends are not part of the active architecture:

```text
chunked_slot_storage.rs
hierarchical_slot_storage.rs
split_slot_storage.rs
```

---

## 7. Core data model

### 7.1 Handles

Use generational handles to avoid stale index bugs.

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ActiveGroupId {
    index: u32,
    generation: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ValueSlotId {
    anchor: PayloadAnchor,
    storage_id: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct AnchorId {
    id: u32,
    generation: u32,
}
```

`ActiveGroupId` is a transient active-table handle.
`AnchorId` is stable across moves and can be invalidated.
`ValueSlotId` is anchor-based because composer-held value handles routinely outlive one active writer frame. It also carries the owning slot-table storage id so stale or cross-table value handles fail in every build mode instead of aliasing another table. Group-and-offset addressing is still useful as a transient internal concept while writing one group body, but it is not robust as the exposed handle shape.

### 7.2 SlotTable

```rust
pub struct SlotTable {
    storage_id: usize,
    groups: Vec<GroupRecord>,
    payloads: Vec<PayloadRecord>,
    nodes: Vec<NodeRecord>,
    anchors: AnchorRegistry,
    payload_anchors: PayloadAnchorRegistry,
    scope_index: ScopeIndex,
    mutation_debug_stats: SlotTableMutationDebugStats,
    next_group_generation: u32,
}
```

`SlotTable` owns active storage and identity registries only. Runtime ownership,
scope registries, retention maps, and live-host routing live outside the table.

### 7.3 GroupRecord

Use a flat preorder group table with exact active sizes.

```rust
pub struct GroupRecord {
    pub key: GroupKey,
    pub parent_anchor: AnchorId,
    pub depth: u32,

    // Exact count of active groups in this subtree, including self.
    pub subtree_len: u32,

    // Payloads directly owned by this group.
    pub payload_start: u32,
    pub payload_len: u32,

    // Direct nodes emitted by this group.
    pub node_start: u32,
    pub node_len: u32,

    // Aggregate node count in subtree, for skip and attach operations.
    pub subtree_node_count: u32,

    pub anchor: AnchorId,
    pub scope_id: Option<ScopeId>,
    pub flags: GroupFlags,
    pub generation: u32,
}
```

`subtree_len` must always mean exact active group count. It must never mean “physical extent including old gaps.”

### 7.4 PayloadTable

```rust
pub struct PayloadTable {
    records: Vec<PayloadRecord>,
}

pub struct PayloadRecord {
    pub owner: AnchorId,
    pub anchor: PayloadAnchor,
    pub type_id: TypeId,
    pub kind: PayloadKind,
    pub value: Box<dyn Any>,
}

pub enum PayloadKind {
    Remember,
    Param,
    Return,
    Effect,
    Internal,
}
```

Remembered state stays in storage. Lifecycle restore is not storage-owned; the composer chooses whether a detached subtree survives.
Recomposition scopes are group metadata, not payload records.

### 7.5 NodeTable

Nodes are no longer full slots in the same sequence as groups and remembered values.

```rust
pub struct NodeTable {
    records: Vec<NodeRecord>,
}

pub struct NodeRecord {
    pub owner: AnchorId,
    pub node_id: NodeId,
    pub generation: u32,
}
```

Each group owns a range of directly emitted node records. The group also stores aggregate subtree node count for skip/reuse.

---

## 8. Slot write-session API

The writer/session surface exposes semantic operations directly on `SlotTable` sessions.

```rust
impl SlotWriteSession<'_> {
    // Groups
    fn begin_group(&mut self, input: BeginGroupInput<DetachedSubtree>) -> GroupStart<ActiveGroupId>;
    fn finish_group_body(&mut self) -> FinishGroupResult;
    fn end_group(&mut self);
    fn skip_group(&mut self);

    // Scopes
    fn set_group_scope(&mut self, group: ActiveGroupId, scope: ScopeId);
    fn begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<ActiveGroupId>;
    fn end_recompose(&mut self);

    // Values
    fn value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> ValueSlotId;
    fn read_value<T: 'static>(&self, slot: ValueSlotId) -> &T;
    fn read_value_mut<T: 'static>(&mut self, slot: ValueSlotId) -> &mut T;
    fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T>;

    // Nodes
    fn record_node(&mut self, id: NodeId, generation: u32) -> NodeSlotUpdate;
    fn nodes_in_current_group(&self) -> Vec<NodeId>;

    // Lifecycle/debug
    fn validate(&self) -> Result<(), SlotInvariantError>;
    fn debug_snapshot(&self) -> SlotDebugSnapshot;
}
```

Explicit detach/restore stays as concrete slot-table behavior instead of trait surface: `finish_group_body()` yields detached children, and `begin_group(BeginGroupInput { restored: Some(subtree), .. })` restores at the current cursor.

These cursor-repair methods are not part of the active API:

```rust
peek_node
advance_after_node_read
step_back
finalize_current_group -> bool
flush as anchor-rebuild workaround
```

Writer finalization only applies queued structural maintenance; it is not a repair path for stale identities.

### 8.1 BeginGroupInput

```rust
pub struct BeginGroupInput {
    pub key: GroupKey,
    pub restored: Option<DetachedSubtree>,
}
```

The composer passes `restored`. The slot table does not search dormant storage to decide restoration.

### 8.2 GroupStart

```rust
pub struct GroupStart<G> {
    pub group: G,
    pub anchor: AnchorId,
    pub kind: GroupStartKind,
    pub scope_id: Option<ScopeId>,
}

pub enum GroupStartKind {
    Inserted,
    Reused,
    Moved,
    Restored,
}
```

`Restored` means the composer explicitly supplied a detached subtree and the storage inserted it. It does not mean “found a gap.”

### 8.3 FinishGroupResult

```rust
pub struct FinishGroupResult {
    pub detached_children: Vec<DetachedSubtree>,
    pub structure_changed: bool,
    pub direct_nodes: Vec<NodeId>,
    pub subtree_nodes: Vec<NodeId>,
}
```

This replaces `finalize_current_group() -> bool`.

---

## 9. Writer model

All mutation goes through a writer state. No mutation should happen by random helper methods that patch the cursor.

```rust
pub struct WriterState {
    cursor: usize,              // group index where next child is expected
    payload_cursor: usize,      // next payload offset within current group
    node_cursor: usize,         // next direct node offset within current group
    stack: Vec<Frame>,
}

pub struct Frame {
    group_index: usize,
    parent_anchor: AnchorId,
    old_end: usize,
    next_child: usize,
    payload_start: usize,
    payload_cursor: usize,
    node_start: usize,
    node_cursor: usize,
    sibling_index: Option<SiblingIndex>,
}
```

The current frame says:

> We are composing this group. The next emitted child must match at `next_child`, be moved from a later sibling position, be restored from a composer-provided detached subtree, or be inserted new.

### 9.1 Begin group algorithm

```rust
fn begin_group(&mut self, input: BeginGroupInput) -> GroupStart<ActiveGroupId> {
    let parent = self.current_group();
    let expected = self.frame().next_child;

    if let Some(restored) = input.restored {
        let group = self.insert_detached_at(expected, restored);
        self.open_frame(group, GroupStartKind::Restored);
        return GroupStart::restored(group);
    }

    if self.group_at(expected).matches_parent_and_key(parent, input.key) {
        let group = self.group_id_at(expected);
        self.open_frame(group, GroupStartKind::Reused);
        return GroupStart::reused(group);
    }

    if let Some(found) = self.find_later_sibling(parent, input.key, expected) {
        self.move_subtree(found, expected);
        let group = self.group_id_at(expected);
        self.open_frame(group, GroupStartKind::Moved);
        return GroupStart::moved(group);
    }

    let group = self.insert_new_group(expected, input.key);
    self.open_frame(group, GroupStartKind::Inserted);
    GroupStart::inserted(group)
}
```

Hard constraints:

- `find_later_sibling` must only inspect siblings of the current parent.
- It must not scan into grandchildren.
- It must not scan beyond the parent range.
- It must not search retained/detached groups. Composer owns retained lookup.
- There must be no fixed rescue-search budget.

### 9.2 Sibling index

For small sibling counts, linear parent-bounded scan is fine. For larger siblings, build a lazy per-frame index.

```rust
pub struct SiblingIndex {
    by_key: HashMap<GroupKey, SmallVec<[usize; 2]>>,
}
```

Build it from the old child range `[next_child, old_end)` and only include direct children.

### 9.3 End group algorithm

```rust
fn finish_group_body(&mut self) -> FinishGroupResult {
    let frame = self.current_frame();
    let detached = self.detach_range(frame.next_child, frame.old_end);
    self.close_payloads(frame.group_index, frame.payload_cursor);
    self.close_nodes(frame.group_index, frame.node_cursor);
    self.recompute_exact_sizes(frame.group_index);
    FinishGroupResult { detached_children: detached, ... }
}

fn end_group(&mut self) {
    let frame = self.stack.pop().expect("unbalanced group");
    self.advance_parent_cursor_past(frame.group_index);
}
```

`finish_group_body` removes unvisited old children from the active table and returns them to the composer. It does not decide retain vs dispose.

---

## 10. Detach and restore

### 10.1 DetachedSubtree

```rust
pub struct DetachedSubtree {
    pub root_key: GroupKey,
    pub groups: Vec<GroupRecord>,
    pub payloads: Vec<PayloadRecord>,
    pub nodes: Vec<NodeRecord>,
    pub root_nodes: Vec<NodeId>,
    pub scope_ids: Vec<ScopeId>,
    pub anchors: DetachedAnchorSet,
    pub generation: u64,
}
```

A detached subtree owns its remembered payloads. Dropping a detached subtree disposes remembered values and effect state.

### 10.2 Detach operation

```rust
fn detach_subtree(&mut self, root_index: usize) -> DetachedSubtree {
    let len = self.groups[root_index].subtree_len as usize;
    let group_range = root_index..root_index + len;

    let groups = self.groups.drain(group_range).collect();
    let payloads = self.extract_payloads_for_groups(&groups);
    let nodes = self.extract_nodes_for_groups(&groups);

    self.anchors.mark_detached(&groups);
    self.scopes.mark_detached(&groups);
    self.repair_indices_after_remove(root_index, len);

    DetachedSubtree { groups, payloads, nodes, ... }
}
```

### 10.3 Restore operation

Restore is initiated by composer retention lookup plus `begin_group(BeginGroupInput { restored: Some(subtree), .. })`. The composer preflights the target cursor, expected root key, detached anchors, node lifecycle, root spans, and scope-index availability before the retained subtree is removed from retention. `SlotTable::restore_subtree` repeats the local preflight before mutating active storage, then reparents the restored root, inserts groups/payloads/nodes at the current cursor, marks anchors and scopes active again, marks retained nodes active, and returns `GroupStartKind::Restored`.

Storage restores bytes/records. Composer reactivates scopes and reattaches nodes.

---

## 11. Composer-owned runtime state and retention

Add a new file:

```text
crates/cranpose-core/src/retention.rs
```

### 11.1 Runtime-state and retention API

```rust
pub enum RetentionMode {
    DisposeWhenInactive,
    RetainWhenInactive,
}

pub(crate) struct ComposerRuntimeState {
    scope_registry: RefCell<HashMap<ScopeId, RecomposeScope>>,
    retention_by_host: RefCell<HashMap<usize, RetentionManager>>,
    live_hosts: RefCell<HashMap<usize, Weak<SlotsHost>>>,
}

pub(crate) struct RetainKey {
    pub parent_scope: Option<ScopeId>,
    pub key: GroupKey,
}

pub(crate) struct RetainedGroup {
    pub subtree: DetachedSubtree,
}

pub struct RetentionBudget {
    pub max_retained_subtrees: Option<usize>,
    pub max_retained_bytes: Option<usize>,
    pub max_age_passes: Option<u64>,
}

pub enum RetentionEvictionPolicy {
    LeastRecentlyDetached,
    LeastRecentlyRestored,
    LargestFirst,
}

pub struct RetentionPolicy {
    pub budget: RetentionBudget,
    pub eviction: RetentionEvictionPolicy,
}

pub(crate) struct RetentionManager {
    groups: HashMap<RetainKey, RetainedGroup>,
}
```

`ComposerRuntimeState` is the semantic owner shared by every composer operating on the same composition family, including subcompose passes that use their own `SlotsHost`.

Rules:

- scopes are registered in `scope_registry`;
- retained subtrees are stored per host storage key in `retention_by_host`;
- `RetentionBudget` uses `None` to mean unbounded and otherwise caps retained subtree count, retained heap bytes, or retained age in composition passes;
- `RetentionEvictionPolicy` names the ordering used when a bounded retention manager has to dispose inactive retained subtrees;
- bounded retention insertion returns evicted `DetachedSubtree` values to the composer so normal disposal paths remove scopes, anchors, payloads, and detached nodes;
- retained diagnostics must report retained subtree/group/payload/node/scope/anchor counts, estimated retained heap bytes, and cumulative eviction count;
- live hosts are resolved through `live_hosts` during recomposition;
- `SlotsHost` owns the runtime-state binding for a table and `ComposerRuntimeState` owns live-host registration; `SlotTable` never carries a runtime-state pointer;
- `SlotsHost::into_table()` and `SlotsHost::reset()` drain retained subtrees and clear host ownership before storage is transferred or replaced;
- a host whose previous runtime owner is gone may be rebound to a new runtime state; a host with a live applier owner rejects mismatched runtime binding;
- every `RecomposeScope` stores both `slots_storage_key` and a weak `slots_runtime_state`, which is the data needed to route invalid scopes back to the correct host.

### 11.2 ComposerCore additions

```rust
pub(crate) struct ComposerCore {
    shared_state: Rc<ComposerRuntimeState>,
    slots: Rc<SlotsHost>,
    pending_scope_options: RefCell<Option<RecomposeOptions>>,
    // other existing fields
}
```

`ComposerCore` does not own retention maps directly. It holds a shared runtime state and uses that for scope lookup, retained-subtree storage, and host resolution. This keeps root composition passes, nested groups, and subcompose passes on one semantic system without forcing `SlotsHost` to make retention policy decisions.

### 11.3 RecomposeOptions V2

Replace:

```rust
pub struct RecomposeOptions {
    pub force_reuse: bool,
    pub force_recompose: bool,
}
```

with:

```rust
pub struct RecomposeOptions {
    pub force_reuse: bool,
    pub force_recompose: bool,
    pub retention: RetentionMode,
}

impl Default for RecomposeOptions {
    fn default() -> Self {
        Self {
            force_reuse: false,
            force_recompose: false,
            retention: RetentionMode::DisposeWhenInactive,
        }
    }
}
```

`cranpose_with_reuse` should use `RetentionMode::RetainWhenInactive` unless explicitly overridden.

### 11.4 Composer with_group V2 flow

```rust
pub fn with_group_seed<R>(&self, key: GroupKeySeed, f: impl FnOnce(&Composer) -> R) -> R {
    let parent_scope = self.current_recranpose_scope();
    let options = self.pending_scope_options().take().unwrap_or_default();
    let parent_scope_id = parent_scope.as_ref().map(RecomposeScope::id);
    let host = self.active_slots_host();
    let reserved_key = self.with_slot_session_mut(|slots| slots.preview_group_key(key));

    let restored = self.core.shared_state.take_retained(
        &host,
        RetainKey {
            parent_scope: parent_scope_id,
            key: reserved_key,
        },
        |subtree| {
            self.with_slot_session_mut(|slots| {
                slots.assert_retained_restore_ready(reserved_key, subtree);
            });
        },
    );

    let GroupStart {
        group,
        anchor,
        scope_id,
        kind,
        ..
    } = self.with_slot_session_mut(|slots| {
        slots.begin_group(BeginGroupInput::new(reserved_key, restored))
    });

    let scope = if let Some(scope) = scope_id.and_then(|scope_id| self.scope_for_id(scope_id)) {
        scope
    } else {
        let scope = RecomposeScope::new(self.runtime_handle());
        self.core.shared_state.register_scope(&scope);
        self.with_slot_session_mut(|slots| slots.set_group_scope(group, scope.id()));
        scope
    };

    scope.reactivate();
    scope.set_group_anchor(anchor);
    scope.set_parent_scope(parent_scope);
    scope.set_retention_mode(options.retention);
    scope.set_slots_host(&host);

    if options.force_recompose || matches!(kind, GroupStartKind::Restored) {
        scope.force_recompose();
    } else if options.force_reuse {
        scope.force_reuse();
    }

    self.scope_stack().push(scope.clone());
    let result = self.observe_scope(&scope, || f(self));
    scope.mark_composed_once();

    let FinishGroupResult {
        detached_children,
        direct_nodes,
        ..
    } = self.with_slot_session_mut(|slots| slots.finish_group_body());
    self.dispose_detached_nodes(direct_nodes);
    self.handle_detached_children(Some(scope.id()), detached_children);

    self.scope_stack().pop();
    scope.mark_recomposed();
    self.with_slot_session_mut(|slots| slots.end_group());
    result
}
```

### 11.5 Scope behavior

When a retained inactive scope is invalidated:

- keep the scope invalid;
- do not enqueue active recomposition while it is inactive;
- preserve the scope's `slots_storage_key` and weak `slots_runtime_state`;
- when restored, call `scope.reactivate()` and `scope.force_recompose()`.

`RecomposeScope::invalidate` follows this rule directly: inactive retained scopes stay invalid but are not scheduled. `reactivate()` re-enqueues them if they are still invalid, and restored groups force one recomposition even if no new invalidation happened while detached.

### 11.6 Host binding and recomposition routing

Each pass binds a `SlotsHost` to exactly one `ComposerRuntimeState`. `Composer::with_slot_host_pass` rejects mismatched binding while the previous runtime still has a live applier owner. If a host outlives an already-dropped runtime owner, it can be rebound after retained state for that host is drained and the old host registration is cleared.

`Composition::process_invalid_scopes_filtered` resolves the host for each invalid scope in this order:

1. `scope.slots_runtime_state() -> host_for_storage_key(scope.slots_storage_key())`
2. composition root `composer_state.host_for_storage_key(scope.slots_storage_key())`
3. root slots host fallback

Scopes are grouped by resolved host and each group is recomposed with `Composer::new_with_shared_state(...)`, using the host's bound runtime state. This is the mechanism that keeps measure/subcompose composers and root recomposition on the same scope/retention graph.

---

## 12. Recomposition entry

### 12.1 ScopeIndex

Use a real active-scope index inside `SlotTable`:

```rust
pub struct ScopeIndex {
    active: HashMap<ScopeId, AnchorId>,
}
```

Active recomposition:

```rust
fn begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<ActiveGroupId> {
    self.group_for_scope(scope)
}
```

Detached scopes are not indexed in `SlotTable`. The slot table only answers active-group lookups. Detached scope routing is a runtime-state concern because retained subtrees are stored outside the active table and are partitioned by host storage key.

Retained-scope recomposition behavior:

- slot storage returns `None` for detached scopes because there is no active group;
- the scope itself keeps the invalid flag while inactive;
- restore later reactivates the scope and forces recomposition;
- `Composition` routes that restore to the correct host through `slots_runtime_state + slots_storage_key`.

Slot storage must not scan all groups for a scope.

---

## 13. Node lifecycle

V2 must distinguish three node states:

```rust
pub enum NodeLifecycle {
    Active,
    RetainedDetached,
    Disposed,
}
```

### 13.1 Record node

`record_node(id, generation)` records a node under the current group. It does not overwrite a slot and does not require `peek_node` / `step_back`. The generation comes from the applier-side stable-node arena and is part of stale-identity protection when node IDs are recycled.

```rust
pub enum NodeSlotUpdate {
    Reused { id: NodeId, generation: u32 },
    Inserted { id: NodeId, generation: u32 },
    Replaced {
        old_id: NodeId,
        old_generation: u32,
        new_id: NodeId,
        new_generation: u32,
    },
}
```

### 13.2 Detaching retained nodes

When a group is removed from the active tree and retained:

- remove its root node IDs from parent child lists;
- do not call `applier.remove(node_id)`;
- do not unmount the nodes;
- mark node IDs as retained in `RetentionManager`;
- preserve the node generation in the deferred-cleanup queue so a later cleanup pass does not dispose the detached node by accident.

The concrete mechanism is `Command::DetachChild`:

```rust
Command::DetachChild { parent_id, child_id }
```

`DetachChild`:

- removes the child from the parent's child list;
- calls `on_removed_from_parent`;
- bubbles layout/measure dirtiness on the parent;
- records `(child_id, generation)` in `DeferredChildCleanupQueue::preserve(...)`.

When restored:

- retention restore preflight runs against the active cursor before the subtree is removed from retention;
- `RetentionManager::take(...)` returns the subtree with nodes still marked `RetainedDetached`;
- `begin_group(... restored ...)` restores the recorded node IDs into the active table and `SlotTable::restore_subtree` marks them active;
- normal parent attach / insert / sync logic reattaches those IDs;
- do not create new nodes unless the composable emits a genuinely different node type/key.

### 13.3 Disposing nodes

When a group is removed and not retained:

- remove from parent;
- unmount;
- call `applier.remove(node_id)`;
- allow payload/effect drops.

### 13.4 Apply-path separation

The apply layer distinguishes disposal from retention by command type, not by a special retained-node query inside `sync_children`.

```rust
match command {
    Command::RemoveChild { .. } => apply_remove_child(..., deferred_cleanup),
    Command::DetachChild { .. } => {
        detach_child_from_parent(...)?;
        deferred_cleanup.preserve(child_id, generation);
    }
    _ => { /* other commands */ }
}
```

`RemoveChild` detaches and queues cleanup, which leads to unmount + `applier.remove(...)` if the node stays parentless.

`DetachChild` detaches and cancels cleanup for that `(NodeId, generation)` pair, so retained nodes stay live while hidden.

This is essential. Retaining only slot payloads while deleting nodes defeats node reuse.

---

## 14. Keying model

### 14.1 Default keying

For ordinary composable calls, the macro should produce a stable source-location key:

```rust
GroupKey {
    static_key: location_key(file!(), line!(), column!()),
    explicit_key: None,
    ordinal: sibling_ordinal_for_this_static_key,
}
```

### 14.2 Explicit keys

For list items or user-defined keyed content:

```rust
GroupKey {
    static_key: location_key(file!(), line!(), column!()),
    explicit_key: Some(hash_key(user_key)),
    ordinal: 0,
}
```

This means item identity is source-location + user key, not just user key.

### 14.3 Collision policy

`Key = u64` can collide. V2 keeps the public key type at `u64`, but source-location keys are deterministic hashes of file contents plus line and column rather than string allocation addresses. Test builds always assert that two different source locations do not resolve to the same key. Debug builds can enable the same registry with `CRANPOSE_LOCATION_KEY_DIAGNOSTICS=1`.

Debug registry shape:

```rust
#[cfg(debug_assertions)]
struct DebugKeyInfo {
    source: Option<&'static str>,
    line: Option<u32>,
    column: Option<u32>,
}
```

---

## 15. Backend strategy

Current `SlotBackendKind` exposes Baseline, Chunked, Hierarchical, and Split. For V2:

### Option A: simplify now

```rust
pub enum SlotBackendKind {
    Default,
}

pub enum SlotBackend {
    Default(SlotTable),
}
```

This is cleanest for a full rewrite.

### Option B: preserve enum surface but route to V2

```rust
pub enum SlotBackendKind {
    Baseline,
    Chunked,
    Hierarchical,
    Split,
}

pub enum SlotBackend {
    Table(SlotTable),
}

impl SlotBackend {
    pub fn new(_kind: SlotBackendKind) -> Self {
        Self::Table(SlotTable::new())
    }
}
```

Use Option B if external code already selects backend kind.

Do not preserve old backend internals.

---

## 16. Invariants and validation

Implement `SlotTable::validate()` and call it in debug tests after each operation.

Required checks:

1. Every active group index is valid.
2. Root groups have `parent = None`; child groups point to a parent whose range contains them.
3. `subtree_len` exactly spans descendants in preorder.
4. Sibling groups are contiguous inside parent range.
5. No child range overlaps another child range.
6. Payload ranges are within `PayloadTable`.
7. Payload owner anchors resolve to active groups.
8. Node ranges are within `NodeTable`.
9. Node owner anchors resolve to active groups.
10. Scope index maps every active `scope_id` to the correct group anchor.
11. Anchor registry resolves active anchors and rejects invalidated anchors.
12. Detached subtrees are not present in active `groups`.
13. Writer stack frames are balanced and inside active group ranges.
14. No two active groups under the same parent have the same full `GroupKey` unless their ordinal differs.
15. Retained subtree root has no active parent.

Validation should return structured errors:

```rust
pub enum SlotInvariantError {
    InvalidParent { group: ActiveGroupId },
    BadSubtreeLen { group: ActiveGroupId, expected: u32, actual: u32 },
    PayloadOutOfRange { group: ActiveGroupId },
    ScopeIndexMismatch { scope: ScopeId },
    AnchorMismatch { anchor: AnchorId },
    WriterFrameOutOfBounds,
    DuplicateSiblingKey { parent: Option<ActiveGroupId>, key: GroupKey },
}
```

---

## 17. Testing plan

### 17.1 Unit tests for slot storage

Create `crates/cranpose-core/src/slot/tests.rs`.

Required tests:

1. empty table validates;
2. first composition inserts root/group/value/node;
3. second identical composition reuses groups and values;
4. conditional child removed returns `DetachedSubtree`;
5. default removed child is disposed when composer chooses dispose;
6. retained child restores remembered value;
7. restored child gets `GroupStartKind::Restored`;
8. keyed sibling reorder preserves values;
9. unkeyed sibling ordinal preserves order semantics;
10. duplicate explicit key under same parent debug-fails or gets deterministic ordinal;
11. nested detach/restore preserves nested payloads and scopes;
12. active scope lookup is O(1) by index behavior, not scan-dependent;
13. detached scope is not recomposed by active-table entry;
14. moving group updates anchors;
15. deleting group invalidates anchors;
16. value type mismatch panics or returns typed error consistently;
17. `skip_group` advances by exact subtree size;
18. node list for skipped group is exact and stable;
19. retained nodes are not disposed;
20. disposed nodes are removed.

### 17.2 Composer integration tests

Required behavior tests:

1. `remember` survives normal recomposition.
2. `remember` resets when a conditional branch disappears with default retention.
3. `remember` survives when branch uses retain/reuse options.
4. switching tabs preserves tab state only when retention is requested.
5. switching tabs disposes state when retention is not requested.
6. list item reorder with explicit keys preserves item state.
7. list item reorder without explicit keys follows positional identity.
8. invalidating active scope recomposes that scope.
9. invalidating inactive retained scope marks dirty and recomposes on restore.
10. `DisposableEffect` cleanup runs on dispose but not on retain.
11. retained node IDs are reused on restore.
12. disposed node IDs are not reused unless applier explicitly reuses IDs.
13. subcompose keeps per-slot compositions and works with V2 storage.

### 17.3 Property tests

Add a model test module using generated operations:

```text
begin group
end group
remember value
record node
conditional include/exclude
move keyed sibling
retain child
dispose child
restore retained child
skip group
```

Compare `SlotTable` against a simple reference tree model.

Properties:

- active tree equals model tree;
- remembered values appear under the same retained identity;
- no duplicate active anchors;
- no active group also exists in retention manager;
- all invariants hold after every operation.

Use `proptest` if acceptable; otherwise write deterministic random tests with a fixed seed.

---

## 18. Implementation status

Slot Table V2 is the active implementation. The implementation has converged on stable semantic identities:

- `ActiveGroupId` is a transient active-table handle.
- `AnchorId` is the stable group identity.
- `PayloadAnchor` is the stable value-slot identity behind `ValueSlotId`.
- `ScopeIndex` owns active scope lookup.
- `DetachedSubtree` owns inactive retained branch state.
- `NodeSlotUpdate` makes node reuse, insertion, and replacement explicit.

Forward work should use the release checklist and a dedicated review roadmap for concrete findings. Do not add alternate slot-table backends, compatibility wrapper modules, or feature-flagged half-states.

---

## 19. Example API sketches

### 19.1 Slot session usage from composer

```rust
let host = self.active_slots_host();
let reserved_key = self.with_slot_session_mut(|slots| slots.preview_group_key(key));
let restored = self.core.shared_state.take_retained(
    &host,
    RetainKey {
        parent_scope,
        key: reserved_key,
    },
    |subtree| {
        self.with_slot_session_mut(|slots| {
            slots.assert_retained_restore_ready(reserved_key, subtree);
        });
    },
);

let start = self.with_slot_session_mut(|slots| {
    slots.begin_group(BeginGroupInput::new(reserved_key, restored))
});

match start.kind {
    GroupStartKind::Inserted => { /* create scope */ }
    GroupStartKind::Reused => { /* normal */ }
    GroupStartKind::Moved => { /* preserve state; maybe mark parent structural change */ }
    GroupStartKind::Restored => { /* reactivate scopes and force recompose */ }
}
```

### 19.2 Retain/dispose handling

```rust
fn handle_detached_children(&self, parent_scope: Option<ScopeId>, detached: Vec<DetachedSubtree>) {
    let host = self.active_slots_host();
    for subtree in detached {
        let mode = subtree
            .root_scope_id()
            .and_then(|scope_id| self.scope_for_id(scope_id))
            .map(|scope| scope.retention_mode())
            .unwrap_or_default();
        match mode {
            RetentionMode::RetainWhenInactive => {
                self.retain_detached_subtree_in_host(&host, parent_scope, subtree)
            }
            RetentionMode::DisposeWhenInactive => {
                self.dispose_detached_subtree_in_host(&host, subtree)
            }
        }
    }
}
```

### 19.3 Retain subtree

```rust
fn retain_detached_subtree_in_host(
    &self,
    host: &Rc<SlotsHost>,
    parent_scope: Option<ScopeId>,
    subtree: DetachedSubtree,
) {
    for scope_id in subtree.scope_ids() {
        if let Some(scope) = self.scope_for_id(scope_id) {
            scope.deactivate();
        }
    }

    for root in self.skipped_group_root_nodes(&subtree.node_ids()) {
        let parent_id = {
            let mut applier = self.borrow_applier();
            applier.get_mut(root).ok().and_then(|node| node.parent())
        };
        if let Some(parent_id) = parent_id {
            self.commands_mut().push(Command::DetachChild { parent_id, child_id: root });
        }
    }

    self.core.shared_state.insert_retained(
        host,
        RetainKey {
            parent_scope,
            key: subtree.root_key(),
        },
        subtree,
    );
}
```

### 19.4 Dispose subtree

```rust
fn dispose_detached_subtree_in_host(&self, host: &Rc<SlotsHost>, subtree: DetachedSubtree) {
    let scope_ids = subtree.scope_ids();
    self.dispose_scope_ids(&scope_ids);
    let roots = self.skipped_group_root_nodes(&subtree.node_ids());
    self.dispose_detached_nodes(roots);

    host.with_table_and_lifecycle_mut(|table, lifecycle| {
        table.invalidate_detached_subtree_anchors(&subtree);
        lifecycle.queue_subtree_disposal(subtree);
    });
}
```

---

## 20. Success criteria

The rewrite is successful when:

1. There is no gap-slot equivalent carrying group metadata.
2. There is no gap-restoration API.
3. There are no rescue scan budgets.
4. Group sizes are exact active subtree sizes.
5. Scope lookup is indexed.
6. Removed children are returned as `DetachedSubtree` objects.
7. Composer decides retain vs dispose.
8. Retained state survives restore.
9. Default removed state disposes cleanly.
10. Keyed reorder preserves state and nodes.
11. Unkeyed reorder follows positional semantics.
12. Subcompose still works with per-slot compositions.
13. Debug `validate()` passes after every core test operation.
14. The design can be explained without special cases like “gap children,” “preserved physical extent,” or “extended rescue search.”

---

## 21. Coding-agent hard rules

When implementing this rewrite:

1. Do not patch the current gap algorithm.
2. Do not preserve group metadata in free storage.
3. Do not scan the whole table to find a group by key.
4. Do not scan the whole table to find a scope.
5. Do not keep stale group lengths.
6. Do not expose cursor repair APIs.
7. Do not make retention automatic for every conditional branch.
8. Do not dispose retained nodes.
9. Do not retain disposed payloads.
10. Do not optimize by weakening invariants.

If stuck, implement the simple model first even if it is slower.

---

## 22. Minimal first passing implementation

For the first working PR, it is acceptable to have:

- plain `Vec<GroupRecord>`;
- plain `Vec<PayloadRecord>`;
- subtree movement by drain/insert;
- O(number of direct siblings) keyed search;
- no chunking;
- no packed arrays;
- no custom allocator;
- validation everywhere in tests.

It is not acceptable to have:

- semantic gaps;
- stale subtree lengths;
- global key rescue scans;
- storage-owned retention decisions.

---

## 23. Future optimizations

After V2 is correct:

1. Replace direct `Vec` subtree moves with a chunked sequence.
2. Pack `GroupRecord` fields into compact arrays.
3. Use gap-relative anchors internally, but keep semantics unchanged.
4. Add retained subtree LRU limits.
5. Add debug instrumentation for retained memory.
6. Add wider `Key128` storage only if the debug collision registry reports real source-location collisions.
7. Add specialized lazy-list retention policy.
8. Support no-std allocator-backed tables only after a concrete target and allocator API exist; the current workspace remains `std`/`Vec` backed.
