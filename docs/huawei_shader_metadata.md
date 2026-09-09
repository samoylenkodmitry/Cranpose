# Huawei shader metadata frame work

Production shaders share immutable specialization declarations across clones, cache their override hash, and construct pipeline constants only on cache misses. A mutation detaches its declarations and invalidates its own hash. Uniforms remain independently mutable; shader code, draw order and pixel arithmetic are unchanged.

## Measured result

Android **simpleperf** sampled CPU-clock and CPU-cycles at 200 Hz with DWARF call stacks. Eight matched, profileable Huawei Showcase routes use the device monotonic clock to select the 4,998 ms scrolling window. Source archives, unchanged app payloads, compiler, NDK and installed native hashes are verified. These diagnostic FPS values are excluded from acceptance.

| Sampled CPU work | Main | Candidate |
| --- | ---: | ---: |
| Override hash path | 0.234 ms/frame | 0.041 ms/frame |
| All profiled application threads, including driver work | 19.468 ms/frame | 17.638 ms/frame |

The hash path saves approximately **0.193 CPU ms/frame (82%)** in this sample. The all-thread difference includes opaque driver/kernel activity and clock variation; it is not an isolated saving attributable to hashing or a display-frame duration. Sampling estimates execution time; it does not count method invocations or allocations.

A separate, uninstrumented ABAB BABA comparison averages **41.32 FPS for main and 41.71 FPS for the candidate** at 35°C. This small difference does not establish an FPS improvement. **The 60 FPS / less than 8 ms target remains unmet.** The retained improvement removes measured repeated CPU work without changing pixels.

## Evidence and design

Before the fix, a focused test observes 25 hash computations for unchanged declarations and distinct allocations for clones. Shared declarations compute the key once. Pipeline tests observe six constants constructions across 108 lookups and six distinct pipelines; moving construction ahead of lookup restores 108 and fails the guard.

Copying a cached key per shader would still duplicate declarations and repeat cold hashing in clones. Eager hashing would repeat work during material construction. Copy-on-write metadata and lazy hashing avoid both. Empty shaders allocate no metadata. The cache is renderer-local and introduces no global memo table.

Correctness tests cover replacement, removal, insertion, signed zero, NaN bits, substrate declarations and clone isolation. Independently removing either set or clear invalidation produces incorrect GPU pixels and fails the pixel guard. Restoring both passes. The pixel test requires a GPU and cannot silently skip rendering. Huawei GLES pipeline and packed-domain guards also pass; these standalone tests do not claim Vulkan coverage.

Focused tests, formatting, clippy, workspace tests and release web completed successfully. The broader Android gate was cancelled and robot gates were not started at the user's direction; CI and the merge step own those checks. The measured Showcase Android build itself completed and ran on the phone.

[Machine-readable results](huawei_shader_metadata.json) contain every acceptance leg, CPU profile summaries and native hashes. Raw evidence is under `/tmp/cranpose-huawei-runtime-20260909`. The unpinned initial build was rejected by provenance validation before device measurement; only the rebuilt compiler-matched candidate was measured.
