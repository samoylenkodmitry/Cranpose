# Mobile renderer: evidence index

Decision: [60 FPS sheet](mobile_60fps_architecture.md). Raw evidence:
`/tmp/cranpose-mobile-watch-60fps`. Reports retain binaries, scene checks,
SurfaceFlinger frames, temperatures, properties and failures.

| Artifact under the raw root | What it proves |
| --- | --- |
| `oriented-arc-first10-analysis.json`, `{watch,huawei}-oriented-arc-v1-first10-matrix.json` | Complete short Megaboss pairs: watch +7–11% at matching endpoint clocks; first pair crosses throttling; Huawei unchanged. Requested 10 s, actual elapsed retained |
| `arc-quad-pixel-proof.json`, `arc-quad-{band-fill,coverage}.log` | Missing cap padding fails; restored pixel and analytic coverage pass |
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
