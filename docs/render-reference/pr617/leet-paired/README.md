# LeetCodeDaily Main vs SOTA

- Main **26.92 FPS**; SOTA **41.20 FPS**; four samples each in uninterrupted ABAB BABA order on 2026-09-07.
- Framework: Main `0d195313`, SOTA `8944038b`; identical LeetCodeDaily `47aaabb5` (0.1.47), UI, assets, draft and harness.
- Build: Rust 1.98.0, `cargo build --release --locked`, normal app release profile; both builds finish without warnings.
- Display: macm3 M3 Pro, 18 GPU cores, physical HISENSE at 60 Hz; 3200×1800 logical and 6400×3600 backing pixels.
- Route: 1480×1560 logical window, normal VSync, five four-second scroll round trips at 480 logical pixels/second.
- Counting: app presentation counter divided by elapsed time; captures occur outside the measured window.
- Temperature: macOS exposes no numeric reading here; every retained thermal snapshot reports no recorded warning.
- Provenance: [binary hashes and 796 verified Main source files](build.json), [harness](../fps-fixed/leetcodedaily/harness.patch), [driver](../fps-fixed/leetcodedaily/driver.rs.txt).
- Dependencies: [SOTA lockfile](../fps-fixed/leetcodedaily/Cargo.lock), [Main lockfile difference](main-lock.patch), [source artifact hashes](files.json).

| Leg | Build | FPS | Frames | Seconds | Inputs |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | main | 26.50 | 531 | 20.041 | 1199 |
| 2 | sota | 41.31 | 827 | 20.017 | 1200 |
| 3 | main | 27.19 | 545 | 20.042 | 1200 |
| 4 | sota | 41.13 | 823 | 20.010 | 1200 |
| 5 | sota | 40.98 | 820 | 20.008 | 1200 |
| 6 | main | 26.99 | 541 | 20.042 | 1200 |
| 7 | sota | 41.37 | 828 | 20.014 | 1200 |
| 8 | main | 27.00 | 541 | 20.040 | 1200 |

- Captures: [Main viewport](pair-1-main/start.png), [SOTA viewport](pair-2-sota/start.png); all eight counters, window records and thermal snapshots remain in their leg directories.
- Full captures and process snapshots remain under `/tmp/cranpose-pr617-artifacts-regression/leet-paired`; all runs are retained in [the summary](summary.json).
