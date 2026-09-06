# Mobile 60 FPS

**Unmet. PR #617 is not ready for main.** Cranpose only; preserve resolution,
effects and coverage. Shared `72f1dd63` beats main `0d195313` in four opening-game
pairs on both devices: watch +10.88–14.48 FPS (36.9–41.9°C), Huawei +1.20–10.59
(31–32°C). Shared watch 38.2–48.5 FPS; Huawei 59.5–59.7.

| Work | Measured cost or result | Decision |
| --- | --- | --- |
| Megaboss CPU | 20–21 ms/frame; recording dominates | Remove repeated preparation before adding concurrency |
| Arc recording + bounds | Direct GPU columns; tighter one-quad padding and butt-cap bound. Five pairs with matching endpoint clocks gain 2.2–5.8%; thermal crossings include losses | Checkpoint; original GPU layout and exact pixel guards |
| Megaboss GPU | Padding-only diagnostic: 21–26 ms/frame. Frozen arc fixture: padding −4.77 ms, butt bound another −2.78 ms | CPU and GPU both need cuts; fixture speed is not app FPS |
| Showcase GPU | Hot: 31.4 ms; cards 11.6, header ~10, blur 6, page 2.4 | Preserve effects; remove redundant shading and passes |
| Compute blur | Huawei ~37→21 FPS; watch loses device | Reject prototype; preserved in stash `59a4098e` |

**Next experiment:** exact radius/sweep preparation reuse. Captured Megaboss
frame: 14,973 arcs, 281 radius/sweep tuples; 16-entry reuse hits 98.1%.
Start angles stay per arc. Measure lookup cost before changing production.

**Larger design:** main matches persistent ranges before preparation and retains
GPU geometry. A restored motion path must place fresh and retained vertices and
evaluate their SDFs in the same local coordinates; dither stays device anchored.
Image caching after preparation loses 6.3–7.6 FPS and adds draws: rejected.
Smaller curve layouts, global vertex pulling and queue threading also lost.

**Gate:** 10-second opening, ABAB BABA, temperatures on every leg, no cooling.
Audio-launch and first-presentation windows stay separately labelled. Retain
thermal crossings and failed legs. Full watch scroll: 16 forward swipes, 1.5 s
apart. Performance guards must fail when their skipped work affects correctness.
Raw comparisons, exact binaries and guard results: [evidence index](mobile_watch_performance.md).
