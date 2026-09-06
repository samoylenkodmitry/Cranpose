# Mobile renderer: measurement index

Decision: [60 FPS sheet](mobile_60fps_architecture.md). Raw root:
`/tmp/cranpose-mobile-watch-60fps`. Reports retain APK hashes, scene validation,
SurfaceFlinger frames and histograms, temperatures, properties and failures.

| Evidence under the raw root | Meaning |
| --- | --- |
| `main-shared-profile-pair-20260906.json` | Eight audited release APKs; main `0d195313`, shared `37bd0ce8`; later integration changes only docs/tests |
| `{watch,huawei}-main-shared-checkpoint-matrix.json` | Complete eight-leg Megaboss comparisons under normal policy |
| `watch-main-shared-checkpoint-analysis.json` | Mixed paired FPS result, presentation tails and thermal status rising 0→4 |
| `prof-huawei-showcase-{1..8}-*` | Complete normal-policy scroll comparison: every shared leg faster |
| `prof-watch-showcase-*` | Current watch scroll sequence; preflight failure retained, no acceptance claim |
| `watch-main-shared-cpu-leaf-*`, `watch-main-shared-cpu-hot-*` | Full 60-second A/B and 30-second reverse B/A CPU profiles; shared uses about 20% fewer cycles/frame in both orders |
| `huawei-main-shared-cpu-*` | Megaboss CPU profiles; shared reduces CPU time and cycles |
| `huawei-showcase-cpu-profileonly-*` | Full-scroll CPU profiles; driver worker 8.28→2.67 ms/frame, presentation thread 8.09→4.35, main thread 4.34→3.59 |
| `showcase-profileable-pair.json` | CPU-only APK copies: only shell profileability added; all 393 other payloads byte-identical, decoded manifest equivalence, signatures/alignment checked |
| `watch-main-shared-gpu-repeat-*`, `profd-*-showcase-*` | GPU/pass diagnostics; compare temperatures/clocks before attributing differences |
| `cranpose-shared-rim-56328905-{gates,ios-gates}.json` | Workspace/native/wasm/Android/iOS gates and both robot partitions pass; `37bd0ce8` correction gates also pass |
| `watch-glass-explicit-669c85a0-proof.json` | Fixed 7 pass; omission mutant fails declarations; wrong-curve mutant fails pixels; restored 7 pass, exact references unchanged |
| `sequence-ownership.jsonl` | Complete-device leases shared with Fable |

CPU captures validate the recorded 200 Hz event attributes and exact installed
native payloads. The deliberately corrupted high-rate capture is rejected.
Profiler runs are not FPS acceptance. Leaf samples leave vendor/kernel symbols
partly unresolved; they do not measure allocation counts or energy.

Presentation percentile values are histogram bucket labels, not exact durations.
GPU windows are frame-weighted and logs are sliced by byte offset. Frequency
samples inside the driver precede app shutdown; earlier outer-script samples
may describe idle clocks. Failed TLS captures remain invalid, including cleanup
failures; the finalizer now preserves the primary report and has red/green tests.

Held/rejected: streaming, owner reuse, shape spans, activity/finite-curve folds,
body interning, GPU template fetch and approximate shader arithmetic. Reopen only
with new causal evidence. Prior matrices with the presentation thread explicitly
enabled and incomplete normal-policy runs remain raw evidence, not acceptance.
Historical analysis is available in Git at `01eb24cf`.
