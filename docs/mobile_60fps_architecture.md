# Mobile 60 FPS

**Target unmet.** Checkpoint `1767a226`; Cranpose only, unchanged pixels.
Frame budget: **16.67 ms**. Shared game build beats main on both devices.
Swipe layout: watch **47.3→56.1 FPS**, four gains, **35.1→41.0°C**.
Recording inlining then reaches **57.7 FPS**; Huawei remains near 59.6 steady.
Full scroll: watch **49.11→49.45**, Huawei **31.45→32.21 FPS**, mixed pairs.

| Cost | Fact | Decision |
| --- | --- | --- |
| Watch layout | Width-only swipe subcomposition ran every frame | Ordinary stable measurement removes it; layout falls to ~0 ms, CPU 22.6→18.6 ms/frame |
| Watch recording | Arc preparation 3.15 ms, column append 2.25, normalization 1.12 before inlining | Two compiler annotations win; preserve arithmetic and direct GPU columns |
| Watch app counter | Primitive-counter TLS ~1.1 ms before layout change | Outside Cranpose-only scope; exclude from promised savings |
| Showcase shading | Substrate specialization helps watch but loses slightly on Huawei warm scroll | Hold; exact pixels alone do not prove a performance win |
| Geometry lifetime | Two recording generations already permit buffer reuse | No new pool, snapshot or thread |
| Huawei scheduling | 1,500–2,400 idle iterations per 120 frames; scheduling ignores exhausted presentation credit | Measure removing blocked wakeups before designing the fix |

**Next:** reduce remaining recording calls and measure credit-aware wakeups.
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
