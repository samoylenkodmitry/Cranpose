# Mobile 60 FPS: decision sheet

**Status: target unmet. PR #617 is not ready for main.**
Only Cranpose changes. Preserve native resolution, effects, app sources and image
correctness. Workloads: Megaboss and complete forward/backward Showcase scrolling
on Huawei Mate 20 X and Pixel Watch 3.

| Checkpoint | State |
| --- | --- |
| Main | `0d195313`, freshly fetched and rebuilt |
| Shared reviewed code | `37bd0ce8`; merged into Fable as `82e829d5` |
| Shared platform gates | `56328905` passes workspace tests, native/wasm/Android/iOS Clippy, web, Android and both robot partitions; `37bd0ce8` passes its correction gates |
| Experiments | Activity, curve specialization, shape spans and streaming remain unaccepted |

## Facts that constrain the design

| Workload / experiment | Observation | Conclusion |
| --- | --- | --- |
| Huawei Megaboss | Shared reaches about 59.9 FPS | Preserve it |
| Huawei full scroll | Shared about 40–41 FPS; isolated curve probe about 43 FPS | Material execution remains expensive |
| Watch Megaboss | Main wins all four pairs in the completed presentation-thread comparison; final hot losses are 1.16% and 5.52% | Recover main's advantage before adding complexity |
| Watch full scroll | Shared ends near 31.8 FPS versus main 21.2 | Improvement, still far from 60 |
| Normal-policy watch comparison | Seven legs retained; TLS disconnect prevents leg eight | Incomplete; no acceptance conclusion |
| Streaming recorder | Two workers / 2,048 events save watch preparation ~1.8 ms; Huawei slows down, including with app core selection | Hold; CPU overlap alone is insufficient |

The completed app comparisons above explicitly enable the presentation thread.
They do not establish normal-policy acceptance. Battery temperature is a thermal
observation, not a measurement of CPU/GPU energy.

## Next decision: measure main's advantage

1. Profile **main and shared**, same unchanged Megaboss, normal runtime policy.
   Alternate ABAB then BABA; record temperature before/after every leg; never
   discard a thermal transition or wait for cooling.
2. Compare presented frame intervals; CPU self time and total CPU time per frame;
   allocations/copies; GPU passes, draws, uploads and available GPU timings;
   CPU frequency, thermal status and thread activity. Record profiler overhead.
3. Inspect main's compact recording, retained work, replay and raster reuse.
   Rank differences by measured cost. Do not infer a benefit from cache hits,
   removed calls, smaller uploads, fewer source lines or more threads.
4. Remove one suspected cost and rerun. Rewrite only the confirmed boundary.
   Require less sustained work, exact images and no FPS regression in all four
   device/workload combinations. Keep the simpler design when evidence is mixed.

**Ownership:** Codex handles main/shared profiling and recording; Fable handles
material execution and renderer structure. Finish current correctness checks,
then reassess together. The external streaming GPU probe is held and unbuilt.
No further production architecture is selected before this comparison.

**Correctness gate:** changed inputs, clipping, blending, draw order, retained
readers and resource lifetime; deliberately break the optimization, observe the
relevant test fail, restore and pass. Preserve existing pixel expectations.
**Performance gate:** sustained presentation near the device's 60 Hz period,
including slow-frame tails and heat. A CPU-only timer is not an FPS result.

Evidence and exact artifact names: [measurement index](mobile_watch_performance.md).
