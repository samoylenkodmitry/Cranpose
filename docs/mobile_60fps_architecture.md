# Mobile 60 FPS

**Target unmet: 16.67 ms/frame.** Cranpose internals may change; Jetpack Compose
API, application sources and picture correctness stay fixed.

- Track the remaining device frame budgets in [#626](https://github.com/samoylenkodmitry/Cranpose/issues/626); renderer correctness fixes do not close this target.
- [Huawei Showcase frame-budget measurements](huawei_showcase_frame_budget.md): two complete blur-specialization comparisons give +3.29% and −2.53%; retain the pixel guard and reject the added pipelines.

**Required workloads on Huawei and Pixel Watch:** Cranorbit Megaboss,
Showcase full scroll, and **Cranscan Settings scroll**.

| Constraint | Evidence | Next action |
| --- | --- | --- |
| Watch CPU | Latest profile: 18.17 ms/frame; main thread 17.22; arc recording + draw scope 5.77 | Remove repeated preparation and memory traffic; keep direct GPU columns |
| Watch GPU | One game pass: 15.20–19.02 ms after startup; diagnostic, 39.9→41.1°C | Reduce GPU work as well as recording; moving CPU work to the GPU spends an already full budget |
| Glass construction | Two diagnostic scroll profiles: source comparison 1.24→0.15 ms/frame; paired watch FPS has no reliable gain | Keep the prototype held; CPU savings alone do not prove frame savings |
| Huawei GPU | Glass removed: 30.79→34.84 FPS. Replacing copies with draws loses all four pairs: 32.19→29.20. All 26,039 sampled batches use whole-recording bounds | Keep copies; selected-run bounds cannot help this workload. Attribute allocation and memory traffic next. GPU timestamps unavailable |
| Heat | Watch scroll crosses throttling near 41°C; both builds slow down | Keep hot legs. Reject changes that improve a cool sample but worsen paired throughput |
| Cranscan | Complete Settings routes: watch 37.22→52.67 FPS with presentation overlap; Huawei 53.96→53.88 | Keep four-core overlap; reduce the remaining frame work using the [paired device results](mobile_present_thread.md) |
| Scheduling | Highest-capacity pair: Huawei scroll 31.51→32.38, mixed; game 58.30→58.25 | Keep wider CPU set; extra affinity restriction has no reliable gain |
| Allocation | Hot watch scroll, 41.9→42.3°C: 5.27 CPU ms/frame; 2.42 attributed to framework/wgpu, 1.60 to vendor driver. Observer allocation removal: watch 54.77→54.60 FPS, Huawei 31.29→31.25 | Hold prototype. Attribute cache misses, allocation sizes/lifetimes and driver costs; unresolved samples remain unresolved |
| Instruction cache | Watch diagnostic: sampled miss weight 37.9% Android libc, 24.8% Adreno, 34.0% application library. Executable code is already 3.3–3.7% smaller than main | Inspect allocation and command submission callers. Total code size and sampled PCs do not establish a cache bottleneck |
| Builds | Megaboss release uses optimization 3, full LTO, one codegen unit; Showcase uses Cargo release defaults | Consumer profiles control library code. Cranpose profile edits cannot change these unchanged apps |

**Architecture:** prepare immutable data once; record only changing data; resolve
only required backdrop dependencies; compose in draw order. No new cache or
thread without a measured saving and a guard against stale pixels.
Use the [performance coding guide](performance_coding_guide.md).

**Acceptance:** game windows are **20 seconds**. Watch uses first presented
seconds; Huawei includes launch. Scroll must expose the last card. Run ABAB
BABA, record temperature before/after every leg, retain failures, never wait
for cooling. Every optimization must fail a correctness guard when deliberately
broken. [Measurements and captures](mobile_watch_performance.md).

**Cranscan Settings:** measure 20 seconds of continuous scrolling from
“On-device intelligence” through “Version, licenses, credits, library stats.”
and back. Verify both endpoints and use the same gesture sequence, app revision,
features, data and expanded sections for main and SOTA. Preserve each run's FPS,
temperatures, route completion and paired screenshots. Do not change settings
or start downloads during the route; record background work already in progress.
