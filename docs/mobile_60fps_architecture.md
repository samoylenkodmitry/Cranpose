# Mobile frame budgets

The target is 60 FPS, or 16.67 ms per display frame. The reports below contain
measurements for specific revisions, routes and temperatures. Each candidate
requires fresh acceptance runs on the slowest shipped device.

## Evidence

| Workload or cost | Recorded result | Report |
| --- | --- | --- |
| Pixel Watch 3, CranScan Settings | 37.22 → 52.67 FPS with presentation overlap | [Presentation worker](mobile_present_thread.md) |
| Huawei, CranScan Settings | 53.96 → 53.88 FPS in the same worker comparison | [Presentation worker](mobile_present_thread.md) |
| Huawei, Showcase blur specialization | +3.29% in one complete comparison; −2.53% in the confirmation | [Frame budget](huawei_showcase_frame_budget.md) |
| Huawei, shader declaration metadata | Sampled hash cost 0.234 → 0.041 CPU ms/frame; acceptance 41.32 → 41.71 FPS | [Shader metadata](huawei_shader_metadata.md) |
| Huawei, observer allocation changes | 41.90 → 41.82 FPS | [Observer allocations](showcase_observer_allocations.md) |
| Earlier watch and phone experiments | CPU, GPU, heat, cache and allocation evidence | [Evidence index](mobile_watch_performance.md) |

The paired blur results support the existing shared kernel. The metadata and
observer reports identify CPU work reductions; the paired device results leave
the 60 FPS target open for those workloads. Issue discussions
[#626](https://github.com/samoylenkodmitry/Cranpose/issues/626) and
[#627](https://github.com/samoylenkodmitry/Cranpose/issues/627) retain earlier
follow-up context.

## Acceptance route

Use unchanged app sources, assets, features and build profiles. Measure CranOrbit
Megaboss, Showcase full scroll and CranScan Settings on Huawei and Pixel Watch.

- Use 20-second game windows. The watch window starts at the first presented
  frame; the Huawei window includes launch.
- Reach the last Showcase card and verify the endpoint.
- Traverse CranScan Settings from “On-device intelligence” through
  “Version, licenses, credits, library stats.” and back. Keep the gesture
  sequence, data and expanded sections equal across arms.
- Run ABAB then BABA with continuous device use. Record temperatures before
  and after every leg. Retain failed routes and hot samples.
- Preserve paired screenshots, per-run FPS, endpoint results and background
  activity with the source and build hashes.

Prepare shared immutable data once, record changed data, and preserve draw
order. A cache or thread change requires measured frame savings and a pixel
guard. Follow the [performance guide](performance_coding_guide.md) and
[device protocol](device_measurement.md).
