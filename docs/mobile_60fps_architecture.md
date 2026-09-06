# Mobile 60 FPS: decision sheet

**Target unmet. PR #617 is not ready for main.** Only Cranpose changes; preserve
native resolution, effects, app sources and image correctness. Compare main
`0d195313` with shared runtime `37bd0ce8`, integrated by both agents.

## Measured checkpoint

Normal policy, unchanged release apps, 60-second legs, ABAB then BABA, no cooling.
Ranges retain all completed legs; thermal transitions remain part of the result.

| Workload | Main FPS | Shared FPS | Decision |
| --- | --- | --- | --- |
| Huawei Megaboss | 57.26–58.38 | 59.84–59.88 | Preserve shared gain |
| Huawei full scroll | 25.12–29.06 | 39.50–45.30 | Preserve gain; below target |
| Watch Megaboss | 39.51 down to 15.71 | 36.45 down to 16.82 | Mixed: first two pairs lose 3.06/2.92 FPS, final two gain 1.23/1.11 |
| Watch full scroll | In progress | In progress | A preflight failure prevents acceptance of this sequence |

CPU profiles are diagnostics, separate from the FPS acceptance runs. Values are
sample-weighted totals across app threads per presented frame, not energy.

| Workload | Main CPU ms / million cycles | Shared CPU ms / million cycles | Conditions |
| --- | --- | --- | --- |
| Huawei Megaboss | 10.32 / 13.64 | 8.08 / 9.14 | 60 seconds; 41→41°C / 41→40°C |
| Huawei full scroll | 22.13 / 32.54 | 11.56 / 16.29 | 60 seconds; both 35→35°C |
| Watch Megaboss | 49.74 / 41.26 | 39.82 / 33.26 | Reverse B/A, 30 seconds; 43.3→43.5°C / 43.0→43.5°C |

Watch CPU cycles also favor shared in the first A/B profile: 33.60M versus 41.98M.
GPU timings remain thermally confounded: main's draw pass measured about 45 ms
in one capture and 28 ms in another; shared measured 42 ms. No GPU speedup or
regression is established by those unequal-clock captures.

## Architecture decision

- **Keep the shared base.** CPU work and Huawei frame rate improved. Watch
  sustained performance still fails the target and the no-regression gate.
- **Reduce both CPU and GPU work.** Thread overlap alone cannot solve the measured
  watch costs. Streaming remains held; every tested Huawei producer variant lost.
- **Learn from main's reuse.** Megaboss upload snapshots have medians 0.285 MB on
  main and 0.895 MB on shared; shared issues fewer draws. Main recognizes retained
  transforms/recolors; shared compares packed bytes. Workload causality is unproven.
- **Remove a measured cost before redesigning.** Fable's fragment replacements and
  discards bound the shape path. They change coverage and compiler inputs, so their
  difference is not an exact ALU/fill decomposition. Codex handles recording/upload
  attribution. No new cache, worker architecture or representation is selected yet.
- **Reject imaginary work.** The builder already seals shared ownership once;
  arc normalization is not duplicated. Neither warrants another abstraction.

Ship only after deliberately broken correctness tests fail, restored tests pass,
existing pixel expectations remain intact, platform gates pass, and all four
workloads show sustained 60 Hz presentation without regression. Battery temperature
and endpoint clocks constrain interpretation; they do not measure energy.

Exact artifacts and validation: [measurement index](mobile_watch_performance.md).
