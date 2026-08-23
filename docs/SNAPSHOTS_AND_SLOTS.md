# Snapshots and Slot Table System: Internals Documentation

This document provides comprehensive documentation of the internals of the Snapshots and Slot Table system in cranpose, which forms the foundation of the composition runtime.

## Table of Contents

1. [Overview](#overview)
2. [Snapshot System](#snapshot-system)
3. [Slot Table System](#slot-table-system)
4. [Integration Points](#integration-points)
5. [Key Algorithms](#key-algorithms)
6. [Design Patterns](#design-patterns)

---

## Overview

The cranpose runtime is built on two fundamental subsystems:

- **Snapshot System**: Provides Multi-Version Concurrency Control (MVCC) for state isolation, conflict detection, and optimistic merging
- **Slot Table System**: Manages the composition tree structure, enabling efficient recomposition and structural preservation

These systems work together but serve distinct purposes:
- Snapshots manage **state values** (what data is visible)
- Slot tables manage **composition structure** (where data is stored in the UI tree)

---

## Snapshot System

### Architecture Overview

The snapshot system implements a sophisticated MVCC mechanism that allows:
- Isolated views of mutable state
- Concurrent modifications without locks
- Optimistic conflict detection and merging
- Efficient garbage collection of obsolete records

### Core Files

```
crates/cranpose-core/src/snapshot_v2/
├── mod.rs              - Main types and coordination
├── runtime.rs          - Global runtime state
├── mutable.rs          - Mutable snapshot implementation
├── readonly.rs         - Read-only snapshot implementation
├── nested.rs           - Nested snapshot support
├── global.rs           - Global snapshot
└── transparent.rs      - Transparent observer snapshots

Supporting files:
├── state.rs                          - State objects and records
├── snapshot_id_set.rs                - Optimized bit-set for IDs
├── snapshot_pinning.rs               - Snapshot GC pinning
├── snapshot_weak_set.rs              - Weak references to state
├── snapshot_double_index_heap.rs     - Heap for pinning
└── snapshot_state_observer.rs        - State observation
```

### Data Structures

#### SnapshotIdSet

An optimized immutable bit-set for tracking snapshot IDs with O(1) access for recent snapshots:

```rust
pub struct SnapshotIdSet {
    upper_set: u64,                    // IDs [lower_bound+64..lower_bound+127]
    lower_set: u64,                    // IDs [lower_bound..lower_bound+63]
    lower_bound: usize,                // Base offset
    below_bound: Box<[SnapshotId]>,    // Sorted array for older IDs
}
```

**Key Properties:**
- **Recent snapshots** (128 most recent): O(1) bit operations
- **Older snapshots**: O(log N) binary search
- **Immutable**: All modifications create new instances (copy-on-write)
- **Memory efficient**: Two 64-bit integers cover 128 IDs

**Operations:**
```rust
get(id)           // O(1) for recent, O(log N) for old
set(id)           // O(1) for recent, O(N) for old (copy-on-write)
or(other)         // Combine two sets
and_not(other)    // Set difference
lowest()          // Find minimum ID
```

#### StateRecord

The fundamental unit of state versioning - a linked list node containing one version of a state value:

```rust
pub struct StateRecord {
    snapshot_id: Cell<SnapshotId>,           // Which snapshot owns this
    tombstone: Cell<bool>,                   // Marked for deletion
    next: Cell<Option<Arc<StateRecord>>>,    // Chain to older records
    value: RwLock<Option<Box<dyn Any>>>,     // Type-erased value
}
```

**Record Chain Example:**
```
SnapshotMutableState<i32>
    head → [id=10, value=100, next] → [id=8, value=50, next] → [id=5, value=0, next] → None
           ↑                          ↑                         ↑
           Latest                     Older                     Oldest
```

**Special IDs:**
- `INVALID_SNAPSHOT` (SnapshotId::MAX): Marks records available for reuse
- Valid IDs: Used to determine visibility to each snapshot

#### SnapshotMutableState&lt;T&gt;

The primary state object that applications interact with:

```rust
pub struct SnapshotMutableState<T> {
    head: RwLock<Arc<StateRecord>>,          // Head of record chain
    policy: Arc<dyn MutationPolicy<T>>,      // Equality/merge policy
    id: ObjectId,                             // Unique object ID
    weak_self: Weak<Self>,                    // Self-reference for callbacks
    apply_observers: Vec<Box<dyn Fn()>>,     // Applied change observers
}
```

**Usage:**
```rust
let state = SnapshotMutableState::new(42, StructuralEqualityPolicy::new());
let value = state.read(&snapshot);  // Read with snapshot isolation
state.write(&snapshot, 100);         // Write creates new record
```

**MutationPolicy:**
Defines how values are compared and merged:
- `StructuralEqualityPolicy`: Uses `PartialEq`
- `ReferentialEqualityPolicy`: Uses `Arc` pointer equality
- Custom policies can implement three-way merging

#### MutableSnapshot

A snapshot that can track writes and be applied to its parent:

```rust
pub struct MutableSnapshot {
    state: SnapshotState,
    base_parent_id: SnapshotId,         // Parent ID when created
    nested_count: Cell<usize>,          // Active nested snapshots
    applied: Cell<bool>,                // Applied flag
}
```

**SnapshotState** (shared between snapshot types):
```rust
pub struct SnapshotState {
    id: Cell<SnapshotId>,
    invalid: RefCell<SnapshotIdSet>,                    // Invalid snapshot IDs
    pin_handle: Cell<PinHandle>,                        // Keep alive for GC
    disposed: Cell<bool>,
    read_observer: Option<ReadObserver>,                // Track reads
    write_observer: Option<WriteObserver>,              // Track writes
    modified: RefCell<HashMap<StateObjectId, (Arc<dyn StateObject>, SnapshotId)>>,
    pending_children: RefCell<HashSet<SnapshotId>>,
}
```

**The `modified` map** tracks all state objects written to in this snapshot:
- Key: `StateObjectId` (unique object identifier)
- Value: `(Arc<StateObject>, SnapshotId)` - the object and writer snapshot ID

### Snapshot Lifecycle

#### 1. Creation

```rust
// In runtime.rs
pub fn allocate_snapshot() -> (SnapshotId, SnapshotIdSet) {
    let id = next_snapshot_id.fetch_add(1);
    let invalid = open_snapshots.clone();
    open_snapshots.set(id);
    (id, invalid)
}
```

**Steps:**
1. Allocate new monotonically increasing ID
2. Capture current `open_snapshots` as the `invalid` set
3. Add new ID to `open_snapshots`
4. Pin the snapshot ID to prevent GC

**Why capture open_snapshots?**
Any snapshot currently open might write to state objects, so their writes should be invisible to this new snapshot until they're applied.

#### 2. Reading State

```rust
pub fn read(&self, snapshot: &dyn Snapshot) -> T {
    snapshot.record_read(self);  // Observer notification

    let head = self.head.read();
    let record = readable_record_for(
        &head,
        snapshot.id(),
        &snapshot.invalid()
    );

    // Read value from record
}
```

**Finding the readable record:**
```rust
fn readable_record_for(
    head: &Arc<StateRecord>,
    snapshot_id: SnapshotId,
    invalid: &SnapshotIdSet
) -> Arc<StateRecord> {
    let mut current = head.clone();
    let mut best: Option<Arc<StateRecord>> = None;

    loop {
        let id = current.snapshot_id.get();

        // Skip tombstones and invalid records
        if !current.tombstone.get()
            && id <= snapshot_id
            && !invalid.get(id) {

            // Keep highest valid ID ≤ snapshot_id
            if best.is_none() || id > best.as_ref().unwrap().snapshot_id.get() {
                best = Some(current.clone());
            }
        }

        match current.next.get() {
            Some(next) => current = next,
            None => break,
        }
    }

    best.expect("No readable record found")
}
```

**Key insight:** Walk the chain, skip invalid/tombstone records, return the record with the highest valid ID ≤ snapshot_id.

#### 3. Writing State

```rust
pub fn write(&self, snapshot: &dyn Snapshot, value: T) {
    snapshot.record_write(self);  // Observer + track in modified map

    let writable = self.writable_record(
        snapshot.id(),
        snapshot.reuse_limit()
    );

    *writable.value.write() = Some(Box::new(value));
}
```

**Creating/reusing writable records:**
```rust
fn writable_record(&self, snapshot_id: SnapshotId, reuse_limit: SnapshotId)
    -> Arc<StateRecord> {

    let head = self.head.read();

    // Fast path: reuse existing record with this snapshot's ID
    if head.snapshot_id.get() == snapshot_id {
        return head.clone();
    }

    // Try to reuse INVALID records below reuse_limit
    if let Some(reusable) = find_reusable_record(&head, reuse_limit) {
        reusable.snapshot_id.set(snapshot_id);
        reusable.tombstone.set(false);
        return reusable;
    }

    // Create new record and prepend to chain
    let new_record = Arc::new(StateRecord {
        snapshot_id: Cell::new(snapshot_id),
        tombstone: Cell::new(false),
        next: Cell::new(Some(head.clone())),
        value: RwLock::new(None),
    });

    *self.head.write() = new_record.clone();
    new_record
}
```

**Record reuse** is critical for performance - instead of creating infinite records, we reuse ones that are no longer visible to any snapshot.

#### 4. Applying (Merging)

The most complex operation - merging a child snapshot's changes into its parent:

```rust
pub fn apply(self) -> SnapshotApplyResult {
    // Collect all modified objects
    let modified = self.state.modified.borrow();

    for (obj_id, (state_obj, writer_id)) in modified.iter() {
        // 1. Find three records for three-way merge
        let applied = find_applied_record(state_obj, writer_id);
        let current = find_current_record(state_obj, parent_snapshot);
        let previous = find_previous_record(state_obj, base_parent_id);

        // 2. Detect conflicts
        let last_write_id = RUNTIME.last_writes.get(obj_id);

        if last_write_id != self.base_parent_id {
            // Another snapshot modified this object!

            // 3. Attempt merge
            match merge_records(previous, current, applied) {
                Some(merged) => {
                    // Merge succeeded
                    commit_merged_record(state_obj, merged);
                }
                None => {
                    // Merge failed - conflict!
                    return SnapshotApplyResult::Failure;
                }
            }
        } else {
            // No conflict - promote child's record
            promote_child_record(state_obj, applied);
        }
    }

    // 4. Update runtime state
    advance_global_snapshot();
    notify_observers();

    SnapshotApplyResult::Success
}
```

**Three-way merge visualization:**
```
Timeline:
t0: Create snapshot S1 (base_parent_id = G0)
    previous = state.read(G0) = "A"

t1: Snapshot S1 writes "B"
    applied = "B"

t2: Snapshot S2 writes "C" and applies
    current = state.read(G1) = "C"

t3: Snapshot S1 tries to apply
    previous = "A"
    current = "C"  (≠ previous, conflict detected!)
    applied = "B"

    Merge attempt: Can we merge "A" → "C" and "A" → "B"?
```

**Merge strategies** (from MutationPolicy):
- **PromoteChild**: No conflict (current == previous), use applied
- **PromoteExisting**: Merged value equals current, use current
- **CommitMerged**: Create new merged record

#### 5. Disposal

```rust
impl Drop for MutableSnapshot {
    fn drop(&mut self) {
        if !self.applied.get() {
            self.state.dispose();
        }
    }
}

fn dispose(&self) {
    self.disposed.set(true);

    // Release pin (allow GC)
    let pin = self.pin_handle.take();
    RUNTIME.release_pin(pin);

    // Close snapshot ID
    RUNTIME.close_snapshot(self.id.get());

    // Decrement parent nested count
    if let Some(parent) = self.parent {
        parent.decrement_nested();
    }

    // Trigger on_dispose callbacks
    for callback in &self.on_dispose {
        callback();
    }
}
```

### Garbage Collection (Record Reuse System)

**IMPORTANT**: This is NOT traditional garbage collection. Rust's `Arc` already provides automatic memory management. This system is about **record chain cleanup** and **reuse optimization**.

#### Why Needed in Rust (Not Just Copy-Paste from Kotlin)

**The Problem:**
```rust
let state = SnapshotMutableState::new(0);

// Without record reuse:
for i in 0..1000 {
    state.set(i);  // Creates new record each time
}
// Result: 1000 records in chain, even though only latest matters!
// Memory: ~64KB for records that will never be read
```

**Why Rust's Arc Doesn't Help:**
- **Arc keeps records alive**: Each record has `next: Cell<Option<Arc<StateRecord>>>`
- **Chain references prevent collection**: Head → Record1 → Record2 → Record3...
- **Arc only frees when refcount = 0**, but head always holds reference to entire chain
- **Without cleanup**: Infinite record chain growth = memory leak

**What This System Actually Does:**
1. **Identifies obsolete records**: Records older than `lowest_pinned_snapshot` can't be read
2. **Marks for reuse**: Set `snapshot_id = INVALID_SNAPSHOT` instead of dropping
3. **Reuses on next write**: `writable_record()` checks for INVALID records first
4. **Prevents chain growth**: Bounded memory regardless of write count

#### Real-World Impact

**Without record reuse** (hypothetical):
```rust
// UI counter that updates every frame (60 FPS)
let counter = SnapshotMutableState::new(0);

for frame in 0..3600 {  // 1 minute at 60 FPS
    counter.set(frame);
}

// Memory: 3600 records × 64 bytes = ~230 KB
// After 1 hour: ~13 MB just for one counter!
```

**With record reuse**:
```rust
// Same scenario, but records are reused
// Memory: ~3-10 records (bounded by concurrent snapshot count)
// Memory: ~200-640 bytes regardless of time
```

#### Actual Usage in Code

The cleanup runs automatically on every global write:

```rust
// state.rs:647
state.set(new_value);
  ↓
advance_global_snapshot(new_id);  // state.rs:647
  ↓
check_and_overwrite_unused_records_locked();  // global.rs:190
  ↓
EXTRA_STATE_OBJECTS.remove_if(|state| {
    state.overwrite_unused_records()  // Cleanup happens here
});
```

**Frequency:** Every write to global snapshot (most common case).

#### The Algorithm Explained

#### Kotlin vs Rust: Why Both Need This

**Kotlin (Original Compose):**
- JVM GC collects unreferenced objects automatically
- **Still needs record reuse** because record chains hold strong references
- JVM GC won't collect records still referenced by chain
- Same problem: unbounded chain growth without manual cleanup

**Rust (This Implementation):**
- `Arc` provides automatic reference counting
- **Same problem as Kotlin**: chain references prevent automatic cleanup
- `Arc` only drops when refcount = 0, but chain maintains references
- **Not a copy-paste bug**: Genuinely required for memory bounds

**Key Insight:** This isn't about memory safety (Rust guarantees that). It's about **memory efficiency**. Without this system, memory usage grows O(n) with write count instead of O(1).

#### Visual Example: Why Arc Alone Fails

```rust
// Initial state
state.head -> [id=1, value=0, next=None]
             Arc::strong_count = 1

// After state.set(10)
state.head -> [id=2, value=10, next] -> [id=1, value=0, next=None]
             Arc::strong_count = 1    Arc::strong_count = 1 ← Still alive!

// After state.set(20)
state.head -> [id=3, value=20, next] -> [id=2, value=10, next] -> [id=1, value=0, next=None]
                                        ↑ Can't drop: still referenced by id=3

// After 1000 writes: Chain of 1000 records, all kept alive by next pointers
// Arc can't help because references form a chain

// WITH record reuse:
state.head -> [id=1003, value=1000, next] -> [id=INVALID, reusable] -> [id=1, value=0, next=None]
             ↑ Latest                       ↑ Marked for reuse        ↑ PREEXISTING (kept)

// Next write reuses INVALID record instead of allocating
```

#### Pinning System

**Problem:** Records can only be reused if no snapshot can read them.

**Solution:** Track the lowest snapshot ID that might read each record:

```rust
pub struct SnapshotPinning {
    pins: DoubleIndexHeap<SnapshotId>,  // Min-heap of pinned IDs
}

pub fn pin_snapshot(id: SnapshotId) -> PinHandle {
    PINNING.pins.insert(id)
}

pub fn lowest_pinned_snapshot() -> SnapshotId {
    PINNING.pins.min().unwrap_or(SnapshotId::MAX)
}
```

**Reuse limit:** Records with `id < lowest_pinned_snapshot()` are safe to reuse.

#### Record Cleanup

```rust
fn overwrite_unused_records_locked(&self, reuse_limit: SnapshotId) {
    let head = self.head.read();
    let mut records_below: Vec<Arc<StateRecord>> = Vec::new();

    // 1. Find records below reuse limit
    let mut current = head.clone();
    loop {
        let id = current.snapshot_id.get();

        if id < reuse_limit && id != INVALID_SNAPSHOT {
            records_below.push(current.clone());
        }

        match current.next.get() {
            Some(next) => current = next,
            None => break,
        }
    }

    if records_below.len() <= 1 {
        return;  // Keep at least one historical record
    }

    // 2. Keep highest record below limit (most recent history)
    records_below.sort_by_key(|r| std::cmp::Reverse(r.snapshot_id.get()));
    let keep = records_below[0].clone();

    // 3. Mark others as INVALID, copy data to reuse
    for record in &records_below[1..] {
        let keep_value = keep.value.read().clone();
        *record.value.write() = keep_value;
        record.snapshot_id.set(INVALID_SNAPSHOT);
        record.tombstone.set(false);
    }
}
```

**Key insight:** Keep one historical record for potential rollback, mark older ones as INVALID for reuse.

### Nested Snapshots

Snapshots can be nested to create isolation boundaries:

```rust
let outer = take_mutable_snapshot();
{
    let inner = outer.take_nested_mutable_snapshot();
    // Modifications in inner are isolated from outer
    inner.apply()?;  // Merge into outer
}
outer.apply()?;  // Merge into global
```

**Nested tracking:**
- Parent tracks `nested_count` of active children
- Parent cannot apply while children are alive
- Children's `base_parent_id` points to parent's ID at creation time

---

## Slot Table System

### Architecture Overview

The active slot-table implementation is **Slot Table V2**, not the historical gap-buffer design.

It separates the runtime into distinct storage layers:
- **Active structure** lives in a preorder `Vec<GroupRecord>`.
- **Remembered payloads** live in a separate payload table.
- **Node identities** live in a separate node table.
- **Inactive preserved branches** live outside the active table as explicit `DetachedSubtree` values.

That gives the runtime much simpler semantics:
- There is no semantic `Gap` inside the active table.
- Retention is explicit detach/restore, not preserved free space.
- Scopes are resolved through an index, not by scanning all groups.
- All structural mutation goes through the writer session state.

The design source of truth is `docs/cranpose_slot_table_v2_design.md`.

### Core Files

```
crates/cranpose-core/src/
├── retention.rs                   - Detached-subtree retention bookkeeping
└── slot/
    ├── types.rs                   - Semantic handles, cursors, and operation result types
    ├── table.rs                   - SlotTable and write-session entry points
    ├── writer.rs                  - begin/finish/end/skip group traversal
    ├── table/                     - SlotTable metadata, mutation, and value helpers
    ├── writer/                    - Writer state-machine helper modules
    ├── groups.rs                  - GroupRecord helpers and child traversal
    ├── payload.rs                 - Payload storage and value-slot records
    ├── payload_anchors.rs         - Stable value-slot identity registry
    ├── nodes.rs                   - Node record storage and subtree extraction
    ├── anchors.rs                 - AnchorRegistry and anchor state tracking
    ├── scope_index.rs             - ScopeId -> active group lookup
    ├── detach.rs                  - DetachedSubtree extraction and restore
    ├── validate/                  - Structural invariant checking
    ├── debug.rs / reader.rs       - Debug snapshots and textual dumps
    └── lifecycle.rs               - Deferred payload disposal
```

### Data Structures

#### SlotTable

The active table stores groups, payloads, nodes, stable identity registries, and scope lookup separately:

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

**Key properties:**
- **Exact active structure**: `subtree_len` always means the active preorder span.
- **Stable addressing**: groups use stable `AnchorId` identities plus transient `ActiveGroupId` handles; values use anchor-based `ValueSlotId`.
- **Indexed scopes**: active scopes map directly to group anchors.
- **Explicit retention**: removed branches are detached from the table before any retain/dispose decision happens.

#### GroupRecord

Each group describes one active composable call:

```rust
pub struct GroupRecord {
    key: GroupKey,
    parent_anchor: AnchorId,
    depth: u32,
    subtree_len: u32,
    payload_start: u32,
    payload_len: u32,
    node_start: u32,
    node_len: u32,
    subtree_node_count: u32,
    generation: u32,
    anchor: AnchorId,
    scope_id: Option<ScopeId>,
}
```

`parent_anchor` stays stable even when active indexes shift. `subtree_len` and
`subtree_node_count` are validated against the actual preorder tree.

#### PayloadRecord and NodeRecord

Remembered values and emitted nodes are stored outside the structural group table:

```rust
pub struct PayloadRecord {
    owner: AnchorId,
    anchor: PayloadAnchor,
    type_id: TypeId,
    type_name: &'static str,
    kind: PayloadKind,
    value: Box<dyn Any>,
}

pub struct NodeRecord {
    owner: AnchorId,
    id: NodeId,
    parent_id: Option<NodeId>,
    generation: u32,
    lifecycle: NodeLifecycle,
}
```

Payload anchors back `ValueSlotId`, and `ValueSlotId` also stores the owning
slot-table storage id. Remembered values remain addressable when sibling
reordering moves the owning group in the active table, while stale cross-table
handles fail instead of aliasing another table.

#### DetachedSubtree

When a branch leaves the active table, storage returns an owned subtree:

```rust
pub struct DetachedSubtree {
    root_key: GroupKey,
    root_scope_id: Option<ScopeId>,
    groups: Vec<GroupRecord>,
    payloads: Vec<PayloadRecord>,
    nodes: Vec<NodeRecord>,
}
```

Detached subtrees carry remembered payloads, anchors, scope IDs, and node identities together.
The slot table itself does not decide whether they are retained or disposed.

### Core Operations

#### begin_group() - Begin Group

Writers match groups only among siblings of the current parent:

```rust
pub fn begin_group(&mut self, input: BeginGroupInput<DetachedSubtree>) -> GroupStart<ActiveGroupId> {
    if let Some(restored) = input.restored {
        return restore_started_group(input.key, restored);
    }

    if expected_sibling_matches(parent, input.key) {
        return GroupStartKind::Reused;
    }

    if let Some(found) = find_later_sibling(parent, input.key) {
        move_subtree(found, insert_index);
        return GroupStartKind::Moved;
    }

    insert_new_group(insert_index, parent, input.key);
    GroupStartKind::Inserted
}
```

There is no fixed global search budget and no recursive rescue scan into grandchildren. Large
sibling ranges build a temporary sibling index inside the active writer frame.

#### finish_group_body() / end_group()

At the end of a group body, the writer trims payloads and direct nodes that were not visited,
detaches unvisited child subtrees, and returns them for retain-or-dispose handling:

```rust
let finish = slots.finish_group_body();
composer.handle_detached_children(parent_scope, finish.detached_children);
slots.end_group();
```

This is where inactive branches become `DetachedSubtree` values. They are no longer present in
the active table after `finish_group_body`.

#### value_slot() / remember()

Remembered state is stored in the payload table and addressed by `ValueSlotId`:

```rust
let slot = slots.value_slot(|| SnapshotMutableState::new(0, policy));
let state = slots.read_value::<SnapshotMutableState<i32>>(slot);
```

If a payload slot is revisited with a different type, the old boxed value is dropped through the
slot lifecycle coordinator and the new typed payload replaces it in place.
Public composer-held value access uses typed value-slot handles; the old untyped
composer write surface is not part of the active API.

#### begin_recompose_at_scope()

Targeted recomposition starts from the indexed scope mapping:

```rust
if let Some(group) = slots.begin_recompose_at_scope(scope_id) {
    // group handle resolves through scope_anchor_to_group -> anchors -> groups
}
```

Detached scopes are intentionally absent from that index. They stay inactive until their retained
subtree is explicitly restored.

### Active-Table Invariants

The slot table validates a small set of invariants after mutations in debug/test builds:
- active groups form one valid preorder forest;
- every `subtree_len` and `subtree_node_count` matches the actual active subtree;
- payload and node ranges are contiguous and owner-correct;
- every active scope index entry resolves to the correct group anchor;
- retained subtree anchors are detached or invalidated, never active.
- debug stats report occupied invalidated anchor slots separately from reusable
  free anchor IDs for both group and payload anchor registries.

The active debug helpers are `SlotTable::validate()`, `SlotTable::debug_snapshot()`,
`SlotTable::debug_dump_groups()`, and `SlotTable::debug_dump_slot_entries()`.

---

## Integration Points

The snapshot and slot table systems integrate at several key points:

### 1. State Storage in Slots

State objects are stored in slot-table payload records and accessed through composer helpers:

```rust
composer.with_group(key, |composer| {
    let state = composer.remember(|| {
        SnapshotMutableState::new(0, policy)
    });
    let value = state.value();
});
```

**Key insight:** Slot table manages **where** state is stored, snapshots manage **what** values are visible.

### 2. Scope-Based Recomposition

Scopes are attached to groups during composition and later resolved through the active scope index:

```rust
let started = slots.begin_group(BeginGroupInput::new(group_key, restored));
slots.set_group_scope(started.group, scope_id);

// Later:
slots.begin_recompose_at_scope(scope_id);
```

**Snapshot integration:**
```rust
// Snapshot observer tracks which scopes read which state
let observer = SnapshotStateObserver::new(|scope_id| {
    invalidate_scope(scope_id);
});

snapshot.set_read_observer(Box::new(move |state_obj| {
    observer.observe_read(current_scope, state_obj);
}));
```

**Flow:**
1. Composition reads state → observer records `(scope, state_obj)` mapping
2. State changes → observer invalidates affected scopes
3. Active scope index resolves the group anchor → recompose at that group

Detached scopes are intentionally absent from the active scope index. When a retained subtree is
restored, its scope is reactivated and can re-enter normal recomposition.

### 3. Invalidation Tracking

```rust
pub struct SnapshotStateObserver {
    observations: HashMap<ScopeId, HashSet<StateObjectId>>,
    reverse: HashMap<StateObjectId, HashSet<ScopeId>>,
}

impl SnapshotStateObserver {
    pub fn observe_read(&mut self, scope: ScopeId, obj: StateObjectId) {
        self.observations.entry(scope).or_default().insert(obj);
        self.reverse.entry(obj).or_default().insert(scope);
    }

    pub fn notify_changed(&mut self, obj: StateObjectId) -> Vec<ScopeId> {
        self.reverse.get(&obj).cloned().unwrap_or_default().collect()
    }
}
```

**Recomposition flow:**
```rust
// 1. Apply snapshot changes
snapshot.apply()?;

// 2. Get invalidated scopes
let invalid_scopes = observer.notify_changed_objects(&changed_objects);

// 3. Recompose each scope
for scope in invalid_scopes {
    composer.recompose_group(scope);
}
```

### 4. Composition Context

The composition runtime coordinates slot passes, command application, and retention policy:

```rust
composition.render(root_key, || ui());
// inside:
// - SlotsHost begins a compose/recompose pass
// - SlotWriteSession mutates the active SlotTable
// - detached children become DetachedSubtree values
// - composer/host decide retain vs dispose
// - command queue applies node attach/move/remove work to the applier
```

---

## Key Algorithms

### Three-Way Merge Algorithm

Used when applying snapshots with concurrent modifications:

```rust
pub fn three_way_merge<T>(
    previous: &T,
    current: &T,
    applied: &T,
    policy: &dyn MutationPolicy<T>
) -> MergeResult<T> {
    // 1. No conflict case
    if policy.equivalent(current, previous) {
        return MergeResult::PromoteChild;
    }

    // 2. Identical modification case
    if policy.equivalent(applied, current) {
        return MergeResult::PromoteExisting;
    }

    // 3. Attempt custom merge
    if let Some(merged) = policy.merge(previous, current, applied) {
        // Check if merge equals current (no-op)
        if policy.equivalent(&merged, current) {
            return MergeResult::PromoteExisting;
        }
        return MergeResult::CommitMerged(merged);
    }

    // 4. Conflict
    MergeResult::Conflict
}
```

**Example merges:**

**Structural equality (integers):**
```rust
previous: 10
current:  15  (another snapshot wrote this)
applied:  20  (our snapshot wrote this)

merge:    Cannot merge conflicting integers → Conflict
```

**Set merge (additive):**
```rust
previous: {A, B}
current:  {A, B, C}  (another snapshot added C)
applied:  {A, B, D}  (our snapshot added D)

merge:    {A, B, C, D}  (union) → CommitMerged
```

**List merge (operational transform):**
```rust
previous: ["a", "b", "c"]
current:  ["a", "x", "b", "c"]  (inserted "x" at 1)
applied:  ["a", "b", "c", "y"]  (appended "y")

merge:    ["a", "x", "b", "c", "y"]  (apply both ops) → CommitMerged
```

### Sibling Matching and Movement

The V2 writer only matches direct siblings under the current parent:

```rust
pub fn begin_group(&mut self, input: BeginGroupInput<DetachedSubtree>) -> GroupStart<ActiveGroupId> {
    if let Some(restored) = input.restored {
        return restore_started_group(input.key, restored);
    }

    if expected_sibling_matches(parent, input.key) {
        return GroupStartKind::Reused;
    }

    if let Some(found) = find_later_sibling(parent, input.key) {
        move_subtree(found, insert_index);
        return GroupStartKind::Moved;
    }

    insert_new_group(insert_index, parent, input.key);
    GroupStartKind::Inserted
}
```

**Why it matters:**
- the search is parent-bounded;
- grandchildren are never treated as sibling matches;
- retention is not mixed into sibling search;
- the semantics depend on exact keys, not rescue budgets.

### Detach and Restore

Conditional structure changes become explicit subtree extraction and reinsertion:

```rust
pub fn detach_subtree(&mut self, anchor: AnchorId) -> DetachedSubtree {
    let groups = drain_group_range(root_index, subtree_len);
    let payloads = detach_payloads_for_groups(root_index, &mut groups);
    let nodes = detach_nodes_for_groups(root_index, &mut groups);
    clear_group_indexes(&groups);
    clear_scope_index_for_groups(&groups);
    DetachedSubtree { groups, payloads, nodes, .. }
}

pub fn restore_subtree(&mut self, insert_index: usize, subtree: DetachedSubtree) -> AnchorId {
    restore_payloads_for_groups(insert_index, &mut groups, subtree.payloads);
    restore_nodes_for_groups(insert_index, &mut groups, subtree.nodes);
    groups.splice(insert_index..insert_index, subtree.groups);
    recompute_all_metadata();
}
```

**Why it matters:**
- removed structure is no longer present in the active table;
- retained state is explicit and owner-controlled;
- restoring a retained subtree preserves remembered payloads, scopes, and node identities.

---

## Design Patterns

### Persistent/Immutable Data Structures (NOT True CoW)

**Used in:** SnapshotIdSet

**IMPORTANT CLARIFICATION:** The documentation claims "CoW" but this is **NOT true copy-on-write**. It's actually a **persistent/immutable data structure** with partial optimization.

**Actual Implementation:**
```rust
pub struct SnapshotIdSet {
    upper_set: u64,                           // Bit set (cheap to copy)
    lower_set: u64,                           // Bit set (cheap to copy)
    lower_bound: usize,                       // Cheap to copy
    below_bound: Option<Box<[SnapshotId]>>,  // FULL COPY on clone!
}

impl SnapshotIdSet {
    pub fn set(&self, id: SnapshotId) -> Self {
        // Returns NEW instance
        Self {
            upper_set: self.upper_set,
            lower_set: self.lower_set | mask,
            below_bound: self.below_bound.clone(),  // ⚠️ Box::clone() = full array copy!
        }
    }
}
```

**Usage Pattern (3 clones per snapshot!):**
```rust
// global.rs:141-143
let mut parent_invalid = self.state.invalid.borrow().clone();  // Clone 1
parent_invalid = parent_invalid.set(new_id);                   // Clone 2
self.state.invalid.replace(parent_invalid.clone());            // Clone 3
```

**What's Actually Happening:**
- ✅ **Immutable**: Can't modify in place (functional correctness)
- ✅ **Fast for recent IDs**: Only bit operations, no allocation
- ❌ **NOT true CoW**: `Box::clone()` does full array copy
- ❌ **Suboptimal**: Could use `Arc<[SnapshotId]>` for O(1) sharing

**True CoW Would Be:**
```rust
below_bound: Option<Arc<[SnapshotId]>>,  // Share via Arc
// .clone() would just bump refcount, no array copy
```

**Real-World Impact:**
- **Best case** (all recent IDs): 24 bytes copied, very fast ✅
- **Worst case** (100 old IDs): ~2.4 KB copied per snapshot creation ⚠️
- **Not dead code**: Works correctly, just not optimally

**Why It Works Despite Not Being True CoW:**
- Most snapshot IDs are recent (fit in bit sets)
- `below_bound` array is typically small or empty
- Correctness > performance (for now)

### Object Pool

**Used in:** StateRecord reuse

**Pattern:**
```rust
// Mark object as reusable
record.snapshot_id.set(INVALID_SNAPSHOT);

// Reuse later
if record.snapshot_id.get() == INVALID_SNAPSHOT {
    record.snapshot_id.set(new_id);
    record.value.write() = new_value;
    return record;
}
```

**Benefits:**
- Reduces allocation pressure
- Maintains stable Arc pointers
- Amortizes allocation cost

### Observer Pattern

**Used in:** Snapshot read/write tracking, invalidation

**Pattern:**
```rust
pub trait Snapshot {
    fn set_read_observer(&self, observer: Box<dyn Fn(&dyn StateObject)>);
    fn set_write_observer(&self, observer: Box<dyn Fn(&dyn StateObject)>);
}

// Usage
snapshot.set_read_observer(Box::new(|obj| {
    println!("Read: {:?}", obj.id());
    invalidation_tracker.record_read(current_scope, obj);
}));
```

**Benefits:**
- Decouple observation from core logic
- Enable multiple observation strategies
- Support transparent snapshots

### Strategy Pattern

**Used in:** MutationPolicy, slot write sessions

**Pattern:**
```rust
pub trait MutationPolicy<T> {
    fn equivalent(&self, a: &T, b: &T) -> bool;
    fn merge(&self, previous: &T, current: &T, applied: &T) -> Option<T>;
}

// Implementations
pub struct StructuralEqualityPolicy<T>(PhantomData<T>);
pub struct ReferentialEqualityPolicy<T>(PhantomData<T>);
pub struct NeverEqualPolicy<T>(PhantomData<T>);
```

**Benefits:**
- Customize state comparison logic
- Different merge strategies per type
- Extensible without modifying core

### Writer Frame Pattern

**Used in:** Slot table composition and recomposition sessions

**Pattern:**
```rust
pub(crate) struct SlotWriteSessionState {
    root: RootFrame,
    group_stack: Vec<GroupFrame>,
}

pub(in crate::slot) struct GroupFrame {
    group_anchor: AnchorId,
    old_children: Vec<AnchorId>,
    old_cursor: usize,
    next_child_index: usize,
    payload_cursor: usize,
    node_cursor: usize,
}
```

**Benefits:**
- Traversal state is scoped to the active writer session instead of living inside table storage
- Group reuse, movement, and restoration operate against sibling lists and anchors
- Composition code works through semantic `begin_group`/`finish_group_body`/`end_group` operations

---

## Performance Characteristics

### Snapshot System

| Operation | Time Complexity | Notes |
|-----------|----------------|-------|
| Create snapshot | O(1) amortized | Allocate ID, copy open set |
| Read state | O(R) | R = record chain length, typically small |
| Write state | O(1) amortized | Reuse or prepend record |
| Apply snapshot | O(M × R) | M = modified objects, R = record chain |
| GC record cleanup | O(R) | Per state object |
| SnapshotIdSet get | O(1) recent, O(log N) old | Recent = last 128 IDs |
| SnapshotIdSet set | O(1) recent, O(N) old | Copy-on-write |

**Optimization opportunities:**
- Keep record chains short via aggressive GC
- Use read-only snapshots when possible (no tracking overhead)
- Batch apply operations
- Use ReferentialEqualityPolicy for cheap equality checks

### Slot Table System

| Operation | Time Complexity | Notes |
|-----------|----------------|-------|
| `begin_group()` reuse | O(1) | Expected sibling already matches |
| `begin_group()` sibling search | O(D) | `D` = number of direct siblings examined |
| `move_subtree()` | O(G + P + N + T) | moved groups, payloads, nodes, and suffix metadata shifts |
| `finish_group_body()` | O(R + C) | trims direct payload/node tails and detaches remaining child subtrees |
| `detach_subtree()` | O(G + P + N + T) | extracts subtree records and repairs active indexes |
| `restore_subtree()` | O(G + P + N + T) | reinserts subtree records and recomputes metadata |
| `value_slot()` / `read_value()` | O(1) | payload anchor lookup plus owner-relative offset |
| `begin_recompose_at_scope()` | O(1) | scope index -> anchor -> active group |
| `validate()` | O(G + P + N + A + S) | full structural check in debug/test builds |

**Optimization opportunities:**
- Reduce temporary `Vec` cloning in detach/retention hot paths
- Profile subtree `Vec::drain` / `Vec::splice` costs before changing storage layout
- Add retained-memory instrumentation before pursuing more complex backends
- Keep validation strong while optimizing

### Memory Usage

**Snapshot system:**
- Each StateRecord: ~64 bytes (Arc, Cell, RwLock overhead)
- SnapshotIdSet: 24 bytes + 8 bytes per old ID
- MutableSnapshot: ~200 bytes + modified map

**Slot table:**
- Group storage, payload storage, and node storage scale independently
- Anchor and scope indexes add hash-map overhead on top of active records
- Retained branches consume separate detached subtree allocations while inactive

**Scaling:**
- 10,000 UI elements no longer imply one flat slot array
- 1,000 state objects with 10 records each: ~640 KB
- Typical app: 1-10 MB for composition runtime

---

## Common Scenarios

### Scenario 1: Simple State Update

```rust
// 1. Create state
let state = SnapshotMutableState::new(0, StructuralEqualityPolicy::new());

// 2. Read in current snapshot
let value = state.read(&current_snapshot);  // Returns 0

// 3. Create mutable snapshot
let snapshot = take_mutable_snapshot();

// 4. Write in snapshot
state.write(&snapshot, 42);

// 5. Apply snapshot
snapshot.apply()?;  // Merge into global

// 6. Read new value
let new_value = state.read(&current_snapshot);  // Returns 42
```

**Internals:**
1. State has single record: `[id=1, value=0]`
2. Write creates new record: `[id=2, value=42] → [id=1, value=0]`
3. Apply promotes child record to global visibility
4. Global snapshot now sees `id=2` as valid

### Scenario 2: Conditional Rendering (Tabs)

```rust
// Initial composition - Tab 1 active
composer.with_group(TAB_GROUP, |composer| {
    composer.cranpose_with_reuse(TAB_1, RecomposeOptions::default(), |composer| {
        // ... tab 1 content ...
    });
});

// Hide Tab 1
composer.with_group(TAB_GROUP, |_composer| {
    // tab 1 branch omitted
});

// Result:
// - finish_group_body() detaches Tab 1 as DetachedSubtree
// - retain policy keeps it outside the active SlotTable
// - its remembered payloads, scopes, and node IDs stay owned by the detached subtree

// Show Tab 1 again
composer.with_group(TAB_GROUP, |composer| {
    composer.cranpose_with_reuse(TAB_1, RecomposeOptions::default(), |composer| {
        // ... tab 1 content restored from DetachedSubtree ...
    });
});
```

**Key benefit:** Tab 1's state is preserved by an explicit retained subtree, not by hidden gap
metadata in the active table.

### Scenario 3: Concurrent Snapshot Conflict

```rust
// t0: Initial state
let state = SnapshotMutableState::new(10);

// t1: Create two snapshots
let snapshot1 = take_mutable_snapshot();  // base_parent_id = 1
let snapshot2 = take_mutable_snapshot();  // base_parent_id = 1

// t2: Snapshot1 writes
state.write(&snapshot1, 20);

// t3: Snapshot2 writes (concurrent)
state.write(&snapshot2, 30);

// t4: Snapshot1 applies first
snapshot1.apply()?;  // Success, now global = 20

// t5: Snapshot2 tries to apply
let result = snapshot2.apply();
// Conflict detected: last_write (snapshot1) != base_parent_id (global before)
// Merge attempt: previous=10, current=20, applied=30
// StructuralEqualityPolicy cannot merge integers
// Result: SnapshotApplyResult::Failure
```

**Handling conflicts:**
```rust
loop {
    let snapshot = take_mutable_snapshot();

    // Perform modifications
    state.write(&snapshot, new_value);

    // Try to apply
    match snapshot.apply() {
        SnapshotApplyResult::Success => break,
        SnapshotApplyResult::Failure => {
            // Retry with fresh snapshot
            continue;
        }
    }
}
```

### Scenario 4: Nested Snapshots (Transaction-like)

```rust
let outer = take_mutable_snapshot();

// Modify state
state1.write(&outer, 10);

{
    let inner = outer.take_nested_mutable_snapshot();

    // Inner can see outer's changes
    assert_eq!(state1.read(&inner), 10);

    // Inner modifications
    state2.write(&inner, 20);

    // Apply inner to outer (not global yet)
    inner.apply()?;
}

// Outer can now see inner's changes
assert_eq!(state2.read(&outer), 20);

// Apply outer to global
outer.apply()?;

// Both changes now visible globally
assert_eq!(state1.read(&global_snapshot), 10);
assert_eq!(state2.read(&global_snapshot), 20);
```

**Use case:** Atomic multi-state updates, rollback on failure.

---

## Future Optimizations

### Snapshot System

1. **Persistent Data Structures**: Replace record chains with persistent trees for O(log N) all operations
2. **Lock-Free Records**: Use atomic operations instead of RwLock for high-contention scenarios
3. **Compressed ID Sets**: Use roaring bitmaps for very large snapshot ID sets
4. **Lazy GC**: Defer record cleanup to background thread

### Slot Table System

1. **Chunked subtree storage**: Replace hot `Vec::drain` / `Vec::splice` paths only if profiling proves they dominate.
2. **Denser group storage**: Pack `GroupRecord` fields or split arrays only if cache pressure matters in real traces.
3. **Retained-memory diagnostics**: Add stronger retained-subtree and anchor-capacity telemetry for leak hunting.
4. **Parallel recomposition**: Only after current invariants, retention semantics, and applier-side node ownership stay deterministic.

---

## Debugging Tips

### Snapshot Debugging

**View record chain:**
```rust
fn debug_record_chain(state: &SnapshotMutableState<T>) {
    let head = state.head.read();
    let mut current = head.clone();

    loop {
        println!("Record {{ id: {}, tombstone: {}, value: {:?} }}",
            current.snapshot_id.get(),
            current.tombstone.get(),
            current.value.read().as_ref().map(|v| format!("{:?}", v))
        );

        match current.next.get() {
            Some(next) => current = next,
            None => break,
        }
    }
}
```

**Check snapshot visibility:**
```rust
fn is_visible_to_snapshot(
    record_id: SnapshotId,
    snapshot_id: SnapshotId,
    invalid: &SnapshotIdSet
) -> bool {
    record_id <= snapshot_id && !invalid.get(record_id)
}
```

### Slot Table Debugging

**Dump the active slot table:**
```rust
fn debug_print_slots(composition: &Composition) {
    for entry in composition.debug_dump_slot_entries() {
        println!("{}: {}", entry.path, entry.line);
    }
}
```

**Inspect structured debug state:**
```rust
fn debug_snapshot(composition: &Composition) {
    let snapshot = composition.debug_slot_snapshot();
    println!("{snapshot:#?}");
}
```

**Force validation in debug/test paths:**
```rust
assert_eq!(slot_table.validate(), Ok(()));
```

**Enable automatic pass dumps:**
```bash
COMPOSE_DEBUG_SLOT_TABLE=1 cargo test -p cranpose-core slot::tests
```

---

## Summary

The Snapshots and Slot Table system provides a sophisticated foundation for cranpose:

**Snapshots** deliver:
- Isolated state views via MVCC
- Optimistic concurrency with conflict detection
- Flexible merge strategies
- Efficient garbage collection

**Slot Tables** deliver:
- Exact active-tree storage with separate payload and node tables
- Stable anchors and indexed scopes for targeted recomposition
- Explicit detached-subtree retention instead of semantic gaps
- Strong validation and debug snapshots for structural correctness

**Together** they enable:
- Predictable recomposition
- State preservation across structural changes
- Concurrent snapshot modifications
- High-performance UI updates

This design mirrors Jetpack Compose's battle-tested architecture while leveraging Rust's ownership model for memory safety and performance.
