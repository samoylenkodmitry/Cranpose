# Showcase observer allocations

The observer captures each callback directly in its shared wrapper and iterates dependency IDs without boxing an iterator. Dependency tracking, callback replacement and scope ownership remain unchanged.

Measured on 2026-09-09 against `99c183ee`, using the actual Showcase 0.1.12 source and local framework dependencies. The release replay advances 300 frames at fixed animation intervals, with three scroll gestures, on samarch-1's Intel UHD 730 Vulkan renderer at 1080 × 2244. Builds use the shared host lock; measurements use the exclusive lock.

| Measurement | Baseline | Candidate |
| --- | ---: | ---: |
| heaptrack allocations under the measured frame stack | 1,338,975 | 1,249,419 |
| Allocations attributed directly to `ObservedIds::iter` | 48,468 | 0 |
| Callgrind instructions in the controlled replay | 4,130,818,156 | 4,118,763,225 |
| Callgrind draw-command observation calls | 7,939 | 7,939 |
| Callgrind graphics-layer observation calls | 31,642 | 31,642 |

Allocation traffic fell by 89,556 calls (6.69%, about 299 per frame). Instructions fell by 0.29%. Whole-process peak heap did **not** improve: heaptrack reported 393.71 MB versus 399.37 MB, including driver allocations and startup. This is an allocation-traffic improvement, not a demonstrated memory-footprint or Huawei FPS improvement.

The two Callgrind final images are byte-identical (SHA-256 `1739a8f0c3fcf1fdee3db78d47b40f511d0a21422d184919839342d95295b571`). Uncontrolled heaptrack runs have small workload-count differences and different final animation/scroll positions, so their allocation delta is an observed replay result, not an exact per-operation saving. Desktop Vulkan does not establish Android driver cost or display timing. The 60 FPS / less than 8 ms Huawei target remains unmet.

Twelve focused observer tests pass. New tests cover dependency sets crossing the inline-storage threshold, duplicate reads, removed dependencies, replaced callbacks and released captures. Deliberately dropping large-set IDs or suppressing callback invocation makes the dependency/callback test fail. Full gates are left to CI and merge validation as requested.

Profilers: **heaptrack 1.5.0** for allocations, **Valgrind Callgrind 3.25.1** for executed call counts and instructions, and **Linux perf** for CPU samples. Callgrind instrumentation starts after warmup and ends before capture. Heaptrack analysis filters stacks containing `measured_frame`. Whole-process perf samples include startup and capture and are not used to claim this change's CPU saving.

## Scope storage follow-up

Profiling the candidate identified 42,435 allocations in `ScopeEntry::update`. Replacing an owned scope's value inside its existing typed box removes those allocations while still replacing the payload and callback. Other storage types retain their existing construction behavior.

The follow-up replay reports 1,208,283 allocations under `measured_frame`, versus 1,249,419 before this change: 41,136 fewer (3.29%). Combined with the iterator/callback change, the observed reduction is 130,692 allocations (9.76%, about 436 per frame). `ScopeEntry::update` accounts for zero allocation leaves after the change. Callgrind instructions decrease from 4,118,763,225 to 4,115,794,599 (0.07%); the final image hash remains identical. Peak heap is 389.19 MB; the three single runs do not establish a peak-memory improvement.

Thirteen focused observer tests pass. The added test fails on allocating replacement storage; deliberately retaining the stale payload also fails it. It verifies the delivered payload, replacement callback and release of both payloads. Evidence uses the `scope-` filename prefix.

## Huawei result

The uninstrumented ABAB BABA comparison on the Huawei Mate 20 X gives **41.90 FPS before and 41.82 FPS after** both observer changes. There are two paired gains and two losses; this does not establish an FPS improvement. All eight routes complete. The device build uses the same Rust 1.98.0 compiler, NDK, application payload and release settings as the control, and emits no warnings. The latest candidate is installed after the comparison.

This agrees with the earlier held observer experiment in [mobile performance evidence](mobile_watch_performance.md): reducing this allocation traffic has not established a Huawei frame-rate gain. Further work on the device frame budget needs attribution beyond these observer allocations.

Raw profiles, replay source, resolved dependency metadata, binaries and mutant logs are retained on samarch-1 under `/home/s/cranpose-profile-loop/`. Local evidence is under `/Users/s/develop/performance-evidence/showcase-observer-20260909/`; `android.tar.gz` contains the Huawei comparison and build provenance.
