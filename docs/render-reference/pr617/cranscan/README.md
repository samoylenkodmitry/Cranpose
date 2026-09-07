# Cranscan Settings measurements

- Acceptance is blocked by a persistent Huawei blank body, a watch route failure and a watch timing overrun; 60 FPS remains unmet.
- Track the Huawei blank body in [#620](https://github.com/samoylenkodmitry/Cranpose/issues/620), the watch regression in [#621](https://github.com/samoylenkodmitry/Cranpose/issues/621), and reusable measurement tooling in [#619](https://github.com/samoylenkodmitry/Cranpose/issues/619).
- Compare main `0d195313` with PR `010d3836` using unchanged Cranscan `c61958e` (1.0.22), Rust 1.98.0, release optimization 3, full LTO and one codegen unit on macm3.
- Both arms use the same SOTA Android host, signer, package, data and 428 non-native payload files; this isolates native framework changes rather than comparing independently built Android hosts.
- Huawei uses ARM64 with `ai-inprocess`, 15 documents and AI/backup off; watch uses the shipped ARMv7 feature set, zero documents and no in-process AI backend.
- Each device ran ABAB then BABA under one sequence lock, with no cooling waits; all runs and failed attempts remain in [the evidence](evidence.json).
- FPS counts physical application presents divided by device monotonic elapsed time; turnaround capture and collection overhead are included, so these are diagnostic round trips rather than exact 20-second acceptance windows.
- Pre-route CPU samples and thermal status are recorded; they do not exclude work starting during a route, and the hot watch displayed its thermal recognition hold.
- Native builds succeeded with unused patch, frozen-app and vendored llama warnings; they do not meet the zero-warning gate.
- Post-build resolution has matching package names, versions and sources; SOTA adds the `cranpose-ui-graphics` to `bytemuck` dependency edge, and build-time lockfiles were not separately archived.

| Device | Main FPS | PR FPS | Change | Complete routes main / PR | Battery °C | Actual seconds |
| --- | ---: | ---: | ---: | --- | --- | --- |
| Huawei | 52.36 | 54.04 | +3.21% | 4/4 / 3/4 | 33.0–35.0 | 20.029–20.047 |
| Watch | 35.03 | 27.15 | -22.51% | 4/4 / 3/4 | 37.4–42.8 | 20.908–22.966 |

- Huawei gains all four FPS pairs but leg 7 returns to a blank Settings body for at least another 4.2 seconds; the same leg failed in the preceding sequence.
- Watch loses three FPS pairs and gains one; leg 2 misses both endpoints, and leg 7 reaches both but takes 22.966 seconds with 3.293 seconds spent on turnaround capture.
- Watch leg 1 visibly reaches the first section despite an OCR spelling error; original automated results and separate capture-review corrections are retained.
- Uncorrected machine text lives in [raw OCR records](raw-ocr.json), the only added spelling exclusion; authored analysis and the evidence metadata remain checked.

| Device / leg | Build | FPS | Seconds | Battery before → after °C | Route | Start / turn / return captures |
| --- | --- | ---: | ---: | --- | --- | --- |
| huawei 1 | main | 52.18 | 20.047 | 33.0 → 33.0 | complete | [start](huawei/1-main/start.png) · [turn](huawei/1-main/turn.png) · [end](huawei/1-main/end.png) |
| huawei 2 | sota | 54.16 | 20.032 | 33.0 → 33.0 | complete | [start](huawei/2-sota/start.png) · [turn](huawei/2-sota/turn.png) · [end](huawei/2-sota/end.png) |
| huawei 3 | main | 53.41 | 20.034 | 33.0 → 33.0 | complete | [start](huawei/3-main/start.png) · [turn](huawei/3-main/turn.png) · [end](huawei/3-main/end.png) |
| huawei 4 | sota | 54.21 | 20.034 | 33.0 → 33.0 | complete | [start](huawei/4-sota/start.png) · [turn](huawei/4-sota/turn.png) · [end](huawei/4-sota/end.png) |
| huawei 5 | sota | 53.97 | 20.030 | 33.0 → 33.0 | complete | [start](huawei/5-sota/start.png) · [turn](huawei/5-sota/turn.png) · [end](huawei/5-sota/end.png) |
| huawei 6 | main | 52.02 | 20.029 | 33.0 → 34.0 | complete | [start](huawei/6-main/start.png) · [turn](huawei/6-main/turn.png) · [end](huawei/6-main/end.png) |
| huawei 7 | sota | 53.81 | 20.032 | 34.0 → 33.0 | failed | [start](huawei/7-sota/start.png) · [turn](huawei/7-sota/turn.png) · [end](huawei/7-sota/end.png) |
| huawei 8 | main | 51.81 | 20.033 | 33.0 → 35.0 | complete | [start](huawei/8-main/start.png) · [turn](huawei/8-main/turn.png) · [end](huawei/8-main/end.png) |
| watch 1 | main | 44.95 | 21.044 | 37.4 → 38.3 | complete | [start](watch/1-main/start.png) · [turn](watch/1-main/turn.png) · [end](watch/1-main/end.png) |
| watch 2 | sota | 24.34 | 20.908 | 38.3 → 38.3 | failed | [start](watch/2-sota/start.png) · [turn](watch/2-sota/turn.png) · [end](watch/2-sota/end.png) |
| watch 3 | main | 46.54 | 21.143 | 38.3 → 40.2 | complete | [start](watch/3-main/start.png) · [turn](watch/3-main/turn.png) · [end](watch/3-main/end.png) |
| watch 4 | sota | 31.68 | 21.182 | 41.0 → 41.0 | complete | [start](watch/4-sota/start.png) · [turn](watch/4-sota/turn.png) · [end](watch/4-sota/end.png) |
| watch 5 | sota | 34.27 | 21.068 | 41.0 → 42.2 | complete | [start](watch/5-sota/start.png) · [turn](watch/5-sota/turn.png) · [end](watch/5-sota/end.png) |
| watch 6 | main | 24.83 | 21.583 | 42.2 → 42.2 | complete | [start](watch/6-main/start.png) · [turn](watch/6-main/turn.png) · [end](watch/6-main/end.png) |
| watch 7 | sota | 18.29 | 22.966 | 42.8 → 42.8 | complete | [start](watch/7-sota/start.png) · [turn](watch/7-sota/turn.png) · [end](watch/7-sota/end.png) |
| watch 8 | main | 23.79 | 21.939 | 42.8 → 42.8 | complete | [start](watch/8-main/start.png) · [turn](watch/8-main/turn.png) · [end](watch/8-main/end.png) |

- Raw protocol snapshots, source inventories, APKs, native build logs and preliminary attempts remain under `/tmp/cranpose-mobile-watch-60fps/cranscan-settings`; committed captures and whitespace-normalized SurfaceFlinger dumps retain both source and committed SHA-256 hashes in `evidence.json`.
