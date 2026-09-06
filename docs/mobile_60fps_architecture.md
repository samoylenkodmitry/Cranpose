# Mobile 60 FPS

**Unmet. PR #617 is not ready for main.** Cranpose changes only; preserve apps,
native resolution, effects and main’s picture correctness.

| Previous 60-second baseline | Main | Shared renderer |
| --- | --- | --- |
| Huawei Megaboss | 57–58 FPS | ~60 FPS |
| Huawei full scroll | 25–29 | 40–45 |
| Watch Megaboss | 40→16 | 36→17; early pairs regress |
| Watch full scroll | Slower in three valid pairs | Below 60; full comparison incomplete |

Megaboss comparisons use the first 10 seconds from launch, ABAB BABA,
without cooling; keep temperatures and failed legs. Watch full scroll requires
16 measured swipes (~24 seconds); ten-second watch scrolls cover the opening.
Prior hot budgets:
Megaboss main thread 37.7 ms, GPU ~58 ms; Showcase GPU 59.5 ms
(glass 24, substrate blur ~9.5, other work ~26). These costs overlap.

**Current decisions**
- Geometry reuse: removed from the working branch. Mac recording −7%;
  hot-watch FPS tied at 16.85–16.88. More cache machinery did not help.
- Short arcs (`7839ac18`): same shading, cheaper vertices. Matched-clock watch
  pairs +9.9/+11.0/+7.0%; Huawei within ±0.12 FPS. First pair crosses throttling.
  Cap-padding mutant fails; restored pixels/coverage pass.
- Backdrops: pin immediately; release when the key changes or node vanishes.
  Mac cache 3–7 entries/4 MB, zero steady allocation. Short scroll pairs:
  watch opening −0.36 to +0.59 FPS; Huawei full scroll −0.48 to +6.10. Below 60.
- Span images: prototype stashed. Translucent grouping reaches 3 channel
  levels of error; main reaches 1 in the corresponding fixture. Removing
  translucency passes, but only 0.22% of first-ten-second game arcs are opaque.

**Next:** Fable's gradient-specific shader interface, with unchanged shading.
Span reuse needs main's blending fidelity before more FPS measurements.
Use the existing surface cache; do not add another matcher tree or threads
to compensate for repeated work.

Raw results: [measurement index](mobile_watch_performance.md).
