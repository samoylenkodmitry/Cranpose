# Huawei Showcase frame budget

**The 60 FPS / less than 8 ms target remains unmet.** Two complete comparisons
reject blur tile specialization as a demonstrated performance improvement.
Production work on shader declaration ownership and pipeline cache hits is
tracked in [Shader metadata frame work](huawei_shader_metadata.md). The blur
shader and its pipelines retain their original implementation; its packed-domain
pixel guard and test harness fix remain.

## Measurement

Huawei Mate 20 X EVR-AL00, Kirin 980 / Mali-G76, native 1080 × 2244 at density
480. Framework base `f4ae9246`; unchanged Showcase 0.1.12 application sources.
The versioned `huawei-showcase.json` route performs three native swipes over
4,998 ms, validates movement and the “Search the sky” / “Proxima Centauri b”
endpoints, and records temperature for every leg. Builds, APK payloads, features,
compiler settings and installed native hashes are verified by the
[Android benchmark tools](android_benchmark.md).

Each comparison uses ABAB BABA, with A as main and B as specialization. Both
binaries in a comparison were built on macm3 with matching toolchains. No
instrumentation runs in the acceptance sequences. These five-second scroll
routes cannot be compared numerically with earlier twenty-second workloads.

| Sequence | A mean FPS | B mean FPS | Change | Battery temperature |
| --- | ---: | ---: | ---: | --- |
| First complete comparison | 44.41 | 45.87 | +3.29% | 33–35°C |
| Confirmation | 45.46 | 44.31 | −2.53% | 34–35°C |

A separate eight-leg identical-APK sequence, built on samarch-1, ranges from
41.02 to 51.22 FPS at 32–33°C. Its alternating labels identify the same binary;
they do not identify a code change. A small mean difference cannot establish a
win against this variability.

The [machine-readable results](huawei_showcase_frame_budget.json) retain every
completed leg, temperatures and native hashes. The incomplete baseline ends in
a screenshot-transfer timeout; the incomplete first comparison ends in a route
timing failure. Both are retained and excluded from acceptance. Two additional
Mali counter sequences each fail in the second leg with “GPU sampler could not
be initialized.” Their first legs cannot establish a counter improvement.

## Experiment and correctness

The hypothesis was repeated tile-mode decisions inside blur samples. Four
specialized modes replace the shared clamp/repeat/mirror kernel, and specialize
the two downsample blocks. The proposed change preserves weights, domains,
arithmetic and draw order. Earlier Mali offline compilation estimated clamp
arithmetic falling from 3.917 to 2.167 cycles; compiler estimates are not device
time, and the paired device results do not justify the additional pipelines.

A layer-level fixture initially failed to detect a forced-clamp mutant because
transparent padding hid the sampling differences. The retained test instead
samples a colored packed source region with clamp, repeat, mirror and decal in
one batch. It covers integer and fractional origins, both downsample blocks,
and Rgba8Unorm / Rgba16Float targets. Its frozen reference uses uniform-driven
tile and decal decisions, independent of production pipeline selection.
Forcing production sampling to clamp makes the guard fail; restoration produces
exact bytes. The rejected specialization also had separate red kernel and
downsample constant mutants.

The on-device GLES guard first failed before rendering: default desktop limits
requested eight color attachments from an adapter supporting four. Requesting
the adapter's supported limits fixes the shared test harness and the Huawei
captures pass. The standalone Vulkan test still cannot enumerate an adapter;
removing debug instance flags does not change that. GLES verification is not a
Vulkan verification claim. Include the GLES backend selected by the application's
Android feature when building this library's Android test executable.

## Remaining frame work

Two separate main-only diagnostic routes enable 30-frame stage telemetry. Four
central windows per route each cover 120 frames; the marker/route overhead
bounds are retained with the results.

| Mean elapsed stage | Route 1 | Route 2 |
| --- | ---: | ---: |
| Update | 3.70 ms | 3.20 ms |
| Sync | 0.12 ms | 0.15 ms |
| Render encoding | 6.27 ms | 5.68 ms |
| Presentation call | 15.76 ms | 15.73 ms |
| Presentation period | 22.49 ms | 21.79 ms |

These are elapsed spans, including driver blocking, not scheduled CPU time or
GPU timestamps. Producer and presentation-worker work overlaps. Acquisition
also includes handoff delay, and summing the stages does not give the display
period. Render encoding alone below 8 ms does not establish the requested frame
budget. The next performance change needs an attributed saving in frame work
and exact pictures; changing tile specialization alone has not demonstrated it.

Raw build proofs, APKs, failures, native tests, counter captures and mutation
logs are under `/tmp/cranpose-huawei-20260909`. The rejected source is preserved
in `rejected-blur-specialization.tar.gz`; it is excluded from production changes.

## Blur experiment validation

The retained blur guards pass 4,885 workspace tests, `just fmt`, `just clippy`,
`just precommit`, `just android`, and the release `just web` size budget
(12,168,009 / 14,680,064 bytes). CI's `just robot-gpu` passes all 164 tests and
`just robot-captures` passes all four, with no capability skips. The final
on-device GLES guard passes. Build, test and clippy logs contain no compiler
warnings.

The blur experiment restored the verified main APK and all 62 diagnostic
properties. A durable copy of that raw evidence is stored at
`/Users/s/develop/performance-evidence/huawei-showcase-20260909.tar.gz`.
