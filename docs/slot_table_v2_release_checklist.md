# Slot Table V2 Release Checklist

Complete this checklist on the release candidate commit. Do not reuse results from a different commit, branch, machine, or feature set.

## Documentation

- [ ] `docs/cranpose_slot_table_v2_design.md` is the only active slot-table design specification.
- [ ] `docs/slot_table_v2_invariants.md` matches the current validator and retained-state behavior.
- [ ] No README or docs page describes gap-table storage as current Slot Table V2 behavior.
- [ ] `roadmap.md` has no unchecked required Slot Table V2 item that is needed for this release.

## Verification

- [ ] `./verify_slot_table.sh` passes on the release candidate commit.
- [ ] `cargo_fmt.tmp`, `1.tmp`, `2.tmp`, `android_release.tmp`, `web-build.tmp`, `robot.tmp`, `robot_test.log`, and `robot_test_summary.txt` were read after the run.
- [ ] `robot_test_summary.txt` reports `FAILED=0` and `TOTAL` equals `PASSED`.
- [ ] Every warning or failure-looking line in verification logs is either fixed or explicitly classified as a known benign robot diagnostic.

## Model And Stress

- [ ] `CRANPOSE_VALIDATE_SLOTS=1 cargo test --workspace` passes.
- [ ] `CRANPOSE_SLOT_MODEL_STRESS_FRAMES=10000 cargo test --release -p cranpose-core deterministic_model_render_frames_match_slot_table -- --nocapture` passes.
- [ ] Any generated-frame failure seed, compact scenario script, active debug snapshot, retained-subtree summary, and failed invariant is persisted in a regression test before release.
- [ ] `./run_robot_test.sh --sequential` passes after the stress/model run.

## Performance

- [ ] `./perf_slot_table_v2.sh --stability-check` passes on the measurement host.
- [ ] `./perf_slot_table_v2.sh --save-baseline slot-v2-main` has been run for the comparison target, or the release notes identify the exact existing baseline.
- [ ] `./perf_slot_table_v2.sh --baseline slot-v2-main` passes on the release candidate.
- [ ] Keyed reorder, tab switching, subcompose scrolling, lazy-list reuse, retained bytes, active anchors, layout allocation counters, slot mutation counters, and group table bytes are within `docs/slot_table_v2_performance_baselines.md` budgets.

## Memory And Lifecycle

- [ ] Retained tab/list/subcompose memory plateau tests pass.
- [ ] Retained bytes plateau after repeated stable retain/restore cycles.
- [ ] Active anchor count and anchor capacity do not grow monotonically after stable recomposition, lazy reuse, or tab switching reaches its warm working set.
- [ ] Retention eviction counters change only when a configured retention budget is exceeded.

## Panic Classification

- [ ] Every Slot Table V2 panic in tests, stress, or robot logs is classified as programmer error, debug invariant violation, or internal bug.
- [ ] Every expected panic has an explicit `should_panic` test or equivalent assertion.
- [ ] Every internal bug panic found during release validation has a targeted regression test before release.
- [ ] No panic is accepted as an unexplained runtime behavior.

## Release Record

- [ ] Release candidate commit SHA:
- [ ] Verification command and timestamp:
- [ ] Stress command and timestamp:
- [ ] Perf baseline name and comparison timestamp:
- [ ] Known benign diagnostics:
- [ ] Reviewer:
