# Mobile renderer: evidence index

Decision: [60 FPS sheet](mobile_60fps_architecture.md). Raw evidence:
`/tmp/cranpose-mobile-watch-60fps`. Reports retain binaries, scene checks,
SurfaceFlinger frames, temperatures, properties and failures.

| Artifact under the raw root | What it proves |
| --- | --- |
| `mixed-atlas-substrate-correctness.json` | Mixed blur/average atlas regression: before preservation, expected RGB ~234/124/68 became black. Red reported from the agent run; original raw log not retained. Shared block-mean guard and all 21 atlas tests pass after the fix |
| `arc-bounds-direct-v1-device-analysis.json` | Direct body/curve recording plus tighter arc quads. Watch game-window deltas −2.677/+0.670/+0.837/+0.957 FPS (42.5→43.9°C); separate presentation-window deltas −13.821/+0.551/+4.839/+0.754 (41.9→43.8°C). First pairs cross throttling; presentation pair three also crosses, favouring the candidate. Five pairs with matching endpoint clocks gain 2.2–5.8%. Huawei presentation-window +0.075/+0.024/+0.121/+0.099, 32→33°C. All eight legs valid in each matrix |
| `arc-quad-bounds-{red,restored}.log`, `watch-arc-quad-bounds-v1-oracle/`, `arc-bounds-direct-recording-payload-proof.json` | Removing cap padding fails the original-quad pixel guard; restored passes on Metal and Adreno. Direct recording preserves all 14,973 fixture body/curve rows byte for byte; swapped radii fail the recording guard |
| `watch-band-margin-half-v1/`, `watch-butt-quad-v1/` | Frozen Megaboss arc fixture GPU reductions: margin 25.64→20.87 ms (35.1→35.0°C); butt bound 21.42→18.64 ms (33.3°C). Four positive pairs each; exact pixels on Metal/Adreno. These are GPU probes, not app FPS |
| `arc-quad-margin-v2-budget-analysis.json`, `watch-arc-quad-margin-v2-game10-budget/` | Ten-second CPU/GPU diagnostic. Arc append 3.21 ms/frame self, shape append 2.76, normalization 1.18; instrumentation included. GPU 21–26 ms. Acceptance runs separately show CPU 20–21 ms |
| `arc-scalar-reuse-frame120-census.json` | Exact normalized radius/sweep tuples: 281 among 14,973 arcs, 98.1% FIFO16 hits; including start angle removes almost all reuse. Cost reduction unproven |
| `fable-compute-blur-held-20260906-*`, `blur-{watch,huawei}-*` | Rejected compute prototype, stash `59a4098e`: Huawei 21.05–21.29 vs fragment 37.06–38.32 FPS; all four watch compute legs lose the GPU device, then fail timestamp readback. Pass timing enabled; diagnostic only |
| `shared72-main-game10-analysis.json` | Exact shared `72f1dd63` vs main `0d195313`, ten seconds after the first ball launch; all eight legs valid on each device. Watch four gains +10.88–14.48 FPS at 36.9→41.9°C; Huawei +1.20–10.59 at 31→32°C. Whole branch comparison; watch 60 remains unmet |
| `aligned-game10-analysis.json`, `aligned-streams-correctness-proof.json` | Aligned 64/16/8 GPU streams rejected: watch −3.23/−0.47/−0.41/+0.02 FPS, 42.6→44.4°C; Huawei nil. Frozen radii fail the pixel guard, restored passes. Stash `9df6b9bf` |
| `oriented-arc-first10-analysis.json`, `{watch,huawei}-oriented-arc-v1-first10-matrix.json` | Complete short Megaboss pairs: watch +7–11% at matching endpoint clocks; first pair crosses throttling; Huawei unchanged. Requested 10 s, actual elapsed retained |
| `arc-quad-pixel-proof.json`, `arc-quad-{band-fill,coverage}.log` | Missing cap padding fails; restored pixel and analytic coverage pass |
| `command-surfaces-rejection.json`, `{watch,huawei}-command-surfaces-v5-first10-matrix.json` | Moving-ring images rejected: all four watch pairs lose 6.3–7.6 FPS; Huawei roughly tied. Earlier picture rejection mixed readback formats and is invalid |
| `direct-columns-cpu-matrix.json`, `direct-columns-radius-swap-red.log`, `direct-columns-first10-analysis.json` | Direct column recording: Mac CPU −6.4–12.1%; all 600 frame fingerprints equal; swapped radii fail. Watch pairs +3.53/+2.08/−2.45/+6.30 FPS; Huawei within ±0.26; incremental FPS mixed |
| `main-direct-columns-first10-analysis.json` | All eight legs valid on each device; Huawei beats main in four pairs by 2.17–5.37 FPS, watch in three by 1.34–2.45 with one −0.48; watch 43–44°C and below 11 FPS. Whole-branch comparison, not direct-recording attribution |
| `huawei-command-surfaces-v5-cpu-cause/cpu-profile.json`, `span-match-cheaper-first10-cpu.json` | Rejected cache matcher ~23% of Huawei self cycles; equivalent matches on 600 frames with 29% lower Mac matcher CPU cost. No native gain claimed |
| `curve24-first10-analysis.json`, `curve24-correctness-proof.json` | 25% fewer curve bytes; three watch pairs lose, last −1.64 FPS at matching endpoint clocks; first candidate has cold shader compilation. Held; exact reconstruction and strip-geometry mutations fail |
| `unclipped-solids-radii-red.log`, `unclipped-solids-rejection.json` | Five-vector solid interface rejected: three watch pairs lose, hot pair −0.88 FPS. Pixels unchanged at four scales; omitted radii change 4,059 pixels |
| `short-scroll-scope-audit.json` | Ten-second watch pinning/gradient samples cover only the opening; full traversal requires 16 measured swipes |
| `{watch,huawei}-semantic-v1-matrix.json` | Geometry reuse: no native FPS gain; stashed |
| `main-shared-profile-pair-20260906.json` | Audited release APKs: main `0d195313`, shared `37bd0ce8`; unchanged application sources |
| `{watch,huawei}-main-shared-checkpoint-{matrix,analysis}.json` | Complete eight-leg Megaboss comparisons; watch result mixed |
| `prof-huawei-showcase-{1..8}-*` | Complete scroll comparison; all shared legs faster, below 60 |
| `prof-watch-showcase-*` | Incomplete watch sequence; failed preflight prevents acceptance |
| `watch-main-shared-cpu-{leaf,hot}-*`, `huawei-main-shared-cpu-*` | Megaboss CPU time/cycles; watch shared uses about 20% fewer cycles in both orders |
| `{watch,huawei}-showcase-cpu-profileonly-*` | Full-scroll CPU diagnostics; Huawei shared halves cycles, watch change smaller |
| `showcase-profileable-pair.json` | Profiling copies differ only by manifest profileability; 393 other payloads unchanged |
| `profd-*-showcase-*`, `watch-main-shared-gpu-repeat-*` | GPU timings; inspect clocks/heat before comparing |
| `huawei-main-feed-removal-{matrix,analysis}.json` | Main feed removal loses every pair; later upload logs incomplete |
| `{watch,huawei}-main-lowering-removal-{matrix,analysis}.json` | Same main APK retains GPU feed while rebuilding retained CPU geometry; Huawei complete, watch pending; no main GPU pass timings |
| `main-lowering-correctness-proof.json` | Existing feed/pixel parity passes; leaked temporary geometry fails; restored passes |
| `{watch,huawei}-showcase-current-memory-stack-*/memory-callers.json` | Current ten-second stacks attribute most memcmp to RuntimeShader construction |
| `override-hash-correctness-proof.json` | Original encoding passes; missing separator fails; restored passes |
| `override-hash-{huawei,watch-short}-matrix.json`, `shader-factory-{watch,huawei}-matrix.json` | Eight-leg ARM microbenchmarks; not app FPS |
| `shader-factory-correctness-proof.json` | Direct-equivalence test passes; corrupt template fails; restored passes |
| `shader-cpu-app-pair.json`, `{watch,huawei}-shader-cpu-checkpoint-matrix.json` | Exact three-file pair; unchanged app payloads; eight valid legs each. Watch gains, Huawei mixed; held |
| `watch-glass-coincident-0d63a76f-proof.json` | Adreno exact candidate passes eight frozen fixtures; both broken guards fail; restored passes |
| `cranpose-shared-rim-56328905-{gates,ios-gates}.json` | Shared platform/robot gates pass; `37bd0ce8` correction also passes |
| `cranpose-override-hash-stream-39c8804a-*-gates.json` | Hash platform gates and 166 robot tests pass |
| `cranpose-shader-factory-2bb5c900-{gates,local-mac-gates}.json` | Platform gates and 166 robot tests pass; separate Mac disk-guard failure retained |
| `sequence-ownership.jsonl` | Whole-sequence device ownership shared with Fable |

Fable owns the `shape`, `shape_fill`, `glass_dispersion`, `glass_refraction`
removal matrices and exact coincident-ray app comparison. These change material
or coverage and bound costs; they cannot prove picture-preserving performance.

CPU captures validate 200 Hz event attributes and exact installed native payloads.
Profiles are diagnostics, not FPS acceptance. Leaf samples do not identify callers;
DWARF captures are limited to ten seconds. Temperature and endpoint clocks do not
measure energy or continuous frequency. Presentation percentiles are bucket labels.
Failed runs remain invalid. No software-display FPS is used for device acceptance.
