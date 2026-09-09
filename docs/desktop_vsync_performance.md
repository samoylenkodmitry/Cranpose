# Desktop VSync

- Two queued VSync frames allow CPU and GPU work to overlap; a one-frame queue serializes them on Metal.

| App and device | Main FPS | SOTA FPS | Change |
| --- | ---: | ---: | ---: |
| LeetCodeDaily · M3 Pro · 60 Hz | 43.95 | 59.76 | +36.0% |

- Protocol: identical app sources, assets and release profile; ABAB BABA; eight 20-second runs with 1,200 scroll inputs each and no cooling waits.
- Route: 960 logical pixels inside scroll bounds; all returns differ by less than 0.001 pixels and all checked endpoint bodies match exactly.
- Guard: nine changing frames across three reused output images; suppressing later surface renders fails at frame 1; restored rendering passes on Metal and Vulkan.
- Footage: [main](https://github.com/user-attachments/assets/01df0b92-7d85-4081-b451-7721e69b66d1) and [SOTA](https://github.com/user-attachments/assets/dc4dd965-0f82-48ac-8f17-110979e8afc2); 1,161 robot readbacks checked separately from FPS measurements.
- Runs, endpoints and thermal snapshots: [legs 1–4](https://github.com/user-attachments/files/31928040/fps-v116-endpoints-1.zip), [legs 5–8](https://github.com/user-attachments/files/31928039/fps-v116-endpoints-2.zip); CPU temperature unavailable, battery sensors retained as raw readings.
- Source inventories, build hashes, red/green logs, frame analysis and reproduction scripts: [measurement data](https://github.com/user-attachments/files/31928167/fps-v116-measurement-data.zip).
- Validation: 4,874 tests, 163 GPU robots and four screenshot robots pass; clippy and precommit pass; [logs and source hashes](https://github.com/user-attachments/files/31928377/fps-v116-verification.zip).
- Tradeoff: one additional pending frame is permitted; input-to-photon latency is unmeasured; explicit Hard60/Hard120 modes keep a one-frame queue.
