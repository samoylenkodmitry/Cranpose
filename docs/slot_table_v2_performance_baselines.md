# Slot Table V2 Performance Baselines

Slot Table V2 performance baselines are captured with `./perf_slot_table_v2.sh`. The script runs the `cranpose-ui` Criterion bench target `slot_table_v2`, applies stable local build defaults, pins to a CPU when `taskset` is available, and writes Criterion artifacts under `target/criterion/`.

Run the full verification gate before saving a baseline:

```bash
./verify_slot_table.sh
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
./perf_slot_table_v2.sh --stability-check
```

The stability check saves a temporary baseline, compares the same tree against it, and fails when benchmark drift exceeds `CRANPOSE_SLOT_TABLE_STABILITY_THRESHOLD_PCT` or `--stability-threshold-pct`. Treat an unstable host as a measurement failure, not as evidence about Slot Table V2.

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

- `CRANPOSE_SLOT_TABLE_CPU_SET` or `--cpu-set` chooses the CPU affinity. Use `none` only when pinning is unavailable.
- `CRANPOSE_SLOT_TABLE_SAMPLE_SIZE` or `--sample-size` controls Criterion samples.
- `CRANPOSE_SLOT_TABLE_WARMUP_TIME` or `--warmup-time` controls warmup seconds.
- `CRANPOSE_SLOT_TABLE_MEASUREMENT_TIME` or `--measurement-time` controls measurement seconds.
- `CRANPOSE_SLOT_TABLE_COOLDOWN_SECS` or `--cooldown-secs` controls the pause after saving a baseline before comparison.
- `CRANPOSE_SIBLING_INDEX_THRESHOLD` sets the compile-time sibling-index threshold for one benchmark run.

Record the commit SHA, command line, benchmark filter, CPU set, sample size, warmup, measurement time, and stability-check result with any performance claim. Do not compare numbers from different machines or different script settings.

## Regression Budgets

Apply these budgets only after the same-tree stability check passes on the host used for the comparison. If stability drift exceeds the threshold, rerun on a quieter machine before accepting or rejecting a candidate.

| Benchmark or counter family | Allowed regression | Required interpretation |
| --- | ---: | --- |
| Keyed reorder (`slot_table_v2_keyed_reverse_*`, rotate, seeded shuffle) | 5% | Structural edits are the most sensitive Slot Table V2 path. A larger regression requires profiling before merge. |
| Tab switching (`slot_table_v2_tab_switch_*`) | 5% | Retention and payload-heavy paths must stay close to baseline. |
| Subcompose scrolling (`slot_table_v2_subcompose_scrolling`) | 7% | This path is noisier because it includes reusable-slot policy work. |
| Lazy list reuse (`slot_table_v2_lazy_scroll_*`) | 7% | Includes composition, layout, measure, and reuse churn. Check allocation counters when timing changes. |
| Retained bytes (`SlotTableDebugStats::retained_heap_bytes`) | No unbounded growth | Repeated retain/restore cycles must plateau. Growth across stable cycles is a leak until proven otherwise. |
| Active anchors (`SlotTableDebugStats::{active_anchor_count, anchor_capacity}`) | No monotonic growth after stable reuse | Stable recomposition, lazy reuse, and tab switching must not keep allocating anchors once the working set is warm. |
| Layout allocation counters (`LayoutAllocationDebugStats`) | No unexplained monotonic growth | `layout_box_*`, `modifier_*`, and `semantics_*` counters should match the visible tree shape and plateau for steady lazy scrolling. |

When a timing budget fails, include the Criterion comparison output plus the relevant debug counters in the investigation notes. Do not claim a backend rewrite is necessary until the regression is tied to a measured Slot Table V2 path such as subtree moves, index refresh, payload-location rebuilds, or retained-state growth.
