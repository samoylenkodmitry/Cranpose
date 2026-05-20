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

- Verified renderer cleanup is ready to commit.
- Next step: run `perf_robot_cpu.sh`, `perf_robot_heap.sh`, and relevant focused robot/perf scenarios, then record the first measured bottleneck here.
