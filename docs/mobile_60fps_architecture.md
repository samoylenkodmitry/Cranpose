# Mobile 60 FPS

**Target unmet: 16.67 ms/frame.** Cranpose internals may change; Jetpack Compose
API, application sources and picture correctness stay fixed.

| Constraint | Evidence | Next action |
| --- | --- | --- |
| Watch CPU | Latest profile: 18.17 ms/frame; main thread 17.22; arc recording + draw scope 5.77 | Remove repeated preparation and memory traffic; keep direct GPU columns |
| Watch GPU | One game pass: 15.20–19.02 ms after startup; diagnostic, 39.9→41.1°C | Reduce GPU work as well as recording; moving CPU work to the GPU spends an already full budget |
| Glass construction | Two diagnostic scroll profiles: source comparison 1.24→0.15 ms/frame; paired watch FPS has no reliable gain | Keep the prototype held; CPU savings alone do not prove frame savings |
| Huawei GPU | Glass removed: 30.79→34.84 FPS. Replacing copies with draws loses all four pairs: 32.19→29.20 | Keep copies; test bounded page updates to avoid full-target loads. GPU timestamps unavailable |
| Heat | Watch scroll crosses throttling near 41°C; both builds slow down | Keep hot legs. Reject changes that improve a cool sample but worsen paired throughput |
| Scheduling | Highest-capacity pair: Huawei scroll 31.51→32.38, mixed; game 58.30→58.25 | Keep wider CPU set; extra affinity restriction has no reliable gain |

**Architecture:** prepare immutable data once; record only changing data; resolve
only required backdrop dependencies; compose in draw order. No new cache or
thread without a measured saving and a guard against stale pixels.

**Acceptance:** game windows are **20 seconds**. Watch uses first presented
seconds; Huawei includes launch. Scroll must expose the last card. Run ABAB
BABA, record temperature before/after every leg, retain failures, never wait
for cooling. Every optimization must fail a correctness guard when deliberately
broken. [Measurements and captures](mobile_watch_performance.md).
