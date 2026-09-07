# Mobile performance evidence

[Budget and decisions](mobile_60fps_architecture.md). Root:
`/tmp/cranpose-mobile-watch-60fps`. Sources, app payloads, binaries, temperatures
and failed runs are preserved there. Fixtures and profiles are diagnostics,
not application FPS; startup and first-presentation windows are distinct.

| Result | Evidence relative to root |
| --- | --- |
| Shared beats main: opening game, four pairs each device | `shared72-main-game10-analysis.json` |
| Direct columns + arc bounds: five watch pairs at matching endpoint clocks gain 2.2–5.8%; thermal crossings include losses | `arc-bounds-direct-v1-device-analysis.json` |
| Exact arc pixels; padding/radius mutants fail | `arc-quad-bounds-{red,restored}.log`, `watch-arc-quad-bounds-v1-oracle/`, `direct-columns-radius-swap-red.log` |
| Mixed blur/average atlas preserves averaged regions | `mixed-atlas-substrate-correctness.json`; original red agent-reported, raw red log absent |
| Shader v4 vs shared: watch scroll 49.14→50.88 FPS, all four gains, 35.8→40.7°C; Huawei 31.50→30.81, mixed, 30–31°C | `{watch,huawei}-substrate-specialization-v4-full-scroll-matrix.json`; first Huawei B includes two ~172 ms shader compiles |
| Shader v4 vs main: Huawei scroll 23.25→30.79 FPS, all four gains, 30–31°C | `huawei-substrate-v4-main-full-scroll-matrix.json`; main built on macm3, candidate on Linux, unchanged app/lock |
| Shader v4 vs main: watch scroll 26.77→49.92 FPS, all four gains, 33.5→40.3°C | `watch-substrate-v4-main-full-scroll20-matrix.json`; common 20-swipe route; initial 16-swipe main leg failed endpoint and remains excluded in `watch-substrate-v4-main-full-scroll-1-A/` |
| Shader v4 game opening: Huawei 59.38–59.77 FPS; watch 46.7–47.8 except final control thermal crossing at 41.8°C | `{watch,huawei}-substrate-specialization-v4-opening10-matrix.json` |
| Presence cache-key and fallback mutants fail; v4 exact watch glass output passes | `gradient-substrate-v1-cache-key-red.log`, `glass-substrate-v3-missing-fallback-red.log`, `watch-substrate-specialization-v4-glass-guard/report.json`, `substrate-v4-reference-guards.log` |
| v3 absent-resource folding changed one watch pixel; v4 keeps that branch dynamic | `watch-substrate-v3-fallback-diagnostic/` |
| Isolated watch GPU savings: gradient 19.2–19.4%, frosted-card interior 30.8–33.4%, exact pixels | `{gradient,glass}-substrate-v1-probe-analysis.json` |
| Swipe v2: watch 47.31→56.12 FPS, four gains, 35.1→41.0°C; Huawei scroll 31.57→31.14, four losses, 30–31°C; combined with inlining below | `watch-swipe-layout-v2-opening10-matrix.json`, `huawei-swipe-layout-v2-full-scroll-matrix.json`; same source paths/toolchain; all gates and three gesture robots pass |
| Column inlining on swipe v2: watch four gains, comparable +0.7–1.6 FPS; one larger thermal crossing, 37.8→42.2°C. Huawei game 59.64→59.68, 31–32°C | `{watch,huawei}-swipe-inline-columns-v1-opening10-matrix.json`; annotation only, no arithmetic change |
| Normalization inlining: watch 57.08→57.66 FPS, four gains, 34.3→40.3°C. Combined scroll vs shared: watch 49.11→49.45, three gains/one loss, 36.0→40.8°C; Huawei 31.45→32.21, two gains/two losses, 31–32°C | `swipe-inline-normalization-v1-checkpoint-analysis.json`; checkpoint `1767a226`; 225 graphics checks pass, zero warnings |
| Huawei combined checkpoint, first ten seconds including launch: 56.43→56.33 FPS, 32°C; separate from steady presentation windows near 59.6 FPS | `huawei-swipe-inline-normalization-v1-shared-launch10-matrix.json`; failed log-based readiness attempts retained under `huawei-swipe-inline-normalization-v1-{opening10,live-opening10}-*` |
| Swipe correctness: original dirties layout on draw changes; controller identity mutant clamps to wrong width; restored 40 checks pass | `swipe-layout-recomposition-red.log`, `swipe-layout-controller-identity-red.log`, `swipe-layout-v2-focused.log` |
| Current CPU: 22.60→18.59 ms/frame; swipe removes recurring layout | `watch-direct-columns-opening-cpu/`, `watch-swipe-layout-v1-opening-cpu/`; profiling overhead included |
| Huawei warm shader repeat: 31.07→30.78 FPS, three losses/one gain; held | `huawei-substrate-v4-warm-full-scroll-matrix.json` |
| Arithmetic probes: three small CPU gains/one flat pair each, exact fingerprints; no app acceptance | `arc-normalization-v1-watch-probe.json`, `arc-normalization-v1-probe-provenance.json`, `band-ring-clamp-v1-watch-probe.json` |
| Manual normalization: no reliable watch game gain; held. Glass-only and gradient-only specialization lose on Huawei scroll; held | `swipe-normalize-v1-held.json`, `swipe-inline-and-shader-isolation-analysis.json` |
| Rejected experiments and source preservation | `previous-row-direct-v1-rejection.json`, `arc-radius-vertex-v1-held.json`, `arc-scalar-direct-rejection.json`, `command-surfaces-rejection.json`, `fable-compute-blur-held-20260906-*` |
| Held shader gates: test, clippy, web, Android, 162 GPU robots and four captures pass; zero warnings | `substrate-specialization-v4-gates.json`, `substrate-specialization-v4-held.json` |
| Device ownership; earlier evidence index | `sequence-ownership.jsonl`, `mobile-watch-evidence-index-before-compression-20260907.md` |
