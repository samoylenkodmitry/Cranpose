# Cranpose Slot Table V2 — Full Rearchitecture Design Doc

Status: proposed full rewrite  
Target crate: `crates/cranpose-core`  
Primary files affected: `slot_table.rs`, `slot_storage.rs`, `slot_backend.rs`, `lib.rs`, `subcompose.rs`, tests  
Principle: do not patch the existing gap-based implementation; replace its model.

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

SlotStorage V2 trait
  exposes structural storage operations, not storage hacks

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
Slot::Gap with group metadata
restored_from_gap
last_start_was_gap
has_gap_children
stale group len as physical extent
SEARCH_BUDGET / EXTENDED_SEARCH_BUDGET rescue scans
step_back cursor repair
advance_after_node_read cursor repair
scope lookup by scanning all slots
```

---

## 2. Source snapshot and current-state observations

This design is based on the public Cranpose 0.1.0 source and docs available on docs.rs and GitHub.

Relevant source URLs:

- `slot_table.rs`: https://docs.rs/cranpose-core/latest/src/cranpose_core/slot_table.rs.html
- `slot_storage.rs`: https://docs.rs/cranpose-core/latest/src/cranpose_core/slot_storage.rs.html
- `slot_backend.rs`: https://docs.rs/cranpose-core/latest/src/cranpose_core/slot_backend.rs.html
- `lib.rs`: https://docs.rs/cranpose-core/latest/src/cranpose_core/lib.rs.html
- `subcompose.rs`: https://docs.rs/cranpose-core/latest/src/cranpose_core/subcompose.rs.html
- repository README: https://github.com/samoylenkodmitry/Cranpose

Current source-level observations:

- `slot_table.rs` describes the baseline implementation as gap-buffer based and claims support for gap-based slot reuse, anchors, group skipping, scope-based recomposition, and batch anchor rebuilds.
- The current `Slot` enum contains `Group`, `Value`, `Node`, and `Gap`. `Gap` preserves `group_key`, `group_scope`, and `group_len`, which makes a storage hole double as semantic retention state.
- `GroupFrame` stores physical `start` and `end`, and the source comments that those physical positions should eventually be phased out.
- `SlotTable::start` includes fast paths, parent-forced recomposition, gap conversion, group-to-gap conversion, limited sibling scans, recursive gap scans, and extended rescue scans.
- `trim_to_cursor` marks unreachable slots as gaps and intentionally keeps group length as physical extent rather than active subtree size.
- `SlotStorage::begin_group` returns `StartGroup { restored_from_gap: bool }`; `Composer::with_group` checks that flag and forces recomposition.
- `SlotStorage` currently exposes cursor-repair methods such as `step_back` and `advance_after_node_read`.
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

- Replace `restored_from_gap: bool` with semantic `GroupStartKind`.
- Replace `finalize_current_group() -> bool` with `finish_group_body() -> FinishGroupResult` returning detached children.
- Remove `step_back` and `advance_after_node_read` from the storage trait.
- Add explicit detach/restore operations.
- Add storage validation methods for tests/debugging.
- Keep public user-facing `remember`, `useState`, `with_key`, and composable macro behavior as stable as possible, but allow internal APIs to break.

---

## 4. Non-goals

This rewrite does not need to preserve the old slot table internals.

Do not try to:

- keep `Slot::Gap` semantics;
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

Replace the current slot table files with this layout:

```text
crates/cranpose-core/src/
  slot_storage.rs              // V2 trait and public-ish handle types
  slot_backend.rs              // simple wrapper around the new SlotTable, optional during V2
  slot/
    mod.rs
    types.rs                   // GroupId, ValueSlotId, GroupKey, flags, errors
    table.rs                   // SlotTable struct and high-level methods
    writer.rs                  // mutation/traversal state machine
    reader.rs                  // read-only traversal and debug dumps
    groups.rs                  // GroupRecord and group-table helpers
    payload.rs                 // PayloadRecord, PayloadTable
    nodes.rs                   // node ranges / node identity helpers
    anchors.rs                 // AnchorRegistry
    scope_index.rs             // ScopeId -> GroupAnchor map
    detach.rs                  // DetachedSubtree and detach/restore helpers
    validate.rs                // invariant checking
  retention.rs                 // Composer-owned retention manager
```

Update `lib.rs` to re-export `SlotTable`, `SlotStorage`, handles, and public APIs from the new modules.

During the rewrite, remove or temporarily disable these old/experimental backends:

```text
chunked_slot_storage.rs
hierarchical_slot_storage.rs
split_slot_storage.rs
```

Coding-agent instruction: do not preserve their internals. Either delete them or turn them into thin wrappers around `SlotTable` until V2 passes tests.

---

## 7. Core data model

### 7.1 Handles

Use generational handles to avoid stale index bugs.

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct GroupId {
    index: u32,
    generation: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ValueSlotId {
    group: GroupId,
    offset: u32,
    generation: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct GroupAnchor {
    id: u32,
    generation: u32,
}
```

`GroupId` is a transient active-table handle.  
`GroupAnchor` is stable across moves and can be invalidated.  
`ValueSlotId` should prefer anchor-based resolution when exposed outside one composition frame.

### 7.2 SlotTable

```rust
pub struct SlotTable {
    groups: Vec<GroupRecord>,
    payloads: PayloadTable,
    nodes: NodeTable,
    anchors: AnchorRegistry,
    scopes: ScopeIndex,
    writer: Option<WriterState>,
    version: u64,
}
```

The first implementation can use plain `Vec` and `Vec::splice` for subtree insert/remove/move. Optimize later.

### 7.3 GroupRecord

Use a flat preorder group table with exact active sizes.

```rust
pub struct GroupRecord {
    pub key: GroupKey,
    pub parent: Option<GroupId>,
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

    pub anchor: GroupAnchor,
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
    pub owner: GroupAnchor,
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
    Scope,
    Internal,
}
```

Remembered state stays in storage. Lifecycle restore is not storage-owned; the composer chooses whether a detached subtree survives.

### 7.5 NodeTable

Nodes are no longer full slots in the same sequence as groups and remembered values.

```rust
pub struct NodeTable {
    records: Vec<NodeRecord>,
}

pub struct NodeRecord {
    pub owner: GroupAnchor,
    pub node_id: NodeId,
    pub generation: u32,
}
```

Each group owns a range of directly emitted node records. The group also stores aggregate subtree node count for skip/reuse.

---

## 8. SlotStorage V2 trait

Replace the trait with semantic operations.

```rust
pub trait SlotStorage {
    type Group: Copy + Eq;
    type ValueSlot: Copy + Eq;

    // Groups
    fn begin_group(&mut self, input: BeginGroupInput) -> GroupStart<Self::Group>;
    fn finish_group_body(&mut self) -> FinishGroupResult;
    fn end_group(&mut self);
    fn skip_group(&mut self) -> SkippedGroup;

    // Explicit detach / restore
    fn detach_unvisited_children(&mut self) -> Vec<DetachedSubtree>;
    fn restore_detached_at_cursor(&mut self, subtree: DetachedSubtree) -> RestoreResult<Self::Group>;

    // Scopes
    fn set_group_scope(&mut self, group: Self::Group, scope: ScopeId);
    fn begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<RecomposeStart<Self::Group>>;
    fn end_recompose(&mut self);

    // Values
    fn value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Self::ValueSlot;
    fn read_value<T: 'static>(&self, slot: Self::ValueSlot) -> &T;
    fn read_value_mut<T: 'static>(&mut self, slot: Self::ValueSlot) -> &mut T;
    fn write_value<T: 'static>(&mut self, slot: Self::ValueSlot, value: T);
    fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T>;

    // Nodes
    fn record_node(&mut self, id: NodeId) -> NodeRecordResult;
    fn nodes_in_current_group(&self) -> Vec<NodeId>;

    // Lifecycle/debug
    fn reset(&mut self);
    fn validate(&self) -> Result<(), SlotInvariantError>;
    fn debug_snapshot(&self) -> SlotDebugSnapshot;
}
```

Remove these methods from the trait:

```rust
peek_node
advance_after_node_read
step_back
finalize_current_group -> bool
flush as anchor-rebuild workaround
```

If `flush` remains for compatibility, it should be a no-op or should only apply queued structural edits, not repair dirty anchors from hacks.

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
    pub anchor: GroupAnchor,
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
    parent: Option<GroupId>,
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
fn begin_group(&mut self, input: BeginGroupInput) -> GroupStart<GroupId> {
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

```rust
fn restore_detached_at_cursor(&mut self, subtree: DetachedSubtree) -> RestoreResult<GroupId> {
    let insert_at = self.writer.cursor;
    self.reparent_root(&mut subtree, self.current_group());
    self.insert_groups(insert_at, subtree.groups);
    self.insert_payloads(subtree.payloads);
    self.insert_nodes(subtree.nodes);
    self.anchors.mark_active(...);
    self.scopes.mark_active(...);
    self.repair_indices_after_insert(insert_at, len);
    RestoreResult { group, nodes, scopes }
}
```

Storage restores bytes/records. Composer reactivates scopes and reattaches nodes.

---

## 11. Composer-owned retention

Add a new file:

```text
crates/cranpose-core/src/retention.rs
```

### 11.1 Retention API

```rust
pub enum RetentionMode {
    DisposeWhenInactive,
    RetainWhenInactive,
}

pub struct RetainKey {
    pub parent_scope: Option<ScopeId>,
    pub key: GroupKey,
}

pub struct RetainedGroup {
    pub retain_key: RetainKey,
    pub subtree: DetachedSubtree,
    pub dirty: bool,
    pub retained_nodes: Vec<NodeId>,
    pub scope_ids: Vec<ScopeId>,
}

pub struct RetentionManager {
    groups: HashMap<RetainKey, RetainedGroup>,
    nodes: HashSet<NodeId>,
}
```

### 11.2 ComposerCore additions

```rust
pub(crate) struct ComposerCore {
    // existing fields
    retention: RefCell<RetentionManager>,
    scope_registry: RefCell<HashMap<ScopeId, RecomposeScope>>,
    current_group_options: RefCell<Vec<GroupOptionsFrame>>,
}
```

`scope_registry` lets the composer deactivate/reactivate scopes by ID without forcing `SlotTable` to store concrete `RecomposeScope` values as semantic metadata.

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
pub fn with_group<R>(&self, key: GroupKey, f: impl FnOnce(&Composer) -> R) -> R {
    let parent_scope = self.current_recranpose_scope().map(|s| s.id());
    let options = self.take_pending_or_default_group_options();
    let retain_key = RetainKey { parent_scope, key };

    let restored = self.core
        .retention
        .borrow_mut()
        .take(&retain_key)
        .map(|retained| retained.subtree);

    let start = self.with_slots_mut(|slots| {
        slots.begin_group(BeginGroupInput { key, restored })
    });

    let scope_ref = self.obtain_scope_for_started_group(&start);
    self.core.scope_registry.borrow_mut().insert(scope_ref.id(), scope_ref.clone());
    self.with_slots_mut(|slots| slots.set_group_scope(start.group, scope_ref.id()));

    self.prepare_scope(&scope_ref, &options, start.kind);
    self.push_scope(scope_ref.clone());

    let result = self.observe_scope(&scope_ref, || f(self));

    let finish = self.with_slots_mut(|slots| slots.finish_group_body());
    self.handle_detached_children(finish.detached_children, options.retention);

    self.pop_scope();
    scope_ref.mark_recomposed();
    self.with_slots_mut(|slots| slots.end_group());

    result
}
```

### 11.5 Scope behavior

When a retained inactive scope is invalidated:

- do not attempt slot-table recomposition by anchor;
- mark the retained group dirty;
- optionally schedule the nearest active ancestor/root;
- when restored, call `scope.reactivate()` and `scope.force_recompose()`.

The current `RecomposeScope::invalidate` already has a useful behavior: if inactive, it sets invalid state but does not enqueue active recomposition. V2 should keep that general rule and make retained restore reactivate the scope.

---

## 12. Recomposition entry

### 12.1 ScopeIndex

Use a real index:

```rust
pub struct ScopeIndex {
    active: HashMap<ScopeId, GroupAnchor>,
    detached: HashMap<ScopeId, RetainKey>,
}
```

Active recomposition:

```rust
fn begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<RecomposeStart<GroupId>> {
    let anchor = self.scopes.active.get(&scope)?;
    let group = self.anchors.resolve_group(*anchor)?;
    self.writer.start_recompose(group);
    Some(RecomposeStart { group, anchor: *anchor })
}
```

Detached recomposition:

- storage returns `None` for detached scopes;
- runtime/composer checks retention manager;
- retained entry is marked dirty;
- restore later forces recomposition.

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

`record_node(id)` records a node under the current group. It does not overwrite a slot and does not require `peek_node` / `step_back`.

```rust
pub struct NodeRecordResult {
    pub reused: bool,
    pub id: NodeId,
}
```

### 13.2 Detaching retained nodes

When a group is removed from the active tree and retained:

- remove its root node IDs from parent child lists;
- do not call `applier.remove(node_id)`;
- optionally call `on_removed_from_parent` / `unmount` if the renderer requires detached nodes to be inactive;
- mark node IDs as retained in `RetentionManager`.

When restored:

- the group records existing node IDs again;
- parent diff reattaches them;
- do not create new nodes unless the composable emits a genuinely different node type/key.

### 13.3 Disposing nodes

When a group is removed and not retained:

- remove from parent;
- unmount;
- call `applier.remove(node_id)`;
- allow payload/effect drops.

### 13.4 Parent diff integration

The current parent diff logic must learn about retained nodes. During child removal:

```rust
if retention.is_retained_node(child) {
    detach_child_from_parent_without_removing_applier_node(child);
} else {
    remove_child_and_dispose_node(child);
}
```

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

`Key = u64` can collide. V2 should initially accept this risk because current Cranpose already hashes to `u64`, but the data model should allow future `Key128` or debug collision assertions.

Suggested debug mode:

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
    InvalidParent { group: GroupId },
    BadSubtreeLen { group: GroupId, expected: u32, actual: u32 },
    PayloadOutOfRange { group: GroupId },
    ScopeIndexMismatch { scope: ScopeId },
    AnchorMismatch { anchor: GroupAnchor },
    WriterFrameOutOfBounds,
    DuplicateSiblingKey { parent: Option<GroupId>, key: GroupKey },
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

## 18. Coding-agent implementation plan

### Phase 0 — prepare branch and safety net

1. Create a rewrite branch.
2. Run current tests to get baseline failures/pass count.
3. Add a `docs/slot_table_v2.md` copy of this design.
4. Add a failing placeholder test: `slot_v2_empty_table_validates`.

### Phase 1 — define V2 types

Create:

```text
src/slot/mod.rs
src/slot/types.rs
src/slot_storage.rs
```

Implement:

- `GroupKey`
- `GroupId`
- `ValueSlotId`
- `GroupAnchor`
- `GroupStartKind`
- `BeginGroupInput`
- `GroupStart`
- `FinishGroupResult`
- `DetachedSubtree`
- `SlotInvariantError`
- V2 `SlotStorage` trait

Do not implement old trait compatibility.

### Phase 2 — implement core tables

Create:

```text
src/slot/table.rs
src/slot/groups.rs
src/slot/payload.rs
src/slot/nodes.rs
src/slot/anchors.rs
src/slot/scope_index.rs
src/slot/validate.rs
```

Implement minimal operations:

- `SlotTable::new`
- insert root/child groups
- value slots
- node records
- exact `subtree_len`
- validation

Tests to pass:

- empty table validates;
- simple group/value/node composition validates.

### Phase 3 — implement writer traversal

Create `src/slot/writer.rs`.

Implement:

- writer stack;
- `begin_group` insert/reuse;
- `end_group`;
- `finish_group_body` without retention;
- `skip_group`.

Tests to pass:

- identical recomposition reuses values;
- skipping advances exactly;
- removing a child returns a detached subtree.

### Phase 4 — implement sibling moves

Implement:

- parent-bounded direct-child scan;
- lazy `SiblingIndex` for larger sibling ranges;
- `move_subtree` using `Vec::splice` or drain/insert;
- anchor repair.

Tests to pass:

- keyed sibling reorder preserves state;
- nested children are not searched/moved as siblings;
- anchors survive moves.

### Phase 5 — implement detach/restore

Implement:

- `detach_subtree`;
- `detach_range`;
- `restore_detached_at_cursor`;
- payload extraction/insertion;
- node extraction/insertion;
- scope active/detached index updates;
- anchor active/detached/invalidated states.

Tests to pass:

- conditional removal returns valid detached subtree;
- restore recreates exact active subtree;
- nested restore preserves payloads;
- disposed subtree drops payloads and invalidates anchors.

### Phase 6 — composer retention manager

Create `src/retention.rs`.

Implement:

- `RetentionMode`;
- `RetainKey`;
- `RetainedGroup`;
- `RetentionManager`;
- retained node set;
- dirty retained scope tracking.

Update `ComposerCore`:

- add `retention`;
- add `scope_registry`;
- update `pending_scope_options` to include retention mode.

Tests to pass:

- default conditional branch disposes state;
- retain mode preserves remembered state;
- invalid retained scope recomposes when restored.

### Phase 7 — update Composer::with_group

Rewrite `with_group` around V2 semantics.

Required flow:

1. compute group key;
2. compute retain key;
3. take retained subtree if present;
4. call `begin_group` with optional restored subtree;
5. obtain/create remembered `RecomposeScope`;
6. register scope ID;
7. apply `force_recompose`, `force_reuse`, and `Restored` behavior;
8. run body under observer;
9. call `finish_group_body`;
10. retain or dispose returned children;
11. pop scope;
12. end group.

Delete all use of `restored_from_gap`.

### Phase 8 — update node/apply logic

Update parent diff/removal logic:

- retained nodes are detached, not removed from applier;
- disposed nodes are removed;
- restored retained nodes can be reattached by existing parent diff.

Tests to pass:

- retained tab nodes are not recreated;
- disposed conditional node is removed;
- moving keyed nodes reorders rather than destroys.

### Phase 9 — update subcompose

Keep the architectural idea already present in `SubcomposeState`: per-slot compositions and policy-owned reuse.

Update:

- `slot_compositions` to use V2 `SlotTable`;
- active/reusable slot registration to mark V2 retention if necessary;
- cleanup to dispose compositions not active/reusable.

Tests to pass:

- subcompose basic;
- lazy list scroll reuse;
- content-type-compatible reuse;
- precompose activation.

### Phase 10 — remove old implementation

Delete or fully replace:

```text
Slot::Gap
last_start_was_gap
has_gap_children
mark_range_as_gaps
trim_to_cursor old behavior
SEARCH_BUDGET
EXTENDED_SEARCH_BUDGET
SHRINK_MIN_DROP
SHRINK_RATIO
force_gap_here
ensure_gap_at_local
find_right_gap_run
step_back
advance_after_node_read
```

Search the repo for these names and remove them.

### Phase 11 — documentation and debug tools

Add:

- `SlotTable::debug_snapshot()`;
- `SlotDebugSnapshot` with active groups, retained counts, anchors, scopes;
- `COMPOSE_DEBUG_SLOT_TABLE=1` dump path;
- docs explaining retention vs disposal.

### Phase 12 — performance pass

Only after correctness tests pass:

- reduce unnecessary `Vec` cloning;
- use `SmallVec` for small node/payload lists;
- lazily build sibling index;
- benchmark keyed list reorder;
- benchmark tab switching;
- benchmark subcompose scrolling;
- consider chunked group storage if large `Vec::splice` appears hot.

Do not reintroduce semantic gaps for performance.

---

## 19. Example API sketches

### 19.1 SlotStorage usage from composer

```rust
let start = slots.begin_group(BeginGroupInput {
    key,
    restored: retained.map(|r| r.subtree),
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
fn handle_detached_children(&self, detached: Vec<DetachedSubtree>, mode: RetentionMode) {
    for subtree in detached {
        match mode {
            RetentionMode::RetainWhenInactive => self.retain_subtree(subtree),
            RetentionMode::DisposeWhenInactive => self.dispose_subtree(subtree),
        }
    }
}
```

### 19.3 Retain subtree

```rust
fn retain_subtree(&self, subtree: DetachedSubtree) {
    for scope_id in subtree.scope_ids() {
        if let Some(scope) = self.core.scope_registry.borrow().get(&scope_id) {
            scope.deactivate();
        }
    }

    for node in subtree.root_nodes() {
        self.core.retention.borrow_mut().mark_node_retained(node);
        self.detach_node_without_dispose(node);
    }

    let key = self.make_retain_key(&subtree);
    self.core.retention.borrow_mut().insert(key, subtree);
}
```

### 19.4 Dispose subtree

```rust
fn dispose_subtree(&self, subtree: DetachedSubtree) {
    for node in subtree.root_nodes() {
        self.dispose_node(node);
    }

    for scope_id in subtree.scope_ids() {
        self.core.scope_registry.borrow_mut().remove(&scope_id);
    }

    drop(subtree); // drops remembered payloads/effects
}
```

---

## 20. Success criteria

The rewrite is successful when:

1. There is no `Slot::Gap` equivalent carrying group metadata.
2. There is no `restored_from_gap` API.
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
6. Add collision-resistant keys in debug/profile builds.
7. Add specialized lazy-list retention policy.
8. Support no-std allocator-backed tables if still desired.

