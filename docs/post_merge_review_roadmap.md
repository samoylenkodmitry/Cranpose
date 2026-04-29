# Post-Merge Review Roadmap

Review target: Slot Table V2 identity refactor after merge to `main`.

## Findings

- [x] Remove the completed top-level `roadmap.md`.

  The merged roadmap was an execution artifact. Keeping it at repository root makes future status ambiguous after every item is already complete.

- [x] Split GitHub robot e2e validation into bounded shards.

  The latest merged PR run reported `robot e2e` at 37m37s. The repo now has split robot validation, but CI still used one monolithic `./run_robot_test.sh --sequential` job with a 90-minute job timeout. CI should build robot examples once, run deterministic shards under a 600-second command guard, and keep a final `robot e2e` aggregate check for branch protection.

- [x] Move checkout steps off the Node 20-deprecated action generation.

  The latest CI logs emitted Node.js 20 deprecation annotations for `actions/checkout@v4`. The current official checkout release is `v6`, so workflow checkout steps should use `actions/checkout@v6`.

- [x] Remove stale active slot-table documentation references.

  Active docs still named deleted or replaced surfaces such as the old top-level slot storage module, flat validator module, and `GroupAnchor` snippets. The docs should describe the current `SlotTable`, `AnchorId`, `PayloadAnchorRegistry`, `ScopeIndex`, and module layout.

- [x] Keep payload-anchor disposal off the storage-compaction path.

  `SlotTable::invalidate_payload_anchors` compacted payload-anchor storage immediately after every invalidation batch. Disposal is a mutation hot path; storage compaction should remain explicit or only run for sparse-path recovery.

## Validation

- [x] Focused payload-anchor regression passes.
- [x] Formatting passes.
- [x] Workspace tests pass.
- [x] Workspace clippy passes with warnings denied.
- [x] Bounded core verification passes, including Android release and wasm build.
- [x] Bounded robot build and representative robot shard pass.
- [x] Script syntax checks pass.
- [x] Workflow syntax is checked.
