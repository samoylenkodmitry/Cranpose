# Snapshots and Slot Table System: Internals Documentation

This document provides comprehensive documentation of the internals of the Snapshots and Slot Table system in rs-compose, which forms the foundation of the composition runtime.

## Table of Contents

1. [Overview](#overview)
2. [Snapshot System](#snapshot-system)
3. [Slot Table System](#slot-table-system)
4. [Integration Points](#integration-points)
5. [Key Algorithms](#key-algorithms)
6. [Design Patterns](#design-patterns)

---

## Overview

The rs-compose runtime is built on two fundamental subsystems:

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
crates/compose-core/src/snapshot_v2/
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

### Garbage Collection

The snapshot system includes sophisticated GC to reclaim memory from obsolete records.

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

The slot table manages the composition tree structure using a **gap-buffer** design that enables:
- Efficient reuse of structure during recomposition
- Preservation of state across conditional rendering
- Stable references via anchors
- Fast cursor-based traversal

### Core Files

```
crates/compose-core/src/
├── slot_table.rs                  - Main gap-buffer implementation (1666 lines)
├── slot_storage.rs                - Abstract storage trait
├── slot_backend.rs                - Backend selection and unified interface
├── chunked_slot_storage.rs        - Chunked storage backend
├── hierarchical_slot_storage.rs   - Hierarchical storage backend
└── split_slot_storage.rs          - Split layout/payload backend

docs/
└── slot_doc.md                    - Documentation
```

### Data Structures

#### SlotTable

The primary gap-buffer implementation:

```rust
pub struct SlotTable {
    slots: Vec<Slot>,                    // Linear array of slots
    cursor: usize,                       // Current position in traversal
    group_stack: Vec<GroupFrame>,        // Runtime group nesting stack
    anchors: Vec<usize>,                 // Anchor ID → position mapping
    anchors_dirty: bool,                 // Needs rebuild
    next_anchor_id: Cell<usize>,         // Allocate unique anchors
    last_start_was_gap: bool,            // Gap reuse tracking
}
```

**Key properties:**
- **Linear storage**: All slots in single `Vec<Slot>`
- **Cursor-based**: Most operations happen at cursor position
- **Gap preservation**: Unused structure marked as gaps, not removed
- **Anchors**: Stable references that survive reorganization

#### Slot Enum

Four variants representing different slot types:

```rust
pub enum Slot {
    /// Group - represents a composable function call
    Group {
        key: Key,                        // Unique key for identity
        anchor: AnchorId,                // Stable reference
        len: usize,                      // Physical extent (self + children)
        scope: Option<ScopeId>,          // For targeted recomposition
        has_gap_children: bool,          // Contains gaps (needs cleanup)
    },

    /// Value - remembered data
    Value {
        anchor: AnchorId,
        data: Box<dyn Any>,              // Type-erased value
    },

    /// Node - UI element reference
    Node {
        anchor: AnchorId,
        id: NodeId,                      // Reference to layout node
    },

    /// Gap - placeholder for conditionally absent structure
    Gap {
        anchor: AnchorId,
        group_key: Option<Key>,          // Preserved for reuse
        group_scope: Option<ScopeId>,    // Preserved scope
        group_len: usize,                // Preserved length
    },
}
```

**Gap metadata preservation** enables efficient structure reuse:
```rust
// Tab A active
Group { key: TabA, len: 100, scope: Some(123) }

// Tab B selected → Tab A deactivated
Gap { group_key: Some(TabA), group_len: 100, group_scope: Some(123) }

// Tab A selected again → Gap promoted back to Group
Group { key: TabA, len: 100, scope: Some(123) }  // Instant reactivation!
```

#### GroupFrame

Runtime stack entry tracking group nesting during composition:

```rust
pub struct GroupFrame {
    key: Key,                            // Group key
    start: usize,                        // Physical position in slots
    end: usize,                          // Physical end (start + len)
    force_children_recompose: bool,      // Force all children to recompose
}
```

**Stack usage:**
```rust
start(key) → push GroupFrame { key, start: cursor, ... }
    // ... nested composition ...
end()      → pop GroupFrame, update group len
```

#### Anchor System

Anchors provide stable references across slot reorganization:

```rust
pub struct AnchorId(usize);  // Opaque ID

// Allocation
let anchor = slot_table.allocate_anchor();

// Registration (during slot creation)
slot_table.register_anchor(anchor, position);

// Resolution (later access)
let position = slot_table.resolve_anchor(anchor)?;
```

**Why needed?**
Slots can move during gap insertion, group rescue, etc. External references (like scope tracking) use anchors to maintain stable references.

**Anchor updates** happen via:
- **Incremental**: `shift_anchor_positions_from(start, delta)` - adjust after local changes
- **Batch**: `rebuild_all_anchor_positions()` - full scan after major reorganization

### Core Operations

#### start(key) - Begin Group

Begins a new group, reusing structure when possible:

```rust
pub fn start(&mut self, key: Key) {
    // Fast path: cursor has matching group with no gaps
    if let Some(Slot::Group {
        key: slot_key,
        len,
        has_gap_children: false,
        ..
    }) = self.slots.get(self.cursor) {
        if *slot_key == key {
            // Perfect match - reuse directly
            let end = self.cursor + len;
            self.group_stack.push(GroupFrame {
                key,
                start: self.cursor,
                end,
                force_children_recompose: false,
            });
            self.cursor += 1;
            self.last_start_was_gap = false;
            return;
        }
    }

    // Check parent force-recompose flag
    let parent_forces = self.group_stack.last()
        .map(|f| f.force_children_recompose)
        .unwrap_or(false);

    if parent_forces {
        // Must search for matching group/gap
        if let Some(found_pos) = self.find_matching_group_or_gap(key) {
            if found_pos != self.cursor {
                self.move_group_to_cursor(found_pos);
            }

            // Convert gap to group if needed, set force flag
            self.ensure_group_at_cursor(key, true);
            // ... push frame with force_children_recompose: true
        } else {
            // Insert new group
            self.insert_new_group(key, true);
        }
        return;
    }

    // Slow path: search and rescue
    self.start_slow_path(key);
}
```

**Search budget:** 16 slots forward scan for matching groups/gaps.

**Group rescue** - When group found elsewhere:
```rust
fn move_group_to_cursor(&mut self, from: usize) {
    let group_len = self.slots[from].group_len();

    // Extract group and its children
    let group_slots: Vec<Slot> = self.slots
        .drain(from..from + group_len)
        .collect();

    // Insert at cursor
    self.slots.splice(
        self.cursor..self.cursor,
        group_slots
    );

    // Update anchors
    self.shift_anchor_positions_from(self.cursor, group_len as isize);

    // Update group stack frames
    for frame in &mut self.group_stack {
        if frame.start >= self.cursor && frame.start < from {
            frame.start += group_len;
            frame.end += group_len;
        }
    }
}
```

#### end() - End Group

Closes the current group and updates its length:

```rust
pub fn end(&mut self) {
    let frame = self.group_stack.pop()
        .expect("end() called without matching start()");

    let new_len = self.cursor - frame.start;
    let old_len = frame.end - frame.start;

    // Update group's len field
    if let Some(Slot::Group { len, has_gap_children, .. }) =
        self.slots.get_mut(frame.start)
    {
        // Shrink threshold: only update if change > 10% to reduce churn
        if new_len != old_len {
            let shrink_threshold = old_len / 10;
            if old_len > new_len + shrink_threshold || new_len > old_len {
                *len = new_len;
            }
        }

        // Mark if we have gaps (for later cleanup)
        if new_len < old_len {
            *has_gap_children = true;
        }
    }

    // Update parent frame's end
    if let Some(parent_frame) = self.group_stack.last_mut() {
        if parent_frame.end < self.cursor {
            parent_frame.end = self.cursor;
        }
    }
}
```

**Shrink threshold** (10%) prevents excessive updates when lengths fluctuate slightly.

#### use_value_slot&lt;T&gt;() - Allocate Value Slot

Allocates or reuses a value slot at the cursor:

```rust
pub fn use_value_slot<T: 'static>(&mut self) -> &mut T {
    let anchor = self.allocate_anchor();

    match self.slots.get_mut(self.cursor) {
        Some(Slot::Value { data, .. }) => {
            // Try to reuse if type matches
            if data.is::<T>() {
                self.cursor += 1;
                return data.downcast_mut::<T>().unwrap();
            }
            // Type mismatch - overwrite
        }
        Some(Slot::Gap { .. }) => {
            // Replace gap with value
        }
        _ => {
            // Insert new value
        }
    }

    // Create new value slot
    let value = Box::new(T::default());
    self.slots[self.cursor] = Slot::Value {
        anchor,
        data: value,
    };
    self.register_anchor(anchor, self.cursor);
    self.cursor += 1;

    self.update_parent_frame_end();

    self.slots[self.cursor - 1]
        .value_data_mut()
        .downcast_mut::<T>()
        .unwrap()
}
```

**Remember pattern:**
```rust
let state_slot = slot_table.use_value_slot::<SnapshotMutableState<i32>>();
if state_slot is empty {
    *state_slot = SnapshotMutableState::new(0, policy);
}
let value = state_slot.read(&snapshot);
```

#### mark_range_as_gaps() - Create Gaps

Converts a range of slots to gaps when structure becomes conditional:

```rust
pub fn mark_range_as_gaps(&mut self, range: Range<usize>) {
    for i in range.clone() {
        self.mark_slot_as_gap_recursive(i);
    }

    // Mark parent groups as having gap children
    self.mark_parents_have_gap_children(range.start);
}

fn mark_slot_as_gap_recursive(&mut self, index: usize) {
    match &self.slots[index] {
        Slot::Group { key, len, scope, anchor, .. } => {
            let key = *key;
            let len = *len;
            let scope = *scope;
            let anchor = *anchor;

            // Recursively mark children
            for child_idx in (index + 1)..(index + len) {
                self.mark_slot_as_gap_recursive(child_idx);
            }

            // Convert group to gap, preserving metadata
            self.slots[index] = Slot::Gap {
                anchor,
                group_key: Some(key),
                group_len: len,
                group_scope: scope,
            };
        }
        Slot::Value { anchor, .. } | Slot::Node { anchor, .. } => {
            // Convert to simple gap
            self.slots[index] = Slot::Gap {
                anchor: *anchor,
                group_key: None,
                group_len: 0,
                group_scope: None,
            };
        }
        Slot::Gap { .. } => {
            // Already a gap
        }
    }
}
```

**Metadata preservation** enables fast reactivation of conditional branches.

### Gap Management

Gaps are the key to efficient recomposition with conditional rendering.

#### Gap Buffer Strategy

**Core idea:** Don't delete unused structure, mark it as gaps and reuse later.

**Example: Tab switching**
```rust
Initial: [TabGroup, Tab1Group, ..., Tab2Group, ..., Tab3Group, ...]

Tab1 active:
[TabGroup, Tab1Group (active), Gap(Tab2), Gap(Tab3)]

Tab2 active:
[TabGroup, Gap(Tab1), Tab2Group (active), Gap(Tab3)]

Tab1 active again:
[TabGroup, Tab1Group (active), Gap(Tab2), Gap(Tab3)]
// Tab1Group restored from Gap instantly!
```

#### ensure_gap_at(cursor)

Ensures there's a gap at the cursor position for insertion:

```rust
pub fn ensure_gap_at(&mut self, position: usize) {
    if matches!(self.slots.get(position), Some(Slot::Gap { .. })) {
        return;  // Already a gap
    }

    // Try to find gaps elsewhere and move them here
    let scan_limit = position + LOCAL_GAP_SCAN;
    if let Some(gap_range) = self.find_right_gap_run(position, scan_limit) {
        self.rotate_gaps_to_cursor(gap_range, position);
        return;
    }

    // No nearby gaps - insert new gap (expensive)
    self.slots.insert(position, Slot::Gap {
        anchor: self.allocate_anchor(),
        group_key: None,
        group_len: 0,
        group_scope: None,
    });

    // Update all anchors and frames
    self.shift_anchor_positions_from(position, 1);
    self.shift_group_frames(position, 1);
}
```

**Local gap search** (budget: 256 slots):
```rust
fn find_right_gap_run(&self, from: usize, limit: usize) -> Option<Range<usize>> {
    let mut start = None;
    let mut end = None;

    for i in from..limit.min(self.slots.len()) {
        match &self.slots[i] {
            Slot::Gap { group_key: None, .. } => {
                // Found an INVALID gap (best for reuse)
                if start.is_none() {
                    start = Some(i);
                }
                end = Some(i + 1);
            }
            _ if start.is_some() => {
                // End of gap run
                break;
            }
            _ => {}
        }
    }

    start.and_then(|s| end.map(|e| s..e))
}
```

**Rotate gaps** (move gaps from elsewhere to cursor):
```rust
fn rotate_gaps_to_cursor(&mut self, gap_range: Range<usize>, to: usize) {
    let gap_count = gap_range.len();

    // Extract gaps
    let gaps: Vec<Slot> = self.slots.drain(gap_range.clone()).collect();

    // Insert at cursor
    self.slots.splice(to..to, gaps);

    // Update anchors (complex shifting logic)
    self.rebuild_all_anchor_positions();
}
```

**Rotation budget:** `MAX_ROTATE_WINDOW` = 4096 slots to prevent O(n²) behavior.

### Anchor Management

Anchors provide stable references that survive slot reorganization.

#### Allocation and Registration

```rust
pub fn allocate_anchor(&self) -> AnchorId {
    let id = self.next_anchor_id.get();
    self.next_anchor_id.set(id + 1);
    AnchorId(id)
}

pub fn register_anchor(&mut self, anchor: AnchorId, position: usize) {
    let id = anchor.0;

    // Grow anchors vec if needed
    if id >= self.anchors.len() {
        self.anchors.resize(id + 1, usize::MAX);
    }

    self.anchors[id] = position;
}
```

#### Resolution

```rust
pub fn resolve_anchor(&self, anchor: AnchorId) -> Option<usize> {
    let id = anchor.0;
    if id < self.anchors.len() {
        let pos = self.anchors[id];
        if pos != usize::MAX {
            return Some(pos);
        }
    }
    None
}
```

#### Updates After Reorganization

**Incremental shift** (for local changes):
```rust
pub fn shift_anchor_positions_from(&mut self, start: usize, delta: isize) {
    for position in &mut self.anchors {
        if *position >= start && *position != usize::MAX {
            *position = (*position as isize + delta) as usize;
        }
    }
}
```

**Batch rebuild** (after major reorganization):
```rust
pub fn rebuild_all_anchor_positions(&mut self) {
    // Reset all anchors
    self.anchors.fill(usize::MAX);

    // Scan all slots and re-register anchors
    for (i, slot) in self.slots.iter().enumerate() {
        let anchor = match slot {
            Slot::Group { anchor, .. } => *anchor,
            Slot::Value { anchor, .. } => *anchor,
            Slot::Node { anchor, .. } => *anchor,
            Slot::Gap { anchor, .. } => *anchor,
        };

        if anchor.0 < self.anchors.len() {
            self.anchors[anchor.0] = i;
        }
    }

    self.anchors_dirty = false;
}
```

**Trade-off:** Incremental updates are O(A) where A = anchor count, rebuild is O(S) where S = slot count. Choose based on change magnitude.

---

## Integration Points

The snapshot and slot table systems integrate at several key points:

### 1. State Storage in Slots

State objects are stored in slot table value slots:

```rust
// During composition
slot_table.start(key);

// Remember state
let state_slot = slot_table.use_value_slot::<SnapshotMutableState<i32>>();
if state_slot.is_none() {
    *state_slot = Some(SnapshotMutableState::new(0, policy));
}

// Read state with snapshot isolation
let value = state_slot.read(&current_snapshot);

slot_table.end();
```

**Key insight:** Slot table manages **where** state is stored, snapshots manage **what** values are visible.

### 2. Scope-Based Recomposition

Slot table scopes enable targeted recomposition:

```rust
// Store scope during composition
slot_table.start_with_scope(key, scope_id);
// ...
slot_table.end();

// Later: recompose specific scope
slot_table.begin_recompose_at_scope(scope_id);
// Cursor positioned at scope's group
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
3. Slot table finds group by scope → recompose at that group

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
    slot_table.begin_recompose_at_scope(scope);
    // Run composition with new snapshot
}
```

### 4. Composition Context

The composition context bridges both systems:

```rust
pub struct CompositionContext {
    slot_table: SlotTable,
    current_snapshot: Arc<dyn Snapshot>,
    observer: SnapshotStateObserver,
    // ...
}

impl CompositionContext {
    pub fn remember<T>(&mut self, init: impl FnOnce() -> T) -> &mut T {
        // Use slot table
        let slot = self.slot_table.use_value_slot::<T>();
        if slot is empty {
            *slot = init();
        }
        slot
    }

    pub fn remember_state<T>(&mut self, init: T) -> SnapshotMutableState<T> {
        let state = self.remember(|| {
            SnapshotMutableState::new(init, StructuralEqualityPolicy::new())
        });
        state.clone()
    }

    pub fn read_state<T>(&mut self, state: &SnapshotMutableState<T>) -> T {
        // Use current snapshot
        state.read(&*self.current_snapshot)
    }
}
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

### Group Rescue Algorithm

Finding and moving groups during recomposition:

```rust
pub fn find_matching_group_or_gap(&self, key: Key) -> Option<usize> {
    let search_start = self.cursor;
    let search_limit = (self.cursor + 16).min(self.slots.len());

    // 1. Linear scan forward (budget: 16 slots)
    for i in search_start..search_limit {
        if self.is_matching_group_or_gap(i, key) {
            return Some(i);
        }
    }

    // 2. Search inside groups for nested gaps
    for i in search_start..search_limit {
        if let Some(Slot::Group { len, .. }) = self.slots.get(i) {
            let group_end = i + len;

            // Search within group
            for j in (i + 1)..group_end {
                if self.is_matching_gap(j, key) {
                    return Some(j);
                }
            }
        }
    }

    None
}

fn is_matching_group_or_gap(&self, index: usize, key: Key) -> bool {
    match &self.slots[index] {
        Slot::Group { key: k, .. } if *k == key => true,
        Slot::Gap { group_key: Some(k), .. } if *k == key => true,
        _ => false,
    }
}
```

**Budget rationale:** 16-slot scan balances:
- **Hit rate**: Most groups are at cursor (fast path) or nearby
- **Complexity**: Prevents O(n) scans in worst case
- **Fairness**: Consistent performance regardless of table size

### Gap Consolidation

Cleaning up fragmented gaps:

```rust
pub fn consolidate_gaps(&mut self) {
    let mut write_pos = 0;
    let mut read_pos = 0;

    while read_pos < self.slots.len() {
        match &self.slots[read_pos] {
            Slot::Gap { group_key: None, .. } => {
                // Skip INVALID gaps
                read_pos += 1;
            }
            _ => {
                // Keep slot
                if write_pos != read_pos {
                    self.slots.swap(write_pos, read_pos);
                }
                write_pos += 1;
                read_pos += 1;
            }
        }
    }

    // Truncate removed slots
    self.slots.truncate(write_pos);

    // Rebuild anchors
    self.rebuild_all_anchor_positions();
}
```

**When to consolidate:**
- After many recompositions
- When `has_gap_children` flags accumulate
- During idle time / GC passes

---

## Design Patterns

### Copy-on-Write (CoW)

**Used in:** SnapshotIdSet, nested snapshots

**Pattern:**
```rust
pub struct SnapshotIdSet {
    lower_set: u64,  // Immutable value
    // ...
}

impl SnapshotIdSet {
    pub fn set(&self, id: SnapshotId) -> Self {
        // Return new instance with modification
        let mut new_set = self.clone();
        new_set.lower_set |= (1u64 << bit);
        new_set
    }
}
```

**Benefits:**
- Snapshot `invalid` sets never change after creation
- Can share sets between snapshots
- No synchronization needed for reads

### Object Pool

**Used in:** StateRecord reuse, Gap reuse

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

**Used in:** MutationPolicy, SlotStorage trait

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

### Cursor Pattern

**Used in:** Slot table traversal

**Pattern:**
```rust
pub struct SlotTable {
    cursor: usize,
    // ...
}

impl SlotTable {
    pub fn start(&mut self, key: Key) {
        // Operations at cursor
        let slot = &self.slots[self.cursor];
        // ...
        self.cursor += 1;
    }
}
```

**Benefits:**
- Stateful traversal without explicit position tracking
- Composition code doesn't manage indices
- Enables fast-path optimizations (check cursor first)

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
| start() fast path | O(1) | Cursor has matching group |
| start() slow path | O(S) | S = search budget (16) |
| end() | O(1) | Update group len |
| use_value_slot() | O(1) | At cursor |
| mark_as_gaps() | O(N) | N = range size (recursive) |
| ensure_gap_at() | O(G) | G = gap search budget (256) |
| Group rescue | O(S + M) | S = search, M = move slots |
| Anchor resolve | O(1) | Array lookup |
| Anchor update | O(A) incremental, O(S) rebuild | A = anchors, S = slots |
| consolidate_gaps() | O(S) | S = total slots |

**Optimization opportunities:**
- Minimize force-recompose propagation
- Consolidate gaps during idle time
- Use anchors only when needed
- Choose appropriate search budgets for workload

### Memory Usage

**Snapshot system:**
- Each StateRecord: ~64 bytes (Arc, Cell, RwLock overhead)
- SnapshotIdSet: 24 bytes + 8 bytes per old ID
- MutableSnapshot: ~200 bytes + modified map

**Slot table:**
- Each Slot: ~40 bytes (enum + largest variant)
- GroupFrame: 32 bytes
- Anchor mapping: 8 bytes per anchor

**Scaling:**
- 10,000 UI elements: ~400 KB for slots
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
slot_table.start(TAB_GROUP);
    slot_table.start(TAB_1);
        // ... tab 1 content ...
    slot_table.end();
    slot_table.start(TAB_2);
        // ... tab 2 content ...
    slot_table.end();
slot_table.end();

// Slots: [Group(TAB_GROUP), Group(TAB_1), ..., Group(TAB_2), ...]

// Recomposition - Tab 2 active
slot_table.start(TAB_GROUP);
    // Tab 1 not rendered
    slot_table.start(TAB_2);
        // ... tab 2 content ...
    slot_table.end();
slot_table.end();

// Slots: [Group(TAB_GROUP), Gap(TAB_1, preserved), Group(TAB_2), ...]

// Recomposition - Tab 1 active again
slot_table.start(TAB_GROUP);
    slot_table.start(TAB_1);  // Gap found and promoted back!
        // ... tab 1 content restored ...
    slot_table.end();
slot_table.end();

// Slots: [Group(TAB_GROUP), Group(TAB_1, restored), Gap(TAB_2), ...]
```

**Key benefit:** Tab 1's state preserved in slots, instantly restored on reactivation.

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

1. **B-Tree Layout**: Hierarchical slot storage for better locality
2. **Incremental Anchors**: Only update anchors actually used
3. **Gap Coalescing**: Merge adjacent gaps automatically
4. **Parallel Recomposition**: Recompose independent subtrees concurrently

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

**Print slot table:**
```rust
fn debug_print_slots(table: &SlotTable) {
    for (i, slot) in table.slots.iter().enumerate() {
        let cursor_marker = if i == table.cursor { " <-" } else { "" };
        println!("{:4}: {:?}{}", i, slot, cursor_marker);
    }
}
```

**Visualize group nesting:**
```rust
fn debug_print_groups(table: &SlotTable) {
    let mut depth = 0;

    for slot in &table.slots {
        match slot {
            Slot::Group { key, len, scope, .. } => {
                println!("{:indent$}Group {{ key: {:?}, len: {}, scope: {:?} }}",
                    "", key, len, scope, indent = depth * 2);
                depth += 1;
            }
            Slot::Value { .. } => {
                println!("{:indent$}Value", "", indent = depth * 2);
            }
            Slot::Node { id, .. } => {
                println!("{:indent$}Node {{ id: {:?} }}", "", id, indent = depth * 2);
            }
            Slot::Gap { group_key, .. } => {
                println!("{:indent$}Gap {{ key: {:?} }}", "", group_key, indent = depth * 2);
            }
        }
    }
}
```

---

## Summary

The Snapshots and Slot Table system provides a sophisticated foundation for rs-compose:

**Snapshots** deliver:
- Isolated state views via MVCC
- Optimistic concurrency with conflict detection
- Flexible merge strategies
- Efficient garbage collection

**Slot Tables** deliver:
- Gap-buffer structure reuse
- Stable anchors across reorganization
- Cursor-based efficient traversal
- Conditional rendering optimization

**Together** they enable:
- Predictable recomposition
- State preservation across structural changes
- Concurrent snapshot modifications
- High-performance UI updates

This design mirrors Jetpack Compose's battle-tested architecture while leveraging Rust's ownership model for memory safety and performance.
