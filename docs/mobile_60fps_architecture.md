# Mobile 60 FPS

**Target unmet.** Checkpoint `minimum-band-v1`; Cranpose only, unchanged pixels.
Frame budget: **16.67 ms**. Shared game build beats main on both devices.
Swipe layout: watch **47.3→56.1 FPS**, four gains, **35.1→41.0°C**.
One-quad classification: watch **56.21→57.73 FPS**, four gains; Huawei launch
**56.41→56.45**, mixed. Scroll means: watch **46.21→48.28**, Huawei
**31.17→31.60 FPS**; the watch mean includes a large thermal crossing.

| Cost | Fact | Decision |
| --- | --- | --- |
| Watch layout | Width-only swipe subcomposition ran every frame | Ordinary stable measurement removes it; layout falls to ~0 ms, CPU 22.6→18.6 ms/frame |
| Watch recording | Post-inlining: recorder 4.70 ms, scope 1.40; total app CPU 17.87 ms/frame | Exact one-quad return improves every watch game pair; keep direct GPU columns |
| Watch app counter | Primitive-counter TLS ~1.1 ms before layout change | Outside Cranpose-only scope; exclude from promised savings |
| Showcase shading | Substrate specialization helps watch but loses slightly on Huawei warm scroll | Hold; exact pixels alone do not prove a performance win |
| Geometry lifetime | Two recording generations already permit buffer reuse | No new pool, snapshot or thread |
| Huawei scheduling | Most idle iterations update without visual change; Android truncates submillisecond waits | Test rounding waits up; the credit-only ablation did not help |

**Next:** remove timeout spinning; reduce GPU work across several effect passes.
Swipe alone lost 0.43 FPS in Huawei scroll; the combined checkpoint recovers
that workload. Showcase never calls swipe; code layout remains an unproven cause.
No new rendering architecture is justified by the remaining CPU profile.
Showcase still needs a separate GPU cost reduction.

**Held shader contract:** specialize actual bound regions; cache keys include
presence. Absent glass regions stay dynamic: folding changed one watch pixel.
No image cache, approximation or larger shader object (208 bytes).

**Rejected by device evidence:** compute blur, moving-ring image caching,
previous-row reuse, scalar dictionaries, smaller curve strides and extra recording threads.
Manual normalized-band construction and isolated substrate specializations
also failed to show a native improvement; their source remains held.

**Acceptance:** ABAB BABA; temperatures before/after each leg; no cooling.
Game: first 10 seconds. Scroll: visible final card. Keep failed/thermal legs.
Huawei launch windows include startup; do not pool them with presentation windows.
Correctness guards must fail when skipped work changes output.
[Evidence](mobile_watch_performance.md).
