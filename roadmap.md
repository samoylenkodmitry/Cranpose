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
