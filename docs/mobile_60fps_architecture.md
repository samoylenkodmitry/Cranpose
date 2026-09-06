# Mobile 60 FPS: decision sheet

**Target unmet; PR #617 is not ready for main.** Change Cranpose only. Preserve
native resolution, effects, application code and main's picture correctness.

## Checkpoint

Main `0d195313` versus shared `37bd0ce8`. Release apps; 60-second legs;
ABAB then BABA without cooling. Heat and failed legs remain in the evidence.

| Workload | Main FPS | Shared FPS | Result |
| --- | --- | --- | --- |
| Huawei Megaboss | 57.26–58.38 | 59.84–59.88 | At the 60 Hz cap |
| Huawei full scroll | 25.12–29.06 | 39.50–45.30 | Faster; short of 60 |
| Watch Megaboss | 39.51→15.71 | 36.45→16.82 | First two pairs regress; hot pairs improve; 37.5→44.8°C |
| Watch full scroll | Three valid pairs favor shared | Eight-leg sequence incomplete | Preflight failure blocks acceptance; below 60 |

## Costs that constrain the design

- **Watch glass:** shared scroll uses 39.5 ms of GPU work per frame: 32.7 ms
  in layers and 6.6 ms in blur. Removing blur alone cannot reach 16.7 ms.
- **Watch Megaboss CPU:** shared main thread 37.67 ms/frame; arc preparation
  and packing account for 13.04 ms. Main records raw arcs in 1.54 ms but spends
  4.92 ms matching spans. These self samples do not prove a net reuse saving.
- **Main's reuse matters:** disabling its command feed on Huawei loses
  2.4/3.5/5.9/5.0 FPS. First-pair upload medians rise 0.35→2.54 MB.
  Later upload logs are incomplete; the switch changes more than bandwidth.
- **CPU work skipped by main:** forcing retained geometry materialization,
  while preserving its GPU feed, adds 40–55% main-thread cycles on Huawei;
  FPS stays 57–58 at 31→39°C. Watch measurement pending.
- **Hot watch Megaboss:** discarding shape fragments cuts GPU span 58→26 ms
  while FPS stays 14–15 at 43.7–45.3°C. CPU work still limits this case.

## Decisions

| Work | Evidence required before adoption | Owner |
| --- | --- | --- |
| Exact ray reuse | Reverted: exact pixels, no FPS gain on either GPU | Fable |
| Factory/hash | Held: Huawei +0.16/−1.86/−1.54/−0.24 FPS; watch +8.16/+0.87/+0.32/+0.55, first pair crosses a thermal step | Codex |
| Reuse before geometry preparation | Watch materialization cost; actual animated input reuse; pixel invariants | Codex |
| Glass / shape GPU work | Stages reflect real dependencies; blur taps already paired. Measure specialization cost | Fable |
| Backdrop pin on first sight | Exact (8 glass-cache, 20 parity contracts). Mac still header 6.00→4.68 uncached items/frame; a pin must live exactly as long as its key or the cache saturates (160 entries at 96 MB). Device 10 s ABAB pending | Fable |

**Architecture candidate:** semantic records → reuse verdict → compile changed
ranges → backend buffers. Main can skip lowering; shared lowers before reuse.
Keep one validity owner; preserve order, transforms, clips, paint and resources.
Hold streaming and extra threads. Measure total work and heat; timing is not energy.

**Merge to main only after:** broken correctness tests fail, restored tests pass,
platform/robot gates pass, and all four workloads sustain 60 Hz without picture
or FPS regression. Artifacts: [measurement index](mobile_watch_performance.md).
