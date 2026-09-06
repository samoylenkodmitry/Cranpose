# Mobile renderer: evidence index

Decision: [60 FPS sheet](mobile_60fps_architecture.md). Raw evidence:
`/tmp/cranpose-mobile-watch-60fps`. Reports retain binaries, scene checks,
SurfaceFlinger frames, temperatures, properties and failures.

| Artifact under the raw root | What it proves |
| --- | --- |
| `main-shared-profile-pair-20260906.json` | Audited release APKs: main `0d195313`, shared `37bd0ce8`; unchanged application sources |
| `{watch,huawei}-main-shared-checkpoint-{matrix,analysis}.json` | Complete eight-leg Megaboss comparisons; watch result mixed |
| `prof-huawei-showcase-{1..8}-*` | Complete scroll comparison; all shared legs faster, below 60 |
| `prof-watch-showcase-*` | Incomplete watch sequence; failed preflight prevents acceptance |
| `watch-main-shared-cpu-{leaf,hot}-*`, `huawei-main-shared-cpu-*` | Megaboss CPU time/cycles; watch shared uses about 20% fewer cycles in both orders |
| `{watch,huawei}-showcase-cpu-profileonly-*` | Full-scroll CPU diagnostics; Huawei shared halves cycles, watch change smaller |
| `showcase-profileable-pair.json` | Profiling copies differ only by manifest profileability; 393 other payloads unchanged |
| `profd-*-showcase-*`, `watch-main-shared-gpu-repeat-*` | GPU timings; inspect clocks/heat before comparing |
| `huawei-main-feed-removal-{matrix,analysis}.json` | Main feed removal loses every pair; later upload logs incomplete |
| `huawei-showcase-current-memory-stack-{A,B}/memory-callers.json` | Ten-second early-scroll stacks attribute most memcmp to RuntimeShader construction |
| `override-hash-correctness-proof.json` | Original encoding passes; missing separator fails; restored passes |
| `override-hash-huawei-matrix.json` | Eight-leg ARM hash microbenchmark; about 30% faster, not app FPS |
| `shader-factory-correctness-proof.json` | Direct-equivalence test passes; corrupt template fails; restored passes |
| `shader-factory-package-tests.log` | 340 graphics/liquid tests pass without warnings |
| `glass-coincident-0d63a76f-*-watch-test-provenance.json` | Exact candidate and two broken-guard binaries; source and native hashes verified; device proof pending |
| `cranpose-shared-rim-56328905-{gates,ios-gates}.json` | Shared platform/robot gates pass; `37bd0ce8` correction also passes |
| `cranpose-override-hash-stream-39c8804a-*-gates.json` | Hash candidate gates; robot partitions still pending |
| `sequence-ownership.jsonl` | Whole-sequence device ownership shared with Fable |

Fable owns the `shape`, `shape_fill`, `glass_dispersion`, `glass_refraction`
removal matrices and exact coincident-ray app comparison. These change material
or coverage and bound costs; they cannot prove picture-preserving performance.

CPU captures validate 200 Hz event attributes and exact installed native payloads.
Profiles are diagnostics, not FPS acceptance. Leaf samples do not identify callers;
DWARF captures are limited to ten seconds. Temperature and endpoint clocks do not
measure energy or continuous frequency. Presentation percentiles are bucket labels.
Failed runs remain invalid. No software-display FPS is used for device acceptance.
