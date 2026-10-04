# Frame cost on the Kirin 980

Measurement dates: 2026-08-29 and 2026-08-30. Device: Huawei Mate 20 X
(EVR-AL00). Apps: CranScan and Cranpose Showcase. These captures describe the
source and workloads from the experiment. Later reports cover
[shader metadata](huawei_shader_metadata.md), [observer allocations](showcase_observer_allocations.md)
and [Showcase frame budgets](huawei_showcase_frame_budget.md).

## Stable backdrop

The experiment compares one radial-gradient rectangle and 160 circles under
three conditions:

| Arm | FPS | Render encode p50 | Present p50 |
| --- | ---: | ---: | ---: |
| Animated layer | 22.5–24.6 | 11.8 ms | 31.9 ms |
| Frozen layer | 33–35 | 5.6–6.0 ms | 21.7–22.9 ms |
| Layer removed | 42.2 | 5.7 ms | 16.4 ms |

The frozen layer reduces CPU work. Its GPU draw cost remains visible in the
present span. The subsequent prefix-snapshot comparison gives 37.2/36.2 FPS
with snapshots off and 40.3/40.3 FPS with snapshots on, at 40–41°C.

Prefix snapshots preserve the direct draw order and blend-round sequence.
The [render architecture](render_arch.md) describes current cache behavior;
[Android surface regression evidence](cranscan_render_regression.md) covers
the later composite-path correction.

## CPU work

The experiment records about 37 µs of work per solid-circle primitive, or
about 6 ms for 160 circles. The figure applies to this device and source.
A frame-driven Showcase animation also produces 13–19 ms of update work and
constructs fourteen runtime shaders per frame.

A separate state-read correction reduces the scroll-route update p50 from
15.5 ms to 5.7 ms. Frame rate stays constant as the acquire wait grows. The
result identifies extra CPU headroom within a GPU-bound route.

## Pass and effect cost

| Workload | Passes | Blur passes | Offscreen acquires | Fill |
| --- | ---: | ---: | ---: | ---: |
| Idle list | 16–17 | 0 | 0–1 | 9.3 Mpx |
| Continuous drag | 38–39 | 9 | 18 | 11.8 Mpx |

The sampled present p50 rises from 16.6 ms to about 31 ms. Full-width glass
surfaces account for most blur work in the sampled frames.

A half-scale backdrop probe reduces present p50 by 2.25 ms and increases
render encode by about 1.8 ms. FPS stays constant. Pixel-space refraction
uniforms produce visible seams under the scale change. Each effect therefore
requires both a pixel check and a device measurement after a resolution change.

A separate quantized-animation comparison gives 27.0–29.6 FPS for continuous
motion and 38.2–38.6 FPS for quantized motion at 39°C. The route reaches
48.7 FPS with the field removed.

## Measurement rules

- Measure production FPS on a physical display. Xvfb measures software presentation.
- Use full-window totals for cache rates. A periodic GPU log describes one frame.
- Compare means or matched per-frame spans for additive costs. Per-stage
  medians can describe different frames.
- Record temperature and alternate the arms. These captures show about 2 FPS
  of thermal drift around 38–39°C.
- Verify the installed APK hash and application source for each arm.
- Preserve captures with each frame-time result. The half-scale probe shows why
  frame rate alone provides incomplete acceptance evidence.

For the current telemetry and device protocol, use
[device measurement](device_measurement.md) and
[Android benchmark tools](android_benchmark.md).
