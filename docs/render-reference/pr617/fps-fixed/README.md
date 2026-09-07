# SOTA FPS after the renderer fix

- LeetCodeDaily [paired comparison](../leet-paired/README.md): Main 26.92 FPS, SOTA 41.20 FPS on the same display and route.
- Measured on 2026-09-07 with Cranpose `8944038b`; all four samples per workload remain in the mean.
- These SOTA-only samples establish current performance; different temperatures and no paired baseline prevent a speedup claim.
- All three workloads remain below 60 FPS; [raw counters, durations and temperatures](summary.json) retain full precision.

| Workload | Mean FPS | Range | Samples | Battery °C |
| --- | ---: | --- | ---: | --- |
| CranScan Settings — Huawei Mate 20 X | **54.12** | 53.67–54.51 | 4 | 31.0–33.0 |
| CranScan Settings — Pixel Watch 3 | **41.76** | 34.26–46.45 | 4 | 33.5–36.8 |
| LeetCodeDaily — M3 Pro, VSync | **41.22** | 40.86–41.77 | 4 | Unavailable |

- CranScan uses unchanged app `c61958e` (1.0.22), the common Android host and existing data; the fixture launch hook is absent.
- Huawei uses ARM64 with in-process AI disabled in settings; watch uses the shipped ARMv7 build without that backend.
- Both native builds use Rust 1.98.0, release optimization 3, full LTO and one codegen unit; frozen consumer/vendor warnings remain.
- Android FPS divides application-layer SurfaceFlinger presents by device elapsed time, including turnaround capture and collection overhead.
- Huawei passes all four endpoint checks; watch passes three, with run 2 returning above the starting section and its heading behind navigation.
- The failed watch run remains included; its [return capture](watch/2/end.png) shows visible content, and [raw OCR](../cranscan/raw-ocr.json) retains the failure.
- These diagnostic round trips take 20.016–20.033 seconds on Huawei and 20.754–21.982 seconds on watch.

| Device / run | FPS | Frames | Seconds | Battery before → after °C | Endpoint check |
| --- | ---: | ---: | ---: | --- | --- |
| Huawei 1 | 54.26 | 1087 | 20.033 | 32.0 → 31.0 | Pass |
| Huawei 2 | 53.67 | 1075 | 20.031 | 32.0 → 32.0 | Pass |
| Huawei 3 | 54.04 | 1082 | 20.022 | 32.0 → 32.0 | Pass |
| Huawei 4 | 54.51 | 1091 | 20.016 | 32.0 → 33.0 | Pass |
| Watch 1 | 34.26 | 753 | 21.982 | 33.5 → 34.3 | Pass |
| Watch 2 | 40.81 | 867 | 21.247 | 35.3 → 35.9 | Fail; retained |
| Watch 3 | 45.52 | 953 | 20.937 | 36.0 → 36.4 | Pass |
| Watch 4 | 46.45 | 964 | 20.754 | 36.3 → 36.8 | Pass |

- LeetCodeDaily uses app `47aaabb5` (0.1.47), Rust 1.98.0 and `cargo build --release --locked` with the fixed framework.
- The isolated harness preserves the app UI, default draft, release profile, 1480×1560 logical window and normal VSync pacing.
- macm3 has an M3 Pro with 18 GPU cores and an active HISENSE display at 60 Hz, 3200×1800 logical and 6400×3600 backing pixels.
- The route aligns “Rust Code,” then scrolls at 480 logical pixels/second in five four-second round trips with 1,200 wheel inputs.
- FPS uses app presentation counters over each 20-second route; [robot captures](leetcodedaily/4/start.png) occur outside the timing window.
- Reproduce with the [launch/storage patch](leetcodedaily/harness.patch), [driver](leetcodedaily/driver.rs.txt), [lockfile](leetcodedaily/Cargo.lock) and [source hashes](leetcodedaily/build.json).
- Runs 2–4 retain identical binary hashes and on-screen window metadata; macOS reports no recorded thermal warning and provides no numeric temperature.
- An [uncapped preflight](leetcodedaily/excluded-uncapped-preflight.json) reached 168.44 FPS and is excluded because the robot driver selected NoVsync.
- The separate native macOS video attempt returned without creating a file; LeetCodeDaily evidence consists of counters and robot captures.

| LeetCodeDaily run | FPS | Frames | Seconds | Rolling p95 / p99 ms |
| --- | ---: | ---: | ---: | --- |
| 1 | 41.77 | 836 | 20.013 | 34.02 / 49.98 |
| 2 | 41.01 | 821 | 20.017 | 47.36 / 49.99 |
| 3 | 40.86 | 818 | 20.018 | 34.24 / 49.81 |
| 4 | 41.24 | 825 | 20.003 | 37.16 / 49.40 |

- The fresh [Huawei recording](https://github.com/user-attachments/assets/453123ef-cdf5-4e0a-aa09-5c748772bdbb) uses the measured APK and checks all 1,462 frames with zero detected card cutouts.
- The card scan covers rows with both white edges visible; [per-frame results](motion/every-frame.csv) and the [worst candidate](motion/worst-00904.png) retain that limited check.
- Video recording is separate from FPS timing; [video/APK hashes](motion/video.json) and [artifact hashes](files.json) tie the evidence together.
- Full Android reports and protocol snapshots remain in `/tmp/cranpose-mobile-watch-60fps/cranscan-sota-fixed-8944038b`; LeetCodeDaily source/results remain in `/tmp/cranpose-pr617-artifacts-regression/leet-results`.
