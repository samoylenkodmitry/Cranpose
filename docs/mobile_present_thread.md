# Android presentation overlap

- Enable the presentation worker when Android reports at least four available cores; retain explicit overrides and synchronous execution below four cores.
- The Pixel Watch 3 serialized UI preparation and rendering; enabling the existing worker in the same Main binary raised the complete Settings diagnostic route from 46.05 to 55.34 FPS.
- The queued-packet guard compares every pixel of twelve changing frames against serial rendering and fails when the consumer reports presentation without drawing.
- Main is `dc1c14498891989ec48e3bf5e36921017f6aed13`; SOTA changes the Android policy, with identical application sources, assets, features, build commands and toolchains within each comparison.

| Device | App | Main FPS | SOTA FPS | Change | Runs/build | Battery °C |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Pixel Watch 3 | CranScan Settings | 37.22 | 52.67 | +41.51% | 4 | 40.8–41.6 |
| Pixel Watch 3 | Showcase | 53.61 | 55.01 | +2.61% | 4 | 33.7–38.7 |
| Pixel Watch 3 | Megaboss | 56.45 | 57.78 | +2.37% | 4 | 36.3–42.0 |
| Huawei Mate 20 X | CranScan Settings | 53.96 | 53.88 | −0.14% | 4 | 31.0–31.0 |
| Huawei Mate 20 X | Showcase | 31.40 | 30.98 | −1.33% | 8 | 29.0–31.0 |
| Huawei Mate 20 X | Megaboss | 58.15 | 58.13 | −0.03% | 4 | 30.0–31.0 |

- Physical SurfaceFlinger measurements use ABAB BABA without cooling waits; videos are separate and excluded from FPS means.
- Huawei Showcase pools both complete batches; their changes are −2.74% and +0.09%, so no phone gain is established.
- CranScan uses a complete forward-and-return Settings route; its first Huawei batch contains one incomplete route and remains diagnostic evidence outside the table.
- Retain the watch footer OCR failure and its visual review: a thermal toast covers the caption while the version heading confirms completion.
- The 60 FPS target remains unmet; one physical watch model is covered, and thermal throttling remains part of the measured workload.
- Continue from the [mobile performance constraints](mobile_60fps_architecture.md) and [device protocol](device_measurement.md).
