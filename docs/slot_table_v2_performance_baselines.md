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

Use the same machine, power profile, CPU set, sample size, warmup, and measurement time for both baseline and candidate runs. The script defaults are intentionally conservative for local comparison:

- `CRANPOSE_SLOT_TABLE_CPU_SET` or `--cpu-set` chooses the CPU affinity. Use `none` only when pinning is unavailable.
- `CRANPOSE_SLOT_TABLE_SAMPLE_SIZE` or `--sample-size` controls Criterion samples.
- `CRANPOSE_SLOT_TABLE_WARMUP_TIME` or `--warmup-time` controls warmup seconds.
- `CRANPOSE_SLOT_TABLE_MEASUREMENT_TIME` or `--measurement-time` controls measurement seconds.
- `CRANPOSE_SLOT_TABLE_COOLDOWN_SECS` or `--cooldown-secs` controls the pause after saving a baseline before comparison.

Record the commit SHA, command line, benchmark filter, CPU set, sample size, warmup, measurement time, and stability-check result with any performance claim. Do not compare numbers from different machines or different script settings.
