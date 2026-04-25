# Slot Table V2 Linked Backend Decision Gate

Slot Table V2 keeps preorder `Vec` group storage as the production backend until
measurements prove that structural group moves dominate real workloads. A linked
or arena-backed group store is a conditional prototype, not an assumed rewrite.

## Required Evidence

Open a linked-backend prototype only when all of these are true:

- The full verification gate passes on the candidate branch:

```bash
./verify_slot_table.sh
```

- Slot model/property tests and behavior integration tests pass without
  filtering:

```bash
cargo test -p cranpose-core slot::model_tests --release
cargo test -p cranpose-core composition_and_recompose_scope_tests
```

- A stable perf baseline exists on the same host:

```bash
./perf_slot_table_v2.sh --save-baseline slot-v2-main
./perf_slot_table_v2.sh --stability-check
```

- Candidate measurements identify Slot Table V2 structural edits as the
  bottleneck:

```bash
./perf_slot_table_v2.sh --baseline slot-v2-main
```

Use `SlotTableDebugStats::mutation` counters to confirm the timing slope is
caused by subtree moves, group-index refresh, payload-location rebuild, or
payload/node relocation rather than lazy layout, modifier, semantics, renderer,
or applier work.

## Prototype Trigger

Prototype linked group storage only if, after current V2 optimizations:

- 1024+ keyed sibling reorder spends more than 25% of total measured time inside
  subtree move, group-index refresh, payload-location rebuild, or payload/node
  relocation paths; or
- the 4096-item keyed reverse curve remains clearly superlinear and the mutation
  counters show structural table edits as the cause; or
- a lazy-list jump benchmark shows the same structural-edit dominance after
  layout allocation counters have already plateaued.

Do not use a single noisy Criterion comparison as evidence. The same-tree
stability check must be green before accepting a regression or improvement.

## Required Win To Ship

A linked backend cannot replace the preorder `Vec` backend unless it meets all
of these budgets against the saved baseline:

| Scenario | Required result |
| --- | ---: |
| Keyed reverse 1024 | at least 30% faster |
| Keyed random shuffle 1024 | at least 30% faster |
| Keyed reverse 4096 | at least 40% faster |
| Normal small composition | no more than 5% slower |
| Tab switching | no more than 10% slower |
| Retained memory and free-list storage | no uncontrolled growth |

The default backend remains preorder `Vec` unless every required win is met and
the normal-case budgets stay inside limits.

## Non-Goals

- Do not rewrite composer, retention, payload, node, or scope semantics for the
  prototype.
- Do not introduce a half-switched backend. The semantic API must remain the V2
  API, with storage hidden behind a narrow group-storage boundary.
- Do not ship a feature flag that fails any existing model/property,
  integration, Android, wasm, or robot gate.
- Do not accept extra memory growth in exchange for reorder speed unless the
  retained/free-list plateau tests remain green.

## Minimum Prototype Shape

Do not add this abstraction speculatively. If the trigger fires, the prototype
starts behind an explicit feature and keeps preorder `Vec` as the default:

```rust
trait GroupStorage {
    type Cursor;

    fn insert_child_after(
        &mut self,
        parent: AnchorId,
        after: Option<AnchorId>,
        record: GroupRecord,
    ) -> AnchorId;

    fn unlink_subtree(&mut self, root: AnchorId) -> DetachedGroupRange;

    fn link_subtree_after(
        &mut self,
        parent: AnchorId,
        after: Option<AnchorId>,
        subtree: DetachedGroupRange,
    );

    fn first_child(&self, parent: AnchorId) -> Option<AnchorId>;
    fn next_sibling(&self, child: AnchorId) -> Option<AnchorId>;
    fn parent(&self, child: AnchorId) -> Option<AnchorId>;

    fn materialize_preorder_for_debug(&self) -> Vec<AnchorId>;
}
```

Parity commands required for the prototype:

```bash
cargo test -p cranpose-core
cargo test -p cranpose-core --features slot-link-storage
./perf_slot_table_v2.sh --baseline slot-v2-main
./verify_slot_table.sh
```

Until the prototype trigger fires, `slot-link-storage` should not exist and the
dual-backend parity commands remain a requirement for future work, not a current
CI target.
