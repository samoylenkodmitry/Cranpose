# Slot Table V2 Stress Commands

Run the bounded stress suite from the repository root:

```bash
./stress_slot_table.sh --core
./stress_slot_table.sh --perf-shard N/18
```

The split gate runs these commands and writes one log per step:

```bash
CRANPOSE_VALIDATE_SLOTS=1 cargo test --workspace
CRANPOSE_SLOT_MODEL_STRESS_FRAMES=10000 cargo test --release -p cranpose-core deterministic_model_render_frames_match_slot_table -- --nocapture
./perf_slot_table_v2.sh --stability-check --stability-shard N/18
```

Each script invocation has a hard 600-second wall-clock budget by default. Override it with `CRANPOSE_SLOT_STRESS_TIMEOUT_SECS`; set it to `0` only for an explicitly supervised long local investigation.

`CRANPOSE_VALIDATE_SLOTS=1` enables slot and retained-subtree validation during debug composition passes. The model stress command extends the deterministic Slot Table V2 render-frame model test with the requested number of generated frames; every failure reports the seed, compact scenario script, active debug snapshot, retained-subtree summary, and failed invariant.

Override the generated-frame count when needed:

```bash
CRANPOSE_SLOT_MODEL_STRESS_FRAMES=25000 ./stress_slot_table.sh --core
```

The perf stability step requires `jq` because `perf_slot_table_v2.sh --stability-check` reads Criterion JSON estimates. Treat a stability failure as an invalid benchmark host until rerun on a quieter machine.

Use `docs/slot_table_v2_release_checklist.md` for release-candidate sign-off
after the stress suite passes.
