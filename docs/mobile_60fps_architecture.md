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
- **Watch Megaboss:** shared needs about 20% fewer CPU cycles than main in both
  measured orders. Discarding shape fragments gains only 2.0–2.8 FPS at 43–45°C.
  Remaining frame time does not distinguish CPU, vertex and tiler costs.
- **Main's reuse matters:** disabling its command feed on Huawei loses
  2.4/3.5/5.9/5.0 FPS. First-pair upload medians rise 0.35→2.54 MB.
  Later upload logs are incomplete; the switch changes more than bandwidth.
- **Repeated construction:** 94% of Huawei scroll's sampled `memcmp` CPU time
  comes through `RuntimeShader::new`. Watch caller attribution is pending.

## Decisions

| Work | Evidence required before adoption | Owner |
| --- | --- | --- |
| Reuse glass samples with exactly equal coordinates | Frozen pixels on Adreno; broken guards fail; eight-leg FPS comparison | Fable + Codex |
| Share immutable built-in shader state; stream override hashing | Equality/mutation tests fail when broken; ARM cost and app comparisons | Codex |
| Recover main's semantic geometry reuse | Watch removal test; CPU/GPU attribution; exact transform/clip/order tests | Codex |
| Change shape geometry | Vertex removal with same-leg timing; fragment bounds alone are insufficient | Fable |

Keep the shared base for investigation. Hold streaming, extra threads and new
caches. Shared recording already seals ownership once and normalizes arcs once.
CPU time, cycles, temperature and endpoint clocks are separate observations;
none alone measures energy. Favor less total work over more overlap.

**Merge to main only after:** broken correctness tests fail, restored tests pass,
platform/robot gates pass, and all four workloads sustain 60 Hz without picture
or FPS regression. Artifacts: [measurement index](mobile_watch_performance.md).
