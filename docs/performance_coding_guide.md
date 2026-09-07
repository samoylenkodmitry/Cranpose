# Cranpose performance coding guide

**Identical pictures; less work per frame; Jetpack Compose API.** Apply these
rules to framework code, generated code and shaders. Use the
[mobile acceptance protocol](mobile_60fps_architecture.md).

| Area | Coding rule | Required evidence |
| --- | --- | --- |
| Work | Remove repeated computation, traversal and representation conversion. Give each prepared result one owner. | Calls/frame, affected frames and saved time. A branch that never runs cannot explain a gain. |
| Builds | Inspect the consuming workspace's effective profile, features, target and flags. Dependency profiles are ignored. Compare optimization level, LTO and codegen units separately. | Source, lockfile, command and installed-library hashes; matching profiling symbols. Cranpose's profile cannot tune an unchanged external app. [Build configuration][build] |
| Allocation | Borrow within a phase; reuse owned collections across phases. Avoid temporary `collect`, boxed iterators and duplicate shared wrappers. Reserve from measured lengths; bound retention. | Allocation count, bytes, lifetimes and time. `SmallVec`, arenas and `clone_from` need a measured fit. Preserve destruction and observation leases. [Heap allocation][heap] |
| Allocators | Attribute allocation cost before comparing supported alternatives. Keep process allocator selection explicit at the executable boundary. | Separate Rust, driver and platform allocations; include RSS, faults and heat. A Rust global allocator does not replace the GPU driver's allocator. No allocator imposed by a reusable library. [Alternative allocators][build] |
| Layout/cache | Keep hot fields together; separate cold payloads. Prefer dense storage and sequential traversal when access patterns fit. | Bytes touched and target layouts; cache/refill counters where available. No assumed cache-line size, universal copy threshold or automatic benefit from padding. [Cache effects][cache], [type sizes][sizes] |
| Copying | Pass small values directly; borrow large data when it removes copies. Share immutable data across actual ownership boundaries. | Attributed copy sites and byte counts. A `memcpy` sample without a usable caller does not identify a structure. [Ownership][cheats] |
| Bounds | Use slices, iterators, exact chunks and necessary range checks before loops. Handle tails and unequal lengths. | Target assembly and unchanged error behavior. No silent `zip` truncation, masked invalid indices, or `debug_assert!` as a release invariant. No `unsafe`. [Bounds checks][bounds] |
| Inlining | Expose small hot helpers to cross-crate optimization. Keep large generic wrappers thin; outline cold work. | Caller assembly, runtime and code size. Inlining is a non-transitive hint and can worsen code generation. No blanket `inline(always)`. [Inlining][inline], [current reference][codegen] |
| Collections | Choose algorithms from actual key/cardinality distributions. Use lazy fallbacks. Discard ordering only when it has no meaning. | Complexity at realistic sizes; collision and ordering correctness. Keep the project hashing abstraction and untrusted-input protection. [Collections/state machines][book] |
| SIMD/math | Batch independent work; inspect auto-vectorization first. Use safe abstractions that support shipped targets. | ARMv7, AArch64 and wasm results, including remainders. Preserve rounding, NaNs, signed zero, blending and clipping. Never cross-compile with host `target-cpu=native`. [Batching/SIMD][cheats] |
| Concurrency | Use async for waiting. Parallelize when transfer, synchronization and thermal costs pay. Keep mutation local; avoid per-item shared counters. | Critical path, queues, waits, wakeups and hot throughput. Relaxed atomics still transfer cache lines. More threads and lock-free code are hypotheses. [Cache sharing][cache], [threads/async][cheats] |
| I/O/diagnostics | Buffer repeated I/O, reuse parsing buffers and keep disabled logging cheap. Preserve validation and errors. | Syscalls and instrumentation overhead. Disable timing instrumentation for acceptance. [I/O][io] |
| Macros/tooling | Inspect expansion, monomorphization and build timings when they dominate. Use maintained APIs and existing `just` gates. | Separate build-time gains from runtime gains. PGO needs representative training and a reproducible consuming build. [Compile times][compile], [macros][book] |
| GPU | Count shaded pixels, samples, transferred bytes and attachment loads/stores. Preserve dependencies and blend order. | Exact captures and device timing. Fewer passes can repeat shading; fewer vertices can increase overdraw. CPU savings cannot resolve a saturated GPU. |

**Experiment contract:** name the cost, remove it diagnostically, confirm the path
runs, then implement. Prove a correctness guard fails when the optimization is
broken. Use a focused check before paired device acceptance. Retain failed legs
and temperatures. Publish the decision and evidence, not a diary. Run required
repository gates on the final change; avoid repeated whole suites per hypothesis.

**Coverage:** all [Performance Book](https://nnethercote.github.io/perf-book/introduction.html) topics, including compilation; cheat-sheet
language/layout/ownership and API design; all seven cache examples; both inlining
and bounds articles. The 265-page 2018 book was surveyed across eleven chapters:
compiler, types/collections, memory, lints, profiling, benchmarks, macros, threads
and async. Its compiler-plugin, futures and nightly instructions are dated, not
current dependency advice. Source speedup percentages are not Cranpose forecasts.

[build]: https://nnethercote.github.io/perf-book/build-configuration.html#alternative-allocators
[heap]: https://nnethercote.github.io/perf-book/heap-allocations.html
[sizes]: https://nnethercote.github.io/perf-book/type-sizes.html
[io]: https://nnethercote.github.io/perf-book/io.html
[compile]: https://nnethercote.github.io/perf-book/compile-times.html
[cheats]: https://cheats.rs/
[cache]: https://igoro.com/archive/gallery-of-processor-cache-effects/
[inline]: https://matklad.github.io/2021/07/09/inline-in-rust.html
[bounds]: https://shnatsel.medium.com/how-to-avoid-bounds-checks-in-rust-without-unsafe-f65e618b4c1e
[codegen]: https://doc.rust-lang.org/reference/attributes/codegen.html#the-inline-attribute
[book]: https://github.com/goldcoders/Rust/blob/master/Eguia%20Moraza%2C%20Iban%20-%20Rust%20high%20performance_%20learn%20to%20skyrocket%20the%20performance%20of%20your%20Rust%20applications-Packt%20Publishing%20%282018%29.pdf
