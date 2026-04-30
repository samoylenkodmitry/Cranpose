# Slot Table V2 Performance Baselines

Slot Table V2 performance baselines are captured with `./perf_slot_table_v2.sh`. The script runs the `cranpose-ui` Criterion bench target `slot_table_v2`, applies stable local build defaults, and writes Criterion artifacts under `target/criterion/`.

Run the full verification gate before saving a baseline:

```bash
./verify_slot_table.sh --core
./verify_slot_table.sh --robot-build
./verify_slot_table.sh --robot-shard N/16
```

Save the reference baseline from the branch or commit that represents the comparison target:

```bash
./perf_slot_table_v2.sh --save-baseline slot-v2-main
```

Compare the candidate branch against that saved baseline:

```bash
./perf_slot_table_v2.sh --baseline slot-v2-main
```

Run the same-tree stability check before trusting a regression result:

```bash
./stress_slot_table.sh --perf-shard N/18
```

The stability check runs two unrecorded warmup passes, saves a temporary baseline,
compares the same tree against it without an extra cooldown gap, and fails when
the lower bound of the same-tree regression confidence interval exceeds the
configured threshold. A full-matrix stability check records and compares each
benchmark with its own adjacent temporary baseline so host phase shifts between
benchmark families do not become a false regression. Each same-tree comparison
gets four attempts by default; one stable attempt is enough to pass because a
single unstable same-tree pair is host noise, not a source regression. By
default, keyed, conditional, and tab-switch benchmarks use the 5% stability
threshold; lazy-list and subcompose benchmarks use their 7% documented timing
budget.
`CRANPOSE_SLOT_TABLE_STABILITY_THRESHOLD_PCT` or `--stability-threshold-pct`
replaces those per-family defaults with one explicit threshold. The summary
reports signed point estimates so large same-tree improvements remain visible as
host or benchmark warmup evidence. Treat an unstable host as a measurement
failure, not as evidence about Slot Table V2.

The stability check has a hard 600-second wall-clock budget by default. Override
it with `CRANPOSE_SLOT_TABLE_STABILITY_TIMEOUT_SECS`; set it to `0` only for an
explicitly supervised long local investigation.

Use filters for focused investigations:

```bash
./perf_slot_table_v2.sh --filter keyed_reverse_1024 --save-baseline keyed-main
./perf_slot_table_v2.sh --filter lazy_scroll_steady --baseline lazy-main
```

## Sibling Index Threshold Matrix

The writer builds a temporary sibling index only after a direct-child scan crosses
`CRANPOSE_SIBLING_INDEX_THRESHOLD`. The production default is `16`. Before
changing it, run the focused matrix:

```bash
./perf_slot_table_v2.sh --sibling-threshold-matrix
```

The matrix tests thresholds `4 8 16 32 64` against:

- `slot_table_v2_keyed_reverse_16`
- `slot_table_v2_keyed_reverse_64`
- `slot_table_v2_keyed_reverse_256`
- `slot_table_v2_keyed_reverse_1024`
- `slot_table_v2_keyed_rotate_front_to_back_1024`
- `slot_table_v2_keyed_random_shuffle_1024_seed_1`

Set `CRANPOSE_SIBLING_INDEX_THRESHOLDS` to replace the threshold list. The script
uses a separate `CARGO_TARGET_DIR` for each threshold under
`target/sibling-threshold-*` so the compile-time `option_env!` value cannot be
accidentally reused between matrix entries.

Use the threshold that improves the 1024-item keyed reorder family without
regressing small sibling lists. Confirm that choice with the full keyed reorder
matrix before changing the default:

```bash
CRANPOSE_SIBLING_INDEX_THRESHOLD=8 ./perf_slot_table_v2.sh --filter keyed_reverse
```

Use the same machine, power profile, CPU set, sample size, warmup, and measurement time for both baseline and candidate runs. The script defaults are intentionally conservative for local comparison:

- `CRANPOSE_SLOT_TABLE_CPU_SET` or `--cpu-set` chooses the CPU affinity. The default is `none`; pin only after proving the selected CPU is stable for same-tree runs.
- `CRANPOSE_SLOT_TABLE_SAMPLE_SIZE` or `--sample-size` controls Criterion samples.
- `CRANPOSE_SLOT_TABLE_WARMUP_TIME` or `--warmup-time` controls warmup seconds.
- `CRANPOSE_SLOT_TABLE_MEASUREMENT_TIME` or `--measurement-time` controls measurement seconds.
- `CRANPOSE_SLOT_TABLE_COOLDOWN_SECS` or `--cooldown-secs` controls the pause after saving a named baseline before comparison. The temporary same-tree stability comparison skips this pause because the warmup and baseline must be adjacent.
- `CRANPOSE_SLOT_TABLE_STABILITY_WARMUP_RUNS` controls the number of unrecorded same-tree runs before the stability baseline. The default is `2` so CPU frequency and process-level warmup do not become the saved baseline.
- `CRANPOSE_SLOT_TABLE_STABILITY_ATTEMPTS` controls same-tree retry attempts per benchmark. The default is `4`.
- `CRANPOSE_SLOT_TABLE_STABILITY_TIMEOUT_SECS` controls the same-tree stability wall-clock budget. The default is `600`; `0` disables the guard for an explicitly supervised long run.
- `CRANPOSE_SIBLING_INDEX_THRESHOLD` sets the compile-time sibling-index threshold for one benchmark run.

Record the commit SHA, command line, benchmark filter, CPU set, sample size, warmup, measurement time, and stability-check result with any performance claim. Do not compare numbers from different machines or different script settings.

## Lazy List Allocation Churn

The lazy-list measurement path should use existing size estimates before adding
new allocation caches. `ItemMeasurer` pre-sizes its per-pass item vector from the
running average item size, viewport span, and beyond-bounds item count, and its
before-bounds buffer reserves enough capacity for the final prepended result.
If lazy scrolling still regresses after this, investigate the
`LayoutAllocationDebugStats` counters and lazy scroll benchmark family before
adding more retained buffers.

## Regression Budgets

Apply these budgets only after the same-tree stability check passes on the host
used for the comparison. If same-tree movement is large enough to make the
candidate result ambiguous, rerun on a quieter machine before accepting or
rejecting the candidate.

| Benchmark or counter family | Allowed regression | Required interpretation |
| --- | ---: | --- |
| Keyed reorder (`slot_table_v2_keyed_reverse_*`, rotate, seeded shuffle) | 5% | Structural edits are the most sensitive Slot Table V2 path. A larger regression requires profiling before merge. |
| Tab switching (`slot_table_v2_tab_switch_*`) | 5% | Retention and payload-heavy paths must stay close to baseline. |
| Subcompose scrolling (`slot_table_v2_subcompose_scrolling`) | 7% | This path is noisier because it includes reusable-slot policy work. |
| Lazy list reuse (`slot_table_v2_lazy_scroll_*`) | 7% | Includes composition, layout, measure, and reuse churn. Check allocation counters when timing changes. |
| Retained state (`SlotTableDebugStats::{retained_*_count, retained_heap_bytes, retained_evictions_total}`) | No unbounded growth | Repeated retain/restore cycles must plateau. Growth across stable cycles is a leak until proven otherwise; eviction count should change only when a configured retention budget is exceeded. |
| Active anchors (`SlotTableDebugStats::{active_anchor_count, active_payload_anchor_count, anchor_capacity, payload_anchor_capacity}`) | No monotonic growth after stable reuse | Stable recomposition, lazy reuse, and tab switching must not keep allocating anchors once the working set is warm. Occupied slots are active + detached + occupied-invalidated; `free_anchor_count` and `free_payload_anchor_count` are reusable invalidated IDs outside occupied slot counts. |
| Layout allocation counters (`LayoutAllocationDebugStats`) | No unexplained monotonic growth | `layout_box_*`, `modifier_*`, and `semantics_*` counters should match the visible tree shape and plateau for steady lazy scrolling. |
| Slot mutation counters (`SlotTableDebugStats::mutation`) | Explain every timing slope | Subtree moves, moved groups/payloads/nodes, payload-location rebuild spans, and group-index refresh spans identify whether structural table edits dominate a regression. |
| Group table bytes (`SlotTableDebugStats::{group_record_size, group_heap_bytes}`) | No unprofiled storage split | Keep `Vec<GroupRecord>` until timing slopes and byte counters show group-table bandwidth or cache pressure is the limiting path. |

When a timing budget fails, include the Criterion comparison output plus the relevant debug counters in the investigation notes. Do not claim a backend rewrite is necessary until the regression is tied to a measured Slot Table V2 path such as subtree moves, index refresh, payload-location rebuilds, or retained-state growth.

Use `docs/slot_table_v2_link_backend_decision.md` as the decision gate before
opening any linked or arena-backed group-storage prototype.

For release sign-off, use `docs/slot_table_v2_release_checklist.md` after the
baseline, comparison, and stability commands have completed on the release
candidate commit.
