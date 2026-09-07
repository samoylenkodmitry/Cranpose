# Mobile 60 FPS

**Target unmet: 16.67 ms/frame.** Cranpose internals may change; Jetpack Compose
API, application sources and picture correctness stay fixed.

| Constraint | Evidence | Next action |
| --- | --- | --- |
| Watch CPU | Latest profile: 18.17 ms/frame; main thread 17.22; arc recording + draw scope 5.77 | Remove repeated preparation and memory traffic; keep direct GPU columns |
| Glass construction | Earlier watch scroll stack: 89.7% of byte-comparison samples come from shader construction | Reuse an immutable program; each effect keeps independent uniforms |
| Huawei GPU | Current inventory: repeated full-screen composition passes; 0.33 MP shape fill. GPU timestamps unavailable | Reduce actual capture dependencies and pass boundaries; attachment area is not GPU time |
| Heat | Watch scroll crosses throttling near 41°C; both builds slow down | Keep hot legs. Reject changes that improve a cool sample but worsen paired throughput |
| Scheduling | Upward timeout rounding removes Huawei busy polling; watch has almost none | Keep the fix; another render thread has no reliable measured gain |

**Architecture:** prepare immutable data once; record only changing data; resolve
only required backdrop dependencies; compose in draw order. No new cache or
thread without a measured saving and a guard against stale pixels.

**Acceptance:** game windows are **20 seconds**. Watch uses first presented
seconds; Huawei includes launch. Scroll must expose the last card. Run ABAB
BABA, record temperature before/after every leg, retain failures, never wait
for cooling. Every optimization must fail a correctness guard when deliberately
broken. [Measurements and captures](mobile_watch_performance.md).
