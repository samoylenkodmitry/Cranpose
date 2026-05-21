# Renderer Performance Plan

Goal: every robot/perf scenario must sustain at least 120 FPS, including the heavy LeetCodeDaily layout scroll path.

## Ground Rules

- Measure before changing architecture.
- Commit each verified improvement as a coherent checkpoint.
- A checkpoint is verified only after formatting, tests, clippy, Android release, wasm build, and the full robot suite pass.
- Prefer root-cause renderer and invalidation fixes over local workarounds.
- Keep the repo in a complete state after every commit.

## Loop

1. Run the available perf scripts and record the slowest scenarios.
2. Add focused telemetry when the scripts do not expose the bottleneck.
3. Rank bottlenecks by measured frame time, not intuition.
4. Implement the highest-impact architectural fix.
5. Run focused tests for the touched path.
6. Run the full verification gate.
7. Commit the verified improvement.
8. Repeat until the minimum measured FPS is at least 120 everywhere.

## Current Suspicions To Prove Or Reject

- Scene rebuild and render-graph traversal still occur too often during scroll.
- Text layout, glyph cache misses, or text upload churn dominate heavy list screens.
- Layer/effect segmentation creates too many render passes in mixed content.
- Upload staging or buffer allocation still causes avoidable CPU/GPU synchronization.
- Lazy-list draw invalidation may still rebuild more scene graph than required.

## Measurement Targets

- Hacker News scroll.
- LeetCodeDaily full layout scroll stability.
- Markdown and text exact-scroll contracts.
- Robot perf harness.
- Lazy-list perf validation.

## Status

- Checkpoint `008d630c` committed the first renderer draw-update cleanup.
- `perf_robot_cpu.sh`/symbol-heavy profiling is not safe on this no-swap machine; it caused an OOM reboot. Use stripped `release-fast` robot binaries for iteration, then full verification before commit.
- Built-in robot perf harness after the current renderer changes:
  - `lazy_list_scroll`: 1552.2 FPS.
  - `text_heavy_scroll`: 1682.8 FPS.
  - `backdrop_blur`: 1793.0 FPS.
  - `opaque_scene`: 6505.5 FPS.
- Heavy LeetCodeDaily full-layout scroll:
  - Baseline with redundant identity alpha mask still active: 98.5 FPS, 66 blur passes/sample, 135 offscreen acquires/sample.
  - After identity alpha-mask no-op: 103.9 FPS, 66 blur passes/sample, 134 offscreen acquires/sample.
  - After translation-invariant shape-shadow surface cache: 196.1 FPS, 0 blur passes/sample, 1 offscreen acquire/sample.
- The current highest remaining renderer bottleneck is composite submit count. LeetCodeDaily still reports 175 submits/sample because cached offscreen composites use immediate command submissions. The proper next renderer architecture work is a batched/dynamic-uniform composite path, not a demo-specific shortcut.

## Current Checkpoint Scope

- Make identity rounded alpha masks a no-op at modifier construction time.
- Add an opt-in LeetCodeDaily perf probe without changing the demo layout/content.
- Share robot perf render-stat accumulation across perf examples.
- Cache translated, text-free shape-shadow surfaces by normalized content hash, pixel size, scale, and blur radius.
- Batch shape-only shadow miss rendering into one encoder before caching the blurred surface.
